//! Entities: things in the world that aren't blocks and aren't players.
//!
//! Right now that means falling blocks. The server owns them; the client
//! receives a snapshot per tick and interpolates between snapshots, the
//! same arrangement remote players use — and for the same reason, since
//! snapshots arrive at the tick rate rather than once per frame.
//!
//! ## Motion
//!
//! Snapshots arrive at the server's tick rate (20 Hz); frames happen far
//! more often. The drawn position is therefore interpolated **between
//! the last two snapshots**, using the measured interval between them.
//!
//! The obvious-looking alternative -- easing the drawn position toward
//! the newest one at some fixed rate -- is what this did first, and it
//! is wrong for anything that moves steadily. An exponential chase
//! settles at a constant lag of `speed / rate`: sand at terminal
//! velocity (18 blocks/s) chased at 18 per second trails a **whole
//! block** behind where the server says it is, all the way down, and
//! then jumps that block when it lands. Interpolating between two known
//! samples has no such lag; it is one snapshot behind (50 ms), which is
//! not visible.
//!
//! ## Despawning
//!
//! The protocol has no despawn message, so there are three ways an
//! entity goes away, in order of how promptly they fire:
//!
//! * **It lands.** The block update that puts it back into the world
//!   also removes the entity, in the same frame. This is the one that
//!   matters: without it a landed block was drawn hovering above the
//!   real one it had just become, for as long as the timeout ran.
//! * **It is missing from a snapshot.** Each snapshot is the complete
//!   set of entities near the player, so anything absent from one is
//!   gone -- no waiting.
//! * **Nothing is heard for a while.** Only reachable when the *last*
//!   nearby entity disappears, since the server sends no message at all
//!   in that case. A backstop, not the normal path.
//!
//! Rendering reuses the chunk pipeline: a falling block is emitted as a
//! textured cube in world space with the same vertex format as terrain,
//! so it picks up the same lighting, fog and textures with no second
//! pipeline to keep in sync.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::{DVec3, Vec3};

use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::{EntityId, EntityKind, EntityState};
use primitive_shared::types::{BlockId, BLOCK_AIR, MAX_LIGHT};

use crate::engine::item_model::ItemVertex;
use crate::engine::mesh::{face_uv, faces, pack_light, Vertex};
use crate::engine::texture::FaceLayers;
use crate::logic::riding::{body_of, Glide, RaftPose, Steering};
use primitive_shared::raft::{Body, Oars, Wind};

/// How long an entity survives with no snapshot mentioning it at all.
///
/// **A backstop for silence, not the way things are removed.** Removal
/// is a snapshot that omits the entity -- the server sends the complete
/// set of what is nearby, including an empty one the moment the last
/// thing leaves, so a picked-up item disappears on the next tick rather
/// than when this expires.
///
/// It used to be 150 milliseconds, which is three ticks, and it was
/// doing both jobs: with removal resting on it, it had to be short. That
/// made it fragile in exactly the case it mattered -- a player streaming
/// chunks shares one outgoing queue with their entity updates, so three
/// dropped messages in a row made a dropped block *blink out of
/// existence* and back. "You cannot always see the block you dropped"
/// is what that looks like from the outside.
///
/// Two seconds now, because the only thing left for it to catch is a
/// connection that has stopped saying anything at all.
const STALE_AFTER: Duration = Duration::from_secs(2);
/// Fallback when two snapshots arrive suspiciously close together or
/// impossibly far apart; roughly one server tick.
const NOMINAL_INTERVAL: Duration = Duration::from_millis(50);

/// How far a glide's length may be pulled either side of the server's own
/// interval to keep the drawn motion on the server's rhythm. See
/// `playout_span`.
///
/// **This is the price of not twitching, and it is small.** Twenty
/// milliseconds against a fifty-millisecond tick is a body drawn crossing
/// its ground up to two fifths fast or slow for one tick when the network
/// wobbles -- against the old behaviour, which was the right speed and then
/// a dead stop for whatever the packet was late by. It covers the two things
/// that actually jitter: a server thread sleeping on a fifteen-millisecond
/// Windows timer, and a client that reads its socket once a frame.
///
/// Rejected: extrapolating past the newest snapshot rather than fitting the
/// glide to the time it has. It costs nothing in lag and it guesses; a
/// guessed position is a deer that slides through the tree it actually
/// stopped at, and this module already refuses to extrapolate for the
/// falling block that would go through its own landing site.
const PLAYOUT_SLACK: Duration = Duration::from_millis(20);

/// How long the glide a snapshot starts is given to cross, where the
/// snapshot arrived `now`, the one before it was due at `previous`, and the
/// server took `interval` between them.
///
/// **The server's rhythm, not the network's, and never a stop.** The glide
/// ends where the rhythm says this snapshot is due -- one interval after the
/// one before it -- so a packet that came late leaves its glide a little
/// less time and one that came early a little more. The body always has
/// somewhere to go, which is the whole point: the old scheme gave every
/// glide exactly one interval from whenever the packet landed, so a late
/// packet was a body that stood still until it came.
///
/// Clamped to `PLAYOUT_SLACK` either side of the interval, which is what
/// keeps the schedule honest: without it, a run of early packets walks the
/// glide further and further ahead of the arrivals and the animal is drawn
/// crawling a third of a second behind where it is.
///
/// A free function because it is the whole of the mechanism, and a test can
/// hand it a sequence of arrival times without a network. See
/// `a_snapshot_that_arrives_early_or_late_still_leaves_the_body_somewhere_to_go`.
fn playout_span(previous: Option<Instant>, now: Instant, rhythm: Duration) -> Duration {
    let low = rhythm.saturating_sub(PLAYOUT_SLACK).max(Duration::from_millis(1));
    let high = rhythm + PLAYOUT_SLACK;
    match previous {
        Some(previous) => (previous + rhythm).saturating_duration_since(now).clamp(low, high),
        None => rhythm,
    }
}

/// How fast a body comes over into its bank and back out of it, in radians
/// a second of turn per second. See `Entity::banked`.
///
/// Ten: a deer reaches a full turn's worth of lean in a third of a second,
/// which is about how long a running animal takes to get its weight over.
const BANK_RATE: f32 = 10.0;

/// The furthest anything may move in one snapshot and still count as
/// having *walked* there.
///
/// Three blocks in a twentieth of a second is sixty blocks a second --
/// five times the fastest thing in the game. Past that it is a
/// teleport, an entity re-entering the interest radius, or a reused id,
/// and none of those are strides. See `apply_snapshot`.
const MAX_STRIDE: f32 = 3.0;

/// ...and the same bound going up or down, in blocks a second: the fastest
/// anything in this world climbs or plunges (`animals::FLIGHT_SPEED` and a
/// gull's `DIVE_SPEED`) with room over it. See `Entity::rising`.
const MAX_RISE: f32 = 16.0;

struct Entity {
    kind: EntityKind,
    /// Where the previous snapshot put it, and where the latest one did.
    /// Where it was and where the server now has it, in `f64` -- the wire's
    /// type (`PROTOCOL_VERSION`'s fifty-seven). An `f32` here put every animal
    /// a million blocks out on a grid of sixteenths, and the glide between two
    /// snapshots stepped across it: a deer walking past shivered.
    previous: DVec3,
    current: DVec3,
    /// When `current` arrived, and how long it took to arrive after
    /// `previous`. Measured rather than assumed, so a server running at
    /// a different tick rate interpolates correctly.
    updated_at: Instant,
    interval: Duration,
    /// When the glide from `previous` to `current` *starts on screen* --
    /// which is not when the packet arrived.
    ///
    /// **This is the whole of the twitch.** The glide used to be given
    /// exactly one server tick to cross, whenever the packet happened to
    /// land, so a packet that arrived five milliseconds late left the animal
    /// standing on its target for five milliseconds -- a whole frame at
    /// sixty -- and then covering the next tick's ground in what was left of
    /// it. Nothing is wrong with the arithmetic and everything is wrong with
    /// the clock it is on: snapshots are *made* on a rhythm and *arrive*
    /// with the jitter of a server thread sleeping on a fifteen-millisecond
    /// timer and a client that reads its socket once a frame. Measured on a
    /// scripted deer: a stalled frame in eight, on every animal at once,
    /// which is exactly "они дёргаются".
    ///
    /// So a glide is given the time it actually has: it starts where the
    /// body is being drawn and ends when `Entities::due` -- a playout clock
    /// kept on the server's own rhythm -- says this snapshot is due. A
    /// packet that ran late leaves its glide a little less time and the body
    /// crosses a little faster; one that came early, a little more. What
    /// varies is a fraction of the speed instead of the whole of it, and the
    /// body never stops.
    ///
    /// Rejected: a deeper buffer that plays whole segments a snapshot behind.
    /// It is the textbook answer and it keeps the drawn speed exact, but it
    /// needs three samples per entity and a promotion step, and it pays
    /// another tick of lag on where a deer is when a spear is thrown at it.
    plays_from: Instant,
    /// How long this glide has to cross, which is not `interval`: see
    /// `plays_from`. The interval is what the *server* took, and the gait
    /// and the turn rate are measured against that.
    span: Duration,
    /// Whether the server has it lying still, how far over it was drawn
    /// when that last changed, and when. See `tipped_over`.
    lying: bool,
    tip_from: f32,
    tip_since: Instant,
    /// How far this thing had walked along the ground when `previous`
    /// was taken, and how much further `current` is.
    ///
    /// The legs are swung from *distance covered* rather than from the
    /// clock, which is what makes a standing animal stand still and a
    /// bolting hare take its strides faster -- and it costs nothing on
    /// the wire, because the positions the swing is measured from were
    /// already being sent.
    walked: f32,
    step: f32,
    /// Which way it was facing when `previous` was taken.
    ///
    /// **The facing is interpolated for the same reason the position
    /// is.** A snapshot lands five times a second and the server turns
    /// an animal at about two hundred degrees a second, so between two
    /// snapshots a fleeing deer can swing forty degrees -- and drawing
    /// the number as it arrives makes that one instant jump per
    /// snapshot. The body slides smoothly to its new place while facing
    /// a direction that ratchets, which reads as the animal skidding.
    ///
    /// Only animals have one; everything else is a cube or a sprite with
    /// no opinion about which way it is round.
    previous_yaw: f32,
    /// How far this animal's head is carried off level right now, in radians:
    /// negative is down. See `animal_model::head_carried`.
    ///
    /// **The one thing about an animal this client remembers between two
    /// frames, and it exists because a head cannot teleport.** The attitude
    /// arrives on a snapshot and changes in a single step -- level one
    /// snapshot, in the grass the next -- and the mesh is rebuilt from
    /// nothing every frame, so without somewhere to keep a partly-lowered
    /// head there is nowhere for the movement to happen. Eased here at
    /// `animal_model::HEAD_RATE`, once per entity per frame, which is one
    /// subtract and one clamp for each animal on screen.
    head: f32,
    /// How hard the body is banked into its turn right now, in radians a
    /// second of turn -- the number `Motion::turning` is handed.
    ///
    /// **Eased, for the reason the head is.** What `turning` measures is a
    /// step function: it is one number for a whole snapshot and another the
    /// instant the next one lands, so a deer that began a turn snapped from
    /// upright to its full bank between two frames and snapped back when it
    /// stopped. A body has mass; it takes a moment to come over and a moment
    /// to come back, and `BANK_RATE` is that moment.
    banked: f32,
    /// Which way the last blow threw it: `+1` to its right, `-1` to its
    /// left. See `animal_model::STAGGER_ROLL`.
    ///
    /// **Rolled per blow, because the flinch used to be one-sided.** The
    /// wire does not say which side a hit came from, so the body always
    /// rolled the same way -- every animal in the world tipping right, every
    /// time anything touched it. A side drawn on the rising edge of `hurt`
    /// from the animal's own id and how many blows it has taken costs
    /// nothing and is never the same twice running.
    flinch_side: f32,
    /// How many blows this client has seen land, for `flinch_side`.
    blows: u32,
    /// Seconds this entity has been on screen: the clock the idle motions
    /// run on. See `animal_model::Motion::age`.
    age: f32,
    /// Where a bird is in its wingbeat, in whole beats: see
    /// `animal_model::Motion::beat`.
    ///
    /// **Kept here, like `head` and for the same reason**, and it is the
    /// only way a beat can answer to the effort of the flight at all: the
    /// rate has to change with the climb (`animal_model::CLIMB_BEATS`), and
    /// a rate multiplied into the argument of a sine snaps the wing to a
    /// different part of the beat the instant it changes. A phase that is
    /// integrated, a frame at a time, changes rate without moving. Nought
    /// and untouched for everything that does not fly.
    beat: f32,
    /// Seconds since the snapshot first said it was dying
    /// (`Attitude::Dying`), for the roll onto its side: see
    /// `animal_model::Motion::fallen`. Nought for everything alive.
    ///
    /// **Counted here, not sent**, for the reason `head` is eased here: the
    /// server says *that* it is going down, once, and how far through the
    /// fall it is follows from when this client heard -- a byte of progress a
    /// tick for a second of animation would be twenty snapshots saying what
    /// one does.
    dying: f32,
}

impl Entity {
    /// How far along the interval between the last two snapshots we
    /// are, 0 to 1.
    ///
    /// Clamped, not extrapolated: overshooting on a late snapshot would
    /// push a block through the floor it is about to land on.
    /// Which way it is facing right now, eased between the last two
    /// snapshots.
    ///
    /// **The short way round.** An animal that turns from 350 degrees to
    /// 10 has turned twenty degrees, and interpolating the numbers
    /// directly spins it the other three hundred and forty -- which is
    /// the one failure mode this is guaranteed to hit, because it
    /// happens every time something crosses due east.
    fn drawn_yaw(&self, now: Instant) -> f32 {
        use std::f32::consts::{PI, TAU};
        // A raft turns as an animal does, and is eased the same way.
        let (EntityKind::Animal { yaw, .. } | EntityKind::Raft { yaw, .. }) = self.kind else {
            return 0.0;
        };
        let mut delta = (yaw - self.previous_yaw).rem_euclid(TAU);
        if delta > PI {
            delta -= TAU;
        }
        self.previous_yaw + delta * self.progress(now)
    }

    /// How far along the glide we are, 0 to 1.
    ///
    /// Against `span`, the time this glide was given, and not against the
    /// server's interval: see `plays_from`.
    fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.plays_from).as_secs_f32();
        let span = self.span.as_secs_f32().max(1e-4);
        (elapsed / span).clamp(0.0, 1.0)
    }

    /// Position to draw at, `now`.
    fn drawn(&self, now: Instant) -> DVec3 {
        self.previous.lerp(self.current, f64::from(self.progress(now)))
    }

    /// How far it has walked and how fast it is going -- the two
    /// numbers a gait needs.
    ///
    /// The distance runs on through the interval rather than stepping
    /// once a snapshot: at 20 snapshots a second, legs that only moved
    /// when one arrived would strobe.
    fn gait(&self, now: Instant) -> (f32, f32) {
        let interval = self.interval.as_secs_f32().max(1e-4);
        (
            self.walked + self.step * self.progress(now),
            self.step / interval,
        )
    }

    /// How fast it is rising, in blocks a second: negative is coming down.
    ///
    /// **From the two heights the snapshots already carry**, for the reason
    /// `turning` is measured here and not sent. Clamped at what nothing in
    /// this world can climb or dive at, so the one interval in which an
    /// entity is teleported -- or reuses an id, or re-enters the interest
    /// radius -- does not hand the wings a rise of two hundred blocks a
    /// second. That is `MAX_STRIDE`'s argument in the other axis.
    fn rising(&self) -> f32 {
        let interval = self.interval.as_secs_f32().max(1e-4);
        (((self.current.y - self.previous.y) as f32) / interval).clamp(-MAX_RISE, MAX_RISE)
    }

    /// How fast it is turning, in radians a second, **positive to its
    /// right**.
    ///
    /// Right, because that is what a growing yaw is everywhere else in this
    /// game: `Camera::right_horizontal` is `forward × Y`, which at a yaw of
    /// nought is `+Z`, and the heading `(cos yaw, sin yaw)` swings toward
    /// `+Z` as the yaw grows. This doc-comment said "left" and the lean was
    /// built on it, so every animal banked *out* of every turn -- the
    /// player's "наклоняются в одну сторону". See `animal_model`'s `lean`.
    ///
    /// **Measured from the two facings the client is already easing between**,
    /// not sent: the server turns an animal at a rate the snapshots describe
    /// exactly, and asking for a third number to say what two already say is
    /// four bytes an animal a tick for nothing. Same short way round as
    /// `drawn_yaw`, for the same reason -- a deer crossing due east turns a
    /// few degrees and not three hundred and fifty.
    fn turning(&self) -> f32 {
        use std::f32::consts::{PI, TAU};
        let (EntityKind::Animal { yaw, .. } | EntityKind::Raft { yaw, .. }) = self.kind else {
            return 0.0;
        };
        let mut delta = (yaw - self.previous_yaw).rem_euclid(TAU);
        if delta > PI {
            delta -= TAU;
        }
        delta / self.interval.as_secs_f32().max(1e-4)
    }

    /// Where its head wants to be, from what the last snapshot said it was
    /// doing. See `head`.
    fn head_wanted(&self) -> f32 {
        match self.kind {
            EntityKind::Animal { attitude, .. } => {
                crate::logic::animal_model::head_carried(attitude)
            }
            _ => 0.0,
        }
    }

    /// What the model needs to know about how this animal is moving.
    fn motion(&self, now: Instant) -> crate::logic::animal_model::Motion {
        let (walked, speed) = self.gait(now);
        let (hurt, growth, tack) = match self.kind {
            EntityKind::Animal { hurt, growth, tack, .. } => ((hurt > 0.0).then_some(hurt), growth, tack),
            _ => (None, u8::MAX, 0),
        };
        crate::logic::animal_model::Motion {
            walked,
            speed,
            rise: self.rising(),
            beat: self.beat,
            hurt,
            head: self.head,
            // The eased bank, not the raw measurement: see `banked`.
            turning: self.banked,
            flinch_side: self.flinch_side,
            age: self.age,
            youth: 1.0 - primitive_shared::youth::from_wire(growth),
            fallen: (self.dying / crate::logic::animal_model::FALL_SECONDS).clamp(0.0, 1.0),
            // The saddle, the bags and the halter are drawn from this byte and
            // nothing else: the client keeps no idea of its own about whose
            // horse is saddled.
            tack,
        }
    }
}

