//! Riding a horse: the client's half of `horse`.
//!
//! **The rower's prediction, on legs** (`riding::Steering`). The rider's keys
//! move this client's horse on the frame they are pressed, through the same
//! `horse::step` the server runs; the server's horse, a round trip behind,
//! only eases the prediction toward itself. The rider's body is the predicted
//! saddle, the drawn horse is the predicted horse (`Entities::set_ridden`), and
//! the reins go up the wire on change and every quarter second while they ask
//! for anything (`rein_message`) -- the oars' rhythm (`riding::ROW_RESEND`).
//!
//! ## Why the correction is not the raft's
//!
//! The raft's easing extrapolates the server's raft by the velocity the
//! snapshot carries. An animal's snapshot carries no velocity (a byte of
//! tack, a yaw and a position -- `EntityKind::Animal`), and adding three
//! floats to every animal in the world for the one being ridden would be the
//! wire paying for a rider it mostly does not have. So the correction is split
//! by direction instead: **across the horse and up, eased in at once** --
//! those are the errors a rider feels as a slide -- and **along it only when
//! the horse has slowed**, because along its line of travel a galloping
//! prediction is *meant* to be ahead of a server a round trip behind, and
//! pulling it back would be the rubber band. A horse that has stopped is where
//! the server says, to the hair, within a second.

use std::time::{Duration, Instant};

use glam::DVec3;
use primitive_shared::horse::{self, Fettle, Gait, Mount, Reins};
use primitive_shared::protocol::{ClientMessage, EntityId, EntityKind, EntityState};
use primitive_shared::types::BlockId;

/// How often the reins are sent again while they ask for anything.
pub const REIN_RESEND: Duration = Duration::from_millis(250);
/// How fast an error across the horse or up is eased out, per second.
pub const EASE_ACROSS: f32 = 6.0;
/// ...and along it, once the horse has slowed below `SLOW`.
pub const EASE_ALONG: f32 = 3.0;
/// Under this, blocks a second, the along-track error is eased too.
pub const SLOW: f32 = 1.5;
/// Past this far from the server's horse the prediction is thrown away and
/// the horse put where the server has it: a blocked gate, a death, a teleport.
pub const SNAP_BEYOND: f32 = 4.0;

/// The horse this client is riding, ahead of the server.
#[derive(Debug, Clone)]
pub struct Horseback {
    pub horse: EntityId,
    /// The predicted body.
    pub body: Mount,
    /// What the server last said the horse has in it for the ride.
    pub fettle: Fettle,
    /// The server's horse as the last snapshot had it: feet and yaw.
    server: Option<(DVec3, f32)>,
    /// The reins last sent, and when.
    sent: Option<(Reins, Instant)>,
    /// The last reins asked for, for the jump's edge.
    asked: Reins,
}

impl Horseback {
    /// On a horse whose feet are at `feet`, facing `yaw`.
    pub fn new(horse: EntityId, feet: DVec3, yaw: f32, wind: f32, fettle: Fettle) -> Self {
        Self {
            horse,
            body: Mount::standing(feet.x, feet.y, feet.z, yaw, wind),
            fettle,
            server: None,
            sent: None,
            asked: Reins::SLACK,
        }
    }

    /// Where the rider's feet are.
    pub fn rider_feet(&self) -> DVec3 {
        let saddle = self.body.saddle();
        DVec3::new(saddle[0], saddle[1], saddle[2])
    }

    /// Where the horse's feet are.
    pub fn feet(&self) -> DVec3 {
        DVec3::new(self.body.x, self.body.y, self.body.z)
    }

    /// Seconds of gallop left, over the most there can be: the gauge.
    pub fn wind_fraction(&self) -> f32 {
        (self.body.wind / horse::GALLOP_SECONDS).clamp(0.0, 1.0)
    }

    /// The server's word on the wind and the horse, twice a second.
    ///
    /// **The wind is taken when it differs by more than a second**, and not
    /// every time: the client spends it on its frames and the server on its
    /// ticks, and a gauge snapped to a number half a second stale every half
    /// second would twitch. A second apart is a real disagreement -- a hungry
    /// horse, a load put in the bags -- and that is taken at once.
    pub fn told(&mut self, wind: f32, fettle: Fettle) {
        self.fettle = fettle.clamped();
        if wind.is_finite() && (wind - self.body.wind).abs() > 1.0 {
            self.body.wind = wind.clamp(0.0, horse::GALLOP_SECONDS);
        }
    }

    /// The server's horse in a snapshot, if it is there.
    pub fn server_saw(&mut self, states: &[EntityState]) {
        for state in states {
            if state.id != self.horse {
                continue;
            }
            if let EntityKind::Animal { species, yaw, growth, .. } = state.kind {
                let size = primitive_shared::youth::size(primitive_shared::youth::from_wire(growth));
                let feet = DVec3::new(state.x, state.y - f64::from(species.height() * size * 0.5), state.z);
                if feet.is_finite() && yaw.is_finite() {
                    self.server = Some((feet, yaw));
                }
            }
        }
    }

