//! Other connected players: interpolated toward the server's snapshots,
//! and treated as solid by the local physics.
//!
//! **What they look like is not here.** This file owns what the network
//! knows -- where somebody is, which way they face, whether their feet
//! are on anything, how far they have walked -- and `logic::player_model`
//! turns that into a figure. The two used to be one function, and the
//! result was a model nobody could change without reading interpolation
//! code.
//!
//! Changed this pass: the server no longer relays movement message by
//! message, it sends one interest-filtered `Snapshot` per tick. Two
//! consequences handled here:
//!
//! - A player can vanish from snapshots simply by walking out of interest
//!   range, with no `PlayerLeft` message. So each player carries a
//!   `last_seen` and is dropped after a timeout -- otherwise their box
//!   would hang frozen in the world forever, and worse, keep acting as a
//!   solid obstacle you'd walk into.
//! - A figure is played back between snapshots on the tick each was built
//!   on, a tick and a half behind the newest -- see `PLAYBACK_DELAY_TICKS`
//!   for the chase this replaced, which moved legs and heads twenty times a
//!   second whatever the frame rate.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::{Outfit, PlayerId, PlayerState, Posture};

use crate::engine::item_model::ItemVertex;
use crate::engine::mesh::Vertex;
use crate::engine::texture::FaceLayers;
use crate::logic::player_model::{PACES_PER_BLOCK, WALKING};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ActorVertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
    /// Face normal, so actors can be lit by the same sun as the terrain
    /// instead of being flat-shaded slabs.
    pub normal: [f32; 3],
    /// Where on the player skin this corner reads, or `UNTEXTURED`.
    ///
    /// **Two kinds of geometry ride this pipeline**: a player, who wears
    /// a picture, and the block outline and its cracks, which are lit
    /// lines with no picture anywhere (see `logic::mining`). Rather than
    /// a second pipeline for sixty triangles, a coordinate outside the
    /// sheet means "no texture, use the colour" -- the branch is on a
    /// value that is uniform across any one draw's worth of triangles
    /// and costs nothing measurable.
    pub uv: [f32; 2],
    /// The light where this figure stands, as the terrain's light word
    /// (`mesh::pack_light`): sky and block light at the player's chest.
    ///
    /// **It was not here, and a player was lit as though standing in the
    /// open** -- `actor.wgsl` said so in as many words and added a quarter
    /// of full brightness so nobody went black in a cave. A figure is lit by
    /// the ground's own `shade_lit` now (`vs_actor` in shader.wgsl), and that
    /// needs what the ground has: a mine has no sky and a camp has a fire,
    /// and without this somebody standing in either glowed like noon.
    pub light: u32,
}

/// The texture coordinate that means "this triangle has no picture".
///
/// Negative rather than, say, a fourth component or a flag byte: the
/// sampler's coordinates are a fraction of the sheet and can never be
/// negative honestly, so there is no value this can collide with.
pub const UNTEXTURED: [f32; 2] = [-1.0, -1.0];

/// The light word of something standing under open sky with no fire near:
/// `pack_light(15, 0, 3, 0)`, written out because a `const` cannot call it.
///
/// What a figure's vertices carry until `build_actor_mesh_into` says where
/// the figure is, and what the block outline carries for good -- the outline
/// is not lit by it (see `outline_colour` in shader.wgsl).
pub const OPEN_AIR: u32 = 15 | (3 << 8);

impl ActorVertex {
    /// One corner of something that carries no picture: an outline, a
    /// crack, anything `logic::mining` draws.
    pub fn flat(position: [f32; 3], color: [f32; 3], normal: [f32; 3]) -> Self {
        ActorVertex {
            position,
            color,
            normal,
            uv: UNTEXTURED,
            light: OPEN_AIR,
        }
    }

