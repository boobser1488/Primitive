//! Standing on a raft, and rowing one: the client's half of `raft`.
//!
//! ## Three clients look at one raft three ways
//!
//! **The rower** predicts it. Their keys move the raft on the frame they are
//! pressed, through the same `raft::step` the server runs, and the server's
//! raft -- a round trip behind -- only eases the prediction toward itself
//! (`Steering::predict`). A rower who waited for the server would feel every
//! stroke arrive a tenth of a second late, and a rower who took the server's
//! raft as it arrived would be pulled back a stroke's length twenty times a
//! second: the rubber band this module exists not to have.
//!
//! **A passenger** stands on the raft the snapshots draw. They are carried by
//! it from one frame to the next (`Riding::carry_player`), walk about on it
//! with the ordinary collider (`physics::Player::decks`), and tell the server
//! where on the deck they are rather than where in the world
//! (`ClientMessage::Deck`) -- so the server can put them on *its* raft.
//!
//! **Everyone else** sees the passenger on the deck. A remote player's
//! position arrives in the same tick's snapshot as the raft, already worked
//! out from it by the server, so their place on the deck is read off the
//! snapshot's raft and put onto the raft as it is drawn this frame
//! (`deck_place`). Interpolating the rider and the raft separately, the way
//! every other body is, eases the two at different rates and the rider slides
//! about the deck of a raft going in a straight line.

use std::time::{Duration, Instant};

use primitive_shared::protocol::{ClientMessage, EntityId, EntityKind};
use primitive_shared::raft::{self, Body, Oars, Wind};
use primitive_shared::types::BlockId;

/// How far above a deck a body may be and still be riding it.
///
/// A jump. A player who jumps on a moving raft lands on the raft, not in the
/// water behind it, which means they are carried while in the air; higher
/// than a jump is somebody falling past it from a cliff.
pub const ABOARD_ABOVE: f32 = 2.0;
/// How long the rower's raft takes to ease most of the way to the server's,
/// in seconds.
///
/// Long enough that a correction is a drift nobody feels under their feet,
/// short enough that a raft the prediction got wrong -- a shore the client had
/// not loaded, a wind a clock tick apart -- is where the server has it within
/// a second.
pub const CORRECTION_SECONDS: f32 = 0.4;
/// Differences smaller than this, in blocks, are not corrected at all: the
/// prediction and the server always disagree by a little, and chasing that
/// little every frame is a tremble.
pub const CORRECTION_DEAD_ZONE: f32 = 0.02;
/// Past this, in blocks, the prediction is abandoned for the server's raft.
/// Nothing a stroke can do puts the two this far apart; a teleport, or a raft
/// that was stopped by something only the server knew about, can.
pub const SNAP_BEYOND: f32 = 3.0;
/// How often the oars are sent while the rower keeps pulling. The server lets
/// go of oars it has not heard about for a second (`rafts::OARS_TIMEOUT`), so
/// four times inside that.
pub const ROW_RESEND: Duration = Duration::from_millis(250);
/// How often the sail's angle is sent while a hand is dragging it.
///
/// **Slower than the mouse and faster than the eye.** A message per frame is
/// a hundred and forty a second up a socket for a control that takes a second
/// to use; a message per quarter second would draw the yard on everyone
/// else's screen in four steps. At thirty a second the angle is absolute
/// (`ClientMessage::Trim`), so what the drops cost is nothing at all.
pub const TRIM_RESEND: Duration = Duration::from_millis(33);
/// How long this client keeps showing the angle its own hand asked for after
/// the hand comes off it.
///
/// **A round trip and a little.** Dropped the instant the button comes up,
/// the yard would flick back to the angle the last snapshot carried -- which
/// is where it was *before* the drag -- and then forward again when the
/// server's answer landed: a control that springs back at the end of every
/// use. Held for ever instead, a sail somebody else braced would never be
/// seen to move. So it is held exactly as long as the answer takes, and if
/// the server refused the trim (a player who walked away from the mast
/// mid-drag) the yard goes back where it belongs a breath later, which is
/// what a refused prediction is supposed to look like.
pub const TRIM_HOLD: Duration = Duration::from_millis(600);
/// How much of a turn a full sweep of the mouse is worth, in radians per
/// unit of accumulated look -- the same units the camera turns in.
///
/// **The same hand movement that would have turned the head through a right
/// angle braces the yard from square to hard round**, so the gesture is
/// learned from the one the player already has. Any finer and trimming a
/// sail is a chore across the mousepad; any coarser and the angle cannot be
/// set, only thrown.
pub const TRIM_PER_LOOK: f32 = raft::SAIL_MAX_ANGLE / std::f32::consts::FRAC_PI_2;

/// A raft as this client sees it this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaftPose {
    pub id: EntityId,
    /// Where it is drawn and stood on this frame.
    pub now: Body,
    /// Where the latest snapshot put it: the same instant as every remote
    /// player's position in the latest player snapshot.
    pub latest: Body,
}

/// The raft an entity is, at a position.
pub fn body_of(kind: EntityKind, at: glam::DVec3) -> Option<Body> {
    let EntityKind::Raft { yaw, vx, vz, spin, sail, sail_angle, .. } = kind else {
        return None;
    };
    Some(Body { x: at.x, y: at.y as f32, z: at.z, yaw, vx, vz, spin, sail, sail_angle })
}

/// A point standing on a deck, carried from where the deck was to where it is.
pub fn carry(from: &Body, to: &Body, point: glam::DVec3) -> glam::DVec3 {
    glam::DVec3::from(to.world_of(from.local_of(point.to_array())))
}

/// How far a deck turned between two poses, the short way round.
pub fn turned(from: &Body, to: &Body) -> f32 {
    use std::f32::consts::{PI, TAU};
    let delta = (to.yaw - from.yaw).rem_euclid(TAU);
    if delta > PI {
        delta - TAU
    } else {
        delta
    }
}

/// Whether a place in a deck's frame is on it: over the timber, and between
/// the planks and a jump above them.
fn on_deck(local: [f32; 3]) -> bool {
    Body::over_deck(local, raft::DECK_SLACK) && (-0.05..=ABOARD_ABOVE).contains(&local[1])
}

/// The deck a pair of feet is on, this frame, and where on it.
pub fn deck_under(feet: glam::DVec3, decks: &[RaftPose]) -> Option<(EntityId, [f32; 3])> {
    decks.iter().find_map(|pose| {
        let local = pose.now.local_of(feet.to_array());
        on_deck(local).then_some((pose.id, local))
    })
}

/// Where on a raft's deck a remote player the latest snapshot put at `target`
/// is standing, if they are on one.
///
/// Read off the raft *in that snapshot*, which is the raft the server carried
/// them with (`rafts::tick`) -- so it is the place they are standing and not
/// a place smeared by however far the raft has been drawn since.
pub fn deck_place(target: glam::DVec3, decks: &[RaftPose]) -> Option<(EntityId, [f32; 3])> {
    decks.iter().find_map(|pose| {
        let local = pose.latest.local_of(target.to_array());
        on_deck(local).then_some((pose.id, local))
    })
}

/// The oars a pair of movement axes asks for: forward and back is the stroke,
/// left and right is the turn. Both from -1 to 1, a thumb's or a key's.
///
/// **The keys that walk are the keys that row.** A phone has a stick and no
/// spare buttons, and a keyboard's rowing keys would be four more bindings to
/// learn for a thing that is, like walking, a direction.
pub fn oars_from_axes(forward: f32, right: f32, breathless: bool) -> Oars {
    let scale = if breathless { raft::TIRED_STROKE } else { 1.0 };
    Oars { stroke: forward * scale, turn: right * scale }.clamped()
}

/// The raft the local player is rowing, ahead of the server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Steering {
    pub id: EntityId,
    /// Where this client has it: the server's raft plus the strokes the
    /// server has not heard yet.
    pub body: Body,
    /// What the rower's keys are asking for, this frame.
    pub oars: Oars,
}

impl Steering {
    /// Taking the oars of a raft the snapshots have at `server`.
    pub fn new(id: EntityId, server: Body) -> Self {
        Self { id, body: server, oars: Oars::REST }
    }