#[derive(Default)]
pub struct Entities {
    entities: HashMap<EntityId, Entity>,
    /// The frame's clock, sampled once in `tick`, so every entity in one
    /// frame is drawn at the same instant.
    now: Option<Instant>,
    last_snapshot: Option<Instant>,
    /// The server tick the last snapshot was built on.
    ///
    /// What the interval is measured in now -- see `apply_snapshot` for
    /// why the wall clock is the wrong ruler for it.
    last_tick: Option<u64>,
    /// How long one server tick is, out of the handshake.
    ///
    /// Kept here rather than threaded through the message handler,
    /// which already takes twenty-odd arguments: it is a fact about the
    /// session that only this module uses, and the session is what sets
    /// it. `None` until a server says, which is what the fallback in
    /// `tick_duration` is for.
    tick_duration: Option<Duration>,
    /// How long a server tick's worth of snapshot actually takes to arrive
    /// here, smoothed: the rhythm the glides are cut to.
    ///
    /// **Not the tick duration, and the difference is the twitch coming
    /// back.** The handshake says what the server *aims* at; what a client
    /// sees is that rate through a sleeping server thread, a socket read
    /// once a frame and two clocks that do not agree to the millisecond. A
    /// glide cut to the nominal fifty milliseconds while the packets really
    /// come every fifty-five finishes early every time and the body stands
    /// still for the difference -- which is the stall this module was
    /// rewritten to be rid of, arriving by a slower road. Smoothed hard
    /// (a seventh of each measurement) so a single late packet moves it
    /// almost not at all.
    rhythm: Option<Duration>,
    /// When the newest snapshot's state is due on screen: the playout
    /// clock.
    ///
    /// **A snapshot arrives when the network feels like it and is drawn on
    /// the server's rhythm.** Each snapshot is scheduled one interval after
    /// the one before, so the glides are laid end to end with no seam and no
    /// stall, however the packets themselves were bunched. See
    /// `Entity::plays_from` for what that fixed, and `PLAYOUT_SLACK` for how
    /// far behind the arrivals the schedule is kept.
    due: Option<Instant>,
    /// When this world started, so the item bob and spin have a clock
    /// that does not restart every frame.
    started: Option<Instant>,
    /// Where an animal was struck since the last frame looked, in world
    /// coordinates.
    ///
    /// **The server's own account of it, not the client's guess.** The
    /// client already knows when it *asked* to hit something -- see
    /// `hand::Strikes` and `send_blow` -- and that is a different fact: the swing may
    /// be refused, it may kill nothing, and it says nothing at all
    /// about the wolf that is eating a deer on the other side of the
    /// clearing. The snapshot's `hurt` going up is the server saying a
    /// blow landed, whoever threw it, so that is what the blood is
    /// drawn from.
    ///
    /// Drained by the frame, which is why it is a list and not a flag:
    /// one snapshot can carry several animals being hit at once.
    blows: Vec<Vec3>,
    /// Where something has just let go since this was last asked, and
    /// what it was: a roof dug out from under, a column of sand peeling
    /// away, a board that burned through.
    ///
    /// **A falling block first seen is a block that has just started
    /// falling.** A fall lasts well under a second (`logic::falling` on
    /// the server turns the block into an entity and lands it again), so
    /// an entity this table has not seen before is the moment it gave
    /// way -- which is the moment there is a sound for and the moment the
    /// player wants a warning. The one exception is walking into view of
    /// a fall already in progress, and that is a crumble played where
    /// something really is coming down.
    ///
    /// Drained like `blows`, for the same reason: several columns can let
    /// go in one snapshot.
    gave_way: Vec<(DVec3, BlockId)>,
    /// The raft this client is rowing, ahead of the server. See
    /// `riding::Steering`.
    ///
    /// **Here, beside the entities, rather than in the frame loop**, because
    /// the raft it predicts *is* one of them: it is drawn from here, stood on
    /// from here, and corrected from the snapshots this table applies. The
    /// server's `Oars` message sets it, and that handler already has this
    /// table and nothing else of the frame's.
    steering: Option<Steering>,
    /// Every other raft in sight, drawn ahead of its snapshots rather than
    /// behind them. See `riding::Glide`.
    glides: HashMap<EntityId, Glide>,
    /// The wind this frame, for the sails. `None` until the frame says.
    wind: Option<Wind>,
    /// The sail angle this client's own hand is asking for, and for which
    /// raft: `riding::Riding::held_trim`, handed in every frame.
    ///
    /// **Applied where a raft is drawn rather than waited for**, and that is
    /// the same argument the rower's prediction makes one field up. A yard
    /// that only moved when the server's snapshot came round would lag the
    /// mouse by a round trip, and a control you drag and watch arrive late
    /// is a control that feels broken however right it is.
    trim: Option<(EntityId, f32)>,
    /// The current of the river under the raft this client is rowing, in
    /// blocks a second: `WorldGen::river_current` at the raft, handed in
    /// every frame by whoever holds the generator (`set_river_current`).
    ///
    /// **Handed in, not looked up**, because the entity list has no world:
    /// the frame does, and the prediction only needs the one number at the
    /// one raft it steps. A frame late is a raft's length in a minute.
    river: (f32, f32),
    /// The horse this client is riding, where its prediction has it: the id,
    /// its feet and its yaw (`horseback::Horseback`), handed in every frame.
    ///
    /// **Drawn from here rather than from its snapshots**, for the rower's
    /// raft's reason: the rider's eye is on the predicted saddle, and a horse
    /// drawn a round trip behind it would be a horse sliding out from under
    /// the camera at every gallop.
    ridden: Option<(EntityId, DVec3, f32)>,
    /// The horse this client rides, ahead of the server (`horseback`).
    ///
    /// **Here for the rower's reason** (`steering` above): the handler that
    /// hears `ServerMessage::Mounted` and the snapshots that correct it has
    /// this table and nothing else of the frame's. Public, because the frame
    /// is what steps it with the keys.
    pub horseback: Option<crate::logic::horseback::Horseback>,
}