    pub const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x3,
        3 => Float32x2,
        4 => Uint32,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ActorVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// How quickly a rider's place on a deck chases the latest snapshot: see
/// `ride`, the one thing here still eased this way.
const INTERP_RATE_PER_SEC: f32 = 12.0;

/// **How far behind the newest snapshot a figure is drawn, in server ticks.**
///
/// "модель игрока рендерится будто в 30 фпс". A figure used to chase the
/// latest snapshot at a fixed rate, with its gait's odometer, its facing and
/// its look taken straight off each snapshot as it came in. So the legs and the
/// head moved twenty times a second at any frame rate, and the body surged on
/// every snapshot and slowed before the next: at 144 frames a second a stride
/// of 0.021 to 0.039 m a frame, measured
/// (`what_a_walking_remote_player_looks_like_frame_by_frame`).
///
/// Three ways to draw between snapshots were weighed:
///
/// * *Chase the latest faster.* The surge *is* the chase, and a quicker one
///   is a bigger surge; the legs still step when a snapshot lands.
/// * *Extrapolate from the last two.* Smooth while somebody walks straight,
///   and wrong at every stop and turn: a figure that walks half a tick into a
///   wall and comes back out of it.
/// * **Play the snapshots back a little late (chosen)**, on the tick each was
///   built on, as the animals are (`Entities::apply_snapshot`). A tick and a
///   half behind the newest there is a snapshot on each side of the moment
///   drawn, and position, gait and facing are all read between the same two.
///   Seventy-five milliseconds at twenty hertz -- the chase it replaced
///   trailed a walker by about eighty-three.
///
/// The half tick over the one a snapshot is apart is how late a snapshot can
/// be before the figure waits for it.
const PLAYBACK_DELAY_TICKS: f64 = 1.5;

/// How much of the gap between the playback clock and where it should be is
/// made up in a second.
///
/// The clock runs on the frame's `dt` and is measured against the snapshots
/// as they arrive. Brought back gently rather than set: a snapshot read a
/// frame late is not a reason to jump every figure a frame forward, and at two
/// a second the correction is a per cent or two of a walking pace.
const CATCH_UP_PER_SEC: f64 = 2.0;

/// How far the playback clock may be off before it is set rather than eased:
/// a new session, a stall, a server that stopped sending for a while.
const SNAP_TICKS: f64 = 6.0;

/// How many snapshots each figure keeps: the delay, and a burst of late ones
/// read in one frame. Those the moment drawn has passed are dropped as it
/// passes them.
const HISTORY: usize = 8;

/// A server tick, until the handshake has said: the default rate.
const NOMINAL_TICK_SECONDS: f64 = 0.05;

/// Further than this between two snapshots, per tick between them, is not a
/// walk: a respawn, a teleport, somebody coming back into range. The figure is
/// there at once rather than slid across the world, and none of it is strides.
/// The animals' number (`entities::MAX_STRIDE`).
const MAX_STRIDE: f32 = 3.0;

/// How far a mounted player's snapshot seat may be from the horse they are
/// put on, in blocks (`RemotePlayers::mount`): a gallop's worth of the two
/// being eased apart and then some. The nearest horse wins inside it, so two
/// horses ridden side by side each keep their own rider; outside it a rider
/// is left where their snapshot is rather than handed to a stranger's horse.
const MOUNT_REACH: f64 = 2.0;

/// How quickly the gait's speed follows the pace between the snapshots drawn,
/// per second: about what it was when it was eased once a snapshot.
const SPEED_EASE_PER_SEC: f32 = 10.0;

/// One snapshot of one player, kept to be played back. See
/// `PLAYBACK_DELAY_TICKS`.
#[derive(Clone, Copy, Debug)]
struct Sample {
    tick: u64,
    at: glam::DVec3,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    /// The gait's odometer here: ground covered since the figure was first
    /// seen, so the distance between two samples is the strides between them.
    walked: f32,
}

/// The short way round from one facing to another, in radians. A player who
/// turns from 350 degrees to 10 has turned twenty, and easing the numbers
/// directly spins them through the other three hundred and forty.
fn shortest_turn(from: f32, to: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let delta = (to - from).rem_euclid(TAU);
    if delta > PI {
        delta - TAU
    } else {
        delta
    }
}
/// How fast another player's arm comes up into digging and goes back down,
/// in fractions of the way a second. See `RemotePlayer::dig_raise`.
const DIG_RAISE_PER_SEC: f32 = 6.0;

/// Drop a player we haven't heard about for this long.
const STALE_AFTER: Duration = Duration::from_secs(3);

/// How many players `pose_for_a_photograph` should stand in front of the
/// camera, if any.
///
/// Read once and remembered: this is asked every frame, and the process
/// environment is a lock and a string parse that has no business in a
/// frame loop.
fn posed_players() -> Option<u32> {
    static POSED: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *POSED.get_or_init(|| {
        std::env::var("PRIMITIVE_POSE_PLAYERS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            // Capped, because the figures are placed in a rank and a
            // typo with an extra zero would fill the horizon with them.
            .map(|count| count.clamp(1, 16))
    })
}

/// One number out of the environment, for the pose knobs above.
fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
}

/// What the posed rank is wearing and carrying.
///
/// **The photograph hook has to reach the clothes, or the clothes cannot
/// be checked at all.** A player model is already the one thing nobody
/// can see in singleplayer -- see `pose_for_a_photograph` -- and what is
/// *on* that model is a second layer with the same problem and one more
/// way to be wrong: a garment on the wrong limb, or a boot wearing the
/// wrong band of the skin, is a fault no test of the geometry can see
/// and no amount of reading can rule out.
///
/// ```text
/// PRIMITIVE_POSE_WEARING=iron_helm,iron_cuirass,iron_greaves,iron_boots
/// PRIMITIVE_POSE_HOLDING=stone_pickaxe
/// ```
///
/// Blocks by name, from `types::ALL_BLOCK_IDS`, because that is the name
/// `blocks.toml` and `/give` already use and inventing a second one for
/// a debugging hook would be a second thing to remember. A name nothing
/// answers to is ignored rather than fatal: this runs inside a frame
/// loop, and a typo in an environment variable is not worth a crash.
///
/// Read once and remembered, for the reason `posed_players` is.
fn posed_outfit() -> Outfit {
    static OUTFIT: std::sync::OnceLock<Outfit> = std::sync::OnceLock::new();
    *OUTFIT.get_or_init(|| {
        let by_name = |name: &str| -> Option<primitive_shared::types::BlockId> {
            let name = name.trim();
            primitive_shared::types::ALL_BLOCK_IDS
                .iter()
                .find(|(_, known)| *known == name)
                .map(|&(id, _)| id)
        };
        let mut outfit = Outfit::BARE;
        for name in std::env::var("PRIMITIVE_POSE_WEARING")
            .unwrap_or_default()
            .split(',')
        {
            // Each garment goes in *its own* slot, on the same rule the
            // server follows for a real one: where a thing is worn is a
            // fact about the thing (`equipment::slot_of`), so the
            // variable is a list of garments rather than four positions
            // somebody has to get in the right order.
            let Some(block) = by_name(name) else { continue };
            if let Some(slot) = primitive_shared::equipment::slot_of(block) {
                outfit.worn[slot.index()] = block;
            }
        }
        if let Some(block) = by_name(&std::env::var("PRIMITIVE_POSE_HOLDING").unwrap_or_default()) {
            outfit.holding = block;
        }
        outfit
    })
}

/// What the posed rank's hands are doing, from the environment.
///
/// ```text
/// PRIMITIVE_POSE_GESTURE=dig|strike|place|eat|drink|cast
/// PRIMITIVE_POSE_PHASE=0.4      # how far into it, 0..1
/// ```
///
/// For the reason the rest of `pose_for_a_photograph` exists: another
/// player's gesture is only ever seen with a second person at a second
/// machine, and a frozen phase is what makes a before and an after the same
/// picture. Answers the gesture, the one-off under way and the digging clock.
fn posed_gesture(
    held: Option<primitive_shared::types::BlockId>,
) -> (primitive_shared::protocol::Gesture, Option<(primitive_shared::protocol::Action, f32)>, f32) {
    use primitive_shared::protocol::{Action, Gesture};
    let phase = env_f32("PRIMITIVE_POSE_PHASE").unwrap_or(0.4).clamp(0.0, 1.0);
    let action = match std::env::var("PRIMITIVE_POSE_GESTURE").as_deref() {
        Ok("dig") => {
            let swing = crate::logic::hand::SWING_SECONDS;
            return (Gesture { digging: true, ..Gesture::default() }, None, phase * swing);
        }
        Ok("strike") => Action::Strike,
        Ok("place") => Action::Place,
        Ok("eat") => Action::Eat,
        Ok("drink") => Action::Drink,
        Ok("cast") => Action::Cast,
        _ => return (Gesture::default(), None, 0.0),
    };
    let gesture = Gesture { last: action, count: 1, digging: false };
    (gesture, Some((action, phase * gesture_seconds(action, held))), 0.0)
}

/// How long a one-off gesture lasts on screen, with `held` in the hand.
///
/// **A blow is as long as it is in the hand of the player striking it**
/// (`hand::blow_seconds`), so a spear's thrust is the same second on both
/// screens; the rest are `player_model`'s, which says why they are that long.
fn gesture_seconds(
    action: primitive_shared::protocol::Action,
    held: Option<primitive_shared::types::BlockId>,
) -> f32 {
    use crate::logic::player_model::{DRINK_SECONDS, EAT_SECONDS, PLACE_SECONDS};
    use primitive_shared::protocol::Action;
    match action {
        Action::Strike => crate::logic::hand::blow_seconds(held),
        Action::Place => PLACE_SECONDS,
        Action::Eat => EAT_SECONDS,
        Action::Drink => DRINK_SECONDS,
        Action::Cast => crate::logic::hand::ROD_WHIP_SECONDS,
        Action::Nothing => 0.0,
    }
}

pub struct RemotePlayer {
    /// What we're actually drawing and colliding against this frame: the
    /// snapshots played back (Этап 4: "используйте интерполяцию для
    /// сглаживания движений других игроков"; `PLAYBACK_DELAY_TICKS`).
    pub interpolated_pos: glam::DVec3,
    /// Where the server last said they are. Not drawn -- the figure is played
    /// back behind it -- but what a rider's place on a deck is found from
    /// (`ride`).
    pub target_pos: glam::DVec3,
    /// Facing and look as drawn this frame, between the two snapshots either
    /// side of it.
    pub yaw: f32,
    pub pitch: f32,
    pub username: Option<String>,
    pub last_seen: Instant,
    /// How far this player has walked, in blocks, ever.
    ///
    /// The gait's clock. Distance rather than time, so a player who
    /// stops mid-stride keeps the leg where it was instead of the legs
    /// scissoring on the spot -- which is the same argument
    /// `animal_model` makes for animals, and the reason this is a
    /// number and not a phase.
    ///
    /// **Read between snapshots**, like the position. It was added to when a
    /// snapshot landed, and a leg that moves twenty times a second strobes.
    pub walked: f32,
    /// How fast they are going between the snapshots drawn, in blocks a
    /// second. What decides how *far* a leg swings: a walk and a sprint are
    /// the same cycle at different amplitudes.
    pub speed: f32,
    /// Whether the server had their feet on something.
    ///
    /// Already in every snapshot and, until now, thrown away here. It
    /// is what stops a jumping player from pedalling through the air:
    /// see `player_model::Pose::airborne`.
    pub on_ground: bool,
    /// How long this figure has been on screen, in seconds.
    ///
    /// The breath's clock, and the one thing in the model that is
    /// driven by time rather than by distance. It has to be: breathing
    /// is what a person does when they are *not* moving, so a clock is
    /// the only thing left to hang it on.
    pub age: f32,
    /// What they have on and what is in their hand, as the last
    /// snapshot said.
    ///
    /// Taken outright rather than eased, unlike the position and the
    /// gait: a helmet is on or it is not, and there is no half-worn
    /// state for an interpolator to be in.
    pub outfit: Outfit,
    /// Standing, sitting or lying, as the last snapshot said. See
    /// `protocol::Posture`: a sleeper used to be drawn standing upright in
    /// the middle of their bed.
    pub posture: primitive_shared::protocol::Posture,
    /// What their hands are doing, as the last snapshot said. See
    /// `protocol::Gesture`.
    pub gesture: primitive_shared::protocol::Gesture,
    /// The one-off gesture being drawn, and how many seconds into it.
    ///
    /// Started when a snapshot's count moves on (`apply_snapshot`), advanced
    /// by `tick`, and dropped when it has run its length -- a clock of the
    /// client's own, so a blow is drawn whole at any frame rate whatever the
    /// snapshots between its start and its end said.
    pub motion: Option<(primitive_shared::protocol::Action, f32)>,
    /// Seconds of swinging at a block so far, while they are: the digging
    /// rhythm's clock. Zero once the arm is back down.
    pub dug_for: f32,
    /// How far the arm is up into the work, 0..1: eased towards 1 while the
    /// snapshot says they are digging and back to 0 after. See `Arm::Chop`.
    pub dig_raise: f32,
    /// How badly they are limping, 0 to 1, as the last snapshot said.
    ///
    /// Taken outright rather than eased, like the outfit and the posture and
    /// unlike the gait: a leg breaks in one moment and the pace it leaves
    /// arrives in the same snapshot as the limp, so easing this one and not
    /// the other would draw somebody walking soundly at an injured pace for a
    /// quarter of a second. What moves gradually already moves gradually --
    /// exhaustion fills over minutes, and this number follows it.
    pub limp: f32,
    /// ...and on which leg: see `protocol::PlayerState::limp_left`.
    pub limp_left: bool,
    /// The snapshots still to be drawn, oldest first, and the one just behind
    /// the moment being drawn. Empty for a posed figure, which stands where it
    /// was put.
    samples: VecDeque<Sample>,
}

impl RemotePlayer {
    /// Puts the figure where its snapshots say it was at `playback`, a server
    /// tick and a fraction of one. See `PLAYBACK_DELAY_TICKS`.
    ///
    /// **Held, not guessed, outside the snapshots it has.** Before the first
    /// is a figure that has just come into view; past the last is a snapshot
    /// that is late, and a figure walked on into whatever it was about to
    /// meet would have to walk back out of it.
    fn follow(&mut self, playback: f64, dt: f32, tick_seconds: f64) {
        while self.samples.len() > 1 && self.samples[1].tick as f64 <= playback {
            self.samples.pop_front();
        }
        let Some(&from) = self.samples.front() else {
            return;
        };
        let to = self.samples.get(1).copied().filter(|_| playback > from.tick as f64).unwrap_or(from);
        let span = to.tick.saturating_sub(from.tick) as f64;
        let t = if span > 0.0 { ((playback - from.tick as f64) / span).clamp(0.0, 1.0) as f32 } else { 0.0 };
        self.interpolated_pos = from.at.lerp(to.at, f64::from(t));
        self.yaw = from.yaw + shortest_turn(from.yaw, to.yaw) * t;
        self.pitch = from.pitch + (to.pitch - from.pitch) * t;
        self.walked = from.walked + (to.walked - from.walked) * t;
        self.on_ground = from.on_ground;
        // The pace between the two drawn, which is nothing while the figure
        // waits on a late snapshot: it is standing still on screen, and legs
        // walking on the spot would say otherwise.
        let pace = if span > 0.0 { (to.walked - from.walked) / (span * tick_seconds) as f32 } else { 0.0 };
        self.speed += (pace - self.speed) * (1.0 - (-SPEED_EASE_PER_SEC * dt).exp());
    }

    /// What is in their hand, as a block or nothing.
    fn held(&self) -> Option<primitive_shared::types::BlockId> {
        (self.outfit.holding != primitive_shared::types::BLOCK_AIR).then_some(self.outfit.holding)
    }