    /// The reins the keys ask for: `forward` and `turn` in -1..1 off the
    /// movement keys or the stick, `gallop` the sprint key, `walk` the walk
    /// key, `jump` the jump key's press.
    ///
    /// **Forward is a trot; with the sprint key a gallop; with the walk key a
    /// walk** (`horse::Gait`'s argument for three). Backing up is always a
    /// walk: a horse is not reversed at a gallop.
    pub fn reins_from_keys(forward: f32, turn: f32, gallop: bool, walk: bool, jump: bool) -> Reins {
        let gait = if walk {
            Gait::Walk
        } else if gallop {
            Gait::Gallop
        } else {
            Gait::Trot
        };
        Reins { forward, turn, gait, jump }.clamped()
    }

    /// One frame of the horse, ahead of the server: `horse::step` with the
    /// rider's reins, then the server's horse eased in. See the module note
    /// for which way each error is eased.
    pub fn predict<F>(&mut self, reins: Reins, block: &F, dt: f32)
    where
        F: Fn(i32, i32, i32) -> Option<BlockId>,
    {
        self.asked = reins;
        horse::step(&mut self.body, reins, self.fettle, block, dt);
        let Some((server, server_yaw)) = self.server else {
            return;
        };
        let here = self.feet();
        let error = server - here;
        if error.length() > f64::from(SNAP_BEYOND) || !error.is_finite() {
            self.body.x = server.x;
            self.body.y = server.y;
            self.body.z = server.z;
            self.body.yaw = server_yaw;
            self.body.vx = 0.0;
            self.body.vz = 0.0;
            self.body.vy = 0.0;
            return;
        }
        let (fx, fz) = self.body.forward();
        let along = error.x * f64::from(fx) + error.z * f64::from(fz);
        // Up only with its feet on the ground: in the air the prediction has
        // jumped a round trip before the server's horse did, and easing the
        // two heights together would flatten every jump into a stumble.
        let up = if self.body.on_ground { error.y } else { 0.0 };
        let across = DVec3::new(error.x - along * f64::from(fx), up, error.z - along * f64::from(fz));
        let k_across = f64::from((EASE_ACROSS * dt).min(1.0));
        let mut step = across * k_across;
        if self.body.speed() < SLOW {
            let k_along = f64::from((EASE_ALONG * dt).min(1.0));
            step += DVec3::new(f64::from(fx), 0.0, f64::from(fz)) * along * k_along;
        }
        // Only where the horse fits: an easing that walked it into a bank
        // would hand the next step a horse inside the ground.
        let (x, y, z) = (self.body.x + step.x, self.body.y + step.y, self.body.z + step.z);
        if horse::fits(x, y, z, block) {
            self.body.x = x;
            self.body.y = y;
            self.body.z = z;
        }
        // The yaw is eased the same way, a little at a time, so a horse the
        // server turned is turned under the rider rather than swapped.
        if self.body.speed() < SLOW {
            let turn = (server_yaw - self.body.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            self.body.yaw = (self.body.yaw + turn * (EASE_ALONG * dt).min(1.0)).rem_euclid(std::f32::consts::TAU);
        }
    }

    /// The reins, if they are due to go up the wire: on any change -- a jump
    /// at once, which is the change that cannot wait -- and every
    /// `REIN_RESEND` while they ask for anything; once more when they go
    /// slack, so the server stops too.
    pub fn rein_message(&mut self, now: Instant) -> Option<ClientMessage> {
        let reins = self.asked;
        let asking = reins.forward.abs() > 0.02 || reins.turn.abs() > 0.02 || reins.jump;
        let due = match self.sent {
            None => asking,
            Some((last, at)) => {
                let changed = (last.forward - reins.forward).abs() > 0.05
                    || (last.turn - reins.turn).abs() > 0.05
                    || last.gait != reins.gait
                    // The jump both ways: pressed at once, and let go at
                    // once, or a horse still told "jump" lands and jumps
                    // again until the next resend.
                    || last.jump != reins.jump;
                // Letting go is a change like any other, so it goes up
                // once; after that slack reins are silence.
                changed || (asking && now.saturating_duration_since(at) >= REIN_RESEND)
            }
        };
        if !due {
            return None;
        }
        self.sent = Some((reins, now));
        Some(ClientMessage::Rein {
            horse: self.horse,
            forward: reins.forward,
            turn: reins.turn,
            gait: reins.gait.to_wire(),
            jump: reins.jump,
        })
    }

    /// Whether the rider may get down now: the horse has as good as stopped.
    /// A rider who stepped off a galloping horse would be a fall nobody asked
    /// for, and a player told to rein in first is told by the horse.
    pub fn may_get_down(&self) -> bool {
        self.body.speed() < SLOW
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_GRASS};

    fn flat(x: i32, y: i32, z: i32) -> Option<BlockId> {
        if x.abs() > 200 || z.abs() > 200 {
            return None;
        }
        Some(if y <= 19 { BLOCK_GRASS } else { BLOCK_AIR })
    }

    fn state(id: EntityId, feet: DVec3, yaw: f32) -> EntityState {
        let species = primitive_shared::animals::Species::Horse;
        EntityState {
            id,
            kind: EntityKind::Animal {
                species,
                yaw,
                hurt: 0.0,
                attitude: primitive_shared::protocol::Attitude::Easy,
                growth: u8::MAX,
                tack: horse::TACK_RIDDEN | horse::TACK_SADDLE,
            },
            x: feet.x,
            y: feet.y + f64::from(species.height() * 0.5),
            z: feet.z,
        }
    }

    #[test]
    fn a_galloping_prediction_is_not_pulled_back_toward_a_server_a_round_trip_behind() {
        let mut ride = Horseback::new(7, DVec3::new(0.5, 20.0, 0.5), 0.0, horse::GALLOP_SECONDS, Fettle::FRESH);
        let gallop = Horseback::reins_from_keys(1.0, 0.0, true, false, false);
        for _ in 0..180 {
            ride.predict(gallop, &flat, 1.0 / 60.0);
        }
        // The server, a round trip (a fifth of a second) behind along the line.
        let behind = ride.feet() - DVec3::new(f64::from(horse::GALLOP) * 0.2, 0.0, 0.0);
        ride.server_saw(&[state(7, behind, 0.0)]);
        let before = ride.feet().x;
        ride.predict(gallop, &flat, 1.0 / 60.0);
        assert!(ride.feet().x > before, "the gallop was pulled back toward the server");
        // ...and across the line, the same error is eased out.
        let aside = ride.feet() + DVec3::new(0.0, 0.0, 0.5);
        ride.server_saw(&[state(7, aside, 0.0)]);
        let z = ride.feet().z;
        ride.predict(gallop, &flat, 1.0 / 60.0);
        assert!(ride.feet().z > z + 0.02, "a sideways error was not eased");
    }

    #[test]
    fn a_stopped_horse_comes_to_where_the_server_has_it() {
        let mut ride = Horseback::new(7, DVec3::new(0.5, 20.0, 0.5), 0.0, horse::GALLOP_SECONDS, Fettle::FRESH);
        ride.server_saw(&[state(7, DVec3::new(1.5, 20.0, 0.5), 0.3)]);
        for _ in 0..120 {
            ride.predict(Reins::SLACK, &flat, 1.0 / 60.0);
        }
        assert!((ride.feet().x - 1.5).abs() < 0.05, "a standing horse stayed at {}", ride.feet().x);
        // A horse the server has somewhere else entirely is put there.
        ride.server_saw(&[state(7, DVec3::new(30.5, 20.0, 0.5), 0.0)]);
        ride.predict(Reins::SLACK, &flat, 1.0 / 60.0);
        assert!((ride.feet().x - 30.5).abs() < 0.01);
    }

    #[test]
    fn the_reins_go_up_on_change_and_while_asking_and_once_more_when_let_go() {
        let mut ride = Horseback::new(7, DVec3::new(0.5, 20.0, 0.5), 0.0, horse::GALLOP_SECONDS, Fettle::FRESH);
        let t0 = Instant::now();
        ride.predict(Reins::SLACK, &flat, 0.016);
        assert!(ride.rein_message(t0).is_none(), "slack reins were sent before anything was asked");
        ride.predict(Horseback::reins_from_keys(1.0, 0.0, false, false, false), &flat, 0.016);
        assert!(ride.rein_message(t0).is_some(), "the first ask was not sent");
        assert!(ride.rein_message(t0 + Duration::from_millis(100)).is_none(), "sent every frame");
        assert!(ride.rein_message(t0 + REIN_RESEND).is_some(), "not sent again while asking");
        ride.predict(Reins::SLACK, &flat, 0.016);
        assert!(ride.rein_message(t0 + REIN_RESEND).is_some(), "letting go was not sent");
        assert!(ride.rein_message(t0 + REIN_RESEND * 3).is_none(), "slack reins sent forever");
    }

    #[test]
    fn forward_is_a_trot_with_sprint_a_gallop_and_with_the_walk_key_a_walk() {
        assert_eq!(Horseback::reins_from_keys(1.0, 0.0, false, false, false).gait, Gait::Trot);
        assert_eq!(Horseback::reins_from_keys(1.0, 0.0, true, false, false).gait, Gait::Gallop);
        assert_eq!(Horseback::reins_from_keys(1.0, 0.0, true, true, false).gait, Gait::Walk);
    }
}