    /// One frame at the oars: the stroke, then a correction toward the
    /// server's raft as it will be by now.
    ///
    /// `age` is how old the server's raft is -- how long since its snapshot
    /// arrived, and a tick besides -- and the server's raft is carried on by
    /// its own velocity for that long before it is compared. Compared where it
    /// *was*, every correction would pull a raft under way back toward the
    /// stern by the distance it covers in a tick, which is exactly the
    /// rubber band.
    #[allow(clippy::too_many_arguments)]
    pub fn predict<F>(
        &mut self,
        server: Body,
        age: f32,
        oars: Oars,
        trim: Option<f32>,
        wind: Wind,
        // The hour of the world: the sea's circulation is worked out from
        // it on both sides rather than sent (`fluid::current_at`), so a
        // prediction that did not know the hour would drift out of the
        // server's sea and be corrected every tick.
        world_days: f32,
        // The current of the river under the raft, from this client's own
        // generator (`WorldGen::river_current`), for the same reason.
        river: (f32, f32),
        block: &F,
        dt: f32,
    ) where
        F: Fn(i32, i32, i32) -> Option<BlockId>,
    {
        self.oars = oars.clamped();
        // The angle this client's own hand is holding, before the step that
        // pushes against it -- the oars' argument exactly, one control over.
        // Without a hand on it, the server's: another player at the mast is
        // trimming a sail this client only watches.
        self.body.sail_angle = raft::trim_clamped(trim.unwrap_or(server.sail_angle));
        raft::step(&mut self.body, self.oars, wind, world_days, river, block, dt);
        // The water decides the height and the server decides whether the
        // sail is up at all: the client has nothing to predict about either.
        self.body.y = server.y;
        self.body.sail = server.sail;
        if !ease_toward(&mut self.body, server, age, dt) {
            return;
        }
        let blend = 1.0 - (-dt.max(0.0) / CORRECTION_SECONDS).exp();
        self.body.vx += (server.vx - self.body.vx) * blend;
        self.body.vz += (server.vz - self.body.vz) * blend;
        self.body.spin += (server.spin - self.body.spin) * blend;
    }
}

/// Eases a raft drawn ahead of the server toward the server's raft as its own
/// velocity will have carried it by now, `age` seconds after the snapshot.
/// Answers `false` when it was too far out to ease and was put there instead.
///
/// One rule for both ways a raft is drawn ahead (`Steering`, `Glide`), so the
/// rower's raft and the one they are watching settle onto the server the same
/// way and by the same numbers.
fn ease_toward(body: &mut Body, server: Body, age: f32, dt: f32) -> bool {
    let age = age.clamp(0.0, 0.5);
    let (tx, tz) = (server.x + f64::from(server.vx * age), server.z + f64::from(server.vz * age));
    let (ex, ez) = (tx - body.x, tz - body.z);
    let off = ex.hypot(ez);
    if off > f64::from(SNAP_BEYOND) || !body.is_sane() {
        *body = server;
        return false;
    }
    let blend = 1.0 - (-dt.max(0.0) / CORRECTION_SECONDS).exp();
    if off > f64::from(CORRECTION_DEAD_ZONE) {
        body.x += ex * f64::from(blend);
        body.z += ez * f64::from(blend);
    }
    let aim = Body { yaw: server.yaw + server.spin * age, ..server };
    let yaw_off = turned(body, &aim);
    if yaw_off.abs() > 0.01 {
        body.yaw = (body.yaw + yaw_off * blend).rem_euclid(std::f32::consts::TAU);
    }
    true
}

/// A raft nobody on this client is rowing, as it is drawn and stood on: carried
/// on from its last snapshot by the velocity that snapshot gave it, and eased
/// toward where that snapshot says it will be by now.
///
/// **Carried forward, not interpolated behind**, and the difference is what a
/// passenger feels. A raft used to be drawn the way every entity is
/// (`Entity::drawn`): eased from the second-last snapshot to the last over one
/// tick, and then *held* until the next one came. A snapshot a few
/// milliseconds late is invisible on a deer. On a raft it is the deck stopping
/// under the feet and lurching on when the snapshot lands -- and the view is
/// carried by that deck (`Riding::carry_player`), so the whole world stuttered
/// round a passenger who was standing still. Over a real socket
/// (`over_a_socket`), 27 of 239 frames under way covered less than half the
/// distance of the frames around them, and the passenger's view trembled by
/// 3.8 cm, while the rower's predicted raft beside it did neither.
///
/// A raft's velocity is in its snapshot, so its path between two of them is
/// known and not guessed, and it changes smoothly -- thrust, drag, the wind --
/// everywhere but at a shore, where it stops. So the velocity is taken as it
/// comes rather than eased: eased, a raft that had struck a bank would run on
/// into it for a correction's length.
///
/// Rejected: interpolating further behind, a tick or two of buffer, which is
/// the usual cure. It puts every passenger's world another tick in the past,
/// and a snapshot later than the buffer still holds the deck -- a debug server
/// that is making chunks runs late by more than any buffer worth having.
/// Rejected too: predicting with `raft::step` as the rower does. A passenger
/// does not know the rower's turn, and a step without it decelerates between
/// snapshots, to be pulled forward again at every one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    pub body: Body,
}

impl Glide {
    pub fn new(from: Body) -> Self {
        Self { body: from }
    }

    /// One frame: on by the server's velocity, then toward the server's raft as
    /// it will be `age` seconds after its snapshot.
    ///
    /// Not tested against the shore (`raft::fits`). The glide is never more
    /// than a snapshot's age ahead of a raft the server has already stopped --
    /// a tenth of a block -- and it is eased back out of the bank in less time
    /// than it takes to see that it went in.
    pub fn advance(&mut self, server: Body, age: f32, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.body.vx = server.vx;
        self.body.vz = server.vz;
        self.body.spin = server.spin;
        self.body.sail = server.sail;
        // Nothing to predict about a sail nobody here has a hand on, and
        // taken as it comes rather than eased for the reason the velocity is:
        // somebody at that mast braced the yard, and a yard that swung there
        // over half a second would be a control with lag drawn onto it.
        self.body.sail_angle = server.sail_angle;
        self.body.x += f64::from(self.body.vx * dt);
        self.body.z += f64::from(self.body.vz * dt);
        self.body.yaw = (self.body.yaw + self.body.spin * dt).rem_euclid(std::f32::consts::TAU);
        // The waterline settles on the server a step at a time (`raft::settle`),
        // and a deck that took those steps as they came would jolt whoever is
        // standing on it.
        let blend = 1.0 - (-dt / CORRECTION_SECONDS).exp();
        self.body.y += (server.y - self.body.y) * blend;
        ease_toward(&mut self.body, server, age, dt);
    }
}

/// What to tell the server about the local player's feet this frame.
#[derive(Debug, Clone)]
pub enum DeckTransform {
    /// Not on a raft: the ordinary world transform is the caller's to send.
    NotAboard,
    /// On a raft, and nothing about it has changed enough to say.
    Quiet,
    /// On a raft: send this.
    Send(ClientMessage),
}

/// The local player's raft, across frames.
#[derive(Debug, Default)]
pub struct Riding {
    /// The deck the feet were on at the end of the last frame, as it was then.
    aboard: Option<(EntityId, Body)>,
    /// The oars last sent, and when.
    last_row: Option<(Instant, Oars)>,
    /// The sail angle this client's own hand is asking for: which raft, the
    /// angle, and when the hand last moved it. See [`TRIM_HOLD`].
    trim: Option<(EntityId, f32, Instant)>,
    /// The angle last sent, for which raft, and when.
    last_trim: Option<(Instant, EntityId, f32)>,
    /// The deck transform last sent: when, where on the deck, and the look.
    last_deck: Option<(Instant, EntityId, [f32; 3], f32, f32)>,
}

impl Riding {
    /// The raft the feet are on, if any. Only the tests ask: the frame reads
    /// the deck through `carry`, which needs the body as well as the id.
    #[cfg(test)]
    pub fn aboard(&self) -> Option<EntityId> {
        self.aboard.map(|(id, _)| id)
    }

    /// Whether the feet ended the last frame on a deck, which is whether the
    /// view is being carried: the frame rebuilds the moving geometry on every
    /// frame that is (`dynamic_rebuild_due`).
    pub fn is_aboard(&self) -> bool {
        self.aboard.is_some()
    }

    /// The deck the feet are on and the raft it belongs to, as that raft was
    /// drawn on the last frame -- which is where the sail's dial reads its
    /// heading and its trim from (`ui::hud::sail_gauge`).
    pub fn aboard_raft(&self) -> Option<(EntityId, Body)> {
        self.aboard
    }

    /// Before physics: carries the feet, and the view, from where the deck was
    /// last frame to where it is now.
    ///
    /// **Before the collider runs, not after.** Carried afterwards, the
    /// collider would already have tested the feet against a deck that has
    /// moved out from under them, and the first frame a raft pulls away is a
    /// player dropped a centimetre into it and lifted back out -- a shudder at
    /// every stroke.
    ///
    /// The view turns with the deck, so a rider looking at the far shore keeps
    /// looking at it as the raft comes about rather than having it swing
    /// across their screen.
    pub fn carry_player(&mut self, decks: &[RaftPose], position: &mut glam::DVec3, yaw: &mut f32) {
        let Some((id, then)) = self.aboard else {
            return;
        };
        let Some(pose) = decks.iter().find(|pose| pose.id == id) else {
            self.aboard = None;
            return;
        };
        *position = carry(&then, &pose.now, *position);
        *yaw += turned(&then, &pose.now);
        self.aboard = Some((id, pose.now));
    }