impl Entities {
    /// Seconds since the first frame of this session.
    fn age(&self, now: Instant) -> f32 {
        match self.started {
            Some(started) => now.duration_since(started).as_secs_f32(),
            None => 0.0,
        }
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Which animal a look ray meets first, and how far off it is.
    ///
    /// The same shape `RemotePlayers::aimed_at` has, and deliberately:
    /// the two answer the same question about two kinds of target, and
    /// the caller picks whichever is nearer. What is tested is the box
    /// the animal is *drawn* as -- `extents` measures it off the model
    /// itself -- so what you can hit is what you can see.
    ///
    /// **In the animal's frame, not the world's.** A boar is twice as
    /// long as it is wide; a world-aligned box big enough to hold it
    /// end to end is also that wide, so half of what a swing connected
    /// with was the air beside the animal. Turning the ray by the
    /// animal's yaw first costs four multiplies and makes the box the
    /// silhouette.
    pub fn aimed_at(&self, eye: DVec3, dir: Vec3, range: f32) -> Option<(EntityId, f32)> {
        self.entities
            .iter()
            .filter_map(|(&id, entity)| {
                let (species, yaw, growth) = match entity.kind {
                    // A body going down is not a thing to swing at: it is
                    // already dead, and the server no longer has it among
                    // the living to be struck (`Animals::falling`).
                    EntityKind::Animal { attitude: primitive_shared::protocol::Attitude::Dying, .. } => return None,
                    EntityKind::Animal { species, yaw, growth, .. } => (species, yaw, growth),
                    // A raft is struck the same way an animal is, and the
                    // server sorts the two apart by the id (`rafts::strike`).
                    EntityKind::Raft { .. } => {
                        return self.raft_hit(id, entity, eye, dir, range, false).map(|distance| (id, distance));
                    }
                    _ => return None,
                };
                let centre = entity.drawn(self.now.unwrap_or_else(Instant::now));
                let (sin, cos) = (-yaw).sin_cos();
                let turn = |v: Vec3| Vec3::new(v.x * cos - v.z * sin, v.y, v.x * sin + v.z * cos);
                // **The shared box, not the model's own.** Both are
                // measured from the same animal, but only one of them
                // the server can see: it validates a blow against
                // `Species::half_extents`, so aiming at anything else
                // means swings that land on screen and miss on the wire.
                // A test in `animal_model` keeps the drawn model inside
                // this box, which is what makes it honest to aim with.
                // **The axes are turned, and that was the bug.** After
                // `turn` the ray is in the animal's own frame at yaw
                // zero, where the *front* is +X and the animal's side is
                // +Z (see `animal_model::append_part`, which maps the
                // model's -Z front onto +X). The box was being passed
                // straight from the model, whose own x is across and
                // whose z is along -- so a deer was tested as though it
                // were 0.6 long and 1.7 wide, and a swing at its flank
                // passed through it.
                let (across, up, along) = species.half_extents();
                // ...at its size, which is what the server checks a blow
                // against: a fawn is a fawn's box (`youth::size`).
                let size = primitive_shared::youth::size(primitive_shared::youth::from_wire(growth));
                let (across, up, along) = (across * size, up * size, along * size);
                ray_hits_box(
                    turn((eye - centre).as_vec3()),
                    turn(dir),
                    Vec3::ZERO,
                    Vec3::new(along, up, across),
                    range,
                )
                .map(|distance| (id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    // ---- rafts ----

    /// Where a raft is this frame: the rower's prediction for the raft this
    /// client is rowing, the raft carried on from its snapshots for any other
    /// (`riding::Glide`), and the snapshots eased only for a raft no frame has
    /// carried on yet.
    fn raft_pose(&self, id: EntityId, entity: &Entity, now: Instant) -> Option<Body> {
        let mut body = if let Some(steering) = self.steering.filter(|steering| steering.id == id) {
            steering.body
        } else if let Some(glide) = self.glides.get(&id) {
            glide.body
        } else {
            let mut body = body_of(entity.kind, entity.drawn(now))?;
            body.yaw = entity.drawn_yaw(now);
            body
        };
        // ...and this client's own hand on the sheets, over whatever the
        // server last said. See `trim`. One place rather than three, because
        // the sail's angle changes nothing about where the deck is: the
        // prediction, the glide and a raft nobody has carried on yet all
        // want the same override and none of them wants a different one.
        if let Some((_, angle)) = self.trim.filter(|(trimmed, _)| *trimmed == id) {
            body.sail_angle = angle;
        }
        Some(body)
    }

    /// What this client's own hand is asking the sail to do, if anything.
    ///
    /// Handed in every frame rather than remembered here, because what
    /// decides it is where the player's feet are, which is the frame's
    /// business and not the entity list's.
    pub fn set_trim(&mut self, trim: Option<(EntityId, f32)>) {
        self.trim = trim;
    }

    /// The river's current under the rowed raft, for this frame's
    /// prediction. See `river`.
    pub fn set_river_current(&mut self, river: (f32, f32)) {
        self.river = river;
    }

    /// The horse this client rides, where the prediction has it this frame:
    /// see `ridden`.
    pub fn set_ridden(&mut self, ridden: Option<(EntityId, DVec3, f32)>) {
        self.ridden = ridden;
    }

    /// What kind of animal an entity is, if it is one this table has seen.
    /// What a right click asks before it is a leg up or a feed.
    pub fn species_of(&self, id: EntityId) -> Option<primitive_shared::animals::Species> {
        match self.entities.get(&id)?.kind {
            EntityKind::Animal { species, .. } => Some(species),
            _ => None,
        }
    }

    /// Every horse with somebody on it, as it is drawn this frame: its feet
    /// and its yaw. What `RemotePlayers::mount` sits the riders on.
    ///
    /// **Drawn, not latest**, for the raft's reason (`rafts`): a rider put on
    /// the horse's newest snapshot sits a tick ahead of the horse under them,
    /// and at a gallop a tick is half a block of saddle. The one this client
    /// rides is where its prediction is, as the horse itself is drawn.
    pub fn ridden_horses(&self, now: Instant) -> Vec<(DVec3, f32)> {
        self.entities
            .iter()
            .filter_map(|(id, entity)| {
                let EntityKind::Animal { species, growth, tack, .. } = entity.kind else {
                    return None;
                };
                if tack & primitive_shared::horse::TACK_RIDDEN == 0 {
                    return None;
                }
                if let Some((_, feet, yaw)) = self.ridden.filter(|(ridden, _, _)| ridden == id) {
                    return Some((feet, yaw));
                }
                let size = primitive_shared::youth::size(primitive_shared::youth::from_wire(growth));
                let feet = entity.drawn(now) - DVec3::Y * f64::from(species.height() * size * 0.5);
                Some((feet, entity.drawn_yaw(now)))
            })
            .collect()
    }

    /// How far along a look ray a raft is, if the ray meets it.
    ///
    /// The deck and the hull under it, in the raft's own frame for the reason
    /// `aimed_at` turns into an animal's: a raft three blocks long is not a
    /// three-block square. With `rigging`, the yard and the sail as well
    /// (`raft_model::rig_box`), for a click rather than a blow -- and not when
    /// the eye is already among them, where every click at anything would be a
    /// click on the raft.
    fn raft_hit(&self, id: EntityId, entity: &Entity, eye: DVec3, dir: Vec3, range: f32, rigging: bool) -> Option<f32> {
        use primitive_shared::raft::{DRAFT, FREEBOARD, HALF_LENGTH, HALF_WIDTH};
        let body = self.raft_pose(id, entity, self.now.unwrap_or_else(Instant::now))?;
        let (sin, cos) = (-body.yaw).sin_cos();
        let turn = |v: Vec3| Vec3::new(v.x * cos - v.z * sin, v.y, v.x * sin + v.z * cos);
        let (eye, dir) = (turn((eye - DVec3::new(body.x, f64::from(body.y), body.z)).as_vec3()), turn(dir));
        let hull = ray_hits_box(
            eye,
            dir,
            Vec3::new(0.0, (FREEBOARD - DRAFT) * 0.5, 0.0),
            Vec3::new(HALF_LENGTH, (FREEBOARD + DRAFT) * 0.5, HALF_WIDTH),
            range,
        );
        if !rigging {
            return hull;
        }
        let (centre, half) = crate::logic::raft_model::rig_box();
        let among = (eye - centre).abs().cmple(half).all();
        let rig = if among { None } else { ray_hits_box(eye, dir, centre, half, range) };
        match (hull, rig) {
            (Some(hull), Some(rig)) => Some(hull.min(rig)),
            (hull, rig) => hull.or(rig),
        }
    }

    /// How many of what `wanted` accepts are lying in one block cell.
    ///
    /// For the one gesture that is about things on the ground rather than
    /// a block: flint struck at a cell with sticks and a log lying on it is
    /// a firepit (`primitive_shared::pit`), and flint struck at bare grass
    /// sets a nodule down. The server counts again from its own store; this
    /// only decides which of the two messages to send.
    pub fn count_lying_in(
        &self,
        cell: (i32, i32, i32),
        wanted: impl Fn(primitive_shared::types::BlockId) -> bool,
    ) -> u32 {
        self.entities
            .values()
            .filter_map(|entity| match entity.kind {
                EntityKind::Item { block, count } if wanted(block) => Some((entity.current, count)),
                _ => None,
            })
            .filter(|(at, _)| (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32) == cell)
            .map(|(_, count)| count)
            .sum()
    }

    /// The raft a look ray meets first, and how far off it is: what a right
    /// click or a tap on the timber or the sail uses (`ClientMessage::UseRaft`).
    pub fn aimed_raft(&self, eye: DVec3, dir: Vec3, range: f32) -> Option<(EntityId, f32)> {
        self.entities
            .iter()
            .filter(|(_, entity)| matches!(entity.kind, EntityKind::Raft { .. }))
            .filter_map(|(&id, entity)| self.raft_hit(id, entity, eye, dir, range, true).map(|distance| (id, distance)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Every raft in sight this frame. See `riding::RaftPose`.
    pub fn rafts(&self) -> Vec<RaftPose> {
        let now = self.now.unwrap_or_else(Instant::now);
        self.entities
            .iter()
            .filter_map(|(&id, entity)| {
                Some(RaftPose {
                    id,
                    now: self.raft_pose(id, entity, now)?,
                    latest: body_of(entity.kind, entity.current)?,
                })
            })
            .collect()
    }

    /// The server gave this client the oars of a raft, or took them back.
    ///
    /// **Handed over where the raft is drawn, both ways.** The oars used to
    /// start from the latest snapshot, which is a tick and a round trip from
    /// the raft on the screen, so taking them jumped the deck -- and everybody
    /// on it -- to there, and letting go of them put it back. The prediction
    /// now starts from the raft as it is drawn and, let go, becomes the raft as
    /// it is drawn
    /// (`taking_and_leaving_the_oars_of_a_raft_under_way_leaves_it_where_it_is_drawn`).
    pub fn steer(&mut self, raft: Option<EntityId>) {
        if self.steering.is_some_and(|steering| Some(steering.id) == raft) {
            return;
        }
        if let Some(steering) = self.steering.take() {
            self.glides.insert(steering.id, Glide::new(steering.body));
        }
        self.steering = raft.and_then(|id| {
            let entity = self.entities.get(&id)?;
            let from = match self.glides.remove(&id) {
                Some(glide) => glide.body,
                None => body_of(entity.kind, entity.current)?,
            };
            Some(Steering::new(id, from))
        });
    }

    /// The raft this client is rowing, if it is rowing one.
    pub fn steering(&self) -> Option<&Steering> {
        self.steering.as_ref()
    }

    /// One frame of the wind, of the rower's raft ahead of the server, and of
    /// every other raft carried on from its snapshot (`riding::Glide`).
    pub fn predict(
        &mut self,
        oars: Oars,
        wind: Wind,
        // The hour of the world, which the sea's circulation is read from
        // on both sides of the wire. See `fluid::current_at`.
        world_days: f32,
        chunks: &crate::logic::chunk_manager::ChunkManager,
        dt: f32,
    ) {
        self.wind = Some(wind);
        let now = self.now.unwrap_or_else(Instant::now);
        let tick = self.tick_duration().as_secs_f32();
        let rowed = self.steering.map(|steering| steering.id);
        let entities = &self.entities;
        self.glides.retain(|id, _| entities.contains_key(id) && Some(*id) != rowed);
        for (&id, entity) in &self.entities {
            if Some(id) == rowed {
                continue;
            }
            let Some(server) = body_of(entity.kind, entity.current) else {
                continue;
            };
            let age = now.saturating_duration_since(entity.updated_at).as_secs_f32() + tick;
            self.glides.entry(id).or_insert_with(|| Glide::new(server)).advance(server, age, dt);
        }
        let Some(id) = rowed else {
            return;
        };
        let server = self.entities.get(&id).and_then(|entity| {
            body_of(entity.kind, entity.current)
                .map(|body| (body, now.saturating_duration_since(entity.updated_at).as_secs_f32() + tick))
        });
        // The raft went -- broken up, or out of sight -- and there is
        // nothing to row.
        let Some((server, age)) = server else {
            self.steering = None;
            return;
        };
        // The rower's own hand on the sheets goes into the prediction as well
        // as into the picture: the angle is what the wind is measured
        // against, so a rower whose raft was drawn braced and *pushed*
        // square would watch the sail come round and feel nothing change
        // until the server's answer arrived.
        let trim = self.trim.filter(|(trimmed, _)| *trimmed == id).map(|(_, angle)| angle);
        if let Some(steering) = self.steering.as_mut() {
            steering.predict(server, age, oars, trim, wind, world_days, self.river, &|x, y, z| chunks.block_at(x, y, z), dt);
        }
    }

    /// Takes one `Entities` message.
    ///
    /// ## Why the tick number matters more than the clock
    ///
    /// This used to measure the interval between snapshots with
    /// `Instant::now()`, and that is wrong in a way that is invisible
    /// until the network hiccups: **the client drains its whole message
    /// queue in one frame**. Two `Entities` messages that were sent a
    /// twentieth of a second apart and arrived together are applied
    /// microseconds apart, so the measured interval is nearly zero --
    /// clamped up to five milliseconds, which is still ten times too
    /// small.
    ///
    /// Everything downstream then goes wrong at once. `progress` runs
    /// from 0 to 1 in five milliseconds, so the animal *teleports*
    /// between snapshots instead of easing; the eased facing lands on
    /// its target instantly and the next snapshot starts a fresh turn,
    /// so it spins; and `speed` is `step / interval`, so the legs run
    /// at ten times the pace of an animal that is barely moving. What a
    /// player sees is exactly what was reported: animals whirling on the
    /// spot shaking their legs.
    ///
    /// The fix is to stop asking the clock. The message carries the
    /// server `tick` it was built on, and the tick rate is in the
    /// handshake, so the interval a snapshot *represents* is arithmetic
    /// rather than a measurement -- and it is immune to when the message
    /// happened to be read.
    ///
    /// The clock is still used for *drawing* (`progress` counts from
    /// when the snapshot was applied), which is right: that is a
    /// question about this frame, not about the server.
    /// Records how fast the server this session is connected to ticks.
    ///
    /// Called once, from the handshake. A rate that is not a sane
    /// positive number is ignored rather than believed -- it comes off a
    /// socket, and a tick duration of zero would divide the gait by
    /// nothing.
    pub fn set_tick_rate(&mut self, hz: f32) {
        if hz.is_finite() && hz > 0.0 {
            self.tick_duration = Some(Duration::from_secs_f32((1.0 / hz).clamp(0.001, 1.0)));
        }
    }

    fn tick_duration(&self) -> Duration {
        self.tick_duration.unwrap_or(NOMINAL_INTERVAL)
    }

    pub fn apply_snapshot(&mut self, tick: u64, states: &[EntityState]) {
        self.apply_snapshot_at(tick, states, Instant::now());
    }

    /// The same, told when the packet arrived.
    ///
    /// **The arrival time is an argument so that a test can be a network.**
    /// What this module is for is drawing smoothly through a ragged
    /// delivery, and a test that cannot say "this one came fifteen
    /// milliseconds late" cannot test that at all -- it can only sleep and
    /// hope, which is how a timing bug survives a suite.
    pub(crate) fn apply_snapshot_at(&mut self, tick: u64, states: &[EntityState], now: Instant) {
        self.started.get_or_insert(now);

        // How many server ticks this snapshot is past the last one.
        //
        // Zero or negative means a duplicate or a message that overtook
        // its predecessor; both are dropped rather than applied, because
        // applying one would restart the interpolation from a position
        // the entity has already left.
        let ticks = match self.last_tick {
            Some(last) if tick <= last => return,
            Some(last) => tick - last,
            None => 1,
        };
        self.last_tick = Some(tick);
        // Clamped at both ends. A gap of one tick is the normal case; a
        // gap of fifty is a client that was not being served, and
        // stretching one snapshot's motion over two and a half seconds
        // would draw an animal gliding rather than admitting it lost
        // touch.
        let steps = ticks.min(u32::MAX as u64) as u32;
        let interval = (self.tick_duration() * steps)
            .clamp(Duration::from_millis(5), Duration::from_millis(500));

        // The rhythm the packets really keep, a tick at a time. Measured
        // before `last_snapshot` is moved on, and only believed when it is
        // somewhere near the rate the server claims -- a gap of two seconds
        // is a client that was not being served, and averaging that in would
        // draw the next second of every animal in slow motion.
        if let Some(last) = self.last_snapshot {
            let measured = now.saturating_duration_since(last) / steps.max(1);
            let tick = self.tick_duration();
            if measured > tick / 4 && measured < tick * 4 {
                self.rhythm = Some(match self.rhythm {
                    Some(rhythm) => (rhythm * 6 + measured) / 7,
                    None => measured,
                });
            }
        }
        self.last_snapshot = Some(now);

        // **When this snapshot is due on screen**, on the server's rhythm
        // rather than on the packet's: one interval after the snapshot
        // before it, so the glides meet exactly. See `PLAYOUT_SLACK` for the
        // two ways the schedule is re-laid -- a packet so late that the
        // buffer is spent, and a client that has drifted further behind
        // than it needs to be.
        // How long the glide this snapshot starts is given, and when it
        // therefore ends: the playout clock, cut to the rhythm the packets
        // are really keeping. See `playout_span` and `rhythm`.
        let span = playout_span(self.due, now, self.rhythm.map_or(interval, |r| r * steps));
        self.due = Some(now + span);

        // Collected here and moved into `self.blows` after the loop: the
        // closure below already holds `self.entities` mutably, and a
        // second field of `self` borrowed inside it is a borrow the
        // checker will not have.
        let mut struck: Vec<Vec3> = Vec::new();
        // ...and the same for what has just let go. Asked before the
        // entry below, which is what creates the row this is testing for.
        let mut fell: Vec<(DVec3, BlockId)> = Vec::new();
        for state in states {
            let current = DVec3::new(state.x, state.y, state.z);
            if let EntityKind::FallingBlock { block } = state.kind {
                if !self.entities.contains_key(&state.id) {
                    fell.push((current, block));
                }
            }
            self.entities
                .entry(state.id)
                .and_modify(|e| {
                    // **A blow is the flash going *up*.** The server
                    // sends `hurt` as a level that decays, so it is
                    // above zero for a fraction of a second after every
                    // hit and arrives that way in several consecutive
                    // snapshots. Drawing blood whenever it is non-zero
                    // would be a fountain for as long as the flash
                    // lasts; the rising edge is the one snapshot that
                    // is news.
                    //
                    // At `current` with no offset because that is the
                    // animal's own centre -- see `animal_model`, whose
                    // origin is the same point.
                    if let (
                        EntityKind::Animal { hurt: before, .. },
                        EntityKind::Animal { hurt: after, .. },
                    ) = (e.kind, state.kind)
                    {
                        if after > before {
                            struck.push(current.as_vec3());
                            // Which way this one throws it. See `flinch_side`.
                            e.blows = e.blows.wrapping_add(1);
                            let spun = (state.id ^ 0x9E37_79B9_7F4A_7C15)
                                .wrapping_mul(0x2545_F491_4F6C_DD1D)
                                .wrapping_add(u64::from(e.blows).wrapping_mul(0xD1B5_4A32_D192_ED03));
                            e.flinch_side = if spun >> 60 & 1 == 0 { 1.0 } else { -1.0 };
                        }
                    }
                    // Continue from where it is being drawn, not from the
                    // last snapshot's position: the glide it was on may not
                    // have finished -- see `plays_from` -- and restarting
                    // from either end of it would be a jump.
                    e.previous = e.drawn(now);
                    // One interval of walking is finished, and the next
                    // one is however far this snapshot moves it.
                    // Horizontal only: falling is not walking, and a
                    // hare dropped off a ledge should not paddle on the
                    // way down.
                    //
                    // **As far as the glide had got at `plays_from`**, which
                    // is a whole interval in the ordinary case and less when
                    // the schedule had to be re-laid. Billing the whole
                    // interval either way would jump the walk phase forward
                    // by whatever was left of the old glide, and a leg that
                    // jumps once a packet is the strobe the phase is
                    // measured from distance to avoid.
                    e.walked += e.step * e.progress(now);
                    let moved = Vec3::new((current.x - e.current.x) as f32, 0.0, (current.z - e.current.z) as f32)
                        .length();
                    // **A jump is not a stride.** An entity that moved
                    // further in one interval than anything alive can
                    // run has not walked there -- it was teleported, or
                    // it left the interest radius and came back, or this
                    // is a fresh entity that reused an id. Billing that
                    // distance to the gait spins the legs for the next
                    // second; the position still eases, because a smooth
                    // slide to the right place is better than a snap
                    // either way.
                    e.step = if moved > MAX_STRIDE { 0.0 } else { moved };
                    e.current = current;
                    // Continue from the facing being *drawn*, for the
                    // reason the position does.
                    e.previous_yaw = e.drawn_yaw(now);
                    e.kind = state.kind;
                    // **When it stopped, not whether it is stopped.**
                    // `updated_at` is refreshed by every snapshot,
                    // including the ones that say "it is still where it
                    // was", so it cannot time anything that happens
                    // *after* a thing settles -- and a dropped item
                    // tipping over on the ground is exactly that. This
                    // is stamped once, on the first snapshot that does
                    // not move it, and again the first one that does --
                    // each time from wherever the pose had got to.
                    let still = (current - e.previous).length_squared() < f64::from(STILL_ENOUGH);
                    if still != e.lying {
                        e.tip_from = tipped_over(e, now);
                        e.lying = still;
                        e.tip_since = now;
                    }
                    e.updated_at = now;
                    e.plays_from = now;
                    e.span = span;
                    e.interval = interval;
                })
                .or_insert(Entity {
                    lying: false,
                    tip_from: 0.0,
                    tip_since: now,
                    kind: state.kind,
                    // A newly seen entity starts where it is rather than
                    // easing in from wherever the last one happened to
                    // be -- otherwise a block that starts falling appears
                    // to fly in from somewhere else.
                    previous: current,
                    current,
                    updated_at: now,
                    plays_from: now,
                    span,
                    interval,
                    walked: 0.0,
                    step: 0.0,
                    // A newly seen animal faces where it faces rather
                    // than turning into position from north.
                    previous_yaw: match state.kind {
                        EntityKind::Animal { yaw, .. } | EntityKind::Raft { yaw, .. } => yaw,
                        _ => 0.0,
                    },
                    // ...and with its head already where the attitude says,
                    // for the same reason the facing is: an animal that comes
                    // into view grazing is grazing, not standing up straight
                    // and then lowering its head over the next half second.
                    head: match state.kind {
                        EntityKind::Animal { attitude, .. } => {
                            crate::logic::animal_model::head_carried(attitude)
                        }
                        _ => 0.0,
                    },
                    // ...and from a different point of the swish than the one
                    // beside it, so a herd's tails do not keep time. The id is
                    // what every other per-animal phase in this game is spread
                    // by (the server's `spread`), and it is all the client has.
                    age: (state.id % 977) as f32 * 0.021,
                    // ...and from a different point of the wingbeat, for
                    // exactly the reason `age` is spread: a covey that came
                    // into view together and beat in unison would be one
                    // bird drawn three times. The server spreads its own
                    // climb-and-glide from the same id (`Animal::air_phase`).
                    beat: (state.id % 331) as f32 * 0.037,
                    // Upright and untouched: an animal that comes into view
                    // is not mid-turn and has not been hit here.
                    banked: 0.0,
                    flinch_side: 1.0,
                    blows: 0,
                    dying: 0.0,
});
        }

        self.blows.append(&mut struck);
        self.gave_way.append(&mut fell);

        // A snapshot is the complete set of nearby entities, so anything
        // it doesn't mention has gone.
        self.entities
            .retain(|id, _| states.iter().any(|state| state.id == *id));
    }

    /// Where animals have been struck since this was last asked, and
    /// forgets them.
    ///
    /// Draining rather than reading, because the caller turns each one
    /// into a burst of particles and a blow drawn twice is a blow that
    /// bleeds twice. Several snapshots can land in one frame (see
    /// `apply_snapshot` on why the queue is drained in a batch), so this
    /// is what makes "one blow, one burst" hold whatever the network
    /// did.
    pub fn take_blows(&mut self) -> Vec<Vec3> {
        std::mem::take(&mut self.blows)
    }

    /// What has let go since this was last asked, and forgets it.
    ///
    /// Drained rather than read, on `take_blows`'s reasoning: a collapse
    /// heard twice is a collapse the player looks round for twice.
    pub fn take_gave_way(&mut self) -> Vec<(DVec3, BlockId)> {
        std::mem::take(&mut self.gave_way)
    }

    /// Every animal, as the soundscape and the small life round the player
    /// need it: who, what, where it is drawn this frame, and how fast.
    ///
    /// **Read off what was drawn, not what was sent**, so a gull's call comes
    /// from the gull on the screen and a frog hushes at the wolf where the
    /// player sees it -- and so a take-off is heard on the frame the wings
    /// open (`animal_model::AIRBORNE_SPEED` and `soundscape::in_the_air`
    /// read the same speed).
    pub fn heard(&self) -> Vec<crate::audio::soundscape::Heard> {
        let now = self.now.unwrap_or_else(Instant::now);
        self.entities
            .iter()
            .filter_map(|(id, entity)| match entity.kind {
                EntityKind::Animal { species, hurt, .. } => Some(crate::audio::soundscape::Heard {
                    id: *id,
                    species,
                    at: entity.drawn(now),
                    speed: entity.gait(now).1,
                    hurt,
                }),
                _ => None,
            })
            .collect()
    }

    /// A block has appeared in the world at this cell.
    ///
    /// If a falling entity was occupying it, that entity just landed and
    /// *is* this block -- so it stops being drawn now, rather than
    /// hovering over its own landing site until the stale timeout.
    ///
    /// **Falling blocks and nothing else.** This used to remove
    /// whatever was standing in the cell, of any kind, and the cell is
    /// matched with a block of slack in either direction -- so putting
    /// a torch down beside a deer, or a landing block coming to rest
    /// where a stack had been dropped, deleted the animal or the stack
    /// until the next snapshot put it back. A dropped item that blinks
    /// every time somebody builds near it is the same complaint this
    /// module's `STALE_AFTER` was widened for, arriving by a different
    /// road. A falling block is the only thing a new block can *be*.
    pub fn on_block_placed(&mut self, gx: i32, gy: i32, gz: i32, block: BlockId) {
        if block == BLOCK_AIR {
            return;
        }
        let now = self.now.unwrap_or_else(Instant::now);
        self.entities.retain(|_, entity| {
            if !matches!(entity.kind, EntityKind::FallingBlock { .. }) {
                return true;
            }
            let at = entity.drawn(now);
            !(at.x.floor() as i32 == gx
                && at.z.floor() as i32 == gz
                // A little vertical slack: the entity is drawn one
                // snapshot behind, so at the moment it lands it can
                // still be up to a cell above its resting place.
                && (at.y.floor() as i32 - gy).abs() <= 1)
        });
    }

    pub fn tick(&mut self, dt: f32) {
        let now = Instant::now();
        self.now = Some(now);
        if let Some(last) = self.last_snapshot {
            if now.duration_since(last) >= STALE_AFTER {
                self.entities.clear();
            }
        }
        // **Heads go down and come up in their own time.** The only piece of
        // per-entity state this client keeps across a frame, and the reason
        // `dt` is read here at all: see `Entity::head`.
        let dt = dt.max(0.0);
        let step = crate::logic::animal_model::HEAD_RATE * dt;
        // **...and the bank comes over with the body.** The measured turn
        // rate is a step function -- one value for a whole snapshot -- so a
        // lean taken straight off it snapped on and off in single frames.
        // See `Entity::banked` and `BANK_RATE`.
        let over = BANK_RATE * dt;
        for entity in self.entities.values_mut() {
            let wanted = entity.head_wanted();
            entity.head += (wanted - entity.head).clamp(-step, step);
            let turning = entity.turning();
            entity.banked += (turning - entity.banked).clamp(-over, over);
            entity.age += dt;
            // **The wingbeat, turned by the flight rather than by the
            // ground covered.** See `Entity::beat`: the rate answers to how
            // hard the bird is climbing, which is a rate that changes, and
            // only an integrated phase may change rate without jumping.
            // Wrapped into whole beats so it stays a small number for the
            // life of the entity -- the same precision argument the
            // server's `turn_towards` makes about an accumulated yaw.
            if matches!(entity.kind, EntityKind::Animal { species, .. } if species.flies()) {
                let rate = crate::logic::animal_model::beat_rate(
                    entity.gait(now).1,
                    entity.rising(),
                );
                entity.beat =
                    (entity.beat + rate * dt).rem_euclid(crate::logic::animal_model::BEAT_WRAP);
            }
            if matches!(entity.kind, EntityKind::Animal { attitude: primitive_shared::protocol::Attitude::Dying, .. }) {
                entity.dying += dt;
            }
        }
    }

    /// Builds one mesh for every entity, in the terrain vertex format.
    /// Convenience wrapper that allocates. The frame loop uses
    /// `build_mesh_into` with buffers it keeps; this is for tests, which
    /// care about the geometry and not about the allocation.
    #[cfg(test)]
    pub fn build_mesh(&self, layers: &FaceLayers, light: &LightMap) -> (Vec<Vertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        self.build_mesh_into(layers, light, &mut vertices, &mut indices);
        (vertices, indices)
    }

    /// The same mesh, appended to buffers the caller keeps.
    ///
    /// The frame loop rebuilds this every frame because the entities
    /// move every frame; reusing the storage means it does not also
    /// allocate every frame.
    #[cfg(test)]
    pub fn build_mesh_into(
        &self,
        layers: &FaceLayers,
        light: &LightMap,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        self.build_meshes_into(
            Vec3::ZERO,
            layers,
            light,
            None,
            vertices,
            indices,
            &mut Vec::new(),
            &mut Vec::new(),
        )
    }

    /// Everything the entity list draws, split by what shape it is.
    ///
    /// Two output pairs because there are two vertex formats. A falling
    /// block, and a dropped block, are cubes and go in the terrain's; a
    /// dropped *item* is a sprite with a thickness and goes in the item
    /// pipeline's. Which is which is asked of `models`: a block that has
    /// no model of its own is a cube, and that is the whole rule.
    #[allow(clippy::too_many_arguments)]
    /// `origin` is the point the frame is drawn around -- see
    /// `engine::renderer::FrameParams::render_origin`. Every position
    /// that reaches a vertex is measured from it; the *world* position
    /// is still what the light is sampled at, because the light map is
    /// indexed by where things actually are.
    #[allow(clippy::too_many_arguments)]
    pub fn build_meshes_into(
        &self,
        origin: Vec3,
        layers: &FaceLayers,
        light: &LightMap,
        models: Option<&crate::engine::texture::TextureManager>,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
        item_vertices: &mut Vec<ItemVertex>,
        item_indices: &mut Vec<u32>,
    ) {
        self.build_meshes_as(origin, layers, light, models, true, vertices, indices, item_vertices, item_indices);
    }

    /// `build_meshes_into`, told whether a dropped block with a model of its
    /// own is drawn as that model.
    ///
    /// **`false` is how such a block was drawn before it was**, kept
    /// reachable so one binary can photograph both (`model_light_repro`):
    /// the arms it falls through to are the ones every other dropped thing
    /// still takes, so what it draws is the old picture and not a copy of
    /// it that could drift.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_meshes_as(
        &self,
        origin: Vec3,
        layers: &FaceLayers,
        light: &LightMap,
        models: Option<&crate::engine::texture::TextureManager>,
        carried_models: bool,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
        item_vertices: &mut Vec<ItemVertex>,
        item_indices: &mut Vec<u32>,
    ) {
        let now = self.now.unwrap_or_else(Instant::now);
        // One model's worth of scratch for the whole list, so a floor of
        // dropped barrels does not allocate per barrel per frame.
        let mut model: (Vec<Vertex>, Vec<u32>) = (Vec::new(), Vec::new());

        for (id, entity) in self.entities.iter() {
            match entity.kind {
                EntityKind::FallingBlock { block } => {
                    append_cube(
                        vertices,
                        indices,
                        entity.drawn(now),
                        origin,
                        block,
                        1.0,
                        layers,
                        light,
                    );
                }
                // **A block with a model of its own lies as that model**: a
                // barrel on the grass is staves round a floor, a bed is a bed.
                // It was a sprite cut from the barrel's icon or, with no icon,
                // a cube of planks -- see `mesh::carried_model` for the report.
                // In the terrain's vertex format, so it goes down the pipeline
                // the animals do: the ground's light at every step, the
                // shadows when they are on, and a shadow of its own.
                EntityKind::Item { block, .. } if carried_models && crate::engine::mesh::has_carried_model(block) => {
                    model.0.clear();
                    model.1.clear();
                    crate::engine::mesh::carried_model(block, layers, &mut model.0, &mut model.1);
                    let (low, high) = crate::engine::mesh::extent(&model.0);
                    let phase = *id as f32 * 0.7 + self.age(now);
                    // It stands and does not tip over: a barrel has a floor to
                    // stand on, which a plate does not. The hover goes as it
                    // comes to rest, over the quarter second a cube takes.
                    let bob = item_bob(phase) * (1.0 - tipped_over(entity, now));
                    let yaw = if has_settled(entity) { settled_yaw(*id) } else { phase * ITEM_SPIN };
                    let scale = MODEL_DRAWN / (high - low).max_element().max(1.0 / 16.0);
                    let resting = entity.drawn(now);
                    // **On its foot, on the floor of the box the server
                    // moves** -- the rule `dropped_lift` keeps for a sprite,
                    // for the reason given there.
                    let floor = (resting - origin.as_dvec3()).as_vec3() + Vec3::new(0.0, -ITEM_SCALE * 0.5 + bob, 0.0);
                    let transform = glam::Mat4::from_translation(floor)
                        * glam::Mat4::from_rotation_y(-yaw)
                        * glam::Mat4::from_scale(Vec3::splat(scale))
                        * glam::Mat4::from_translation(-Vec3::new((low.x + high.x) * 0.5, low.y, (low.z + high.z) * 0.5));
                    crate::engine::mesh::place_carried(
                        &model.0,
                        &model.1,
                        transform,
                        sampled_light(resting, light),
                        vertices,
                        indices,
                    );
                }
                EntityKind::Item { block, .. }
                    if models.and_then(|m| m.item_model(block)).is_some() =>
                {
                    // Not a cube. Same bob and spin as one -- see below
                    // for why they are there at all -- but the shape
                    // comes out of the texture.
                    let textures = models.expect("guarded by the match");
                    let model = textures.item_model(block).expect("guarded by the match");
                    let phase = *id as f32 * 0.7 + self.age(now);
                    // **A thing that has landed lies still.** The hover
                    // and the turn are what a falling item does; once
                    // the server has parked it, it is an object on the
                    // ground and it behaves like one -- see
                    // `has_settled`.
                    let tip = tipped_over(entity, now);
                    let settled = has_settled(entity);
                    // The hover goes as it tips: an item on its side
                    // resting a finger's width above the ground is the
                    // fault `item_bob` was written to prevent, wearing a
                    // different hat.
                    let bob = item_bob(phase) * (1.0 - tip);
                    let scale = dropped_scale(model);
                    // **Standing on the box, not centred in it.** The
                    // server's position is the middle of the collision
                    // cube, and its bottom face is the surface the item
                    // came to rest on; where the drawing sits inside its
                    // own frame is a margin the artist chose. Putting
                    // the frame's middle at the box's middle therefore
                    // let the margin decide how the thing met the
                    // ground: ash, drawn to the bottom edge of its tile,
                    // stood five centimetres inside the floor, and a
                    // flint pick head, drawn in the middle of its tile,
                    // hovered two thirds of its own height above it.
                    // Both were photographed. Lifting by the model's own
                    // foot puts the lowest drawn texel on the resting
                    // surface for every picture, whatever its margins --
                    // which is the invariant `item_bob` was written to
                    // protect and could not protect on its own.
                    // **The lift is the pose's, and this is what put
                    // every settled item in the air.** `dropped_lift`
                    // raises the drawing until its lowest *upright*
                    // texel sits on the box's floor -- a pick head drawn
                    // in the middle of its tile is lifted most of its
                    // own height. Tipped flat, that height is gone and
                    // the lift is not: the thing hovers, which is
                    // exactly what a player reported. Lying, the plate
                    // has no height to speak of, so it sits on the floor
                    // of the box and nothing else.
                    let lift = dropped_lift(model, scale) * (1.0 - tip)
                        + (-ITEM_SCALE * 0.5) * tip;
                    let centre = entity.drawn(now) + DVec3::new(0.0, f64::from(lift + bob), 0.0);
                    let (sky, block_light) = sampled_light(centre, light);
                    let at = (centre - origin.as_dvec3()).as_vec3();
                    model.append_tipped(
                        item_vertices,
                        item_indices,
                        [at.x, at.y, at.z],
                        scale,
                        if settled { settled_yaw(*id) } else { phase * ITEM_SPIN },
                        tip,
                        // The *carried* picture where a block has one,
                        // and the face only as a fallback -- because
                        // that is the picture the silhouette was cut
                        // from. Taking the face regardless put a paving
                        // slab of ash on a model shaped like a handful
                        // of it: the outline said one thing and the
                        // texture on it said another.
                        layers
                            .layer_for_item(block)
                            .unwrap_or_else(|| layers.layer_for_face(block, 0)),
                        sky,
                        block_light,
                    );
                }
                EntityKind::Animal { species, growth, .. } => {
                    // Everything about the shape lives in a table -- see
                    // `logic::animal_model`, which is the one file to
                    // edit to change what an animal looks like. This arm
                    // works out where it is, how far it has walked and
                    // how fast it is going, and hands those over.
                    //
                    // **The horse under this client is drawn where its
                    // prediction is** (see `ridden`), placed by its feet the
                    // way the server places an animal by its middle. Its legs
                    // and head still run on the snapshots: they are how it
                    // moves, not where it is.
                    let predicted = self.ridden.filter(|(ridden, _, _)| ridden == id);
                    let centre = match predicted {
                        Some((_, feet, _)) => {
                            let size = primitive_shared::youth::size(primitive_shared::youth::from_wire(growth));
                            feet + DVec3::Y * f64::from(species.height() * size * 0.5)
                        }
                        None => entity.drawn(now),
                    };
                    let (sky, block_light) = sampled_light(centre, light);
                    let motion = entity.motion(now);
                    // **A body going down turns toward the carcass it is about
                    // to be.** The carcass is drawn in the middle of its cell at
                    // a yaw hashed from the cell (`animal_model::carcass_yaw`),
                    // and the body lies wherever and however it fell -- so
                    // without this the last frame of the fall and the first of
                    // the carcass were two animals, a hand's breadth and a
                    // quarter turn apart. Eased in with the roll, so it is part
                    // of going down rather than a jump at the end of it.
                    let (centre, yaw) = if motion.fallen > 0.0 {
                        let size = primitive_shared::youth::size(primitive_shared::youth::from_wire(growth));
                        let feet = centre.y - f64::from(species.height() * size * 0.5);
                        let cell = (centre.x.floor(), (feet + 0.01).floor(), centre.z.floor());
                        let lie = crate::logic::animal_model::carcass_yaw(cell.0 as i32, cell.1 as i32, cell.2 as i32);
                        let yaw = entity.drawn_yaw(now);
                        let turn = (lie - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                        let f = f64::from(motion.fallen);
                        (
                            DVec3::new(
                                centre.x + (cell.0 + 0.5 - centre.x) * f,
                                centre.y,
                                centre.z + (cell.2 + 0.5 - centre.z) * f,
                            ),
                            yaw + turn * motion.fallen,
                        )
                    } else {
                        (centre, predicted.map_or_else(|| entity.drawn_yaw(now), |(_, _, yaw)| yaw))
                    };
                    crate::logic::animal_model::build(
                        species,
                        (centre - origin.as_dvec3()).as_vec3(),
                        yaw,
                        motion,
                        layers,
                        (sky, block_light),
                        vertices,
                        indices,
                    );
                }
                EntityKind::Raft { stroke, sail, hurt, .. } => {
                    // `sail` comes off the snapshot and the angle off the
                    // pose, because the pose is where this client's own hand
                    // on the sheets is already applied (`raft_pose`) -- the
                    // yard has to be drawn where the player is dragging it,
                    // not a round trip behind.
                    let Some(body) = self.raft_pose(*id, entity, now) else {
                        continue;
                    };
                    // The rower's own oars move with their own keys; anyone
                    // else's with what the server says they are doing.
                    let stroke = self
                        .steering
                        .filter(|steering| steering.id == *id)
                        .map_or(stroke, |steering| steering.oars.stroke);
                    /// Strokes a second at a full pull, in radians of the
                    /// oars' cycle: a stroke every second and a half.
                    const OAR_RATE: f32 = 4.2;
                    let phase = (stroke.abs() > 0.05).then(|| self.age(now) * OAR_RATE * stroke.signum());
                    let lit = sampled_light(DVec3::new(body.x, f64::from(body.deck_top() + 0.5), body.z), light);
                    crate::logic::raft_model::build(
                        &body,
                        &crate::logic::raft_model::Rigging {
                            sail,
                            angle: body.sail_angle,
                            wind: self.wind.unwrap_or(Wind::CALM),
                            stroke: phase,
                            hurt: (hurt > 0.0).then_some(hurt),
                        },
                        origin,
                        layers,
                        lit,
                        vertices,
                        indices,
                    );
                }
                EntityKind::Item { block, .. } => {
                    // The server's position is the item's *centre*, so
                    // the cube is built back from it. Plus a slow bob
                    // and spin: without them a dropped block is
                    // indistinguishable from a very small block someone
                    // left lying around, which is exactly how it read.
                    // Offset per entity, so a pile does not bob and
                    // turn as one object.
                    let phase = *id as f32 * 0.7 + self.age(now);
                    let settled = has_settled(entity);
                    // A cube has no pose to tip into -- it is the same
                    // shape whichever way up it is -- so it only stops
                    // turning and stops hovering, and it does both over
                    // the same quarter second the sprites take.
                    let bob = item_bob(phase) * (1.0 - tipped_over(entity, now));
                    let centre = entity.drawn(now) + DVec3::new(0.0, f64::from(bob), 0.0);
                    append_spinning_cube(
                        vertices,
                        indices,
                        centre,
                        origin,
                        block,
                        ITEM_SCALE,
                        if settled { settled_yaw(*id) } else { phase * ITEM_SPIN },
                        layers,
                        light,
                    );
                }
            }
        }
    }
}

/// Side of a dropped item, as a fraction of a block. Must match
/// `primitive_server::items::ITEM_SIZE`, which is what collides.
const ITEM_SCALE: f32 = 0.3;
/// How big the longest side of a dropped *sprite* is drawn.
///
/// Sprites read smaller than a cube of the same measurement -- a plate
/// has no bulk -- so they get half again, which is the same trade
/// `logic::hand` makes in the other direction for a held block.
const ITEM_DRAWN: f32 = ITEM_SCALE * 1.5;
/// How long the longest side of a dropped *model* is drawn -- a barrel, a
/// jug, a bed (see `mesh::carried_model`).
///
/// The sprites' size, for the reason a sprite is sized by its silhouette: a
/// row of things emptied from a pack should be one size, whatever each of
/// them is drawn as.
const MODEL_DRAWN: f32 = ITEM_DRAWN;
/// How far a dropped item rises above where it is resting, in blocks.
const ITEM_BOB: f32 = 0.06;

/// How far above its resting height an item is drawn, at `phase`.
///
/// **Always at or above, never below**, and that is the fix rather than
/// a preference. The server rests an item exactly on the surface that
/// stopped it -- its centre half a side up, so the bottom face touches
/// the ground -- and the bob used to be `sin(phase) * BOB`, which is
/// *negative* for half of every cycle. A dropped block therefore spent
/// half its life a fifth of its own height inside the ground it was
/// lying on, which on a dark floor or against a matching texture is a
/// block you cannot see.
///
/// A raised sine costs the same arithmetic and hovers instead.
fn item_bob(phase: f32) -> f32 {
    (phase.sin() * 0.5 + 0.5) * ITEM_BOB
}
/// Radians of spin per second, while it is still moving.
const ITEM_SPIN: f32 = 1.1;

/// How still is still: a millimetre squared. Well under the smallest
/// step a falling item takes in a tick, well over float noise in a
/// position that was copied rather than computed.
const STILL_ENOUGH: f32 = 1e-6;

/// Whether the server has stopped moving this thing.
///
/// **Two snapshots in the same place, and no third opinion.** The server
/// already decides when a drop has landed (`items::step`, which parks
/// the velocity and sets `resting`), and it is not on the wire -- so
/// this reads the consequence instead of the flag: an item that is in
/// the same place in the last two snapshots it sent is one that stopped.
/// The epsilon is a millimetre, well under the smallest step a falling
/// item takes in a tick and well over float noise in a position that was
/// copied rather than computed.
///
/// Reading it rather than adding a protocol field is the cheap half of
/// the trade; the expensive half would be a version bump on every server
/// and client for a fact both of them can already see.
fn has_settled(entity: &Entity) -> bool {
    (entity.current - entity.previous).length_squared() < f64::from(STILL_ENOUGH)
}

/// How far over a settled item has tipped, 0 (upright) to 1 (flat).
///
/// **A quarter of a second of falling over, not a switch.** The first
/// version chose the pose from `has_settled` alone, and the pose changed
/// between one frame and the next: a spinning plate became a plate lying
/// flat with nothing in between, which reads as a glitch rather than as
/// an object coming to rest. It is the same complaint as a bob that
/// jumps -- an instantaneous change to something the eye is already
/// tracking is the one thing it always notices.
///
/// **And back up the same way, from wherever it had got to.** Only the
/// lying down was eased: the first snapshot that moved a flat item again
/// stood it upright in one frame, and one that stopped it half way over
/// started the fall from upright. Anything nudged on and off -- a stack
/// on the water, a drop a block landed beside -- flicked between the two
/// poses, which is the same complaint again.
fn tipped_over(entity: &Entity, now: Instant) -> f32 {
    const TIP_SECONDS: f32 = 0.25;
    let t = (now.saturating_duration_since(entity.tip_since).as_secs_f32() / TIP_SECONDS)
        .clamp(0.0, 1.0);
    let to = if entity.lying { 1.0 } else { 0.0 };
    entity.tip_from + (to - entity.tip_from) * t
}

/// Which way a settled item lies, in radians.
///
/// Hashed from the entity id so that two loaves dropped in the same
/// place do not lie in exactly the same direction, and so that the angle
/// does not change between frames -- an item that picked a new angle
/// each frame would spin, which is the thing this replaced.
fn settled_yaw(id: u64) -> f32 {
    let mixed = id.wrapping_mul(2_654_435_761);
    (mixed % 3600) as f32 / 3600.0 * std::f32::consts::TAU
}

/// The turn a thing set down lies at, from the facing its cell was written
/// with (`types::BLOCK_SET_DOWN`), in `ItemModel::append_tipped`'s yaw.
///
/// **The top of the picture points the way the player was looking**, less
/// an eighth of a turn. The server writes the facing `Facing::toward_viewer`
/// gives, whose front is toward the player; lying flat, a picture's top goes
/// to -z before the yaw (`append_tipped`), so a quarter turn plus a half
/// brings it round to the look. The eighth is how the pictures are drawn: a
/// knife, an axe, a bone or a stick runs corner to corner of its tile, handle
/// low on the left, and laid with the tile square to the look it lay at an
/// angle to it. With the eighth it lies along the look, point away, the way
/// a hand puts a knife down in front of itself.
pub(crate) fn set_down_yaw(block: BlockId) -> f32 {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    primitive_shared::types::block_facing(block).quarters() as f32 * FRAC_PI_2 + PI - FRAC_PI_4
}

/// Every thing set down by hand, drawn lying where it was put: the item
/// model a dropped stack of it settles as, on the floor of its cell and
/// turned the way it was laid (`set_down_yaw`).
///
/// **Here, beside the dropped stacks, and not in the terrain mesh.** An item
/// model is its own vertex format and its own pass (`ItemVertex`), and a
/// thing that is drawn like a dropped knife in one place and like a block in
/// another is two knives. What is not a stack of anything -- the cell -- is
/// drawn by the mesher as nothing.
///
/// `things` is `ChunkManager::set_down_items`: cell, the cell's id, the item.
#[allow(clippy::too_many_arguments)]
pub fn build_set_down_into(
    things: impl Iterator<Item = ((i32, i32, i32), BlockId, BlockId)>,
    origin: Vec3,
    layers: &FaceLayers,
    light: &LightMap,
    models: Option<&crate::engine::texture::TextureManager>,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    item_vertices: &mut Vec<ItemVertex>,
    item_indices: &mut Vec<u32>,
) {
    for (cell, block, item) in things {
        let yaw = set_down_yaw(block);
        let floor = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1), f64::from(cell.2) + 0.5);
        match models.and_then(|models| models.item_model(item)) {
            Some(model) => {
                // Lit by the cell it lies in, as a dropped one is; drawn a
                // hair over the floor, the settled stack's own height.
                let (sky, block_light) = sampled_light(floor, light);
                // **Wet clay lying out is drawn darker** (`clay::shade`), and
                // by the light it is given rather than by a tint: an item
                // vertex has a layer and a light word and no colour, and a
                // colour channel for one shade on one kind of thing is a
                // vertex format change for every item drawn. A pot drying in
                // the sun reads as wet, then leather-hard, then pale. Light
                // levels off rather than a share of them, because the light
                // curve is steep: two levels is the shade the icon wears,
                // and six tenths of the level was a pot in a cave.
                let off = ((1.0 - primitive_shared::clay::shade(item)[0]) * 5.0).round() as u8;
                let (sky, block_light) = (sky.saturating_sub(off), block_light.saturating_sub(off));
                let at = (floor - origin.as_dvec3()).as_vec3();
                model.append_tipped(
                    item_vertices,
                    item_indices,
                    [at.x, at.y, at.z],
                    dropped_scale(model),
                    yaw,
                    1.0,
                    layers.layer_for_item(item).unwrap_or_else(|| layers.layer_for_face(item, 0)),
                    sky,
                    block_light,
                );
            }
            // Nothing to cut a silhouette from: the cube a dropped one is.
            None => append_spinning_cube(
                vertices,
                indices,
                floor + DVec3::new(0.0, f64::from(ITEM_SCALE * 0.5), 0.0),
                origin,
                item,
                ITEM_SCALE,
                yaw,
                layers,
                light,
            ),
        }
    }
}

/// How much a dropped sprite is scaled by.
///
/// **Measured across the silhouette, not across the frame.** A model's
/// coordinates run -0.5..0.5 over the whole PNG, so one scale applied to
/// every model hands the artist's margins the job of deciding how big
/// the object is: a handful of fibre fills its tile and came out 0.45 of
/// a block, a flint pick head is drawn eight texels by five and came out
/// a speck of 0.14 -- three times smaller, for no reason a player could
/// see or a designer chose. `ItemModel::silhouette` exists for exactly
/// this question and `logic::hand::held_scale` was the only thing asking
/// it; the same lump of ore was being sized properly in the hand and by
/// its margins on the ground.
///
/// The target is the size the *largest* dropped items were already drawn
/// at rather than a new number, so this evens the row up instead of
/// growing it.
///
/// A one-texel sliver would divide by a sixteenth and come out sixteen
/// times life size; the floor is one texel of a 16x16 sprite, which is
/// the smallest silhouette that can exist.
fn dropped_scale(model: &crate::engine::item_model::ItemModel) -> f32 {
    let [width, height] = model.silhouette();
    ITEM_DRAWN / width.max(height).max(1.0 / 16.0)
}

/// How far the middle of a dropped sprite's *frame* sits above the
/// middle of the box the server moves.
///
/// The one line that makes an item stand on the ground rather than in
/// it -- see the arm that calls it for what the two failures looked
/// like. Its own function so the property can be tested without a GPU:
/// what has to hold is that the model's lowest drawn texel lands on the
/// bottom face of the collision box, for every picture.
fn dropped_lift(model: &crate::engine::item_model::ItemModel, scale: f32) -> f32 {
    -ITEM_SCALE * 0.5 - model.foot() * scale
}

/// Where a ray enters an axis-aligned box, if it does at all.
///
/// The slab method, which is four lines and no trigonometry: for each
/// axis, the interval of `t` in which the ray is inside that pair of
/// planes; the box is hit where all three intervals overlap. Written
/// here rather than reached for because the one other thing in the
/// client that does this -- `geometry::ray_hits_player` -- tests a
/// *player's* box, which is a fixed size and rooted at the feet, and
/// generalising it would mean changing a function the anti-cheat also
/// reads.
fn ray_hits_box(eye: Vec3, dir: Vec3, centre: Vec3, half: Vec3, range: f32) -> Option<f32> {
    let (mut near, mut far) = (0.0f32, range);
    for axis in 0..3 {
        let (origin, direction) = (eye[axis], dir[axis]);
        let (low, high) = (centre[axis] - half[axis], centre[axis] + half[axis]);
        if direction.abs() < 1e-6 {
            // Parallel to this pair of planes: either always between
            // them or never.
            if origin < low || origin > high {
                return None;
            }
            continue;
        }
        let (mut t0, mut t1) = ((low - origin) / direction, (high - origin) / direction);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        near = near.max(t0);
        far = far.min(t1);
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// Which world direction a face of a spinning cube points, snapped to
/// the nearest axis.
///
/// The light word holds one of six directions and the spin is
/// continuous, so the nearest is the most that can be said -- exact at
/// the quarter turns, a little off between them, which is all a lambert
/// term needs. The top and the bottom never move: the spin is about Y.
fn spun_face(face_index: usize, sin: f32, cos: f32) -> u8 {
    if face_index <= 1 {
        return face_index as u8;
    }
    // The mesher's face order: 2 +X, 3 -X, 4 +Z, 5 -Z.
    let (nx, nz) = match face_index {
        2 => (1.0, 0.0),
        3 => (-1.0, 0.0),
        4 => (0.0, 1.0),
        _ => (0.0, -1.0),
    };
    let (wx, wz) = (nx * cos - nz * sin, nx * sin + nz * cos);
    if wx.abs() >= wz.abs() {
        if wx >= 0.0 { 2 } else { 3 }
    } else if wz >= 0.0 {
        4
    } else {
        5
    }
}

/// A cube centred on a point and turned about its vertical axis.
///
/// Separate from `append_cube` because falling blocks want neither: a
/// block in mid-fall is a block, aligned to the grid it came from and
/// going back into one.
///
/// Reachable from `logic::player_model`, which draws the block in
/// another player's hand: a block somebody is carrying and a block lying
/// on the floor are the same cube in the same light, and two builders
/// for that would be two answers about one picture.
#[allow(clippy::too_many_arguments)]
pub(crate) fn append_spinning_cube(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    centre: DVec3,
    origin: Vec3,
    block: primitive_shared::types::BlockId,
    scale: f32,
    yaw: f32,
    layers: &FaceLayers,
    light: &LightMap,
) {
    let (sin, cos) = yaw.sin_cos();
    // Lit by where it *is*; drawn from where the frame's origin is.
    let (sky, block_light) = sampled_light(centre, light);
    let centre = (centre - origin.as_dvec3()).as_vec3();

    for (face_index, face) in faces().iter().enumerate() {
        let layer = layers.layer_for_face(block, face_index);
        let base = vertices.len() as u32;
        for corner in face.corners.iter() {
            // Corner in the cube's own space, centred on the origin.
            let local = Vec3::new(
                (corner[0] - 0.5) * scale,
                (corner[1] - 0.5) * scale,
                (corner[2] - 0.5) * scale,
            );
            vertices.push(Vertex::new(
                [
                    centre.x + local.x * cos - local.z * sin,
                    centre.y + local.y,
                    centre.z + local.x * sin + local.z * cos,
                ],
                face_uv(face_index, *corner),
                layer,
                // The face index *after* the spin, not before. The
                // shader turns it into a normal, so writing the
                // model-space one welds the shading to the cube: a
                // dropped block turned on the spot with every side
                // holding its brightness, which reads as a lamp rather
                // than as an object catching the light.
                pack_light(sky, block_light, 3, spun_face(face_index, sin, cos)),
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// How lit something floating in the world is.
///
/// The brightest of the cell it is in and the one above it, for two
/// reasons that both show. An entity only just out of the grid can be
/// sampled in the cell it vacated before the light there has been
/// recomputed, which reads as a black object for a frame or two. And a
/// chunk whose lighting has not been computed yet answers 0 everywhere,
/// so anything at the streaming frontier rendered as a silhouette.
/// Neither is a case where darkness is the honest answer: the thing is
/// in mid-air, and the air above it is lit.
///
/// Public because the first-person hand asks the same question about the
/// player's own head, and for the same reasons: it is a thing floating
/// in the world that must not turn black because the chunk under it has
/// not been lit yet.
pub fn sampled_light(at: DVec3, light: &LightMap) -> (u8, u8) {
    let (cx, cy, cz) = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
    let sky = light.sky(cx, cy, cz).max(light.sky(cx, cy + 1, cz));
    let block_light = light.block(cx, cy, cz).max(light.block(cx, cy + 1, cz));
    let sky = if light.is_lit(primitive_shared::types::ChunkPos::from_world(at.x, at.z)) {
        sky
    } else {
        MAX_LIGHT
    };
    (sky, block_light)
}

#[allow(clippy::too_many_arguments)]
fn append_cube(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    at: DVec3,
    origin: Vec3,
    block: primitive_shared::types::BlockId,
    scale: f32,
    layers: &FaceLayers,
    light: &LightMap,
) {
    // Light is sampled once at the entity's centre rather than per face.
    // A falling block is in the air and in motion; per-face sampling
    // would make it flicker as it crosses cell boundaries.
    let sample = at + DVec3::splat(f64::from(scale * 0.5));
    // Same split as above: the light comes from the world, the geometry
    // from the frame's origin.
    let origin = (at - origin.as_dvec3()).as_vec3();
    let (cx, cy, cz) = (
        sample.x.floor() as i32,
        sample.y.floor() as i32,
        sample.z.floor() as i32,
    );

    // Take the brightest of the cell it is in and the one above it.
    //
    // Two reasons, both visible. A block only *just* out of the grid can
    // still be sampled in the cell it vacated before the light there has
    // been recomputed, which reads as a black cube for a frame or two.
    // And a chunk whose lighting has not been computed yet answers 0 for
    // every cell -- so sand falling at the streaming frontier rendered
    // as a silhouette. Neither is a case where darkness is the honest
    // answer: the block is in mid-air, and the air above it is lit.
    let sky = light.sky(cx, cy, cz).max(light.sky(cx, cy + 1, cz));
    let block_light = light.block(cx, cy, cz).max(light.block(cx, cy + 1, cz));
    let sky = if light.is_lit(primitive_shared::types::ChunkPos::from_world(sample.x, sample.z)) {
        sky
    } else {
        // Nothing is known about this column at all. Full sky is the
        // better guess than none: the frontier reads slightly bright
        // rather than as a hole.
        MAX_LIGHT
    };

    // A falling drift is as deep in the air as it was on the ground.
    // Drawing it as a full cube would show a block of sand collapsing
    // and an eighth of one landing.
    let height = primitive_shared::types::block_height(block);

    for (face_index, face) in faces().iter().enumerate() {
        let layer = layers.layer_for_face(block, face_index);
        let base = vertices.len() as u32;
        for corner in face.corners.iter() {
            vertices.push(Vertex::new(
                [
                    origin.x + corner[0],
                    origin.y + corner[1] * height,
                    origin.z + corner[2],
                ],
                face_uv(face_index, *corner),
                layer,
                // Full ambient occlusion (3 = unoccluded): an entity is
                // surrounded by air by definition.
                pack_light(sky, block_light, 3, face_index as u8),
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    /// **The twitch, stated as a property.** The glide used to start when a
    /// packet was read and end one server tick later, so a packet five
    /// milliseconds late left the animal standing on its target for a frame
    /// and then covering the next tick's ground in what was left of it --
    /// measured at one stalled frame in eight, on every animal at once.
    /// Snapshots are *made* on a rhythm and *arrive* with the jitter of a
    /// sleeping server thread and a socket read once a frame, so the
    /// schedule has to keep the rhythm and not the arrivals.
    #[test]
    fn a_snapshot_that_arrives_early_or_late_still_leaves_the_body_somewhere_to_go() {
        let interval = Duration::from_millis(50);
        let start = Instant::now();
        // Arrivals wobbling by a third of a tick either way, the way they do
        // between a server on a fifteen-millisecond timer and a client that
        // reads its socket once a frame.
        let wobble = [0i64, 12, -9, 15, -14, 7, -3, 11, -12, 4];
        let mut due = None;
        let mut spans = Vec::new();
        let mut ends = Vec::new();
        for (i, off) in wobble.iter().enumerate() {
            let arrival = start + interval * (i as u32 + 1);
            let arrival = if *off >= 0 {
                arrival + Duration::from_millis(*off as u64)
            } else {
                arrival - Duration::from_millis(off.unsigned_abs())
            };
            let span = playout_span(due, arrival, interval);
            assert!(
                span >= Duration::from_millis(25),
                "snapshot {i} was given {span:?} to cross a whole tick of ground, which is a lurch"
            );
            due = Some(arrival + span);
            spans.push(span);
            ends.push(arrival + span);
        }
        // The schedule tracks the server rather than the arrivals: over ten
        // snapshots the ends stay within a slack of the rhythm they were
        // made on, however the packets were bunched.
        for (i, end) in ends.iter().enumerate() {
            let rhythm = ends[0] + interval * i as u32;
            let off = if *end > rhythm { *end - rhythm } else { rhythm - *end };
            assert!(
                off <= PLAYOUT_SLACK,
                "snapshot {i} was drawn {off:?} off the server's rhythm"
            );
        }
        assert!(
            spans.iter().any(|s| *s != interval),
            "nothing about this test's jitter reached the glide at all"
        );
    }

    /// ...and the same thing end to end: with the snapshots arriving raggedly
    /// the drawn position keeps moving every frame, rather than freezing on
    /// one and jumping on the next.
    #[test]
    fn a_walking_animal_is_drawn_moving_on_every_frame_however_the_packets_land() {
        use primitive_shared::animals::Species;
        use primitive_shared::protocol::Attitude;
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let interval = Duration::from_millis(50);
        let start = Instant::now();
        let at = |tick: u64| {
            vec![EntityState {
                id: 1,
                kind: EntityKind::Animal {
                    species: Species::Deer,
                    yaw: 0.0,
                    hurt: 0.0,
                    attitude: Attitude::Easy,
                    growth: u8::MAX,
                    tack: 0,
                },
                // Four blocks a second, along +X.
                x: tick as f64 * 0.2,
                y: 20.0,
                z: 0.0,
            }]
        };
        // A ragged network against a steady sixty frames a second: the
        // packets land where they land and the frames are drawn on their own
        // clock, which is the arrangement the game is in.
        let wobble = [0u64, 13, 4, 16, 2, 9, 15, 1, 11, 6];
        let arrivals: Vec<Instant> = wobble
            .iter()
            .enumerate()
            .map(|(i, off)| start + interval * (i as u32 + 1) + Duration::from_millis(*off))
            .collect();
        let mut next = 0;
        let mut drawn: Vec<(Instant, f64)> = Vec::new();
        let mut frame = start;
        while frame < *arrivals.last().expect("arrivals") {
            while next < arrivals.len() && arrivals[next] <= frame {
                entities.apply_snapshot_at(next as u64 + 1, &at(next as u64 + 1), arrivals[next]);
                next += 1;
            }
            if let Some(entity) = entities.entities.get(&1) {
                drawn.push((frame, entity.drawn(frame).x));
            }
            frame += Duration::from_millis(16);
        }
        // Past the first two snapshots -- an entity seen for the first time
        // is placed, not eased -- every frame moves it on.
        let moving = &drawn[drawn.len() / 2..];
        for pair in moving.windows(2) {
            let step = pair[1].1 - pair[0].1;
            assert!(
                step > 0.01,
                "the deer was drawn at {:.3} and then at {:.3}: a frame it stood still on",
                pair[0].1,
                pair[1].1
            );
        }
    }

    /// **The walk phase does not jump when a packet lands.** The legs are
    /// swung from distance covered, and the distance is billed a glide at a
    /// time; billing a whole glide for one that was only part drawn skips
    /// whatever was left of it, and a leg that skips once a packet is the
    /// strobe the whole distance-driven gait exists to avoid.
    #[test]
    fn the_walk_phase_is_continuous_across_a_server_update() {
        use primitive_shared::animals::Species;
        use primitive_shared::protocol::Attitude;
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let interval = Duration::from_millis(50);
        let start = Instant::now();
        let at = |tick: u64| {
            vec![EntityState {
                id: 1,
                kind: EntityKind::Animal {
                    species: Species::Deer,
                    yaw: 0.0,
                    hurt: 0.0,
                    attitude: Attitude::Easy,
                    growth: u8::MAX,
                    tack: 0,
                },
                x: tick as f64 * 0.2,
                y: 20.0,
                z: 0.0,
            }]
        };
        let mut worst: f32 = 0.0;
        let mut before = 0.0f32;
        for tick in 1..=8u64 {
            let arrival = start + interval * tick as u32;
            // What the last frame before the packet was read drew, and
            // what the first frame after it draws: the same instant, either
            // side of the update.
            if tick > 2 {
                let entity = entities.entities.get(&1).expect("the deer");
                before = entity.gait(arrival).0;
            }
            entities.apply_snapshot_at(tick, &at(tick), arrival);
            if tick > 2 {
                let entity = entities.entities.get(&1).expect("the deer");
                let after = entity.gait(arrival).0;
                worst = worst.max((after - before).abs());
            }
        }
        // A millimetre of walking: a twentieth of what one frame of a walk
        // covers, and nothing an eye can find.
        assert!(
            worst < 0.001,
            "the walk phase jumped {worst:.4} blocks when a snapshot landed"
        );
    }

    /// **A turn to the right leans right, and a turn to the left leans
    /// left**, measured on the drawn geometry and not on the number that
    /// makes it -- because the number is what was wrong. `Entity::turning`
    /// is positive when the yaw grows, which is a turn to the animal's
    /// right; the pose's roll takes the top of the model to its left. The
    /// two were multiplied together, so every animal in the world banked
    /// *out* of every corner: "наклоняются в одну сторону", whichever way
    /// they went.
    #[test]
    fn a_turn_to_the_right_leans_the_body_right_and_a_turn_to_the_left_leans_it_left() {
        use primitive_shared::animals::Species;
        use primitive_shared::protocol::Attitude;
        // The top of the drawn body, across the animal: at a yaw of nought
        // it faces +X and its right hand is +Z (`Camera::right_horizontal`),
        // so a bank to its right takes the top of it toward +Z.
        let tops_at = |turn: f32| {
            let mut entities = Entities::default();
            entities.set_tick_rate(20.0);
            let at = |yaw: f32, x: f64| {
                vec![EntityState {
                    id: 1,
                    kind: EntityKind::Animal {
                        species: Species::Deer,
                        yaw,
                        hurt: 0.0,
                        attitude: Attitude::Easy,
                        growth: u8::MAX,
                        tack: 0,
                    },
                    x,
                    y: 20.0,
                    z: 0.0,
                }]
            };
            // Running, and turning `turn` radians in one server tick.
            entities.apply_snapshot(1, &at(0.0, 0.0));
            entities.apply_snapshot(2, &at(turn, 0.3));
            // A whole second of easing, so the bank has fully come over.
            entities.tick(1.0);
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            entities.build_meshes_into(
                Vec3::ZERO,
                &FaceLayers::empty_for_test(),
                &LightMap::new(),
                None,
                &mut vertices,
                &mut indices,
                &mut Vec::new(),
                &mut Vec::new(),
            );
            assert!(!vertices.is_empty(), "no deer was drawn");
            // The mean z of everything drawn above the animal's middle.
            let high: Vec<&Vertex> = vertices.iter().filter(|v| v.position[1] > 20.0).collect();
            let sum: f32 = high.iter().map(|v| v.position[2]).sum();
            sum / high.len() as f32
        };
        let straight = tops_at(0.0);
        let right = tops_at(0.3);
        let left = tops_at(-0.3);
        assert!(
            right > straight + 0.01,
            "turning right, the top of the deer sat at {right:.3} against {straight:.3} running straight: it leant the wrong way"
        );
        assert!(
            left < straight - 0.01,
            "turning left, the top of the deer sat at {left:.3} against {straight:.3} running straight: it leant the wrong way"
        );
    }

    /// **A blow does not always tip an animal the same way.** The side a hit
    /// came from is not on the wire, so the flinch used to roll every animal
    /// the same way every time -- a herd being driven all leaning together.
    /// The side is drawn per blow on the client that sees it.
    #[test]
    fn a_second_blow_does_not_always_throw_an_animal_the_same_way() {
        use primitive_shared::animals::Species;
        use primitive_shared::protocol::Attitude;
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let mut sides = Vec::new();
        let mut tick = 0u64;
        // Twelve animals, each hit once: what a player driving a herd sees.
        for id in 1..=12u64 {
            let at = |hurt: f32| {
                vec![EntityState {
                    id,
                    kind: EntityKind::Animal {
                        species: Species::Deer,
                        yaw: 0.0,
                        hurt,
                        attitude: Attitude::Easy,
                        growth: u8::MAX,
                        tack: 0,
                    },
                    x: 0.0,
                    y: 20.0,
                    z: 0.0,
                }]
            };
            tick += 1;
            entities.apply_snapshot(tick, &at(0.0));
            tick += 1;
            entities.apply_snapshot(tick, &at(1.0));
            sides.push(entities.entities.get(&id).expect("the deer").flinch_side);
        }
        assert!(
            sides.contains(&1.0) && sides.contains(&-1.0),
            "a dozen animals were all thrown the same way: {sides:?}"
        );
    }

    /// **A diagnostic, not a guard**: plays a scripted animal through the
    /// client's own interpolation on a real clock, with the snapshots
    /// arriving as they do in the game (a frame's worth of jitter on the way
    /// in), and prints, per frame, what the model would be handed.
    ///
    /// ```text
    /// cargo test -p primitive_client --lib what_an_animal_is_handed_frame_by_frame -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a diagnostic: sleeps through three seconds of frames and prints a table"]
    fn what_an_animal_is_handed_frame_by_frame() {
        use primitive_shared::animals::Species;
        use primitive_shared::protocol::Attitude;
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let (mut x, mut z, mut yaw) = (0.0f64, 0.0f64, 0.0f32);
        let mut tick = 0u64;
        let dt = 0.05f32;
        let mut jitter = 7u64;
        let start = Instant::now();
        let mut next_packet = start;
        let mut last_frame = start;
        let mut last_draw = (0.0f64, 0.0f64);
        println!("    t  frame   moved   speed  turning     lean    swing    yaw");
        loop {
            let now = Instant::now();
            let t = now.duration_since(start).as_secs_f32();
            if t > 3.0 {
                break;
            }
            if now >= next_packet {
                let speed = if t < 2.4 { 4.0 } else { 0.0 };
                let turn = if (0.6..1.0).contains(&t) {
                    3.5
                } else if (1.6..2.0).contains(&t) {
                    -3.5
                } else {
                    0.0
                };
                yaw = (yaw + turn * dt).rem_euclid(std::f32::consts::TAU);
                x += f64::from(yaw.cos() * speed * dt);
                z += f64::from(yaw.sin() * speed * dt);
                tick += 1;
                entities.apply_snapshot(
                    tick,
                    &[EntityState {
                        id: 1,
                        kind: EntityKind::Animal {
                            species: Species::Deer,
                            yaw,
                            hurt: 0.0,
                            attitude: Attitude::Easy,
                            growth: u8::MAX,
                            tack: 0,
                        },
                        x,
                        y: 20.0,
                        z,
                    }],
                );
                jitter = jitter.wrapping_mul(6364136223846793005).wrapping_add(1);
                let wobble = (jitter >> 33) % 17;
                next_packet = now + Duration::from_millis(42 + wobble);
            }
            let frame_dt = now.duration_since(last_frame).as_secs_f32();
            last_frame = now;
            entities.tick(frame_dt);
            let entity = entities.entities.get(&1).expect("the deer");
            let at = entity.drawn(now);
            let motion = entity.motion(now);
            let moved = ((at.x - last_draw.0).powi(2) + (at.z - last_draw.1).powi(2)).sqrt();
            last_draw = (at.x, at.z);
            let pace = if motion.speed < 0.6 { 0.0 } else { (motion.speed / 4.0).clamp(0.35, 1.0) };
            let lean = (motion.turning * 0.16 * pace).clamp(-0.2, 0.2);
            let swing = (motion.walked * 2.0).sin() * 0.7 * pace;
            println!(
                "{t:5.2} {frame_dt:6.4} {moved:7.4} {:7.3} {:8.3} {lean:8.3} {swing:8.3} {:6.2}",
                motion.speed,
                motion.turning,
                entity.drawn_yaw(now),
            );
            std::thread::sleep(Duration::from_millis(16));
        }
    }


    #[test]
    fn a_knife_set_down_points_the_way_its_player_was_looking() {
        // The facing the server writes is the one a block put down by that
        // look gets (`Facing::toward_viewer`); lying flat, the corner-to-
        // corner line a tool is drawn along -- the picture's up and right
        // together -- has to run along the look, point away.
        use primitive_shared::types::{faced, Facing, BLOCK_SET_DOWN};
        for quarter in 0..4 {
            let look = quarter as f32 * std::f32::consts::FRAC_PI_2;
            let yaw = set_down_yaw(faced(BLOCK_SET_DOWN, Facing::toward_viewer(look)));
            // `append_tipped`: lying, the picture's up is (sin, -cos) on
            // (x, z) and its right is (cos, sin).
            let (sin, cos) = yaw.sin_cos();
            let along = Vec3::new(sin + cos, 0.0, sin - cos).normalize();
            let wanted = Vec3::new(look.cos(), 0.0, look.sin());
            assert!(
                along.dot(wanted) > 0.999,
                "looking along {wanted:?} laid the knife along {along:?}"
            );
        }
    }

    #[test]
    fn a_thing_set_down_is_drawn_where_it_lies_and_nothing_is_drawn_for_an_empty_hand() {
        use primitive_shared::types::{faced, Facing, BLOCK_BREAD, BLOCK_SET_DOWN};
        let layers = FaceLayers::empty_for_test();
        let light = LightMap::new();
        let (mut v, mut i, mut iv, mut ii) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let lying = faced(BLOCK_SET_DOWN, Facing::South);
        build_set_down_into(
            [((4, 20, -3), lying, BLOCK_BREAD)].into_iter(),
            Vec3::ZERO,
            &layers,
            &light,
            None,
            &mut v,
            &mut i,
            &mut iv,
            &mut ii,
        );
        assert!(!v.is_empty(), "a loaf set down was not drawn");
        for vertex in &v {
            let [x, y, z] = vertex.position;
            assert!(
                (4.0..=5.0).contains(&x) && (20.0..=20.5).contains(&y) && (-3.0..=-2.0).contains(&z),
                "a loaf set down in (4, 20, -3) is drawn at {:?}",
                vertex.position
            );
        }
    }

    /// Two snapshots of one animal, `interval` apart, with the facing
    /// changing between them.
    fn turning(from: f32, to: f32) -> (Entities, Instant) {
        use primitive_shared::animals::Species;
        let mut entities = Entities::default();
        let at = |yaw: f32| {
            vec![EntityState {
                id: 1,
                kind: EntityKind::Animal { species: Species::Deer, yaw, hurt: 0.0, attitude: primitive_shared::protocol::Attitude::Easy, growth: u8::MAX, tack: 0 },
                x: 0.0,
                y: 20.0,
                z: 0.0,
            }]
        };
        entities.apply_snapshot(1, &at(from));
        entities.apply_snapshot(2, &at(to));
        // Pin the interval so the eased facing can be read at known
        // points along it, the way the movement tests pin theirs.
        let now = Instant::now();
        let entity = entities.entities.get_mut(&1).expect("the deer");
        entity.interval = Duration::from_millis(200);
        // The glide, not the arrival: `plays_from` and `span` are what the
        // drawing runs on now (see `Entity::plays_from`).
        entity.span = Duration::from_millis(200);
        entity.plays_from = now;
        (entities, now)
    }

    /// One snapshot of one deer, at a given hurt level.
    fn deer_hurt(hurt: f32) -> Vec<EntityState> {
        use primitive_shared::animals::Species;
        vec![EntityState {
            id: 1,
            kind: EntityKind::Animal { species: Species::Deer, yaw: 0.0, hurt, attitude: primitive_shared::protocol::Attitude::Easy, growth: u8::MAX, tack: 0 },
            x: 4.0,
            y: 31.0,
            z: -2.0,
        }]
    }

    /// **A dropped thing lies on the floor, not a hand above it.**
    ///
    /// The lift that makes a drawing *stand* on the ground is the height
    /// of its own picture -- a pick head drawn in the middle of its tile
    /// is raised most of its own height so its lowest texel touches. Tip
    /// that same drawing flat and the height is gone while the lift is
    /// not, and the item hovers: a player reported it as "things fly in
    /// the air instead of falling", which is exactly what it looks like.
    ///
    /// So the lift belongs to the *pose* and eases with it. This checks
    /// the two ends and the middle, because a fault here is a smooth
    /// wrong number rather than a crash.
    #[test]
    fn a_settled_item_sits_on_the_ground_rather_than_hovering_over_it() {
        // A drawing with a margin under it, which is the case the
        // lift exists for: a pick head floating in the middle of its
        // own tile rather than drawn to the bottom edge of it.
        use crate::engine::item_model::{ItemModel, Quad};
        let model = ItemModel {
            quads: vec![Quad {
                corners: [
                    [-0.2, -0.1, 0.0],
                    [0.2, -0.1, 0.0],
                    [0.2, 0.3, 0.0],
                    [-0.2, 0.3, 0.0],
                ],
                uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            }],
        };
        let scale = dropped_scale(&model);
        let upright = dropped_lift(&model, scale);
        let lying = -ITEM_SCALE * 0.5;
        // Upright, the drawing is held up by its own margin...
        assert!(
            upright > lying,
            "an upright drawing should be lifted above the box floor"
        );
        // ...and flat it is not, or it floats.
        for (tip, want) in [(0.0, upright), (1.0, lying)] {
            let eased = upright * (1.0 - tip) + lying * tip;
            assert!(
                (eased - want).abs() < 1e-6,
                "at tip {tip} the lift is {eased}, not {want}"
            );
        }
        let middle = upright * 0.5 + lying * 0.5;
        assert!(
            middle < upright && middle > lying,
            "half tipped should be half lifted"
        );
    }

    #[test]
    fn an_animal_bleeds_once_per_blow_and_not_once_per_snapshot() {
        // **The flash is a level that decays, so it arrives above zero
        // in several snapshots running.** Blood drawn whenever it is
        // non-zero is a fountain for as long as the flash lasts; the
        // rising edge is the one snapshot that is news, and this is
        // what says so.
        let mut entities = Entities::default();
        entities.apply_snapshot(1, &deer_hurt(0.0));
        assert!(entities.take_blows().is_empty(), "an unhurt deer bled");

        entities.apply_snapshot(2, &deer_hurt(1.0));
        let blows = entities.take_blows();
        assert_eq!(blows.len(), 1, "the blow was missed or counted twice");
        // At the animal's own centre, which is the point the server
        // sends and the point the model is built about.
        assert_eq!(blows[0], Vec3::new(4.0, 31.0, -2.0));

        // The flash fading is not a second blow...
        entities.apply_snapshot(3, &deer_hurt(0.6));
        entities.apply_snapshot(4, &deer_hurt(0.2));
        assert!(entities.take_blows().is_empty(), "the fading flash bled");

        // ...and being hit again while still flashing is.
        entities.apply_snapshot(5, &deer_hurt(1.0));
        assert_eq!(entities.take_blows().len(), 1, "a second blow was swallowed");
    }

    #[test]
    fn an_animal_that_walks_into_view_already_hurt_does_not_bleed() {
        // There is no earlier snapshot to have risen from, and the
        // alternative -- treating the first sight of a hurt animal as a
        // blow -- means every wounded deer that comes over a ridge
        // sprays blood at the horizon.
        let mut entities = Entities::default();
        entities.apply_snapshot(1, &deer_hurt(1.0));
        assert!(entities.take_blows().is_empty());
    }

    #[test]
    fn an_animal_turns_between_snapshots_rather_than_at_them() {
        // The server sends a facing five times a second and turns an
        // animal at about two hundred degrees a second, so a fleeing
        // deer swings forty degrees between two of them. Drawn as it
        // arrives, that is one jump per snapshot -- a body sliding
        // smoothly while the direction it faces ratchets, which reads as
        // the animal skidding.
        let (entities, start) = turning(0.0, 1.0);
        let entity = entities.entities.get(&1).expect("the deer");
        let quarter = entity.drawn_yaw(start + Duration::from_millis(50));
        let half = entity.drawn_yaw(start + Duration::from_millis(100));
        assert!(
            (0.05..half).contains(&quarter),
            "it snapped instead of turning: {quarter} then {half}"
        );
        assert!(
            (entity.drawn_yaw(start + Duration::from_millis(200)) - 1.0).abs() < 1e-3,
            "it never arrived"
        );
    }

    #[test]
    fn a_dropped_item_never_sinks_into_the_ground_it_rests_on() {
        // The server rests an item exactly on the surface that stopped
        // it -- centre half a side up, bottom face touching. So the bob
        // has to be a *hover*: any downward part of it puts the block
        // inside the floor, which on a dark or matching surface is a
        // block the player cannot see.
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        for n in 0..400 {
            let bob = item_bob(n as f32 * 0.031);
            lowest = lowest.min(bob);
            highest = highest.max(bob);
        }
        assert!(lowest >= 0.0, "an item was drawn {lowest} below its rest");
        assert!(
            highest > ITEM_BOB * 0.9,
            "the bob barely moves: {highest} of {ITEM_BOB}"
        );
    }

    /// A sprite model with a chosen margin: `rows` of a 16-tall tile,
    /// solid where the character is a hash.
    fn sprite(rows: &[&str]) -> crate::engine::item_model::ItemModel {
        let width = rows[0].len();
        let mut solid = Vec::with_capacity(width * rows.len());
        for row in rows {
            solid.extend(row.chars().map(|c| c == '#'));
        }
        crate::engine::item_model::ItemModel::from_mask(&solid, width, rows.len())
    }

    #[test]
    fn a_dropped_sprite_stands_on_the_ground_whatever_margin_its_picture_has() {
        // **The bug this is here for**, and it is the bob's own
        // invariant one layer up. `item_bob` guarantees an item is never
        // drawn below where it rests; that guarantee was being made
        // about the *frame* of the picture, and what a player sees is
        // the drawing inside it. Ash reaches the bottom edge of its
        // tile and stood five centimetres inside the floor; a flint
        // pick head is drawn in the middle of its tile and hovered two
        // thirds of its own height clear of it.
        //
        // What has to hold, for any picture: the lowest drawn texel
        // lands exactly on the bottom face of the collision box -- which
        // is the surface the server rested the item on.
        for rows in [
            // Drawn to the bottom edge, drawn in the middle, drawn in
            // the top half, and a single texel in a corner.
            &["........", "........", "..####..", "..####..", "..####..", "..####..", "..####..", "..####.."][..],
            &["........", "..####..", "..####..", "..####..", "..####..", "........", "........", "........"][..],
            &["..####..", "..####..", "........", "........", "........", "........", "........", "........"][..],
            &["........", "........", "........", "........", "........", "........", "........", "......#."][..],
        ] {
            let model = sprite(rows);
            let scale = dropped_scale(&model);
            let bottom = dropped_lift(&model, scale) + model.foot() * scale;
            assert!(
                (bottom + ITEM_SCALE * 0.5).abs() < 1e-6,
                "the lowest texel sits {bottom} from the entity's middle,                  and the box's floor is at {}",
                -ITEM_SCALE * 0.5
            );
        }
    }

    #[test]
    fn two_dropped_sprites_are_the_same_size_however_much_of_their_tiles_they_fill() {
        // The other half of "the frame is not the object": one scale for
        // every model hands the artist's margins the job of deciding how
        // big a thing is. A handful of fibre fills its tile and came out
        // three times the size of a flint pick head, which is drawn
        // eight texels by five -- a difference nobody chose and nobody
        // could explain.
        let wide = sprite(&["########", "########", "########", "########",
                            "########", "########", "########", "########"]);
        let small = sprite(&["........", "........", "...##...", "...##...",
                             "........", "........", "........", "........"]);
        let size = |model: &crate::engine::item_model::ItemModel| {
            let [w, h] = model.silhouette();
            w.max(h) * dropped_scale(model)
        };
        assert!((size(&wide) - size(&small)).abs() < 1e-6,
                "{} against {}", size(&wide), size(&small));
        // ...and neither of them is drawn bigger than the biggest was
        // before: this evens the row up, it does not grow it.
        assert!((size(&wide) - ITEM_DRAWN).abs() < 1e-6);
    }

    #[test]
    fn an_empty_snapshot_clears_everything_at_once() {
        // How removal works now: the server sends the complete nearby
        // set, *including* an empty one the moment the last thing
        // leaves. The old contract made removal a matter of falling
        // silent, and silence is exactly what a full outgoing queue
        // produces -- so a dropped block blinked out and back while
        // chunks were streaming.
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let dropped = EntityState {
            id: 3,
            x: 0.0,
            y: 20.0,
            z: 0.0,
            kind: EntityKind::Item {
                block: primitive_shared::types::BLOCK_DIRT,
                count: 1,
            },
        };
        entities.apply_snapshot(1, &[dropped]);
        assert_eq!(entities.len(), 1);
        entities.apply_snapshot(2, &[]);
        assert!(entities.is_empty(), "an empty snapshot left something behind");
    }

    #[test]
    fn a_gap_in_the_snapshots_does_not_blink_a_dropped_block_out() {
        // Three dropped messages in a row -- what a shared outgoing
        // queue does while terrain is streaming -- must not remove
        // anything. That is what the old 150 ms timeout got wrong.
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        entities.apply_snapshot(
            1,
            &[EntityState {
                id: 3,
                x: 0.0,
                y: 20.0,
                z: 0.0,
                kind: EntityKind::Item {
                    block: primitive_shared::types::BLOCK_DIRT,
                    count: 1,
                },
            }],
        );
        // A tick and a half of silence: well past three ticks and well
        // inside the backstop.
        assert!(
            STALE_AFTER > Duration::from_millis(600),
            "the backstop is short enough for a queue hiccup to clear an entity"
        );
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn a_head_goes_down_to_the_grass_over_time_rather_than_between_two_frames() {
        // **The attitude changes in one step and a head cannot.** The server
        // decides once a second and the snapshot lands five times a second,
        // so a model that drew `Attitude::Feeding` the moment it arrived put
        // the deer's nose from level to the ground inside twenty
        // milliseconds -- which reads as the model breaking, not as an animal
        // eating. See `Entity::head` and `animal_model::HEAD_RATE`.
        use primitive_shared::protocol::Attitude;
        let deer = |attitude: Attitude| EntityState {
            id: 1,
            x: 0.0,
            y: 20.0,
            z: 0.0,
            kind: EntityKind::Animal {
                species: primitive_shared::animals::Species::Deer,
                yaw: 0.0,
                hurt: 0.0,
                attitude,
                growth: u8::MAX,
                tack: 0,
            },
        };
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        entities.apply_snapshot(1, &[deer(Attitude::Easy)]);
        entities.tick(0.05);
        let level = entities.entities[&1].head;
        assert_eq!(level, 0.0, "a deer with nothing to say about it was drawn with its head somewhere");

        entities.apply_snapshot(2, &[deer(Attitude::Feeding)]);
        entities.tick(0.05);
        let part_way = entities.entities[&1].head;
        let grass = crate::logic::animal_model::head_carried(Attitude::Feeding);
        assert!(
            grass < part_way && part_way < 0.0,
            "one frame of a feeding deer put its head at {part_way:.2} of {grass:.2}"
        );
        // ...and it gets there. Half a second is comfortably more than
        // `HEAD_RATE` needs and comfortably less than a thought.
        for _ in 0..10 {
            entities.tick(0.05);
        }
        assert!(
            (entities.entities[&1].head - grass).abs() < 1e-3,
            "half a second on, the head was still at {:.2}",
            entities.entities[&1].head
        );
        // **A deer that comes into view feeding is feeding.** Easing from
        // level would be a fresh entity standing up straight and then
        // lowering its head, every time one crossed the interest radius.
        let mut fresh = Entities::default();
        fresh.set_tick_rate(20.0);
        fresh.apply_snapshot(1, &[deer(Attitude::Feeding)]);
        assert_eq!(fresh.entities[&1].head, grass);
    }

    #[test]
    fn a_burst_of_queued_snapshots_does_not_make_an_animal_bolt() {
        // **The bug that made animals whirl on the spot.** The client
        // drains its whole message queue in one frame, so two snapshots
        // sent a tick apart and delivered together used to be measured
        // as arriving microseconds apart -- and the interval is what the
        // gait divides by. The legs ran at ten times the animal's pace
        // and the eased facing landed instantly, so every snapshot
        // started a fresh turn.
        //
        // Applying them back to back is exactly that burst. What the
        // ticks say is a twentieth of a second apart, and that is what
        // the gait has to believe.
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let walked = |x: f32| EntityState {
            id: 1,
            x: f64::from(x),
            y: 20.0,
            z: 0.0,
            kind: EntityKind::Animal {
                species: primitive_shared::animals::Species::Deer,
                yaw: 0.0,
                hurt: 0.0,
                attitude: primitive_shared::protocol::Attitude::Easy,
                growth: u8::MAX,
                tack: 0,
            },
        };
        entities.apply_snapshot(100, &[walked(0.0)]);
        entities.apply_snapshot(101, &[walked(0.1)]);
        let entity = entities.entities.get(&1).expect("the deer went missing");
        let (_, speed) = entity.gait(Instant::now());
        // A tenth of a block in a twentieth of a second is two blocks a
        // second, which is a deer walking. Anything past a sprint means
        // the interval collapsed.
        assert!(
            speed < 6.0,
            "a queued snapshot burst read as {speed} blocks a second"
        );
    }

    /// A collapse is heard once, when it starts, and a block already in
    /// the air is not heard again every snapshot it is drawn in.
    #[test]
    fn a_block_that_lets_go_is_reported_once_and_a_block_still_falling_is_not() {
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let sand = |id: EntityId, y: f64| EntityState {
            id,
            x: 4.0,
            y,
            z: -2.0,
            kind: EntityKind::FallingBlock { block: BLOCK_SAND },
        };
        entities.apply_snapshot(1, &[sand(1, 30.0)]);
        let gave = entities.take_gave_way();
        assert_eq!(gave.len(), 1, "the moment it let go was missed or counted twice");
        assert_eq!(gave[0].1, BLOCK_SAND);
        assert_eq!(gave[0].0.y, 30.0, "the sound is at the cell that let go");

        // The same block, one cell lower: it is falling, not letting go.
        entities.apply_snapshot(2, &[sand(1, 29.0)]);
        assert!(entities.take_gave_way().is_empty(), "a block already in the air let go again");

        // ...and a second column beside it is its own collapse.
        entities.apply_snapshot(3, &[sand(1, 28.0), sand(2, 30.0)]);
        assert_eq!(entities.take_gave_way().len(), 1, "a second column was swallowed");

        // A dropped item is not a collapse, however new it is.
        entities.apply_snapshot(4, &[EntityState {
            id: 9,
            x: 0.0,
            y: 30.0,
            z: 0.0,
            kind: EntityKind::Item { block: BLOCK_SAND, count: 1 },
        }]);
        assert!(entities.take_gave_way().is_empty(), "a dropped stack rumbled");
    }

    #[test]
    fn a_snapshot_that_overtook_its_predecessor_is_dropped() {
        // Applying an older tick would restart the interpolation from a
        // position the animal has already left, which reads as a stutter
        // backwards -- and, because the tick delta would be negative,
        // there is no sensible interval to give it either.
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let at = |x: f32| EntityState {
            id: 1,
            x: f64::from(x),
            y: 20.0,
            z: 0.0,
            kind: EntityKind::Item {
                block: primitive_shared::types::BLOCK_DIRT,
                count: 1,
            },
        };
        entities.apply_snapshot(200, &[at(5.0)]);
        entities.apply_snapshot(199, &[at(0.0)]);
        let where_it_is = |e: &Entities| e.entities.get(&1).expect("gone").current.x;
        assert_eq!(where_it_is(&entities), 5.0, "a stale snapshot was applied");
        // ...and a repeat of the same tick is not two intervals either.
        entities.apply_snapshot(200, &[at(9.0)]);
        assert_eq!(where_it_is(&entities), 5.0);
    }

    #[test]
    fn a_teleport_is_not_a_stride() {
        // An entity that left the interest radius and came back, or one
        // whose id was reused, moves further in one snapshot than
        // anything alive can run. Billing that to the gait spins the
        // legs for a second afterwards.
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        let at = |x: f32| EntityState {
            id: 7,
            x: f64::from(x),
            y: 20.0,
            z: 0.0,
            kind: EntityKind::Animal {
                species: primitive_shared::animals::Species::Hare,
                yaw: 0.0,
                hurt: 0.0,
                attitude: primitive_shared::protocol::Attitude::Easy,
                growth: u8::MAX,
                tack: 0,
            },
        };
        entities.apply_snapshot(10, &[at(0.0)]);
        entities.apply_snapshot(11, &[at(400.0)]);
        let entity = entities.entities.get(&7).expect("gone");
        let (_, speed) = entity.gait(Instant::now());
        assert_eq!(speed, 0.0, "a teleport was billed to the legs");
        // The position still eases to the right place -- a smooth slide
        // beats a snap either way.
        assert_eq!(entity.current.x, 400.0);
    }

    #[test]
    fn a_turn_past_due_east_takes_the_short_way() {
        // **The failure this is guaranteed to hit.** An animal going
        // from 350 degrees to 10 has turned twenty; interpolating the
        // numbers directly spins it the other three hundred and forty,
        // and every animal in the world crosses that line.
        use std::f32::consts::TAU;
        let (entities, start) = turning(TAU - 0.15, 0.15);
        let entity = entities.entities.get(&1).expect("the deer");
        let midway = entity.drawn_yaw(start + Duration::from_millis(100));
        // Halfway round the short way is the wrap itself: either just
        // under a full turn or just over zero.
        assert!(
            !(0.02..=TAU - 0.02).contains(&midway),
            "it span the long way round: {midway}"
        );
    }

    use super::*;
    // Only the tests name a species now: the drawing code takes whatever
    // the snapshot carried and hands it straight to the model table.
    use primitive_shared::animals::Species;
    use primitive_shared::types::BLOCK_SAND;


    fn state(id: EntityId, y: f32) -> EntityState {
        EntityState {
            id,
            kind: EntityKind::FallingBlock { block: BLOCK_SAND },
            x: 1.0,
            y: f64::from(y),
            z: 2.0,
        }
    }

    /// Lowest vertex of the drawn geometry -- the entity's own y, since
    /// the cube is built upward from its origin.
    fn drawn_y(entities: &Entities) -> f32 {
        let (vertices, _) = entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min)
    }

    #[test]
    fn a_new_entity_appears_where_it_is() {
        // Not eased in from somewhere else -- a block that starts
        // falling must not appear to fly in from the origin.
        let mut entities = Entities::default();
        entities.apply_snapshot(3, &[state(1, 40.0)]);
        assert_eq!(drawn_y(&entities), 40.0);
    }

    /// **What a far-off animal or falling block looks like is what it
    /// looks like at home.** The glide between two snapshots was an `f32` in
    /// the world, so a deer a million blocks out was drawn stepping across a
    /// grid of sixteenths between snapshots -- and ten million out, a whole
    /// block at a time. Now the glide is `f64` and the mesh is measured from
    /// the frame's origin before anything is narrowed: the same bodies, the
    /// same moment of the glide, the same origin a few blocks off, have to
    /// come out as the same vertices to a tenth of a millimetre.
    #[test]
    fn a_body_far_from_zero_is_drawn_exactly_as_it_is_at_home() {
        let draw = |base: DVec3| {
            let mut entities = Entities::default();
            let at = |x: f64| {
                vec![
                    EntityState { id: 1, kind: EntityKind::Animal { species: primitive_shared::animals::Species::Deer, yaw: 0.4, hurt: 0.0, attitude: primitive_shared::protocol::Attitude::Easy, growth: u8::MAX, tack: 0 }, x: base.x + x, y: base.y + 21.0, z: base.z + 3.3 },
                    EntityState { id: 2, kind: EntityKind::FallingBlock { block: BLOCK_SAND }, x: base.x + 5.0, y: base.y + 25.0 - x, z: base.z + 1.0 },
                ]
            };
            entities.apply_snapshot(1, &at(2.31));
            entities.apply_snapshot(2, &at(2.38));
            for entity in entities.entities.values_mut() {
                entity.interval = Duration::from_secs(1000);
                entity.span = Duration::from_secs(1000);
                entity.plays_from = Instant::now() - Duration::from_secs(370);
            }
            entities.tick(1.0 / 60.0);
            let origin = (base + DVec3::new(3.0, 20.0, 2.0)).as_vec3();
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            entities.build_meshes_into(origin, &FaceLayers::empty_for_test(), &LightMap::new(), None, &mut vertices, &mut indices, &mut Vec::new(), &mut Vec::new());
            // In an order of their own: the bodies are kept in a hash map,
            // whose order is not a property of anything drawn.
            let mut drawn = vertices.iter().map(|v| Vec3::from(v.position)).collect::<Vec<_>>();
            drawn.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).expect("a number"));
            drawn
        };
        let home = draw(DVec3::ZERO);
        assert!(!home.is_empty(), "nothing was drawn at home");
        for base in [DVec3::new(1_000_000.0, 0.0, -1_000_000.0), DVec3::new(-10_000_000.0, 0.0, 10_000_000.0)] {
            let far = draw(base);
            assert_eq!(far.len(), home.len(), "a different number of vertices at {base}");
            for (i, (a, b)) in home.iter().zip(&far).enumerate() {
                // A tenth of a millimetre: the two runs read the wall clock a
                // few microseconds apart, and the glide is a function of it.
                assert!((*a - *b).abs().max_element() < 1e-4, "vertex {i} at {base}: {a} at home and {b} out there");
            }
        }
    }

    #[test]
    fn movement_is_interpolated_between_snapshots() {
        let mut entities = Entities::default();
        entities.apply_snapshot(4, &[state(1, 40.0)]);
        entities.apply_snapshot(5, &[state(1, 30.0)]);

        // Halfway through the interval between the two snapshots.
        let entity = entities.entities.get_mut(&1).expect("entity");
        entity.interval = Duration::from_millis(100);
        entity.span = Duration::from_millis(100);
        entity.plays_from = Instant::now() - Duration::from_millis(50);
        entities.tick(1.0 / 60.0);

        let y = drawn_y(&entities);
        assert!(y < 40.0 && y > 30.0, "expected an eased position, got {y}");
    }

    #[test]
    fn interpolation_reaches_the_target_rather_than_trailing_behind_it() {
        // Regression. The drawn position used to chase the latest
        // snapshot exponentially, which settles at a constant lag
        // proportional to speed: sand at terminal velocity was drawn a
        // whole block above where the server said it was, the whole way
        // down, and jumped that block when it landed.
        let mut entities = Entities::default();
        entities.apply_snapshot(6, &[state(1, 40.0)]);
        entities.apply_snapshot(7, &[state(1, 39.1)]);

        // One full interval later, it must be exactly at the sample --
        // not merely approaching it.
        let entity = entities.entities.get_mut(&1).expect("entity");
        entity.interval = Duration::from_millis(50);
        entity.span = Duration::from_millis(50);
        entity.plays_from = Instant::now() - Duration::from_millis(50);
        entities.tick(1.0 / 60.0);

        assert!(
            (drawn_y(&entities) - 39.1).abs() < 1e-3,
            "still lagging at {}",
            drawn_y(&entities)
        );
    }

    fn raft_state(id: EntityId, x: f32, vx: f32) -> EntityState {
        EntityState {
            id,
            kind: EntityKind::Raft { yaw: 0.0, vx, vz: 0.0, spin: 0.0, sail: true, sail_angle: 0.0, stroke: 0.0, hurt: 0.0 },
            x: f64::from(x),
            y: 19.88,
            z: 10.0,
        }
    }

    #[test]
    fn taking_and_leaving_the_oars_of_a_raft_under_way_leaves_it_where_it_is_drawn() {
        let id = primitive_shared::protocol::entity_id(primitive_shared::protocol::EntitySource::Raft, 1);
        let chunks = crate::logic::chunk_manager::ChunkManager::new(4);
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        entities.apply_snapshot(1, &[raft_state(id, 10.0, 2.0)]);
        entities.apply_snapshot(2, &[raft_state(id, 10.1, 2.0)]);
        for _ in 0..10 {
            entities.tick(1.0 / 144.0);
            entities.predict(Oars::REST, Wind::CALM, 0.0, &chunks, 1.0 / 144.0);
        }
        let drawn = |entities: &Entities| entities.rafts()[0].now;
        let before = drawn(&entities);
        assert!((before.x - 10.1).abs() > 1e-3, "the raft is drawn at its snapshot, which would prove nothing");
        entities.steer(Some(id));
        assert_eq!(drawn(&entities), before, "taking the oars moved the raft");
        entities.steer(None);
        assert_eq!(drawn(&entities), before, "letting go of the oars moved the raft");
    }

    #[test]
    fn a_click_at_the_sail_from_the_oars_uses_the_raft_and_a_swing_along_it_does_not_strike_it() {
        use primitive_shared::raft::{Body, FREEBOARD, SEAT};
        let id = primitive_shared::protocol::entity_id(primitive_shared::protocol::EntitySource::Raft, 1);
        let body = Body::at_rest(10.0, 19.88, 10.0, 0.0);
        let mut entities = Entities::default();
        entities.apply_snapshot(1, &[raft_state(id, body.x as f32, 0.0)]);
        entities.tick(0.0);
        // Sat at the stern, eyes a metre over the planks, looking at the sail.
        let eye = glam::DVec3::from(body.world_of(SEAT)).as_vec3() + Vec3::Y;
        let (centre, _) = crate::logic::raft_model::rig_box();
        let sail = glam::DVec3::from(body.world_of([centre.x, centre.y - FREEBOARD, centre.z])).as_vec3();
        let dir = (sail - eye).normalize();
        assert!(entities.aimed_raft(eye.as_dvec3(), dir, 4.5).is_some(), "a click at the sail met nothing");
        assert!(entities.aimed_at(eye.as_dvec3(), dir, 4.5).is_none(), "a swing at the sail struck the raft");
        // Stood under the yard, a click at the far shore is not a click on the raft.
        assert!(entities.aimed_raft(sail.as_dvec3(), Vec3::X, 4.5).is_none(), "standing in the rigging, every click used the raft");
    }

    #[test]
    fn a_landing_block_update_removes_the_entity_at_once() {
        // This is what stops a landed block being drawn hovering over
        // the real block it just became.
        let mut entities = Entities::default();
        entities.apply_snapshot(8, &[state(1, 1.4)]);
        assert_eq!(entities.len(), 1);

        // The server places sand in the cell the entity is in.
        entities.on_block_placed(1, 1, 2, BLOCK_SAND);
        assert!(entities.is_empty(), "the landed entity is still drawn");
    }

    #[test]
    fn a_block_going_down_beside_an_animal_does_not_delete_the_animal() {
        // Landing removes a falling block because the block *became*
        // the cell. Nothing else did, and nothing else may be removed
        // for standing near one: a deer in the cell a player is
        // building in, or a dropped stack a falling block comes to rest
        // beside, used to blink out until the next snapshot.
        let mut entities = Entities::default();
        let deer = EntityState {
            id: 2,
            kind: EntityKind::Animal {
                species: Species::Deer,
                yaw: 0.0,
                hurt: 0.0,
                attitude: primitive_shared::protocol::Attitude::Easy,
                growth: u8::MAX,
                tack: 0,
            },
            x: 1.0,
            y: 1.4,
            z: 2.0,
        };
        let stack = EntityState {
            id: 3,
            kind: EntityKind::Item {
                block: BLOCK_SAND,
                count: 1,
            },
            x: 1.0,
            y: 1.4,
            z: 2.0,
        };
        entities.apply_snapshot(8, &[state(1, 1.4), deer, stack]);
        assert_eq!(entities.len(), 3);

        entities.on_block_placed(1, 1, 2, BLOCK_SAND);
        assert_eq!(
            entities.len(),
            2,
            "the landing took the animal or the stack with it"
        );
    }

    #[test]
    fn a_block_update_somewhere_else_leaves_the_entity_alone() {
        let mut entities = Entities::default();
        entities.apply_snapshot(9, &[state(1, 20.0)]);
        entities.on_block_placed(1, 5, 2, BLOCK_SAND); // far below
        entities.on_block_placed(9, 20, 9, BLOCK_SAND); // another column
        entities.on_block_placed(1, 20, 2, BLOCK_AIR); // a break, not a landing
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn an_entity_missing_from_a_snapshot_disappears_immediately() {
        // Each snapshot is the complete nearby set, so absence is
        // information -- waiting out a timeout would leave a ghost.
        let mut entities = Entities::default();
        entities.apply_snapshot(10, &[state(1, 40.0), state(2, 30.0)]);
        assert_eq!(entities.len(), 2);
        entities.apply_snapshot(11, &[state(2, 29.0)]);
        assert_eq!(entities.len(), 1, "entity 1 should be gone");
    }

    #[test]
    fn silence_eventually_clears_the_last_entity() {
        // The server sends nothing at all once no entity is near, so
        // this timeout is the only thing that clears the final one.
        let mut entities = Entities::default();
        entities.apply_snapshot(12, &[state(1, 40.0)]);
        entities.last_snapshot = Some(Instant::now() - Duration::from_secs(5));
        entities.tick(0.016);
        assert!(entities.is_empty(), "stale entity was not dropped");
    }

    #[test]
    fn a_falling_block_is_a_closed_cube() {
        let mut entities = Entities::default();
        entities.apply_snapshot(13, &[state(1, 10.0), state(2, 20.0)]);
        let (vertices, indices) =
            entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        assert_eq!(vertices.len(), 2 * 24, "6 faces x 4 corners per entity");
        assert_eq!(indices.len(), 2 * 36);
    }

    fn animal(id: EntityId, species: primitive_shared::animals::Species, x: f32) -> EntityState {
        EntityState {
            id,
            kind: EntityKind::Animal {
                species,
                yaw: 0.0,
                hurt: 0.0,
                attitude: primitive_shared::protocol::Attitude::Easy,
                growth: u8::MAX,
                tack: 0,
            },
            x: f64::from(x),
            y: 20.0,
            z: 0.0,
        }
    }

    #[test]
    fn an_animal_is_the_model_the_table_describes() {
        let mut entities = Entities::default();
        entities.apply_snapshot(14, &[animal(1, Species::Deer, 0.0)]);
        let (vertices, indices) =
            entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        // One box per part, six faces each, four corners a face -- see
        // `logic::animal_model`, which is where the parts are listed.
        let parts = crate::logic::animal_model::parts(Species::Deer).len();
        assert_eq!(vertices.len(), parts * 6 * 4);
        assert_eq!(indices.len(), parts * 6 * 6);
    }

    #[test]
    fn a_bigger_animal_is_drawn_bigger() {
                let extent = |species| {
            let mut entities = Entities::default();
            entities.apply_snapshot(15, &[animal(1, species, 0.0)]);
            let (vertices, _) =
                entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
            let top = vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            let bottom = vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            top - bottom
        };
        assert!(
            extent(Species::Deer) > extent(Species::Hare),
            "a deer is no taller than a hare"
        );
    }

    #[test]
    fn a_swing_finds_the_animal_it_is_pointed_at_and_no_other() {
                let mut entities = Entities::default();
        entities.apply_snapshot(16, &[
            animal(1, Species::Deer, 4.0),
            animal(2, Species::Boar, 9.0),
        ]);
        entities.tick(0.016);

        // Down the +X axis from the origin at the animals' height: the
        // near one takes it.
        let eye = Vec3::new(0.0, 20.0, 0.0);
        let along_x = Vec3::new(1.0, 0.0, 0.0);
        assert_eq!(entities.aimed_at(eye.as_dvec3(), along_x, 12.0).map(|(id, _)| id), Some(1));
        // Out of range is a miss rather than the far one.
        assert!(entities.aimed_at(eye.as_dvec3(), along_x, 2.0).is_none());
        // ...and so is looking the other way.
        assert!(entities.aimed_at(eye.as_dvec3(), -along_x, 12.0).is_none());
    }

    #[test]
    fn a_dropped_block_is_not_something_you_can_swing_at() {
        // Only animals. A ray that stopped at a dropped stack would mean
        // hitting your own mining output instead of the wall behind it.
        let mut entities = Entities::default();
        entities.apply_snapshot(17, &[EntityState {
            id: 1,
            kind: EntityKind::Item {
                block: BLOCK_SAND,
                count: 4,
            },
            x: 3.0,
            y: 20.0,
            z: 0.0,
        }]);
        entities.tick(0.016);
        assert!(entities
            .aimed_at((Vec3::new(0.0, 20.0, 0.0)).as_dvec3(), Vec3::new(1.0, 0.0, 0.0), 12.0)
            .is_none());
    }

    #[test]
    fn no_entities_means_no_geometry() {
        let entities = Entities::default();
        let (vertices, indices) =
            entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        assert!(vertices.is_empty() && indices.is_empty());
    }

    /// **A dropped barrel is a barrel standing on the ground.** It was the
    /// sprite cut from the barrel's icon, lying flat -- "у предметов нету 3д
    /// модели только блок или текстура". Dropped, it is every quad of the
    /// model the world stands it as, its lowest point on the floor of the box
    /// the server moves (never under it, and above it only by the hover), and
    /// as long along its longest side as a dropped sprite is.
    #[test]
    fn a_dropped_block_with_a_model_of_its_own_stands_on_its_floor_as_that_model() {
        use primitive_shared::types::BLOCK_BARREL;
        let item = || EntityState {
            id: 9,
            kind: EntityKind::Item { block: BLOCK_BARREL, count: 1 },
            x: 3.5,
            y: 20.15,
            z: 0.5,
        };
        let mut entities = Entities::default();
        entities.apply_snapshot(1, &[item()]);
        entities.apply_snapshot(2, &[item()]);
        let (vertices, indices) = entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        let (mut model, mut model_indices) = (Vec::new(), Vec::new());
        crate::engine::mesh::carried_model(BLOCK_BARREL, &FaceLayers::empty_for_test(), &mut model, &mut model_indices);
        assert_eq!(vertices.len(), model.len(), "a dropped barrel is not drawn as its model");
        assert_eq!(indices.len(), model_indices.len());
        let floor = 20.15 - ITEM_SCALE * 0.5;
        let (low, high) = crate::engine::mesh::extent(&vertices);
        assert!(
            low.y >= floor - 1e-3 && low.y <= floor + ITEM_BOB + 1e-3,
            "the barrel's foot is at {}, the floor it rests on at {floor}",
            low.y
        );
        // **The height, not the box round it.** A settled barrel lies at the
        // yaw its id hashes to, and the axis-aligned box of a turned model is
        // wider than the model: the first draft of this line measured that box
        // and read the barrel's diagonal, 0.58, as its size. A barrel's longest
        // side is its height, and a turn about the vertical does not change it.
        assert!(
            ((high.y - low.y) - MODEL_DRAWN).abs() < 1e-3,
            "a dropped barrel is {} tall where its longest side should be {MODEL_DRAWN}",
            high.y - low.y
        );
    }

    /// What a field as full as the server lets it get costs the frame, on the
    /// CPU.
    ///
    /// ```text
    /// cargo test -p primitive_client --release --lib what_a_full_field_costs_a_frame -- --ignored --nocapture
    /// ```
    ///
    /// Two lines, because the frame pays this table twice over: the geometry
    /// (`build_meshes_into`, at `DYNAMIC_REBUILD_HZ`), and `heard`, which the
    /// frame loop asks at every frame rate -- once for the frogs' dangers and
    /// once for the soundscape. No `tick`: it would forget the whole field if
    /// a slow round outlasted `STALE_AFTER`, and every entity reads the same
    /// `Instant::now` without it.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn what_a_full_field_costs_a_frame() {
        use primitive_shared::animals::{Species, MAX_ANIMALS};
        let at = |shift: f32| -> Vec<EntityState> {
            (0..MAX_ANIMALS)
                .map(|i| {
                    let species = Species::ALL[i % Species::ALL.len()];
                    EntityState {
                        id: i as u64 + 1,
                        kind: EntityKind::Animal { species, yaw: i as f32 * 0.4, hurt: 0.0, attitude: primitive_shared::protocol::Attitude::Easy, growth: u8::MAX, tack: 0 },
                        x: f64::from((i % 10) as f32 * 3.0 + shift),
                        y: f64::from(20.0 + species.height() * 0.5),
                        z: f64::from((i / 10) as f32 * 3.0),
                    }
                })
                .collect()
        };
        let mut entities = Entities::default();
        entities.set_tick_rate(20.0);
        entities.apply_snapshot(1, &at(0.0));
        entities.apply_snapshot(2, &at(0.2));
        let layers = FaceLayers::empty_for_test();
        let light = LightMap::new();
        let (mut v, mut i, mut iv, mut ii) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        const FRAMES: usize = 400;
        let (mut build, mut heard) = (Vec::new(), Vec::new());
        for _ in 0..9 {
            let started = Instant::now();
            for _ in 0..FRAMES {
                v.clear();
                i.clear();
                iv.clear();
                ii.clear();
                entities.build_meshes_into(Vec3::ZERO, &layers, &light, None, &mut v, &mut i, &mut iv, &mut ii);
            }
            build.push(started.elapsed().as_secs_f64() * 1e6 / FRAMES as f64);
            let started = Instant::now();
            for _ in 0..FRAMES {
                std::hint::black_box(entities.heard());
            }
            heard.push(started.elapsed().as_secs_f64() * 1e6 / FRAMES as f64);
        }
        let median = |times: &mut Vec<f64>| {
            times.sort_by(f64::total_cmp);
            times[times.len() / 2]
        };
        println!(
            "[entities] {} animals: build {:.1} us a rebuild ({} vertices, {} indices) | heard {:.2} us a call",
            entities.len(),
            median(&mut build),
            v.len(),
            i.len(),
            median(&mut heard)
        );
    }
}