    /// The right arm, out of the gesture on the wire.
    ///
    /// **A one-off before the digging**: a player can strike while their swing
    /// at a block is still being reported, and the blow is the shorter and
    /// rarer thing to see. **A strike with a spear in hand is a thrust** and
    /// with anything else a swing, the same split the player's own hand makes.
    /// The digging rhythm is `hand::blow_seconds(None)` whatever is held, for
    /// the reason `Hand::update` gives: a block comes apart at a pick's pace.
    fn arm(&self) -> Option<crate::logic::player_model::Arm> {
        use crate::logic::player_model::Arm;
        use primitive_shared::protocol::Action;
        if let Some((action, elapsed)) = self.motion {
            let held = self.held();
            let phase = (elapsed / gesture_seconds(action, held).max(1e-3)).clamp(0.0, 1.0);
            return match action {
                Action::Strike if held.is_some_and(primitive_shared::types::is_weapon) => {
                    Some(Arm::Thrust(phase))
                }
                Action::Strike => Some(Arm::Swing(phase)),
                Action::Place => Some(Arm::Place(phase)),
                Action::Eat => Some(Arm::Eat(phase)),
                Action::Drink => Some(Arm::Drink(phase)),
                Action::Cast => Some(Arm::Cast(phase)),
                Action::Nothing => None,
            };
        }
        // **Working a rod is drawing it back**, not chopping with it: the
        // same flag on the wire, read by what is in the hand. See
        // `Gesture::digging`.
        if self.dig_raise > 0.0 && self.held().map(primitive_shared::types::block_kind) == Some(primitive_shared::types::BLOCK_FISHING_ROD) {
            return Some(Arm::Wind(self.dig_raise));
        }
        if self.dig_raise > 0.0 {
            let swing = crate::logic::hand::SWING_SECONDS;
            return Some(Arm::Chop { phase: self.dug_for.rem_euclid(swing) / swing, raised: self.dig_raise });
        }
        None
    }

    /// One frame of the gesture clocks. See `motion` and `dug_for`.
    fn advance_gesture(&mut self, dt: f32) {
        if let Some((action, elapsed)) = self.motion {
            let elapsed = elapsed + dt;
            self.motion = (elapsed < gesture_seconds(action, self.held())).then_some((action, elapsed));
        }
        // Up into the work in about a sixth of a second and down out of it
        // in the same: a blow's worth either way, so the first chop starts
        // from the arm coming up and the last does not leave it in the air.
        let wanted = if self.gesture.digging { 1.0 } else { 0.0 };
        let step = DIG_RAISE_PER_SEC * dt;
        self.dig_raise += (wanted - self.dig_raise).clamp(-step, step);
        self.dug_for = if self.dig_raise > 0.0 { self.dug_for + dt } else { 0.0 };
    }

    /// What the model needs to know, out of what the network knows.
    pub fn pose(&self) -> crate::logic::player_model::Pose {
        crate::logic::player_model::Pose {
            // A sleeper's snapshot yaw is the way their head lies along the
            // bed, and the lying figure's head is laid out behind it -- see
            // `player_model::postured` -- so it is turned half round.
            yaw: match self.posture {
                primitive_shared::protocol::Posture::Lying => self.yaw + std::f32::consts::PI,
                _ => self.yaw,
            },
            posture: self.posture,
            pitch: self.pitch,
            walked: self.walked,
            speed: self.speed,
            airborne: !self.on_ground,
            // What the snapshot says their hands are doing, and nothing
            // guessed from the world round them. See `arm`.
            arm: self.arm(),
            age: self.age,
            outfit: self.outfit,
            limp: self.limp,
            limp_left: self.limp_left,
        }
    }
}

#[derive(Default)]
pub struct RemotePlayers {
    players: HashMap<PlayerId, RemotePlayer>,
    names: HashMap<PlayerId, String>,
    /// Who is standing on a raft, and where on its deck, eased. See `ride`.
    riding: HashMap<PlayerId, (primitive_shared::protocol::EntityId, [f32; 3])>,
    /// The moment every figure is drawn at, in server ticks. See
    /// `PLAYBACK_DELAY_TICKS`.
    playback: Option<f64>,
    /// How far `playback` was from where it should be when the last snapshot
    /// arrived, less what has been made up since (`CATCH_UP_PER_SEC`).
    behind: f64,
    /// The newest tick a snapshot has been applied for. One at or before it
    /// is a duplicate or was overtaken, and is dropped.
    newest_tick: Option<u64>,
    /// How long a server tick is, out of the handshake.
    tick_seconds: Option<f64>,
    /// The player's *own* gesture count, as the last snapshot carried it,
    /// and the one-off it last moved on for that nobody has taken yet.
    ///
    /// **The first-person hand's way of knowing a mouthful went down.** A
    /// block set down, a meal and a drink are the server's to decide -- a
    /// click on food the body cannot hold is nothing -- and the snapshot
    /// already carries the answer for everybody else. The local player's
    /// own row was skipped, so their hand did nothing at all while everybody
    /// watching saw them eat. See `take_own_gesture`.
    own_count: Option<u8>,
    own_pending: Option<primitive_shared::protocol::Action>,
}

impl RemotePlayers {
    /// Puts everyone standing on a raft onto the raft as it is drawn.
    ///
    /// **After `tick`, and it overrules it for riders.** `tick` eases a body
    /// toward its last snapshot at its own rate and the entity table eases a
    /// raft toward its last snapshot at another, so a rider on a raft going in
    /// a straight line was drawn sliding toward the stern and catching up,
    /// twenty times a second. A rider's place *on the deck* is what the
    /// snapshot says (`riding::deck_place`), and that place is eased instead
    /// -- so walking across a deck is smooth and standing still on one is
    /// still -- and put onto the deck being drawn.
    pub fn ride(&mut self, decks: &[crate::logic::riding::RaftPose], dt: f32) {
        let ease = (INTERP_RATE_PER_SEC * dt).clamp(0.0, 1.0);
        for (id, player) in self.players.iter_mut() {
            let Some((raft, place)) = crate::logic::riding::deck_place(player.target_pos, decks) else {
                self.riding.remove(id);
                continue;
            };
            let eased = match self.riding.get(id) {
                // A step of a block or more between snapshots is not a walk
                // across the deck; it is a player who has just boarded.
                Some(&(was_on, was)) if was_on == raft && (0..3).all(|a| (was[a] - place[a]).abs() < 1.0) => {
                    [0, 1, 2].map(|a| was[a] + (place[a] - was[a]) * ease)
                }
                _ => place,
            };
            self.riding.insert(*id, (raft, eased));
            if let Some(pose) = decks.iter().find(|pose| pose.id == raft) {
                player.interpolated_pos = glam::DVec3::from(pose.now.world_of(eased));
            }
        }
    }