    /// After physics: which deck the feet ended up on.
    pub fn settle(&mut self, decks: &[RaftPose], feet: glam::DVec3) {
        self.aboard = deck_under(feet, decks)
            .and_then(|(id, _)| decks.iter().find(|pose| pose.id == id))
            .map(|pose| (pose.id, pose.now));
    }

    /// The raft whose sail is at this player's hand, if any: theirs to brace.
    ///
    /// **At the oars, or standing by the mast** -- the player's own two
    /// cases. The rower reaches the sheets from the stern because a rower is
    /// the one working the raft; anybody else has to walk forward to the
    /// mast, which is what makes a sail something a second person aboard is
    /// *for*. Nothing to trim while the sail is furled.
    ///
    /// The same rule the server checks (`rafts::trim`), which is the point:
    /// the client must not offer a drag the server is going to throw away.
    pub fn sail_at_hand(&self, rowing: Option<EntityId>, feet: glam::DVec3) -> Option<EntityId> {
        let (id, body) = self.aboard?;
        if !body.sail {
            return None;
        }
        (rowing == Some(id) || raft::at_the_sail(body.local_of(feet.to_array()))).then_some(id)
    }

    /// Turns the sail of `raft` by `by` radians and answers where it now is.
    ///
    /// Measured from wherever the yard already is -- this client's own held
    /// angle if it has one, and otherwise the angle the deck under the feet
    /// came with -- so a drag picks the yard up where it was left rather than
    /// snapping it to some middle first.
    pub fn turn_sail(&mut self, raft: EntityId, by: f32, now: Instant) -> f32 {
        let from = match self.trim {
            Some((id, angle, _)) if id == raft => angle,
            _ => self.aboard.filter(|(id, _)| *id == raft).map_or(0.0, |(_, body)| body.sail_angle),
        };
        let angle = raft::trim_clamped(from + if by.is_finite() { by } else { 0.0 });
        self.trim = Some((raft, angle, now));
        angle
    }

    /// The angle this client is holding the sail at, for the frame to draw
    /// and predict with. `None` once the hand has been off it long enough for
    /// the server's answer to have come round -- see [`TRIM_HOLD`].
    pub fn held_trim(&self, now: Instant) -> Option<(EntityId, f32)> {
        self.trim
            .filter(|(_, _, at)| now.saturating_duration_since(*at) < TRIM_HOLD)
            .map(|(id, angle, _)| (id, angle))
    }

    /// The sail angle to send, if it is time to send it: on any change, and
    /// no faster than [`TRIM_RESEND`].
    ///
    /// **Not repeated while nothing is moving**, unlike the oars. The oars
    /// are repeated because the server drops a stroke it has not heard about
    /// for a second, on purpose, so that a client that went away does not row
    /// on by itself; a sail braced and left alone is just a sail, and a raft
    /// whose yard sprang square because its owner stopped touching it would
    /// be a raft nobody could moor rigged.
    pub fn trim_message(&mut self, now: Instant) -> Option<ClientMessage> {
        let (raft, angle) = self.held_trim(now)?;
        let due = match self.last_trim {
            None => true,
            // The raft as well as the angle: braced to the same figure on a
            // second raft, the message still has to go, or the sail the
            // player is actually standing at never moves.
            Some((at, last_raft, last)) => {
                (last_raft != raft || (last - angle).abs() > 1e-3)
                    && now.saturating_duration_since(at) >= TRIM_RESEND
            }
        };
        if !due {
            return None;
        }
        self.last_trim = Some((now, raft, angle));
        Some(ClientMessage::Trim { raft, angle })
    }

    /// The oars to send, if it is time to send them: on any change, and every
    /// `ROW_RESEND` while anyone is pulling.
    pub fn row_message(&mut self, raft: EntityId, oars: Oars, now: Instant) -> Option<ClientMessage> {
        let oars = oars.clamped();
        let due = match self.last_row {
            None => oars.pulling(),
            Some((at, last)) => {
                let changed = (last.stroke - oars.stroke).abs() > 0.05 || (last.turn - oars.turn).abs() > 0.05;
                changed || (oars.pulling() && now.saturating_duration_since(at) >= ROW_RESEND)
            }
        };
        if !due {
            return None;
        }
        self.last_row = Some((now, oars));
        Some(ClientMessage::Row { raft, stroke: oars.stroke, turn: oars.turn })
    }