    /// Puts everyone on horseback onto the saddle of the horse as it is drawn
    /// (`Entities::ridden_horses`): their feet `horse::RIDER_LIFT` over its
    /// feet, facing its way.
    ///
    /// **After `tick`, and it overrules it for riders**, as `ride` does for a
    /// raft and for the same reason: the player and the horse are eased toward
    /// their snapshots at two different rates, so a rider drawn at their own
    /// eased position bobs a hand's breadth fore and aft of the saddle on
    /// every snapshot. The server already put the rider exactly on the horse,
    /// so there is nothing of the rider's own position to keep -- only which
    /// horse, which is the nearest one with somebody on it to where the
    /// snapshot says they sit, within `MOUNT_REACH`.
    pub fn mount(&mut self, horses: &[(glam::DVec3, f32)]) {
        let lift = glam::DVec3::Y * f64::from(primitive_shared::horse::RIDER_LIFT);
        for player in self.players.values_mut() {
            if player.posture != primitive_shared::protocol::Posture::Mounted {
                continue;
            }
            let under = player.target_pos - lift;
            let nearest = horses
                .iter()
                .map(|&(feet, yaw)| (feet.distance(under), feet, yaw))
                .filter(|&(apart, _, _)| apart < MOUNT_REACH)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, feet, yaw)) = nearest {
                player.interpolated_pos = feet + lift;
                player.yaw = yaw;
            }
        }
    }

    /// Records how fast the server this session is connected to ticks, out
    /// of the handshake -- the length a snapshot's tick is played back over.
    /// A rate that is not a sane positive number is ignored rather than
    /// believed, for `Entities::set_tick_rate`'s reason.
    pub fn set_tick_rate(&mut self, hz: f32) {
        if hz.is_finite() && hz > 0.0 {
            self.tick_seconds = Some((1.0 / hz as f64).clamp(0.001, 1.0));
        }
    }

    /// Applies one tick's worth of server truth: the snapshot built on
    /// server tick `tick`.
    ///
    /// **On the tick, not the clock**, for the reason `Entities::apply_snapshot`
    /// gives: the client drains its whole queue in one frame, so two snapshots
    /// sent a twentieth of a second apart can be read a microsecond apart.
    /// The moment each one describes is arithmetic on its tick. The gait's
    /// speed used to be measured over the gap between readings, which is that
    /// same mistake.
    pub fn apply_snapshot(&mut self, tick: u64, states: &[PlayerState], my_id: Option<PlayerId>) {
        if self.newest_tick.is_some_and(|newest| tick <= newest) {
            return;
        }
        self.newest_tick = Some(tick);
        let wanted = tick as f64 - PLAYBACK_DELAY_TICKS;
        match self.playback {
            Some(playback) if (wanted - playback).abs() <= SNAP_TICKS => self.behind = wanted - playback,
            _ => {
                self.playback = Some(wanted);
                self.behind = 0.0;
            }
        }
        let now = Instant::now();
        for state in states {
            if Some(state.id) == my_id {
                // Not the first snapshot: a count already standing when the
                // session began is a gesture made before anybody here could
                // see it.
                if self.own_count.is_some_and(|count| count != state.gesture.count)
                    && state.gesture.last != primitive_shared::protocol::Action::Nothing
                {
                    self.own_pending = Some(state.gesture.last);
                }
                self.own_count = Some(state.gesture.count);
                continue;
            }
            let target = glam::DVec3::new(state.x, state.y, state.z);
            // `get_mut` first rather than the entry API: the common case
            // is a known player, and the entry form would clone the
            // username per player per snapshot just to throw it away.
            if let Some(p) = self.players.get_mut(&state.id) {
                let last = p.samples.back().copied();
                // **A jump is not a walk, and lying down is not either.** Both
                // start the figure over where the snapshot puts it. A respawn
                // slid across the world is worse than a figure that is simply
                // there, and lying down or getting up eased over a quarter
                // second was a figure gliding across the mattress.
                // Into the water and out of it is not a jump: a wader at the
                // waist line crosses it back and forth, and starting the glide
                // over at every crossing was a figure stuttering along a bank.
                let swam = |posture| matches!(posture, Posture::Standing | Posture::Swimming);
                let restart = (p.posture != state.posture && !(swam(p.posture) && swam(state.posture)))
                    || last.is_none_or(|s| {
                        let ticks = tick.saturating_sub(s.tick).clamp(1, 20) as f32;
                        (target - s.at).length() > f64::from(MAX_STRIDE * ticks)
                    });
                let walked = match last {
                    // The gait's odometer, and it counts the *ground* it
                    // covered. Height is left out on purpose: a player
                    // falling down a shaft has not taken a step, and legs
                    // that cycled on the way down would be walking on air.
                    Some(s) if !restart => s.walked + Vec3::new((target.x - s.at.x) as f32, 0.0, (target.z - s.at.z) as f32).length(),
                    _ => p.walked,
                };
                if restart {
                    p.samples.clear();
                    p.interpolated_pos = target;
                    p.yaw = state.yaw;
                    p.pitch = state.pitch;
                    p.on_ground = state.on_ground;
                }
                p.samples.push_back(Sample {
                    tick,
                    at: target,
                    yaw: state.yaw,
                    pitch: state.pitch,
                    on_ground: state.on_ground,
                    walked,
                });
                while p.samples.len() > HISTORY {
                    p.samples.pop_front();
                }
                p.target_pos = target;
                p.outfit = state.outfit;
                p.posture = state.posture;
                p.limp = f32::from(state.limp) / 255.0;
                p.limp_left = state.limp_left;
                // A new one-off, from its beginning. Not on the first snapshot
                // of somebody (the branch below): a count already standing
                // when they came into view is a blow struck before anybody
                // here could see it, and drawing it would be a figure that
                // swings at nothing the moment it appears.
                if state.gesture.count != p.gesture.count
                    && state.gesture.last != primitive_shared::protocol::Action::Nothing
                {
                    p.motion = Some((state.gesture.last, 0.0));
                }
                p.gesture = state.gesture;
                p.last_seen = now;
            } else {
                self.players.insert(
                    state.id,
                    RemotePlayer {
                        interpolated_pos: target,
                        target_pos: target,
                        yaw: state.yaw,
                        pitch: state.pitch,
                        username: self.names.get(&state.id).cloned(),
                        last_seen: now,
                        walked: 0.0,
                        speed: 0.0,
                        on_ground: state.on_ground,
                        age: 0.0,
                        outfit: state.outfit,
                        posture: state.posture,
                        gesture: state.gesture,
                        motion: None,
                        dug_for: 0.0,
                        dig_raise: 0.0,
                        limp: f32::from(state.limp) / 255.0,
                        limp_left: state.limp_left,
                        samples: VecDeque::from([Sample {
                            tick,
                            at: target,
                            yaw: state.yaw,
                            pitch: state.pitch,
                            on_ground: state.on_ground,
                            walked: 0.0,
                        }]),
                    },
                );
            }
        }
    }

    /// A one-off the server has confirmed the local player made since this
    /// was last asked, for the first-person hand to act out. See `own_count`.
    pub fn take_own_gesture(&mut self) -> Option<primitive_shared::protocol::Action> {
        self.own_pending.take()
    }

    pub fn on_join(&mut self, id: PlayerId, username: String) {
        if let Some(player) = self.players.get_mut(&id) {
            player.username = Some(username.clone());
        }
        self.names.insert(id, username);
    }

    pub fn remove(&mut self, id: PlayerId) {
        self.players.remove(&id);
        self.names.remove(&id);
    }

    pub fn name_of(&self, id: PlayerId) -> Option<&str> {
        self.names.get(&id).map(|s| s.as_str())
    }

    /// Plays the snapshots on by one frame and forgets anyone who's gone
    /// quiet.
    pub fn tick(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        let tick_seconds = self.tick_seconds.unwrap_or(NOMINAL_TICK_SECONDS);
        if let Some(playback) = self.playback.as_mut() {
            let made_up = self.behind * (1.0 - (-CATCH_UP_PER_SEC * dt as f64).exp());
            *playback += dt as f64 / tick_seconds + made_up;
            self.behind -= made_up;
        }
        let playback = self.playback;
        let now = Instant::now();
        for p in self.players.values_mut() {
            if let Some(playback) = playback {
                p.follow(playback, dt, tick_seconds);
            }
            p.age += dt;
            p.advance_gesture(dt);
        }
        self.players
            .retain(|_, p| now.duration_since(p.last_seen) < STALE_AFTER);
    }

    /// **A rank of players standing in front of the camera, so the model
    /// can be photographed without a second person.**
    ///
    /// A player model is the one thing in this game nobody can see in
    /// singleplayer: there is no third-person view, and the local player
    /// is never drawn. Checking a change to it therefore meant two
    /// clients, two accounts and a hand on each mouse -- which is the
    /// situation this repository's rule covers: when something is only
    /// reachable by hand, the fix is the missing hook, not an experiment
    /// nobody can repeat.
    ///
    /// ```text
    /// PRIMITIVE_POSE_PLAYERS=4     # four of them, each turned a quarter further
    /// PRIMITIVE_POSE_SPEED=0       # ...standing rather than walking
    /// PRIMITIVE_POSE_STRIDE=0.25   # ...all at one point in the stride
    /// ```
    ///
    /// **Every pose is fixed rather than running**, and that is the
    /// whole reason this is worth writing down: a gait advanced by the
    /// frame clock puts the legs somewhere different in every
    /// photograph, and then a before-and-after pair of shots differs by
    /// the phase as well as by the change. Frozen, the same command
    /// gives the same picture twice.
    ///
    /// Refreshed every frame because `tick` drops anyone who has gone
    /// quiet for `STALE_AFTER` -- these arrive in no snapshot, so
    /// without this they would appear for three seconds and vanish.
    pub fn pose_for_a_photograph(&mut self, feet: Vec3, camera_yaw: f32) {
        let Some(count) = posed_players() else {
            return;
        };
        // Four blocks out: far enough that a whole figure is in frame at
        // any field of view the settings allow, near enough that the
        // face is more than a smear.
        const DISTANCE: f32 = 4.0;
        const SPACING: f32 = 1.4;
        let (sin, cos) = camera_yaw.sin_cos();
        // Yaw zero looks along +X -- see `Camera::forward`.
        let forward = Vec3::new(cos, 0.0, sin);
        let across = Vec3::new(-sin, 0.0, cos);
        let speed = env_f32("PRIMITIVE_POSE_SPEED").unwrap_or(WALKING);
        let now = Instant::now();
        for index in 0..count {
            let offset = (index as f32 - (count as f32 - 1.0) * 0.5) * SPACING;
            let at = feet + forward * DISTANCE + across * offset;
            // A quarter turn further round for each: four of them show
            // the front, both profiles and the back in one frame, which
            // is the whole of "walk round it and check" done once.
            let yaw = camera_yaw
                + std::f32::consts::PI
                + index as f32 * std::f32::consts::FRAC_PI_2;
            // Spread round one stride, so a rank shows the cycle rather
            // than four copies of one instant.
            let stride = env_f32("PRIMITIVE_POSE_STRIDE")
                .unwrap_or(index as f32 / count.max(1) as f32);
            let walked = stride / PACES_PER_BLOCK;
            let outfit = posed_outfit();
            let posed = posed_gesture(
                (outfit.holding != primitive_shared::types::BLOCK_AIR).then_some(outfit.holding),
            );
            let posed = RemotePlayer {
                interpolated_pos: at.as_dvec3(),
                target_pos: at.as_dvec3(),
                yaw,
                pitch: 0.0,
                username: Some(format!("pose{index}")),
                last_seen: now,
                walked,
                speed,
                on_ground: true,
                // A posed figure is not hurt: a limp is a thing the
                // snapshot says, and a photograph has no snapshot.
                limp: 0.0,
                limp_left: false,
                // Fixed, like everything else about a posed figure: a
                // breath advancing with the frame clock would put the
                // shoulders somewhere different in every photograph.
                age: 0.0,
                outfit,
                // `PRIMITIVE_POSE_POSTURE=sitting` or `lying` photographs
                // the rank in that posture, on the same terms as the rest
                // of this hook: nobody can sit down in front of their own
                // camera, so without it a seated figure is only ever seen
                // by a second player.
                posture: match std::env::var("PRIMITIVE_POSE_POSTURE").as_deref() {
                    Ok("sitting") => primitive_shared::protocol::Posture::Sitting,
                    Ok("lying") => primitive_shared::protocol::Posture::Lying,
                    // Astride, with nothing under them: the legs bent round
                    // where a horse's barrel would be (`player_model::astride`).
                    Ok("mounted") => primitive_shared::protocol::Posture::Mounted,
                    _ => primitive_shared::protocol::Posture::Standing,
                },
                // `PRIMITIVE_POSE_GESTURE`, frozen at `PRIMITIVE_POSE_PHASE`
                // for the reason the stride is. See `posed_gesture`.
                gesture: posed.0,
                motion: posed.1,
                dug_for: posed.2,
                dig_raise: if posed.0.digging { 1.0 } else { 0.0 },
                samples: VecDeque::new(),
            };
            // Counting down from the top of the range: a real id comes
            // from the server and starts at one, so these cannot collide
            // with anybody actually connected.
            self.players.insert(PlayerId::MAX - index as u64, posed);
        }
    }

    /// Where everyone who can be walked into is.
    ///
    /// **Not a sleeper.** The collider is an upright box, and a body lying
    /// across a bed is not one: it was a pillar standing in the middle of
    /// the bed that nobody could see and everybody walked into. The bed
    /// under them is solid already.
    pub fn iter_positions(&self) -> impl Iterator<Item = glam::DVec3> + '_ {
        // ...and not the dead, for the same reason: a body lies on the ground.
        // Nor the downed, who are crawling on it -- and who somebody is about
        // to kneel over to help up, which a pillar would stop them reaching.
        self.players
            .values()
            .filter(|p| {
                !matches!(
                    p.posture,
                    primitive_shared::protocol::Posture::Lying
                        | primitive_shared::protocol::Posture::Fallen
                        | primitive_shared::protocol::Posture::Crawling
                )
            })
            .map(|p| p.interpolated_pos)
    }

    /// Whether this player is on the ground and not yet dead, as the last
    /// snapshot said -- the one a right click can help up
    /// (`ClientMessage::HelpUp`).
    pub fn is_down(&self, id: PlayerId) -> bool {
        self.players
            .get(&id)
            .is_some_and(|p| p.posture == primitive_shared::protocol::Posture::Crawling)
    }

    /// Everyone, whole. `iter_positions` is kept beside this because
    /// the collider wants points and only the model wants the rest.
    pub fn iter(&self) -> impl Iterator<Item = &RemotePlayer> + '_ {
        self.players.values()
    }

    /// Who is under the crosshair, and how far off.
    ///
    /// The nearest one the ray reaches, so someone behind a player being
    /// aimed at is not hit through them. `dir` must be normalised, since
    /// the distance comes back in blocks and is compared against the
    /// distance to whatever block is behind them.
    ///
    /// Drawn against the *interpolated* position rather than the last
    /// snapshot -- the crosshair has to agree with what is on screen,
    /// and the server's tolerance is what covers the difference between
    /// that and where the server thinks they are. See
    /// `primitive_shared::combat`.
    pub fn aimed_at(&self, eye: glam::DVec3, dir: Vec3, range: f32) -> Option<(PlayerId, f32)> {
        self.players
            .iter()
            // A dead player cannot be struck -- the server refuses it -- and
            // a crosshair that turned red on the air over a body would say
            // otherwise.
            .filter(|(_, player)| player.posture != primitive_shared::protocol::Posture::Fallen)
            .filter_map(|(&id, player)| {
                let feet = player.interpolated_pos;
                primitive_shared::geometry::ray_hits_player(
                    eye.into(),
                    (dir.x, dir.y, dir.z),
                    feet.into(),
                    range,
                )
                .map(|distance| (id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Whether nobody is drawn at all -- which is what lets the frame build
    /// the figures on the clock rather than on every frame
    /// (`figures_rebuild_due`).
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }
}

/// One combined mesh for every remote player's hitbox, in a single
/// vertex/index buffer -- cheap enough to rebuild every frame at the
/// player counts a single client can actually see.
#[cfg(test)]
pub fn build_actor_mesh(players: &RemotePlayers) -> (Vec<ActorVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    build_actor_mesh_into(players, None, Vec3::ZERO, &LightMap::new(), &mut vertices, &mut indices);
    (vertices, indices)
}

/// How far above the feet a figure's light is taken: the chest, so that
/// `entities::sampled_light`'s pair of cells is the two a standing player
/// fills.
const CHEST: f32 = 0.9;

/// The same mesh, appended to buffers the caller keeps.
///
/// Rebuilt every frame because the players move every frame; reusing the
/// storage means it does not also allocate every frame.
///
/// **What a player looks like is not decided here.** The shape, the
/// skin and the gait are a table in `logic::player_model`; this file
/// owns what the *network* knows -- where somebody is, which way they
/// face, how far they have walked -- and hands it over. The two used to
/// be one function, and the result was a model nobody could edit
/// without reading interpolation code.
///
/// `light` is the world's light map: a figure is lit where it stands, the
/// way an animal is -- see `ActorVertex::light`.
pub fn build_actor_mesh_into(
    players: &RemotePlayers,
    chunks: Option<&crate::logic::chunk_manager::ChunkManager>,
    origin: Vec3,
    light: &LightMap,
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    let lit = |at: glam::DVec3| {
        let (sky, block) = crate::logic::entities::sampled_light(at, light);
        crate::engine::mesh::pack_light(sky, block, 3, 0)
    };
    // Every body lying in the loaded world, in the skin and the clothes of
    // the player it was. See `player_model::append_lying` for why a body is
    // drawn here and not in the chunk mesh; the yaw is the cell's hash, as it
    // was there, so the cracks of `mining` still land on it.
    for body in chunks.into_iter().flat_map(|chunks| chunks.bodies()) {
        let (x, y, z) = body.cell;
        let first = vertices.len();
        let ground = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
        crate::logic::player_model::append_lying(
            ground - origin,
            crate::logic::animal_model::carcass_yaw(x, y, z),
            &body.worn,
            vertices,
            indices,
        );
        let word = lit(glam::DVec3::new(f64::from(x) + 0.5, f64::from(y) + 0.3, f64::from(z) + 0.5));
        for vertex in &mut vertices[first..] {
            vertex.light = word;
        }
    }
    for player in players.iter() {
        let first = vertices.len();
        let word = lit(player.interpolated_pos + glam::DVec3::Y * f64::from(CHEST));
        if player.posture == primitive_shared::protocol::Posture::Fallen {
            // **Dead, and drawn once.** The body their death left lies in
            // their own column a little above or below their feet
            // (`corpse_cell` on the server), and that body *is* them -- a
            // figure drawn beside it would be the same person lying down
            // twice. Only a death that left nothing (an empty pack, and
            // nothing worn) leaves the figure itself to fall where it stood.
            let feet = player.interpolated_pos;
            let (fx, fy, fz) = (feet.x.floor() as i32, feet.y.floor() as i32, feet.z.floor() as i32);
            if chunks.is_some_and(|chunks| chunks.body_in_column(fx, fz, fy - 2, fy + 3)) {
                continue;
            }
            crate::logic::player_model::append_lying(
                (feet - origin.as_dvec3()).as_vec3(),
                player.yaw,
                &player.outfit.worn,
                vertices,
                indices,
            );
        } else {
            crate::logic::player_model::append(
                &player.pose(),
                (player.interpolated_pos - origin.as_dvec3()).as_vec3(),
                // White: the skin is drawn as it should look, and a tint is
                // a multiplier. The salmon pink this used to be was not a
                // choice about colour at all -- it was the only way to see
                // an untextured box.
                [1.0, 1.0, 1.0],
                vertices,
                indices,
            );
        }
        // Written after the figure is built rather than threaded through it:
        // the model is a table of boxes that knows nothing of the world, and
        // one light for a whole figure is what an animal gets too.
        for vertex in &mut vertices[first..] {
            vertex.light = word;
        }
    }
}

/// What everybody nearby is carrying, appended to the meshes the world
/// is drawn with.
///
/// **Not part of `build_actor_mesh_into`, and the reason is the
/// pipeline.** The actor pass samples one 64x32 player skin and nothing
/// else -- see `vs_actor` in `engine/shader.wgsl` -- so it has no way to put the
/// picture of a pickaxe on anything. A carried thing therefore rides
/// exactly where a dropped one does: a sprite with a thickness in the
/// item pass, or a cube in the terrain pass, both of which already
/// address the block atlas and are already lit and fogged by the world.
///
/// The alternative -- widening the actor pipeline to reach the atlas as
/// well -- was rejected because it makes one shader answer two questions
/// about which texture a vertex means, to save a function call.
#[allow(clippy::too_many_arguments)]
pub fn build_held_items_into(
    players: &RemotePlayers,
    origin: Vec3,
    layers: &FaceLayers,
    light: &LightMap,
    models: Option<&crate::engine::texture::TextureManager>,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    item_vertices: &mut Vec<ItemVertex>,
    item_indices: &mut Vec<u32>,
) {
    for player in players.iter() {
        crate::logic::player_model::append_held(
            &player.pose(),
            // The world position, not the one measured from the render
            // origin: the hand's light is sampled where the hand
            // actually is, and `append_held` does the subtraction
            // itself for the geometry.
            player.interpolated_pos,
            origin,
            layers,
            light,
            models,
            vertices,
            indices,
            item_vertices,
            item_indices,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Somebody walking a long way out is drawn walking, not stepping.**
    /// The other player's feet were an `f32` from the snapshot on, so a player
    /// ten million blocks out was drawn a whole block at a time and a million
    /// out on a grid of sixteenths. The same snapshots, played back to the
    /// same moment, have to put the figure the same distance along -- and its
    /// mesh, measured from an origin a few blocks off, has to be the same mesh.
    #[test]
    fn a_player_far_from_zero_is_played_back_and_drawn_as_at_home() {
        let run = |base: glam::DVec3| {
            let mut players = RemotePlayers::default();
            for tick in 0..6u64 {
                let x = base.x + 0.3 + tick as f64 * 0.137;
                players.apply_snapshot(tick + 1, &[PlayerState { x, y: base.y + 30.0, z: base.z + 0.61, ..state(7, 0.0) }], None);
                players.tick(0.021);
            }
            let feet = players.iter().next().expect("the player").interpolated_pos - base;
            let origin = (base + glam::DVec3::new(2.0, 28.0, -1.0)).as_vec3();
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            build_actor_mesh_into(&players, None, origin, &LightMap::new(), &mut vertices, &mut indices);
            (feet, vertices.iter().map(|v| Vec3::from(v.position)).collect::<Vec<_>>())
        };
        let (home_feet, home) = run(glam::DVec3::ZERO);
        assert!(home_feet.x > 0.3, "the figure at home never moved: {home_feet}");
        for base in [glam::DVec3::new(1_000_000.0, 0.0, 1_000_000.0), glam::DVec3::new(-10_000_000.0, 0.0, -10_000_000.0)] {
            let (feet, far) = run(base);
            assert!((feet - home_feet).abs().max_element() < 1e-6, "played back to {feet} out at {base}, {home_feet} at home");
            assert_eq!(far.len(), home.len());
            for (i, (a, b)) in home.iter().zip(&far).enumerate() {
                assert!((*a - *b).abs().max_element() < 1e-5, "vertex {i} at {base}: {a} at home and {b} out there");
            }
        }
    }

    fn state(id: PlayerId, x: f32) -> PlayerState {
        PlayerState {
            id,
            x: f64::from(x),
            y: 30.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            outfit: Outfit::BARE,
            posture: primitive_shared::protocol::Posture::Standing,
            gesture: primitive_shared::protocol::Gesture::default(),
            limp: 0,
            limp_left: false,
        }
    }

    /// **A figure is lit by where it stands.** The actor shader used to light
    /// every player as though standing in the open, and added a quarter of
    /// full brightness besides, so somebody down a mine glowed like noon
    /// beside walls lit by nothing. Lit by the ground's `shade_lit` now, a
    /// figure needs the ground's light: every corner of one standing in a
    /// sealed pocket of rock says there is no sky, and every corner of one
    /// in the open says there is all of it.
    #[test]
    fn a_figure_is_lit_by_the_place_it_stands_in_and_not_by_open_sky() {
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, CHUNK_VOLUME};
        assert_eq!(OPEN_AIR, crate::engine::mesh::pack_light(15, 0, 3, 0));
        let pos = ChunkPos::new(0, 0);
        let mut blocks = vec![BLOCK_STONE; CHUNK_VOLUME];
        for x in 7..=9 {
            for y in 29..=32 {
                for z in 7..=9 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_AIR;
                }
            }
        }
        let mut chunks = crate::logic::chunk_manager::ChunkManager::new(4);
        chunks.insert(Chunk { pos, blocks });
        let mut sealed = LightMap::new();
        sealed.load_chunk(&chunks, pos);

        let mut players = RemotePlayers::default();
        players.apply_snapshot(1, &[PlayerState { x: 8.5, y: 29.0, z: 8.5, ..state(7, 0.0) }], None);
        for (light, sky) in [(&sealed, 0), (&LightMap::new(), 15)] {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            build_actor_mesh_into(&players, None, Vec3::ZERO, light, &mut vertices, &mut indices);
            assert!(!vertices.is_empty());
            for vertex in &vertices {
                assert_eq!(vertex.light & 15, sky, "a corner of the figure carries sky {}", vertex.light & 15);
            }
        }
    }

    #[test]
    fn everything_a_snapshot_says_about_somebody_reaches_the_pose_they_are_drawn_from() {
        // **The check nobody was doing.** `PlayerState` grew a field at a
        // time -- what they have on, what their hands are doing, whether they
        // are sitting, lying, dead or swimming, and now how badly they are
        // walking -- and every one of those was added because it had been
        // known to the server and invisible to everybody else. This is the
        // test that says the next one will not be: it sets each of them to
        // something that is not the default and asks the pose.
        let mut players = RemotePlayers::default();
        players.set_tick_rate(20.0);
        let mut worn = Outfit::BARE;
        worn.holding = primitive_shared::types::BLOCK_STONE;
        worn.worn[primitive_shared::equipment::Slot::Chest.index()] =
            primitive_shared::types::BLOCK_LEATHER_TUNIC;
        let said = PlayerState {
            yaw: 0.9,
            pitch: -0.4,
            on_ground: false,
            outfit: worn,
            posture: primitive_shared::protocol::Posture::Sitting,
            gesture: primitive_shared::protocol::Gesture {
                digging: true,
                ..primitive_shared::protocol::Gesture::default()
            },
            limp: 170,
            limp_left: true,
            ..state(7, 0.0)
        };
        players.apply_snapshot(1, &[said], None);
        // One frame, so the digging arm has begun to come up: it is eased
        // rather than snapped (see `dig_raise`).
        players.tick(0.1);
        let pose = players.players.get(&7).expect("the player").pose();
        assert_eq!(pose.outfit, worn, "what they have on never reached the model");
        assert_eq!(pose.posture, primitive_shared::protocol::Posture::Sitting, "they were drawn standing");
        assert!(pose.airborne, "a player off the ground was drawn with their feet down");
        assert!((pose.pitch - (-0.4)).abs() < 1e-6, "they were drawn looking somewhere else");
        assert!(
            matches!(pose.arm, Some(crate::logic::player_model::Arm::Chop { .. })),
            "somebody swinging at a block was drawn with their arm down"
        );
        assert!(
            (pose.limp - 170.0 / 255.0).abs() < 0.01,
            "the limp on the wire arrived as {:.2}",
            pose.limp
        );
        assert!(pose.limp_left, "a broken left leg arrived as a limp on the right");
        // ...and a sound player is a sound pose, so the field cannot be a
        // constant that happens to look right.
        players.apply_snapshot(2, &[PlayerState { limp: 0, ..state(7, 0.0) }], None);
        assert_eq!(players.players.get(&7).expect("the player").pose().limp, 0.0);
        // ...and the side follows the snapshot back to the right.
        players.apply_snapshot(3, &[PlayerState { limp: 170, limp_left: false, ..state(7, 0.0) }], None);
        assert!(!players.players.get(&7).expect("the player").pose().limp_left, "the left stuck");
    }

    /// **A remote player breaking a block is drawn with an arm that moves.**
    ///
    /// The report was "не видно как игрок ломает": the snapshot said nothing
    /// about a swing, so the figure beside a block coming apart stood still
    /// until the block vanished. A second of frames, stepped the way the frame
    /// loop steps them, against the same second of somebody standing there.
    #[test]
    fn a_remote_player_breaking_a_block_is_drawn_with_an_arm_that_moves() {
        use crate::logic::player_model::{joint_angle, Joint};
        let arm_travel = |digging: bool| {
            let mut players = RemotePlayers::default();
            let mut digger = state(7, 0.0);
            players.apply_snapshot(1, &[digger], None);
            digger.gesture.digging = digging;
            players.apply_snapshot(2, &[digger], None);
            let (mut low, mut high) = (f32::MAX, f32::MIN);
            for _ in 0..60 {
                players.tick(1.0 / 60.0);
                let figure = players.iter().next().expect("the digger is there");
                let angle = joint_angle(Joint::ArmRight, &figure.pose());
                low = low.min(angle);
                high = high.max(angle);
            }
            high - low
        };
        let digging = arm_travel(true);
        assert!(digging > 1.5, "a player digging moved their arm {digging} radians in a second");
        let standing = arm_travel(false);
        assert!(standing < 0.1, "a player standing still moved their arm {standing} radians");
    }

    /// A blow seen on somebody else starts when the snapshot's count moves on,
    /// is a thrust with a spear and a swing without, lasts as long as it does
    /// in the striker's own hand, and is not drawn for a count that was
    /// already standing when they came into view.
    #[test]
    fn a_blow_seen_on_another_player_is_their_weapons_own_length_and_starts_on_the_count() {
        use crate::logic::player_model::Arm;
        use primitive_shared::protocol::Action;
        use primitive_shared::types::BLOCK_FLINT_SPEAR;

        let mut players = RemotePlayers::default();
        let mut striker = state(3, 0.0);
        striker.outfit.holding = BLOCK_FLINT_SPEAR;
        striker.gesture.made(Action::Strike);
        players.apply_snapshot(1, &[striker], None);
        players.tick(0.0);
        let arm = |players: &RemotePlayers| players.iter().next().expect("the striker").pose().arm;
        assert_eq!(arm(&players), None, "a blow struck before they came into view was drawn");

        striker.gesture.made(Action::Strike);
        players.apply_snapshot(2, &[striker], None);
        assert!(matches!(arm(&players), Some(Arm::Thrust(_))), "a spear was not thrust: {:?}", arm(&players));
        let dt = 1.0 / 60.0;
        let mut seconds = 0.0;
        while arm(&players).is_some() {
            players.tick(dt);
            seconds += dt;
            assert!(seconds < 3.0, "a thrust never ended");
        }
        let expected = primitive_shared::combat::swing_seconds(Some(BLOCK_FLINT_SPEAR));
        assert!(
            (seconds - expected).abs() < 2.0 * dt,
            "a thrust lasted {seconds}s on another screen and {expected}s in the hand"
        );

        striker.outfit.holding = primitive_shared::types::BLOCK_AIR;
        striker.gesture.made(Action::Strike);
        players.apply_snapshot(3, &[striker], None);
        assert!(matches!(arm(&players), Some(Arm::Swing(_))), "a fist was not swung: {:?}", arm(&players));
    }

    #[test]
    fn your_own_state_is_ignored() {
        let mut players = RemotePlayers::default();
        players.apply_snapshot(1, &[state(1, 0.0), state(2, 5.0)], Some(1));
        assert_eq!(players.len(), 1, "should not render yourself as a remote box");
    }

    #[test]
    fn a_figure_is_drawn_between_the_two_snapshots_either_side_of_the_moment_shown() {
        // A walker a fifth of a block a tick, a frame a tick. Played back a
        // tick and a half late, the frame after the fourth snapshot shows the
        // moment half way between the third and the fourth -- neither of them.
        let mut players = RemotePlayers::default();
        players.set_tick_rate(20.0);
        for tick in 1..=4u64 {
            players.apply_snapshot(tick, &[state(2, 0.2 * tick as f32)], Some(1));
            players.tick(0.05);
        }
        let x = players.iter_positions().next().unwrap().x;
        assert!(x > 0.62 && x < 0.78, "expected a place between the third and fourth snapshots, got {x}");

        // ...and a respawn a field away is not slid across the field.
        players.apply_snapshot(5, &[state(2, 60.0)], Some(1));
        players.tick(0.05);
        assert_eq!(players.iter_positions().next().unwrap().x, 60.0, "a jump was eased across the world");

        // A snapshot that was overtaken changes nothing.
        players.apply_snapshot(3, &[state(2, -40.0)], Some(1));
        players.tick(0.05);
        assert_eq!(players.iter_positions().next().unwrap().x, 60.0, "an old snapshot was drawn");
    }

    /// What each frame drew of a player walking along x at a walking pace and
    /// turning, with snapshots at the server's twenty a second, applied the
    /// way the frame loop applies them -- the socket drained, then `tick` --
    /// and drawn through the frame's own rebuild rule.
    struct Walk {
        frames: u32,
        legs: u32,
        turns: u32,
        pictures: u32,
        steps: Vec<f32>,
    }

    fn walk_past(fps: f32, seconds: f32) -> Walk {
        use crate::logic::player_model::{joint_angle, Joint};
        const SNAPSHOT: f32 = 0.05;
        let frame = 1.0 / fps;
        let mut players = RemotePlayers::default();
        players.set_tick_rate(1.0 / SNAPSHOT);
        let (mut clock, mut next, mut tick) = (0.0f32, 0.0f32, 0u64);
        let gate = Instant::now();
        let mut last_rebuild: Option<Instant> = None;
        let mut last: Option<(glam::DVec3, f32, f32)> = None;
        let mut shown: Option<(glam::DVec3, f32, f32)> = None;
        let mut walk = Walk { frames: 0, legs: 0, turns: 0, pictures: 0, steps: Vec::new() };
        let mut index = 0u32;
        while clock < seconds {
            while next <= clock {
                tick += 1;
                let mut walker = state(7, WALKING * next);
                walker.yaw = next * 1.5;
                players.apply_snapshot(tick, &[walker], None);
                next += SNAPSHOT;
            }
            players.tick(frame);
            let figure = players.iter().next().expect("the walker");
            let pose = figure.pose();
            let here = (figure.interpolated_pos, joint_angle(Joint::LegRight, &pose), pose.yaw);
            // Half a second in, when the gait has come up to pace.
            if clock >= 0.5 {
                walk.frames += 1;
                if let Some(before) = last {
                    walk.legs += u32::from((here.1 - before.1).abs() > 1e-6);
                    walk.turns += u32::from((here.2 - before.2).abs() > 1e-6);
                    walk.steps.push((here.0.x - before.0.x) as f32);
                }
                let now = gate + Duration::from_secs_f32(frame * index as f32);
                let due = crate::dynamic_rebuild_due(false, false, last_rebuild, now);
                if due {
                    last_rebuild = Some(now);
                }
                if crate::figures_rebuild_due(due, !players.is_empty()) {
                    walk.pictures += u32::from(shown != Some(here));
                    shown = Some(here);
                }
            }
            last = Some(here);
            clock += frame;
            index += 1;
        }
        walk
    }

    /// **What a remote player's figure does frame by frame.**
    ///
    /// ```text
    /// cargo test -p primitive_client --lib \
    ///     what_a_walking_remote_player_looks_like_frame_by_frame -- --ignored --nocapture
    /// ```
    ///
    /// Before the playback, the same walk (paced against the wall clock,
    /// because the old gait measured its speed on it):
    ///
    /// ```text
    /// 75 fps:  legs move on 20/s, yaw on 20/s, pictures 75/s, step 0.0416/0.0573/0.0755 m, cv 0.19
    /// 144 fps: legs move on 20/s, yaw on 20/s, pictures 72/s, step 0.0212/0.0298/0.0391 m, cv 0.18
    /// ```
    #[test]
    #[ignore = "a measurement"]
    fn what_a_walking_remote_player_looks_like_frame_by_frame() {
        for fps in [75.0f32, 144.0] {
            let walk = walk_past(fps, 2.5);
            let seconds = walk.frames as f32 / fps;
            let mean = walk.steps.iter().sum::<f32>() / walk.steps.len() as f32;
            let (lo, hi) = walk.steps.iter().fold((f32::MAX, f32::MIN), |(a, b), &s| (a.min(s), b.max(s)));
            let cv = (walk.steps.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / walk.steps.len() as f32).sqrt() / mean;
            println!(
                "[remote] {fps} fps: legs move on {:.0}/s, yaw on {:.0}/s, pictures of the figure {:.0}/s, step min/mean/max {:.4}/{:.4}/{:.4} m, cv {:.3}",
                walk.legs as f32 / seconds,
                walk.turns as f32 / seconds,
                walk.pictures as f32 / seconds,
                lo,
                mean,
                hi,
                cv
            );
        }
    }

    #[test]
    fn a_walking_player_is_drawn_moving_on_every_frame_and_not_on_every_snapshot() {
        // **"модель игрока рендерится будто в 30 фпс".** With snapshots at
        // twenty a second the legs and the head moved on twenty frames a
        // second at any frame rate, the body surged on every snapshot (a
        // stride of 0.021 to 0.039 m a frame at 144), and at 144 the figure
        // was drawn anew on only 72 of the frames. See the measurement above.
        for fps in [75.0f32, 144.0] {
            let walk = walk_past(fps, 2.5);
            // Every frame counted is compared with the one before it, the
            // first of them included.
            let moved = walk.frames;
            assert_eq!(walk.legs, moved, "at {fps} fps the legs moved on {} of {moved} frames", walk.legs);
            assert_eq!(walk.turns, moved, "at {fps} fps the head turned on {} of {moved} frames", walk.turns);
            assert_eq!(walk.pictures, walk.frames, "at {fps} fps the figure was drawn on {} of {} frames", walk.pictures, walk.frames);
            let mean = walk.steps.iter().sum::<f32>() / walk.steps.len() as f32;
            let worst = walk.steps.iter().map(|s| (s - mean).abs() / mean).fold(0.0f32, f32::max);
            assert!(worst < 0.1, "at {fps} fps a frame's stride was {:.0}% off the walking pace", worst * 100.0);
        }
    }

    /// **What the other players cost a frame now that they are drawn on every
    /// one**: playing them back, and building their figures and what they hold.
    ///
    /// ```text
    /// cargo test --release -p primitive_client --lib \
    ///     what_drawing_other_players_costs_a_frame -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement: run in release"]
    fn what_drawing_other_players_costs_a_frame() {
        use primitive_shared::lighting::LightMap;
        use primitive_shared::types::BLOCK_STONE_PICKAXE;
        let light = LightMap::new();
        let layers = FaceLayers::empty_for_test();
        for count in [1u64, 4, 16] {
            let mut players = RemotePlayers::default();
            let states: Vec<PlayerState> = (0..count)
                .map(|i| {
                    let mut someone = state(i + 2, i as f32 * 2.0);
                    someone.outfit.holding = BLOCK_STONE_PICKAXE;
                    someone
                })
                .collect();
            players.apply_snapshot(1, &states, Some(1));
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            let (mut held, mut held_indices, mut items, mut item_indices) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            const ROUNDS: u32 = 4000;
            let started = Instant::now();
            for _ in 0..ROUNDS {
                players.tick(1.0 / 144.0);
                vertices.clear();
                indices.clear();
                held.clear();
                held_indices.clear();
                items.clear();
                item_indices.clear();
                build_actor_mesh_into(&players, None, Vec3::ZERO, &light, &mut vertices, &mut indices);
                build_held_items_into(&players, Vec3::ZERO, &layers, &light, None, &mut held, &mut held_indices, &mut items, &mut item_indices);
            }
            println!(
                "[remote] {count} player(s): {:.3} ms a frame to play back and build",
                started.elapsed().as_secs_f64() * 1e3 / ROUNDS as f64
            );
        }
    }

    #[test]
    fn a_player_who_walks_out_of_range_is_forgotten() {
        // No PlayerLeft arrives in this case -- they simply stop appearing
        // in snapshots -- so the timeout is the only thing stopping a
        // ghost hitbox from blocking the path forever.
        let mut players = RemotePlayers::default();
        players.apply_snapshot(1, &[state(2, 0.0)], Some(1));
        assert_eq!(players.len(), 1);

        if let Some(p) = players.players.get_mut(&2) {
            p.last_seen = Instant::now() - Duration::from_secs(10);
        }
        players.tick(0.016);
        assert_eq!(players.len(), 0);
    }

    /// Every player is closed geometry, and every player costs the same.
    ///
    /// **It used to say "one box each", and that stopped being true**
    /// when a player became six of them -- a head, a torso, two arms
    /// and two legs. What the test was actually holding down is worth
    /// keeping and is not the number one: that the mesh is closed
    /// (six faces of four corners, six indices a face, per part), and
    /// that two players cost exactly twice one. An open box shows the
    /// inside of a player from behind, and a per-player cost that is
    /// not flat is a field of players that gets slower for a reason
    /// nobody notices until it is a room full.
    #[test]
    fn every_player_is_closed_geometry_and_costs_the_same_as_the_last() {
        let mut players = RemotePlayers::default();
        players.apply_snapshot(1, &[state(2, 0.0)], Some(1));
        let (one_v, one_i) = build_actor_mesh(&players);

        let parts = crate::logic::player_model::PARTS.len();
        assert_eq!(one_v.len(), parts * 24, "6 faces x 4 corners per part");
        assert_eq!(one_i.len(), parts * 36, "two triangles a face");

        players.apply_snapshot(2, &[state(2, 0.0), state(3, 4.0)], Some(1));
        let (two_v, two_i) = build_actor_mesh(&players);
        assert_eq!(two_v.len(), 2 * one_v.len(), "the second player cost a different amount");
        assert_eq!(two_i.len(), 2 * one_i.len());
    }

    /// What the snapshot says somebody is wearing is what the model is
    /// asked to draw.
    ///
    /// The seam this file exists to keep honest: the network knows what
    /// a player has on, the model knows how to draw it, and the only
    /// thing joining them is `pose()`. A field added to the wire and not
    /// passed on is a change that compiles, runs, and does nothing.
    #[test]
    fn what_a_player_is_wearing_reaches_the_model_that_draws_them() {
        use primitive_shared::equipment::Slot;
        use primitive_shared::types::{BLOCK_AIR, BLOCK_LEATHER_BOOTS, BLOCK_STONE_PICKAXE};

        let mut dressed = state(2, 0.0);
        dressed.outfit.worn[Slot::Feet.index()] = BLOCK_LEATHER_BOOTS;
        dressed.outfit.holding = BLOCK_STONE_PICKAXE;

        let mut players = RemotePlayers::default();
        players.apply_snapshot(1, &[dressed], Some(1));
        let pose = players.iter().next().expect("the player").pose();
        assert_eq!(pose.outfit.worn_in(Slot::Feet), BLOCK_LEATHER_BOOTS);
        assert_eq!(pose.outfit.holding, BLOCK_STONE_PICKAXE);

        // ...and taking them off arrives too. A snapshot is the whole
        // truth about a player every tick, so an update that only ever
        // added would leave somebody visibly shod after they had put
        // their boots in a chest.
        players.apply_snapshot(2, &[state(2, 0.0)], Some(1));
        let pose = players.iter().next().expect("the player").pose();
        assert_eq!(pose.outfit.worn_in(Slot::Feet), BLOCK_AIR);
        assert!(pose.outfit.is_bare());
    }

    /// A dressed player costs more geometry than a bare one, and a
    /// carried thing is drawn.
    ///
    /// Deliberately weak about the *shape* -- that is
    /// `player_model`'s to state -- and specific about the seam: the
    /// mesh builders here have to hand the outfit on, and a builder
    /// that quietly dropped it would leave every test in `player_model`
    /// green and every player in the world naked.
    #[test]
    fn a_dressed_player_reaches_the_meshes_this_file_builds() {
        use primitive_shared::equipment::Slot;
        use primitive_shared::lighting::LightMap;
        use primitive_shared::types::{BLOCK_DIRT, BLOCK_IRON_CUIRASS};

        let mut players = RemotePlayers::default();
        players.apply_snapshot(1, &[state(2, 0.0)], Some(1));
        let (bare, _) = build_actor_mesh(&players);

        let mut dressed = state(2, 0.0);
        dressed.outfit.worn[Slot::Chest.index()] = BLOCK_IRON_CUIRASS;
        dressed.outfit.holding = BLOCK_DIRT;
        players.apply_snapshot(2, &[dressed], Some(1));
        let (worn, _) = build_actor_mesh(&players);
        assert!(worn.len() > bare.len(), "the cuirass was never drawn");

        // ...and the block in their hand goes down the world's own
        // pipeline rather than the actor one, which cannot reach the
        // block atlas at all.
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let (mut item_vertices, mut item_indices) = (Vec::new(), Vec::new());
        build_held_items_into(
            &players,
            Vec3::ZERO,
            &FaceLayers::empty_for_test(),
            &LightMap::new(),
            None,
            &mut vertices,
            &mut indices,
            &mut item_vertices,
            &mut item_indices,
        );
        assert_eq!(vertices.len(), 24, "the block in their hand was not drawn");
    }

    /// The posed rank stands in front of the camera and stays there.
    ///
    /// The hook is how every picture of a player model in this
    /// repository is taken, and a hook that quietly stops working takes
    /// the evidence with it. Two things are worth holding: the figures
    /// are in front of the camera rather than behind it, and `tick` --
    /// which drops anybody who has gone quiet -- does not take them.
    #[test]
    fn a_posed_rank_stands_in_front_of_the_camera_and_does_not_go_stale() {
        // The environment decides whether this runs at all, and a test
        // must not set one: the whole process shares it, and the other
        // tests in this binary run at the same time.
        let Some(count) = posed_players() else {
            return;
        };
        let mut players = RemotePlayers::default();
        let feet = Vec3::new(10.0, 64.0, -3.0);
        players.pose_for_a_photograph(feet, 0.0);
        assert_eq!(players.len() as u32, count);
        for p in players.iter() {
            assert!(
                p.interpolated_pos.x > f64::from(feet.x),
                "a yaw of zero looks along +X and the rank is at {}",
                p.interpolated_pos,
            );
        }
        players.tick(0.016);
        assert_eq!(players.len() as u32, count, "the rank went stale in one frame");
    }

    /// **Somebody who dies is seen lying down, once.** A dead player stood
    /// over their own body on every other screen. Now a death that left a
    /// body is drawn as that body alone -- the standing figure is gone, and
    /// nobody walks into it or aims at it -- and a death that left nothing
    /// is the figure itself fallen where it stood, lower than a standing
    /// one and as long as it was tall.
    #[test]
    fn a_dead_player_is_drawn_lying_down_once_and_not_standing() {
        use primitive_shared::protocol::Posture;
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_CORPSE, CHUNK_VOLUME};
        let height = |vertices: &[ActorVertex]| {
            let low = vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max) - low
        };
        let mut players = RemotePlayers::default();
        let dead = PlayerState { x: 8.5, y: 30.0, z: 8.5, posture: Posture::Fallen, ..state(7, 0.0) };
        players.apply_snapshot(1, &[dead], None);
        assert_eq!(players.iter_positions().count(), 0, "a body is still a pillar to walk into");
        assert!(players.aimed_at((Vec3::new(4.0, 31.0, 8.5)).as_dvec3(), Vec3::X, 10.0).is_none(), "a body can be struck");

        // Nothing left behind: the figure falls where it stood.
        let mut chunks = crate::logic::chunk_manager::ChunkManager::new(4);
        chunks.insert(Chunk { pos: ChunkPos::new(0, 0), blocks: vec![BLOCK_AIR; CHUNK_VOLUME] });
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        build_actor_mesh_into(&players, Some(&chunks), Vec3::ZERO, &LightMap::new(), &mut vertices, &mut indices);
        assert!(!vertices.is_empty(), "a player who died with nothing vanished");
        assert!(height(&vertices) < 0.8, "the dead player stands {} tall", height(&vertices));

        // A body in their column a cell below the feet: that is them.
        chunks.apply_block_update(8, 29, 8, BLOCK_CORPSE);
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        build_actor_mesh_into(&players, Some(&chunks), Vec3::ZERO, &LightMap::new(), &mut vertices, &mut indices);
        assert_eq!(vertices.len(), crate::logic::player_model::PARTS.len() * 24, "not exactly one body is drawn");
        assert!(height(&vertices) < 0.8);
    }

    /// **The local player's hand hears about the mouthful the server let
    /// them take**, once, and not about one made before the session began.
    #[test]
    fn the_local_players_own_confirmed_gesture_is_handed_to_their_hand_once() {
        use primitive_shared::protocol::Action;
        let mut players = RemotePlayers::default();
        let mut me = state(3, 0.0);
        me.gesture.made(Action::Strike);
        players.apply_snapshot(1, &[me], Some(3));
        assert_eq!(players.take_own_gesture(), None, "a gesture from before the session was replayed");
        me.gesture.made(Action::Eat);
        players.apply_snapshot(2, &[me], Some(3));
        players.apply_snapshot(3, &[me], Some(3));
        assert_eq!(players.take_own_gesture(), Some(Action::Eat));
        assert_eq!(players.take_own_gesture(), None);
        assert!(players.is_empty(), "the local player was drawn as somebody else");
    }

    #[test]
    fn a_rider_is_drawn_on_the_saddle_of_the_horse_as_it_is_drawn_and_nobody_else_is() {
        use primitive_shared::horse::RIDER_LIFT;
        let mut players = RemotePlayers::default();
        players.set_tick_rate(20.0);
        let lift = f64::from(RIDER_LIFT);
        // Their snapshot has them a little behind the horse under them --
        // the two are eased at different rates -- and a second horse with a
        // rider on it stands a block and a half away.
        let rider = PlayerState { posture: primitive_shared::protocol::Posture::Mounted, y: 30.0 + lift, ..state(1, 10.3) };
        let walker = state(2, 10.3);
        players.apply_snapshot(1, &[rider, walker], None);
        players.tick(0.05);
        let under = glam::DVec3::new(10.0, 30.0, 0.0);
        let neighbour = glam::DVec3::new(10.0, 30.0, 1.5);
        players.mount(&[(neighbour, 2.0), (under, 0.7)]);

        let drawn = &players.players[&1];
        assert!(
            (drawn.interpolated_pos - (under + glam::DVec3::Y * lift)).length() < 1e-9,
            "the rider was drawn at {:?}, not on the saddle of the horse under them",
            drawn.interpolated_pos
        );
        assert!((drawn.yaw - 0.7).abs() < 1e-6, "the rider faces {:.2} on a horse facing 0.70", drawn.yaw);
        // Somebody standing beside a ridden horse is not put on it.
        let beside = &players.players[&2];
        assert!(beside.interpolated_pos.y < 30.5, "a player on their feet was lifted onto a horse");
    }
}