    /// Where the feet are on the deck, for the server, at the rate and on the
    /// terms the world transform is sent (`maybe_send_transform`): no more
    /// often than `interval`, and not at all while nothing has changed.
    ///
    /// **A rider standing still on a moving raft sends nothing**, because
    /// nothing about them has changed: their place on the deck is the same,
    /// and it is the server that carries them.
    #[allow(clippy::too_many_arguments)]
    pub fn deck_message(
        &mut self,
        feet: glam::DVec3,
        yaw: f32,
        pitch: f32,
        grounded: bool,
        now: Instant,
        interval: Duration,
        sequence: &mut u32,
    ) -> DeckTransform {
        let Some((id, body)) = self.aboard else {
            self.last_deck = None;
            return DeckTransform::NotAboard;
        };
        let local = body.local_of(feet.to_array());
        if let Some((at, last_id, last, last_yaw, last_pitch)) = self.last_deck {
            if now.saturating_duration_since(at) < interval {
                return DeckTransform::Quiet;
            }
            let moved = (0..3).any(|a| (local[a] - last[a]).abs() > 0.01);
            let looked = (yaw - last_yaw).abs() > 0.01 || (pitch - last_pitch).abs() > 0.01;
            if last_id == id && !moved && !looked {
                return DeckTransform::Quiet;
            }
        }
        self.last_deck = Some((now, id, local, yaw, pitch));
        *sequence = sequence.wrapping_add(1);
        DeckTransform::Send(ClientMessage::Deck {
            raft: id,
            x: f64::from(local[0]),
            y: f64::from(local[1]),
            z: f64::from(local[2]),
            yaw,
            pitch,
            on_ground: grounded,
            sequence: *sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use crate::logic::physics::tests::lake_world;
    use crate::logic::physics::Player;

    fn block(chunks: &crate::logic::chunk_manager::ChunkManager) -> impl Fn(i32, i32, i32) -> Option<BlockId> + '_ {
        move |x, y, z| chunks.block_at(x, y, z)
    }

    fn afloat(chunks: &crate::logic::chunk_manager::ChunkManager, x: f32, z: f32, yaw: f32) -> Body {
        raft::launch([f64::from(x), 19.5, f64::from(z)], yaw, &block(chunks)).expect("the lake has room for a raft")
    }

    #[test]
    fn a_player_standing_on_a_moving_raft_moves_with_it_and_does_not_fall_through() {
        let chunks = lake_world();
        let mut body = afloat(&chunks, 5.0, 8.0, 0.2);
        let mut player = Player::new((glam::DVec3::from(body.world_of([0.6, 0.0, -0.3])).as_vec3()).as_dvec3(), 4.3);
        let mut riding = Riding::default();
        let mut yaw = 0.0f32;
        let pose = |body: Body| [RaftPose { id: 1, now: body, latest: body }];
        riding.settle(&pose(body), player.position);
        assert_eq!(riding.aboard(), Some(1), "a player put on the deck is not aboard it");
        let dt = 1.0 / 60.0;
        for frame in 0..150 {
            raft::step(&mut body, Oars { stroke: 1.0, turn: 0.4 }, Wind::CALM, 0.0, (0.0, 0.0), &block(&chunks), dt);
            riding.carry_player(&pose(body), &mut player.position, &mut yaw);
            player.decks = vec![body];
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, dt);
            riding.settle(&pose(body), player.position);
            assert_eq!(riding.aboard(), Some(1), "the rider fell off the deck on frame {frame}");
            assert!(
                (player.position.y - f64::from(body.deck_top())).abs() < 0.02,
                "the rider's feet are at {} and the deck at {} on frame {frame}",
                player.position.y,
                body.deck_top()
            );
        }
        let place = body.local_of(player.position.to_array());
        assert!((place[0] - 0.6).abs() < 0.1 && (place[2] + 0.3).abs() < 0.1, "the rider slid across the deck to {place:?}");
        assert!(body.x > 6.5, "the raft did not move: {body:?}");
        assert!(player.grounded, "a rider standing on a deck is not standing on anything");
        assert!(yaw > 0.05, "the rider's view did not turn with the raft");
    }

    #[test]
    fn a_swimmer_who_swims_up_beside_a_deck_climbs_onto_it() {
        let chunks = lake_world();
        let body = afloat(&chunks, 8.0, 8.0, 0.0);
        let beside = glam::DVec3::from(body.world_of([0.0, -1.0, raft::HALF_WIDTH + 0.1])).as_vec3();
        let mut player = Player::new(beside.as_dvec3(), 4.3);
        player.decks = vec![body];
        for _ in 0..240 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, true, false, 1.0 / 60.0);
        }
        assert!((player.position.y - f64::from(body.deck_top())).abs() < 0.05, "the swimmer is at {} below a deck at {}", player.position.y, body.deck_top());
    }

    /// Runs a body at a deck until its feet are standing on it, and answers
    /// how far the view moved on the frame they arrived and on the frame
    /// before -- the view as it is drawn, the eye less the step it is still
    /// catching up (`Player::view_step_lag`).
    fn boarding(
        chunks: &crate::logic::chunk_manager::ChunkManager,
        player: &mut Player,
        body: Body,
        mut input: impl FnMut(usize) -> (Vec3, bool, bool),
    ) -> (f32, f32) {
        let dt = 1.0 / 60.0;
        let view = |p: &Player| p.eye_position().as_vec3() - Vec3::Y * p.view_step_lag();
        let mut last = view(player);
        let mut before = 0.0;
        for frame in 0..600 {
            let (wish, pressed, held) = input(frame);
            player.decks = vec![body];
            player.update(chunks, &[], wish, Vec3::X, pressed, held, false, dt);
            let now = view(player);
            let step = now.distance(last);
            let local = body.local_of(player.position.to_array());
            if player.grounded && local[1].abs() < 0.02 && Body::over_deck(local, raft::DECK_SLACK) {
                return (step, before);
            }
            before = step;
            last = now;
        }
        panic!("never got onto the deck: feet at {}, deck at {}", player.position, body.deck_top());
    }

    /// The most a frame may add to the one before it: a walking stride's
    /// worth, the most a player's own keys change in a frame.
    const STRIDE: f32 = crate::logic::physics::DEFAULT_MOVE_SPEED / 60.0;

    #[test]
    fn a_swimmer_climbing_onto_a_deck_arrives_where_the_last_stroke_left_them() {
        let chunks = lake_world();
        let body = afloat(&chunks, 8.0, 8.0, 0.0);
        let mut player = Player::new((glam::DVec3::from(body.world_of([0.0, -1.0, raft::HALF_WIDTH + 0.1])).as_vec3()).as_dvec3(), 4.3);
        let (arrived, before) = boarding(&chunks, &mut player, body, |_| (Vec3::ZERO, false, true));
        assert!(arrived - before <= STRIDE, "the view moved {arrived} on the frame the swimmer arrived, {before} the frame before");
    }

    #[test]
    fn a_wader_jumping_onto_a_deck_from_the_shallows_lands_on_it_rather_than_being_lifted() {
        // Stone to 19, a block of water over it: shallow enough to stand in,
        // deep enough to float a raft.
        let chunks = crate::logic::physics::tests::world_of(|y| {
            if y < 19 {
                primitive_shared::types::BLOCK_STONE
            } else if y < 20 {
                primitive_shared::types::BLOCK_WATER
            } else {
                primitive_shared::types::BLOCK_AIR
            }
        });
        let body = afloat(&chunks, 8.0, 8.0, 0.0);
        let beside = body.world_of([0.0, 0.0, raft::HALF_WIDTH + 0.25]);
        let mut player = Player::new((Vec3::new((beside[0]) as f32, 19.0, (beside[2]) as f32)).as_dvec3(), 4.3);
        for _ in 0..30 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let (arrived, before) = boarding(&chunks, &mut player, body, |frame| (Vec3::NEG_Z * 0.3, frame == 0, frame < 20));
        assert!(arrived - before <= STRIDE, "the view moved {arrived} on the frame the wader arrived, {before} the frame before");
    }

    #[test]
    fn stepping_onto_a_deck_from_a_bank_moves_the_view_no_further_than_a_walking_stride() {
        // The lake, with a bank of stone up to 20 along its western edge: the
        // bank's top a hand under the deck's.
        let mut chunks = lake_world();
        let mut chunk = chunks.get(primitive_shared::types::ChunkPos::new(0, 0)).unwrap().clone();
        for x in 0..4 {
            for z in 0..16 {
                for y in 10..20 {
                    chunk.set(x, y, z, primitive_shared::types::BLOCK_STONE);
                }
            }
        }
        chunks.insert(chunk);
        let body = afloat(&chunks, 6.0, 8.0, 0.0);
        assert!(body.deck_top() > 20.0, "the deck is not above the bank: {}", body.deck_top());
        let mut player = Player::new((Vec3::new(2.5, 20.0, 8.0)).as_dvec3(), 4.3);
        for _ in 0..30 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let (arrived, before) = boarding(&chunks, &mut player, body, |_| (Vec3::X, false, false));
        assert!(arrived - before <= STRIDE, "the view moved {arrived} on the frame the walker stepped aboard, {before} the frame before");
    }

    #[test]
    fn another_player_on_a_raft_is_drawn_where_they_stand_on_the_deck_that_is_drawn() {
        let latest = Body::at_rest(10.0, 19.88, 10.0, 0.5);
        let drawn = Body::at_rest(9.6, 19.88, 9.8, 0.45);
        let decks = [RaftPose { id: 9, now: drawn, latest }];
        let standing = [1.0, 0.0, 0.5];
        let target = glam::DVec3::from(latest.world_of(standing)).as_vec3();
        let (id, place) = deck_place(target.as_dvec3(), &decks).expect("a player on the snapshot's deck is on it");
        assert_eq!(id, 9);
        let at = glam::DVec3::from(drawn.world_of(place)).as_vec3();
        assert!(at.distance(glam::DVec3::from(drawn.world_of(standing)).as_vec3()) < 1e-3, "drawn at {at} rather than on the drawn deck");
        assert!(deck_place((Vec3::new(30.0, 19.88, 30.0)).as_dvec3(), &decks).is_none(), "somebody on the shore rides a raft");
    }

    #[test]
    fn the_rowers_raft_eases_toward_the_server_and_only_snaps_when_it_is_far_out() {
        let chunks = lake_world();
        let server = afloat(&chunks, 8.0, 8.0, 0.0);
        let mut steering = Steering::new(3, server);
        steering.body.x += 0.5;
        let before = steering.body.x - server.x;
        steering.predict(server, 0.0, Oars::REST, None, Wind::CALM, 0.0, (0.0, 0.0), &block(&chunks), 1.0 / 60.0);
        let after = steering.body.x - server.x;
        assert!(after < before && after > before * 0.8, "a half-block difference was not eased: {before} -> {after}");
        steering.body.x = server.x + 5.0;
        steering.predict(server, 0.0, Oars::REST, None, Wind::CALM, 0.0, (0.0, 0.0), &block(&chunks), 1.0 / 60.0);
        assert!((steering.body.x - server.x).abs() < 1e-4, "a raft five blocks out was not snapped back");
    }

    #[test]
    fn a_raft_under_way_is_not_pulled_back_toward_where_the_last_snapshot_saw_it() {
        // The server's raft, moving at two blocks a second, as it was a tenth
        // of a second ago -- and the rower's raft where it truly is now.
        let chunks = lake_world();
        let mut server = afloat(&chunks, 6.0, 8.0, 0.0);
        server.vx = 2.0;
        let mut steering = Steering::new(3, server);
        steering.body.x = server.x + 0.2;
        steering.body.vx = 2.0;
        let x = steering.body.x;
        steering.predict(server, 0.1, Oars { stroke: 1.0, turn: 0.0 }, None, Wind::CALM, 0.0, (0.0, 0.0), &block(&chunks), 1.0 / 60.0);
        assert!(steering.body.x >= x, "the rower's raft went backwards: {x} -> {}", steering.body.x);
    }

    #[test]
    fn a_raft_nobody_here_rows_is_drawn_on_through_a_late_snapshot_rather_than_held() {
        // Two blocks a second along x; snapshots every twentieth, and the third
        // comes thirty milliseconds late.
        let dt = 1.0 / 144.0;
        let mut server = Body { vx: 2.0, ..Body::at_rest(10.0, 19.88, 10.0, 0.0) };
        let mut glide = Glide::new(server);
        let tick = 0.05;
        let mut since = 0.0f32;
        let mut steps = Vec::new();
        for frame in 0..144 {
            let due = if frame < 72 { tick } else { tick + 0.03 };
            if since >= due {
                server.x += f64::from(server.vx * since);
                since = 0.0;
            }
            let was = glide.body.x;
            glide.advance(server, since + tick, dt);
            since += dt;
            steps.push(glide.body.x - was);
        }
        let stride = 2.0 * dt;
        for (frame, step) in steps.iter().enumerate().skip(1) {
            assert!(
                (step - f64::from(stride)).abs() < f64::from(stride * 0.25),
                "frame {frame} moved the raft {step} where two blocks a second is {stride}"
            );
        }
    }

    #[test]
    fn the_keys_that_walk_row_and_a_breathless_rower_pulls_weakly() {
        assert_eq!(oars_from_axes(1.0, -1.0, false), Oars { stroke: 1.0, turn: -1.0 });
        let tired = oars_from_axes(1.0, 0.0, true);
        assert!((tired.stroke - raft::TIRED_STROKE).abs() < 1e-6);
        assert_eq!(oars_from_axes(3.0, f32::NAN, false), Oars { stroke: 1.0, turn: 0.0 });
    }

    /// A `Riding` whose feet are standing at `local` on a raft with the sail
    /// up, which is the state every question about the sheets is asked in.
    fn aboard_with_a_sail(local: [f32; 3]) -> (Riding, Body, Vec3) {
        let mut body = Body::at_rest(10.0, 19.88, 10.0, 0.4);
        body.sail = true;
        let feet = glam::DVec3::from(body.world_of(local)).as_vec3();
        let mut riding = Riding::default();
        riding.settle(&[RaftPose { id: 5, now: body, latest: body }], feet.as_dvec3());
        (riding, body, feet)
    }

    #[test]
    fn a_hand_at_the_mast_or_at_the_oars_reaches_the_sheets_and_one_at_the_stern_does_not() {
        // The player's two cases, and the one that is not a case: "стоя у
        // паруса (или управляя плотом)". A passenger sitting at the stern of
        // somebody else's raft is not trimming its sail from there.
        let (riding, _, feet) = aboard_with_a_sail([raft::MAST_ALONG, 0.0, 0.0]);
        assert_eq!(riding.sail_at_hand(None, feet.as_dvec3()), Some(5), "standing at the mast reaches nothing");

        let (riding, _, stern) = aboard_with_a_sail(raft::SEAT);
        assert_eq!(riding.sail_at_hand(None, stern.as_dvec3()), None, "a passenger at the stern trims the sail");
        assert_eq!(riding.sail_at_hand(Some(5), stern.as_dvec3()), Some(5), "the rower cannot reach their own sheets");

        // ...and a furled sail has nothing to brace, so the button keeps its
        // ordinary meaning and a rower can still swing at what is beside them.
        let mut furled = Body::at_rest(10.0, 19.88, 10.0, 0.4);
        furled.sail = false;
        let feet = glam::DVec3::from(furled.world_of([raft::MAST_ALONG, 0.0, 0.0])).as_vec3();
        let mut riding = Riding::default();
        riding.settle(&[RaftPose { id: 5, now: furled, latest: furled }], feet.as_dvec3());
        assert_eq!(riding.sail_at_hand(Some(5), feet.as_dvec3()), None, "a furled sail was trimmed");

        // ...and nobody aboard anything reaches anything.
        assert_eq!(Riding::default().sail_at_hand(Some(5), (Vec3::ZERO).as_dvec3()), None);
    }

    #[test]
    fn the_yard_follows_the_hand_and_lets_go_once_the_server_has_had_time_to_answer() {
        // **The two ways this can be wrong are opposite ones.** Dropped the
        // moment the hand comes off, the yard springs back to the angle the
        // last snapshot carried and then forward again when the answer lands.
        // Held for ever, a sail somebody else braced is never seen to move.
        let (mut riding, body, _) = aboard_with_a_sail([raft::MAST_ALONG, 0.0, 0.0]);
        assert_eq!(body.sail_angle, 0.0);
        let t = Instant::now();
        assert!(riding.held_trim(t).is_none(), "a hand nobody put on the sheets is holding them");

        let angle = riding.turn_sail(5, 0.4, t);
        assert!((angle - 0.4).abs() < 1e-6, "the yard went to {angle} rather than 0.4");
        assert_eq!(riding.held_trim(t), Some((5, 0.4)));
        // A second drag carries on from where the first one left the yard,
        // rather than starting from the angle the deck came with.
        let angle = riding.turn_sail(5, 0.4, t);
        assert!((angle - 0.8).abs() < 1e-6, "a second pull put the yard at {angle} rather than 0.8");
        // ...and it never goes through its own mast, however long the drag.
        for _ in 0..40 {
            riding.turn_sail(5, 0.4, t);
        }
        assert_eq!(riding.held_trim(t), Some((5, raft::SAIL_MAX_ANGLE)));

        // Still held a moment after the hand comes off -- the answer is in
        // flight -- and let go of once it has had time to arrive.
        assert!(riding.held_trim(t + TRIM_HOLD / 2).is_some(), "the yard sprang back before the answer could arrive");
        assert!(riding.held_trim(t + TRIM_HOLD * 2).is_none(), "the yard is held against the server for ever");
    }

    #[test]
    fn the_sail_angle_goes_up_the_wire_when_it_moves_and_not_once_a_frame() {
        let (mut riding, _, _) = aboard_with_a_sail([raft::MAST_ALONG, 0.0, 0.0]);
        let t = Instant::now();
        assert!(riding.trim_message(t).is_none(), "a sail nobody touched was reported");
        riding.turn_sail(5, 0.3, t);
        assert!(matches!(riding.trim_message(t), Some(ClientMessage::Trim { angle, .. }) if angle == 0.3));
        // A frame later, at the same angle, there is nothing to say: this is
        // an angle and not a push, so repeating it says nothing new.
        assert!(riding.trim_message(t + TRIM_RESEND * 4).is_none(), "a sail standing still was reported again");
        // ...and a drag at a hundred and forty frames a second is not a
        // hundred and forty messages a second.
        riding.turn_sail(5, 0.05, t + Duration::from_millis(7));
        assert!(riding.trim_message(t + Duration::from_millis(7)).is_none(), "every frame of a drag went up the wire");
        riding.turn_sail(5, 0.05, t + TRIM_RESEND);
        assert!(matching_angle(riding.trim_message(t + TRIM_RESEND), 0.4), "the drag was never reported at all");
    }

    fn matching_angle(message: Option<ClientMessage>, wanted: f32) -> bool {
        matches!(message, Some(ClientMessage::Trim { angle, .. }) if (angle - wanted).abs() < 1e-5)
    }

    #[test]
    fn the_oars_are_sent_on_change_and_again_while_pulling_and_not_while_idle() {
        let mut riding = Riding::default();
        let t = Instant::now();
        assert!(riding.row_message(1, Oars::REST, t).is_none(), "resting oars were sent before anything happened");
        let pull = Oars { stroke: 1.0, turn: 0.0 };
        assert!(riding.row_message(1, pull, t).is_some());
        assert!(riding.row_message(1, pull, t + Duration::from_millis(50)).is_none(), "the same stroke sent twice in a frame");
        assert!(riding.row_message(1, pull, t + ROW_RESEND).is_some(), "a held stroke was not repeated");
        assert!(riding.row_message(1, Oars::REST, t + ROW_RESEND).is_some(), "letting go was not said");
        assert!(riding.row_message(1, Oars::REST, t + ROW_RESEND * 4).is_none(), "idle oars were repeated");
    }

    #[test]
    fn a_rider_standing_still_on_a_moving_raft_says_nothing_and_one_walking_says_where() {
        let mut riding = Riding::default();
        let mut sequence = 0;
        let t = Instant::now();
        let interval = Duration::from_millis(50);
        let mut body = Body::at_rest(10.0, 19.88, 10.0, 0.0);
        let feet = glam::DVec3::from(body.world_of([0.5, 0.0, 0.0])).as_vec3();
        riding.settle(&[RaftPose { id: 2, now: body, latest: body }], feet.as_dvec3());
        assert!(matches!(riding.deck_message(feet.as_dvec3(), 0.0, 0.0, true, t, interval, &mut sequence), DeckTransform::Send(_)));
        // The raft moves on, and carries the feet with it.
        let mut carried = feet.as_dvec3();
        let mut yaw = 0.0;
        let before = body;
        body.x += 1.0;
        riding.carry_player(&[RaftPose { id: 2, now: body, latest: body }], &mut carried, &mut yaw);
        let carried = carried.as_vec3();
        assert_eq!(carried, (carry(&before, &body, feet.as_dvec3())).as_vec3());
        let later = t + interval * 2;
        assert!(matches!(riding.deck_message(carried.as_dvec3(), 0.0, 0.0, true, later, interval, &mut sequence), DeckTransform::Quiet));
        let walked = carried + Vec3::X * 0.3;
        assert!(matches!(riding.deck_message(walked.as_dvec3(), 0.0, 0.0, true, later + interval, interval, &mut sequence), DeckTransform::Send(_)));
        riding.settle(&[], walked.as_dvec3());
        assert!(matches!(
            riding.deck_message(walked.as_dvec3(), 0.0, 0.0, false, later + interval * 3, interval, &mut sequence),
            DeckTransform::NotAboard
        ));
    }

    /// ## A real server, a real socket, and the frame loop's own order
    ///
    /// The tests above hold one piece still and move another. What a player
    /// reported -- "moves in jerks", "the raft trembles" -- is a property of
    /// every piece at once: snapshots arriving when the server's timer lets
    /// them, the interpolation reading the wall clock, the rower's prediction
    /// easing toward a raft a round trip old, the deck carrying the feet
    /// before the collider runs. So these drive a client the way
    /// `lib.rs`'s frame does -- pump the socket, tick the entities, predict,
    /// carry, collide, settle, send -- against `primitive_server::start` over
    /// loopback, at 144 frames a second of real time, and measure what a
    /// frame would draw.
    mod over_a_socket {
        use std::time::{Duration, Instant};

        use glam::Vec3;
        use primitive_shared::protocol::{ClientMessage, EntityId, ServerMessage};
        use primitive_shared::raft::{self, Body, Oars, Wind};
        use primitive_shared::types::{ChunkPos, BLOCK_AIR, BLOCK_RAFT, BLOCK_STONE, BLOCK_WATER};
        use primitive_shared::weather::Weather;

        use crate::logic::chunk_manager::ChunkManager;
        use crate::logic::entities::Entities;
        use crate::logic::physics::{Player, DEFAULT_MOVE_SPEED};
        use crate::logic::riding::{oars_from_axes, DeckTransform, RaftPose, Riding};
        use crate::net::network::{self, Incoming, NetworkHandle};

        /// A frame at 144 Hz: past the 120 at which the moving geometry stops
        /// being rebuilt every frame, and the rate a desktop without vsync
        /// runs well above.
        const FRAME: f32 = 1.0 / 144.0;
        /// How often a transform goes out: `player_update_hz`'s default.
        const SEND_EVERY: Duration = Duration::from_millis(50);

        pub(super) fn runtime() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("a runtime")
        }

        pub(super) fn settings(anticheat: bool) -> primitive_server::settings::ServerSettings {
            primitive_server::settings::ServerSettings {
                bind_addr: "127.0.0.1:0".to_string(),
                server_name: "test".to_string(),
                world_dir: String::new(),
                plugin_dir: String::new(),
                mod_dir: String::new(),
                stats_interval_secs: 0.0,
                world_preset: primitive_shared::worldgen::Preset::Test,
                anticheat: primitive_server::settings::AntiCheatSettings { enabled: anticheat, ..Default::default() },
                ..Default::default()
            }
        }

        /// One client, holding what the frame loop holds.
        pub(super) struct Client {
            net: NetworkHandle,
            pub chunks: ChunkManager,
            pub entities: Entities,
            pub riding: Riding,
            pub player: Player,
            yaw: f32,
            sequence: u32,
            sent: Option<(Instant, Vec3)>,
            pub spawn: Vec3,
            /// `PositionCorrection`s obeyed.
            pub corrections: usize,
            /// `Posture`s that put the body somewhere.
            pub placed: usize,
            pub heard: Vec<ServerMessage>,
            clock: (f32, Instant, f32),
            weather: Weather,
        }

        impl Client {
            pub fn join(runtime: &tokio::runtime::Runtime, address: &str, name: &str) -> Self {
                let connection = runtime.block_on(network::connect(address, name)).expect("connect");
                let welcome = connection.welcome;
                let mut entities = Entities::default();
                entities.set_tick_rate(welcome.tick_rate_hz);
                let spawn = Vec3::new(welcome.spawn.0 as f32, welcome.spawn.1 as f32, welcome.spawn.2 as f32);
                Self {
                    net: connection.handle,
                    chunks: ChunkManager::new(4),
                    entities,
                    riding: Riding::default(),
                    player: Player::new(spawn.as_dvec3(), DEFAULT_MOVE_SPEED),
                    yaw: 0.0,
                    sequence: 0,
                    sent: None,
                    spawn,
                    corrections: 0,
                    placed: 0,
                    heard: Vec::new(),
                    clock: (welcome.world_days, Instant::now(), welcome.day_length_seconds.max(1.0)),
                    weather: Weather::Clear,
                }
            }

            pub fn send(&mut self, message: ClientMessage) {
                self.net.send(message);
            }

            /// The five chunks either way of the spawn, and a wait until the
            /// nine in the middle are here.
            pub fn ask_for_ground(&mut self) {
                let centre = ChunkManager::chunk_for_world_pos(self.spawn.x, self.spawn.z);
                let wanted = (-2..=2)
                    .flat_map(|dx| (-2..=2).map(move |dz| ChunkPos::new(centre.x + dx, centre.z + dz)))
                    .collect();
                self.send(ClientMessage::RequestChunks(wanted));
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    self.pump();
                    let here = (-1..=1).all(|dx| {
                        (-1..=1).all(|dz| {
                            self.chunks.block_at((centre.x + dx) * 16 + 8, 1, (centre.z + dz) * 16 + 8).is_some()
                        })
                    });
                    if here {
                        return;
                    }
                    assert!(Instant::now() < deadline, "the ground never arrived");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }

            /// Everything in the socket, applied as `drain_network` applies it.
            pub fn pump(&mut self) {
                while let Ok(incoming) = self.net.to_game.try_recv() {
                    let message = match incoming {
                        Incoming::Chunk(chunk) => {
                            self.chunks.insert(*chunk);
                            continue;
                        }
                        Incoming::Message(message) => message,
                    };
                    match &message {
                        ServerMessage::Entities { tick, states } => self.entities.apply_snapshot(*tick, states),
                        ServerMessage::Oars { raft } => self.entities.steer(*raft),
                        ServerMessage::PositionCorrection { x, y, z, .. } => {
                            self.player.teleport((Vec3::new((*x) as f32, (*y) as f32, (*z) as f32)).as_dvec3());
                            self.corrections += 1;
                        }
                        ServerMessage::Posture { at: Some((x, y, z)), .. } => {
                            self.player.teleport((Vec3::new((*x) as f32, (*y) as f32, (*z) as f32)).as_dvec3());
                            self.placed += 1;
                        }
                        ServerMessage::TimeSync { world_days, .. } => self.clock = (*world_days, Instant::now(), self.clock.2),
                        ServerMessage::WeatherSync { weather } => self.weather = *weather,
                        _ => {}
                    }
                    // Snapshots are many and nothing below reads them back.
                    if !matches!(message, ServerMessage::Entities { .. } | ServerMessage::Snapshot { .. }) {
                        self.heard.push(message);
                    }
                }
            }

            /// Pumps until `want` finds something, for up to ten seconds.
            pub fn wait_for<T>(&mut self, mut want: impl FnMut(&Self) -> Option<T>) -> T {
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    self.pump();
                    if let Some(found) = want(self) {
                        return found;
                    }
                    assert!(Instant::now() < deadline, "waited ten seconds for something that never came");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }

            pub fn wind(&self) -> Wind {
                raft::wind(self.world_days(), self.weather)
            }

            /// The hour of the world this rig is standing in: the same
            /// number the wind and the sea's circulation are both read
            /// from, so a test that moves the clock moves both.
            pub fn world_days(&self) -> f32 {
                let (days, at, length) = self.clock;
                days + at.elapsed().as_secs_f32() / length
            }

            /// One frame, in the order `lib.rs` takes it. Answers the decks
            /// the frame stood on.
            pub fn frame(&mut self, wish: Vec3, row: Option<(f32, f32)>, dt: f32, now: Instant) -> Vec<RaftPose> {
                self.pump();
                self.entities.tick(dt);
                let rowing = self.entities.steering().map(|steering| steering.id);
                let oars = match (rowing, row) {
                    (Some(_), Some((stroke, turn))) => oars_from_axes(stroke, turn, false),
                    _ => Oars::REST,
                };
                let wish = if rowing.is_some() { Vec3::ZERO } else { wish };
                let wind = self.wind();
                self.entities.predict(oars, wind, self.world_days(), &self.chunks, dt);
                if let Some(message) = rowing.and_then(|raft| self.riding.row_message(raft, oars, now)) {
                    self.send(message);
                }
                let decks = self.entities.rafts();
                self.riding.carry_player(&decks, &mut self.player.position, &mut self.yaw);
                self.player.decks = decks.iter().map(|pose| pose.now).collect();
                let mut left = dt;
                while left > 0.0 {
                    let step = left.min(crate::PHYSICS_STEP);
                    self.player.update(&self.chunks, &[], wish, Vec3::X, false, false, false, step);
                    left -= step;
                }
                self.riding.settle(&decks, self.player.position);
                let (feet, grounded) = (self.player.position, self.player.grounded);
                match self.riding.deck_message(feet, self.yaw, 0.0, grounded, now, SEND_EVERY, &mut self.sequence) {
                    DeckTransform::Send(message) => self.send(message),
                    DeckTransform::Quiet => {}
                    DeckTransform::NotAboard => {
                        let due = self.sent.is_none_or(|(at, was)| {
                            now.saturating_duration_since(at) >= SEND_EVERY && was.distance(feet.as_vec3()) > 0.01
                        });
                        if due {
                            self.sequence += 1;
                            let sequence = self.sequence;
                            self.send(ClientMessage::UpdateTransform {
                                x: feet.x,
                                y: feet.y,
                                z: feet.z,
                                yaw: self.yaw,
                                pitch: 0.0,
                                on_ground: grounded,
                                sequence,
                            });
                            self.sent = Some((now, feet.as_vec3()));
                        }
                    }
                }
                decks
            }
        }

        /// Frames at `FRAME` of real time, for `seconds`, handing each one its
        /// own measured `dt` the way the frame loop measures it.
        ///
        /// **Each frame waits from the last one, never from a schedule.** A
        /// schedule catches up after a late frame by running the next few back
        /// to back a few microseconds apart, which no swapchain does, and which
        /// the first version of this measured as the walker stalling.
        pub(super) fn run_frames(seconds: f32, mut each: impl FnMut(f32, Instant)) {
            let start = Instant::now();
            let mut last = start;
            while last.duration_since(start).as_secs_f32() < seconds {
                let due = last + Duration::from_secs_f32(FRAME);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
                let now = Instant::now();
                let dt = (now - last).as_secs_f32().min(0.1);
                last = now;
                each(dt, now);
            }
        }

        /// How something drawn along a straight line moved, frame by frame.
        #[derive(Debug)]
        pub(super) struct Motion {
            pub frames: usize,
            /// Blocks a second, over the whole run.
            pub mean_speed: f32,
            /// Frames that covered under half, or over half again, the
            /// distance the frames around them did per second.
            pub stalls: usize,
            pub surges: usize,
            /// Frames that went back along the line.
            pub backwards: usize,
            /// The furthest a frame was drawn from where a steady motion
            /// between the frames five either side of it would have put it at
            /// its own moment, in centimetres: the tremble itself. Against
            /// time and not against the frame count, because frames are not
            /// evenly spaced and a steady motion sampled unevenly is not a
            /// tremble.
            pub wobble_cm: f32,
            /// Frames that took the frame loop's whole clamp (`dt` of a tenth of
            /// a second): a test process the machine did not schedule, which
            /// the game would not have drawn either. The tremble is not
            /// measured across them -- a prediction stepped a tenth of a second
            /// through half a second of real time is behind by the rest, and
            /// easing it back is the clamp's doing, not the raft's.
            pub hitches: usize,
        }

        /// `samples` are (dt, distance along the line) per frame.
        pub(super) fn motion(samples: &[(f32, f32)]) -> Motion {
            let speeds: Vec<f32> = samples.windows(2).map(|w| (w[1].1 - w[0].1) / w[1].0.max(1e-4)).collect();
            let mut stalls = 0;
            let mut surges = 0;
            let mut backwards = 0;
            for i in 0..speeds.len() {
                let around = &speeds[i.saturating_sub(10)..(i + 11).min(speeds.len())];
                let local = around.iter().sum::<f32>() / around.len() as f32;
                if speeds[i] < local * 0.5 {
                    stalls += 1;
                }
                if speeds[i] > local * 1.5 {
                    surges += 1;
                }
                if speeds[i] * samples[i + 1].0 < -1e-4 {
                    backwards += 1;
                }
            }
            let times: Vec<f32> = samples
                .iter()
                .scan(0.0f32, |t, s| {
                    *t += s.0;
                    Some(*t)
                })
                .collect();
            let hitch = |s: &(f32, f32)| s.0 >= 0.099;
            let mut wobble: f32 = 0.0;
            for i in 5..samples.len().saturating_sub(5) {
                let (a, b) = (i - 5, i + 5);
                if samples[a..=b].iter().any(hitch) {
                    continue;
                }
                let share = (times[i] - times[a]) / (times[b] - times[a]).max(1e-4);
                let steady = samples[a].1 + (samples[b].1 - samples[a].1) * share;
                wobble = wobble.max((samples[i].1 - steady).abs());
            }
            let time: f32 = samples.iter().skip(1).map(|s| s.0).sum();
            let distance = samples.last().map_or(0.0, |s| s.1) - samples.first().map_or(0.0, |s| s.1);
            Motion {
                frames: samples.len(),
                mean_speed: distance / time.max(1e-4),
                stalls,
                surges,
                backwards,
                wobble_cm: wobble * 100.0,
                hitches: samples.iter().filter(|s| hitch(s)).count(),
            }
        }

        #[test]
        fn walking_straight_through_the_whole_client_frame_is_never_corrected_and_never_jerks() {
            for anticheat in [false, true] {
                let runtime = runtime();
                let server = runtime
                    .block_on(primitive_server::start(settings(anticheat), primitive_server::RunOptions::embedded()))
                    .expect("start");
                let mut walker = Client::join(&runtime, &server.address().to_string(), "walker");
                // A flat, clear lane east of the spawn, so a wall on the plaza
                // is not what stops the walk.
                let (bx, by, bz) = (walker.spawn.x.floor() as i32, walker.spawn.y.floor() as i32, walker.spawn.z.floor() as i32);
                for x in bx - 2..=bx + 24 {
                    for z in bz - 2..=bz + 2 {
                        server.place_block(x, by - 1, z, BLOCK_STONE);
                        for y in by..=by + 3 {
                            server.place_block(x, y, z, BLOCK_AIR);
                        }
                    }
                }
                walker.ask_for_ground();
                walker.player.teleport((Vec3::new(bx as f32 + 0.5, by as f32, bz as f32 + 0.5)).as_dvec3());

                let mut shake = crate::logic::shake::Shake::new(0.7);
                let mut feet = Vec::new();
                let mut views = Vec::new();
                let mut time = 0.0f32;
                let mut trace = Vec::new();
                run_frames(3.0, |dt, now| {
                    let was = walker.player.position.x;
                    walker.frame(Vec3::X, None, dt, now);
                    trace.push((time, dt, (walker.player.position.x - was) as f32, walker.player.velocity.x, walker.player.grounded));
                    let footed = walker.player.grounded && !walker.player.swimming && walker.player.horizontal_speed() > 0.5;
                    shake.update(dt, walker.player.horizontal_speed(), footed, false);
                    let view = walker.player.eye_position().as_vec3() + shake.offset(Vec3::Z, Vec3::Y)
                        - Vec3::Y * walker.player.view_step_lag();
                    time += dt;
                    // Under way: a walker reaches their pace in a fraction of a second.
                    if time > 0.5 {
                        feet.push((dt, walker.player.position.x as f32));
                        views.push((dt, view.x));
                    }
                });
                // **Every frame's stride is its own dt at walking pace**: the
                // strongest thing a straight walk can say, and the one a jerk
                // -- a correction, a snap, a frame that moved the body twice --
                // cannot hide from however the frames happen to be spaced.
                let worst = trace
                    .iter()
                    .filter(|(t, ..)| *t > 0.5)
                    .map(|(_, dt, step, _, _)| (step / dt - DEFAULT_MOVE_SPEED).abs())
                    .fold(0.0f32, f32::max);
                let ungrounded = trace.iter().filter(|(t, .., grounded)| *t > 0.5 && !grounded).count();
                let feet = motion(&feet);
                let views = motion(&views);
                println!(
                    "[walk, anticheat {}] feet {feet:?}\n    view {views:?}\n    corrections {} placed {}; worst stride off pace {worst:.4} b/s; frames off the ground {ungrounded}",
                    if anticheat { "on" } else { "off" },
                    walker.corrections,
                    walker.placed
                );
                // A machine busy with other builds hands this loop a tenth of a
                // second at a time; a stride is still a stride then, but a run
                // that is nothing else has measured nothing.
                assert!(feet.frames - feet.hitches >= 10, "hardly a frame was scheduled: {feet:?}");
                assert!(worst < DEFAULT_MOVE_SPEED * 0.05, "a frame's stride was {worst} b/s off the walking pace");
                assert_eq!(ungrounded, 0, "a straight walk on flat stone left the ground");
                assert_eq!(walker.corrections, 0, "a straight walk was corrected by the server");
                assert_eq!(walker.placed, 0, "a straight walk was put somewhere by the server");
                assert!(feet.mean_speed > DEFAULT_MOVE_SPEED * 0.9, "the walker never got up to pace: {feet:?}");
                assert_eq!(feet.stalls + feet.surges + feet.backwards, 0, "the feet jerked: {feet:?}");
                assert_eq!(views.stalls + views.surges + views.backwards, 0, "the view jerked: {views:?}");
                runtime.block_on(server.stop());
            }
        }

        /// The raft in the latest snapshot, and its id.
        fn raft_in(client: &Client) -> Option<(EntityId, Body)> {
            client.entities.rafts().first().map(|pose| (pose.id, pose.latest))
        }

        #[test]
        fn a_raft_rowed_in_a_straight_line_is_drawn_moving_steadily_for_its_rower_and_its_passenger() {
            let runtime = runtime();
            let server = runtime
                .block_on(primitive_server::start(settings(false), primitive_server::RunOptions::embedded()))
                .expect("start");
            let address = server.address().to_string();
            let mut rower = Client::join(&runtime, &address, "rower");
            // A long pond east of the spawn, dug before the ground is asked
            // for so the chunks arrive with the water in them.
            let (bx, by, bz) = (rower.spawn.x.floor() as i32, rower.spawn.y.floor() as i32, rower.spawn.z.floor() as i32);
            for x in bx + 2..=bx + 36 {
                for z in bz - 6..=bz + 6 {
                    server.place_block(x, by - 1, z, BLOCK_WATER);
                    for y in by..=by + 3 {
                        server.place_block(x, y, z, BLOCK_AIR);
                    }
                }
            }
            rower.ask_for_ground();

            // The raft, out of the pack and onto the pond.
            assert_eq!(server.give(BLOCK_RAFT, 1), 0);
            let slot = rower.wait_for(|client| {
                client.heard.iter().rev().find_map(|m| match m {
                    ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_RAFT) > 0 => {
                        (0..primitive_shared::inventory::SLOTS).find(|&s| inventory.block_in(s) == Some(BLOCK_RAFT))
                    }
                    _ => None,
                })
            });
            rower.send(ClientMessage::SelectSlot { slot: slot as u8 });
            std::thread::sleep(Duration::from_millis(100));
            rower.send(ClientMessage::UseBlock { global_x: bx + 4, global_y: by - 1, global_z: bz });
            let (id, afloat) = rower.wait_for(raft_in);

            // Aboard at the stern, by the frame's own carry and settle, and
            // then at the oars.
            // Put where the server has them first, as walking there would have:
            // a `Deck` from further than `BOARDING_REACH` off is refused.
            let stand_at = |client: &mut Client, at: Vec3| {
                client.player.teleport(at.as_dvec3());
                client.sequence += 1;
                let sequence = client.sequence;
                client.send(ClientMessage::UpdateTransform { x: f64::from(at.x), y: f64::from(at.y), z: f64::from(at.z), yaw: 0.0, pitch: 0.0, on_ground: true, sequence });
                std::thread::sleep(Duration::from_millis(150));
            };
            let stern = glam::DVec3::from(afloat.world_of([-1.0, 0.0, 0.0])).as_vec3();
            stand_at(&mut rower, stern);
            run_frames(0.3, |dt, now| {
                rower.frame(Vec3::ZERO, None, dt, now);
            });
            assert_eq!(rower.riding.aboard(), Some(id), "the rower never stood on the deck");
            rower.send(ClientMessage::UseRaft { raft: id });
            rower.wait_for(|client| client.entities.steering().map(|_| ()));

            // A passenger, forward of the mast.
            let mut passenger = Client::join(&runtime, &address, "passenger");
            passenger.ask_for_ground();
            let (_, seen) = passenger.wait_for(raft_in);
            stand_at(&mut passenger, glam::DVec3::from(seen.world_of([0.9, 0.0, 0.4])).as_vec3());
            run_frames(0.3, |dt, now| {
                rower.frame(Vec3::ZERO, None, dt, now);
                passenger.frame(Vec3::ZERO, None, dt, now);
            });
            assert_eq!(passenger.riding.aboard(), Some(id), "the passenger never stood on the deck");

            // Rowing, straight.
            let heading = afloat.forward();
            let along = |body: &Body| (body.x * f64::from(heading.0) + body.z * f64::from(heading.1)) as f32;
            let pose_of = |decks: &[RaftPose]| decks.iter().find(|pose| pose.id == id).map(|pose| pose.now);
            let (mut rowed, mut carried, mut stood) = (Vec::new(), Vec::new(), Vec::new());
            let mut drawn_at: Vec<(Instant, Body, bool)> = Vec::new();
            let mut time = 0.0f32;
            run_frames(6.0, |dt, now| {
                let rower_decks = rower.frame(Vec3::ZERO, Some((1.0, 0.0)), dt, now);
                let passenger_decks = passenger.frame(Vec3::ZERO, None, dt, now);
                time += dt;
                // Under way: past the first two seconds of the stroke.
                if time < 2.0 {
                    return;
                }
                if let Some(body) = pose_of(&rower_decks) {
                    rowed.push((dt, along(&body)));
                }
                if let Some(body) = pose_of(&passenger_decks) {
                    carried.push((dt, along(&body)));
                    let feet = passenger.player.position;
                    stood.push((dt, (feet.x * f64::from(heading.0) + feet.z * f64::from(heading.1)) as f32));
                    drawn_at.push((now, body, passenger.riding.is_aboard()));
                }
            });

            // The deck as the dynamic mesh draws it: rebuilt when the frame's
            // own rule says (`dynamic_rebuild_due`), while the feet it carries
            // move every frame.
            let mut built: Option<(Instant, Body)> = None;
            let mut stale_cm: f32 = 0.0;
            for (now, body, aboard) in &drawn_at {
                if crate::dynamic_rebuild_due(false, *aboard, built.map(|(at, _)| at), *now) {
                    built = Some((*now, *body));
                }
                let (_, mesh) = built.expect("built on the first frame");
                stale_cm = stale_cm.max(((mesh.x - body.x).hypot(mesh.z - body.z) * 100.0) as f32);
            }

            let (rowed, carried, stood) = (motion(&rowed), motion(&carried), motion(&stood));
            let slips = passenger.riding.aboard() != Some(id);
            let server_speed = raft_in(&passenger).map_or(0.0, |(_, body)| body.speed());
            println!(
                "[raft] rower's raft {rowed:?}\n    passenger's raft {carried:?}\n    passenger's feet {stood:?}\n    deck mesh behind the carried feet by up to {stale_cm:.2} cm\n    corrections: rower {} passenger {}; the server's raft at {server_speed:.2} b/s",
                rower.corrections, passenger.corrections
            );
            assert!(!slips, "the passenger fell off the deck");
            // A debug server generating chunks for two players ticks late, and
            // its raft covers less than a release one: under way is enough.
            assert!(rowed.mean_speed > 0.5 && carried.mean_speed > 0.5, "the raft never got under way");
            for (who, m) in [("rower's raft", &rowed), ("passenger's raft", &carried), ("passenger", &stood)] {
                assert_eq!(m.backwards, 0, "the {who} was drawn going backwards: {m:?}");
                assert_eq!(m.stalls + m.surges, 0, "the {who} was drawn stopping and lurching: {m:?}");
            }
            // **As steady as the raft you row yourself.** The rower's raft is
            // predicted and has never stalled, so it is the measure of what this
            // machine, busy or idle, can draw steadily; a passenger's raft is
            // held to it rather than to a number. Before `Glide` it was 3.8 cm
            // against the rower's 0.4.
            //
            // **The number for the rower is a floor under "broken", not a
            // measure of smooth.** It was 2 cm, which is what an idle machine
            // draws (0.4) with room to spare -- and what a machine building
            // three other crates at once does not: 2.8 to 3.4 cm with no stall,
            // no surge and no hitch, failing every full run while every run of
            // this test alone passed. That is this machine's steadiness, which
            // the paragraph above says is exactly what the rower's raft is for
            // measuring. A rower's prediction that has come apart -- snapped to
            // each server tick, the fault `Glide` was written against -- stops
            // and lurches a tick's travel at a time, and the `stalls + surges`
            // assertion above is what catches that, strictly. This bound is the
            // backstop for a wobble with no stall in it, at a level no busy
            // machine has come near.
            assert!(rowed.wobble_cm < 5.0, "even the rower's own raft trembled by {} cm: {rowed:?}", rowed.wobble_cm);
            for (who, m) in [("passenger's raft", &carried), ("passenger", &stood)] {
                assert!(
                    m.wobble_cm < rowed.wobble_cm + 0.5,
                    "the {who} trembled by {} cm where the rower's raft trembled by {}: {m:?}",
                    m.wobble_cm,
                    rowed.wobble_cm
                );
            }
            assert!(stale_cm < 0.01, "the deck was drawn {stale_cm} cm from the deck the feet stood on");
            assert_eq!(rower.corrections + passenger.corrections, 0);
            runtime.block_on(server.stop());
        }
    }
}
