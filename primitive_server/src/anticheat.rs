//! Базовый серверный античит.
//!
//! The rule this module exists to enforce is the one from the plan:
//! **никогда не доверяйте клиенту**. Everything a client asserts about
//! itself -- where it is, whether it's standing on something, what block
//! it just broke six hundred metres away -- is treated as a claim to be
//! checked, not a fact to be applied.
//!
//! What it checks
//! - **Sanity**: NaN/infinite coordinates, positions outside the world
//!   border. These are instant kicks; no legitimate client produces them.
//! - **Speed**, as a distance budget rather than an instantaneous
//!   `distance / dt`. Naive per-update speed checks are the classic
//!   source of false positives: after a network stall, several updates
//!   arrive back-to-back, so `dt` is tiny while the distance is real. A
//!   budget that refills at the legal speed and is spent by actual
//!   movement tolerates that burst while still capping *sustained* speed.
//! - **Teleport**: a single jump larger than any plausible step, which no
//!   amount of lag explains.
//! - **Flight/hover**: sustained climb while airborne, and floating in
//!   place with nothing underneath. Both cross-checked against the real
//!   world -- but only against chunks already in cache, so a client can't
//!   use these checks to make the server generate terrain on demand.
//! - **Reach**: block edits are measured from the player's *last
//!   server-known* eye position, not from a position supplied with the
//!   edit.
//! - **Rate limits**: per-message-class token buckets, so a flood of
//!   chunk requests or block edits costs the attacker a disconnect rather
//!   than costing the server its tick budget.
//!
//! What it deliberately does not do: full server-side movement
//! simulation. That's the real fix for movement cheating, and it needs
//! the server to run the same collision code as the client. This is the
//! "basic" layer -- it catches the obvious things cheaply, and the
//! violation scoring below is designed so a laggy honest player is never
//! kicked for one bad second.

use std::time::{Duration, Instant};

use primitive_shared::geometry::{EYE_HEIGHT, PLAYER_HEIGHT};
use primitive_shared::types::{
    is_collidable, is_liquid, is_placeable, BlockId, ChunkPos, BLOCK_AIR, CHUNK_SIZE_Y,
};

use crate::settings::AntiCheatSettings;
use crate::world::World;

// Violation weights. A kick needs several of these (default threshold
// 12.0, decaying 0.5/s), so one lag spike is never fatal.
const W_SPEED: f32 = 1.0;
const W_TELEPORT: f32 = 3.0;
const W_FLIGHT: f32 = 2.0;
const W_HOVER: f32 = 1.5;
const W_FAKE_GROUND: f32 = 2.0;
const W_REACH: f32 = 2.0;
const W_BAD_BLOCK: f32 = 3.0;
const W_OUT_OF_RANGE_CHUNK: f32 = 0.25;
const W_RATE_LIMIT: f32 = 1.5;
const W_REPLAY: f32 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Request is plausible; apply it.
    Allow,
    /// Request is refused. `correction` is where the server believes the
    /// player actually is -- when present, the client is rubber-banded
    /// back there.
    Reject {
        reason: String,
        correction: Option<(f32, f32, f32)>,
    },
    /// Enough accumulated violations; close the connection.
    Kick(String),
}

impl Verdict {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Verdict::Allow)
    }
}

/// Token bucket used for both rate limiting and the movement distance
/// budget.
#[derive(Debug)]
struct TokenBucket {
    tokens: f32,
    capacity: f32,
    refill_per_sec: f32,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(refill_per_sec: f32, burst_seconds: f32, now: Instant) -> Self {
        let capacity = (refill_per_sec * burst_seconds).max(1.0);
        Self {
            tokens: capacity,
            capacity,
            refill_per_sec,
            last_refill: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last_refill).as_secs_f32();
        if dt > 0.0 {
            self.tokens = (self.tokens + dt * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }

    fn take(&mut self, amount: f32, now: Instant) -> bool {
        self.refill(now);
        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }
}

pub struct AntiCheat {
    cfg: AntiCheatSettings,
    view_distance_chunks: i32,

    last_pos: Option<(f32, f32, f32)>,
    last_update: Instant,
    last_sequence: Option<u32>,

    /// Distance budget in blocks (see module docs).
    move_budget: TokenBucket,
    /// Height gained since the last time the player was on the ground or
    /// descended.
    ascent_run: f32,
    airborne_since: Option<Instant>,

    msg_bucket: TokenBucket,
    edit_bucket: TokenBucket,
    chunk_bucket: TokenBucket,
    transform_bucket: TokenBucket,
    chat_bucket: TokenBucket,

    score: f32,
    last_decay: Instant,
    pub total_violations: u32,
    pub last_reason: Option<String>,
}

impl AntiCheat {
    pub fn new(cfg: AntiCheatSettings, view_distance_chunks: i32, spawn: (f32, f32, f32)) -> Self {
        let now = Instant::now();
        Self {
            move_budget: TokenBucket::new(cfg.max_horizontal_speed, 1.5, now),
            msg_bucket: TokenBucket::new(cfg.max_messages_per_sec, 2.0, now),
            edit_bucket: TokenBucket::new(cfg.max_block_edits_per_sec, 2.0, now),
            chunk_bucket: TokenBucket::new(cfg.max_chunk_requests_per_sec, 3.0, now),
            transform_bucket: TokenBucket::new(cfg.max_transform_updates_per_sec, 2.0, now),
            chat_bucket: TokenBucket::new(cfg.max_chat_per_sec, 3.0, now),
            cfg,
            view_distance_chunks,
            last_pos: Some(spawn),
            last_update: now,
            last_sequence: None,
            ascent_run: 0.0,
            airborne_since: None,
            score: 0.0,
            last_decay: now,
            total_violations: 0,
            last_reason: None,
        }
    }

    pub fn score(&self) -> f32 {
        self.score
    }

    pub fn known_position(&self) -> Option<(f32, f32, f32)> {
        self.last_pos
    }

    fn decay(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last_decay).as_secs_f32();
        if dt > 0.0 {
            self.score = (self.score - dt * self.cfg.violation_decay_per_sec).max(0.0);
            self.last_decay = now;
        }
    }

    /// Records a violation and decides whether it's still survivable.
    fn flag(&mut self, weight: f32, reason: impl Into<String>) -> Verdict {
        let reason = reason.into();
        self.score += weight;
        self.total_violations += 1;
        self.last_reason = Some(reason.clone());
        if self.score >= self.cfg.violation_kick_threshold {
            Verdict::Kick(format!("{reason} (score {:.1})", self.score))
        } else {
            Verdict::Reject {
                reason,
                correction: self.last_pos,
            }
        }
    }

    /// Global per-connection message rate limit, checked before anything
    /// else looks at the message.
    pub fn check_message(&mut self) -> Verdict {
        if !self.cfg.enabled {
            return Verdict::Allow;
        }
        let now = Instant::now();
        self.decay(now);
        if !self.msg_bucket.take(1.0, now) {
            return self.flag(W_RATE_LIMIT, "message rate limit exceeded");
        }
        Verdict::Allow
    }

    pub fn check_transform(
        &mut self,
        x: f32,
        y: f32,
        z: f32,
        on_ground: bool,
        sequence: u32,
        world: &World,
    ) -> Verdict {
        if !self.cfg.enabled {
            self.last_pos = Some((x, y, z));
            return Verdict::Allow;
        }

        let now = Instant::now();
        self.decay(now);

        // --- sanity: never negotiable ---
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Verdict::Kick("non-finite position".to_string());
        }
        let border = self.cfg.world_border;
        if x.abs() > border || z.abs() > border || y < -256.0 || y > (CHUNK_SIZE_Y as f32 + 256.0) {
            return Verdict::Kick(format!("position outside world bounds ({x:.0},{y:.0},{z:.0})"));
        }

        if !self.transform_bucket.take(1.0, now) {
            return self.flag(W_RATE_LIMIT, "movement update rate limit exceeded");
        }

        // Out-of-order or replayed updates: cheap to detect, and they
        // would otherwise corrupt the speed budget below.
        if let Some(previous) = self.last_sequence {
            if sequence <= previous && previous.wrapping_sub(sequence) < u32::MAX / 2 {
                return self.flag(W_REPLAY, "out-of-order movement update");
            }
        }
        self.last_sequence = Some(sequence);

        let dt = now
            .saturating_duration_since(self.last_update)
            .as_secs_f32()
            .clamp(0.0, 1.0);
        self.last_update = now;

        let Some((px, py, pz)) = self.last_pos else {
            self.last_pos = Some((x, y, z));
            return Verdict::Allow;
        };

        let dx = x - px;
        let dy = y - py;
        let dz = z - pz;
        let horizontal = (dx * dx + dz * dz).sqrt();

        // --- teleport ---
        let total = (dx * dx + dy * dy + dz * dz).sqrt();
        if total > self.cfg.max_teleport_distance {
            return self.flag(W_TELEPORT, format!("teleport of {total:.1} blocks"));
        }

        // --- vertical speed ---
        if dt > 0.0 && dy.abs() / dt.max(0.05) > self.cfg.max_vertical_speed {
            return self.flag(
                W_SPEED,
                format!("vertical speed {:.1} b/s", dy.abs() / dt.max(0.05)),
            );
        }

        // --- horizontal speed, as a refillable budget ---
        self.move_budget.refill(now);
        if !self.move_budget.take(horizontal, now) {
            return self.flag(
                W_SPEED,
                format!("sustained speed above {:.1} b/s", self.cfg.max_horizontal_speed),
            );
        }

        // --- water ---
        // Swimming upward is a sustained climb with no ground contact,
        // which is precisely the flight signature below. Without this
        // check, adding swimming to the game would start kicking anyone
        // who got into a lake. The world is authoritative here: we ask
        // whether there is actually water where the player claims to be,
        // so a cheat can't just assert "I'm swimming" in mid-air.
        let in_water = [0.0f32, PLAYER_HEIGHT * 0.5, PLAYER_HEIGHT * 0.9]
            .iter()
            .any(|offset| {
                matches!(
                    world.cached_block(
                        x.floor() as i32,
                        (y + offset).floor() as i32,
                        z.floor() as i32,
                    ),
                    Some(id) if is_liquid(id)
                )
            });

        // --- flight / hover ---
        // `below` is None when we simply don't have that chunk cached; in
        // that case we give the player the benefit of the doubt rather
        // than generating terrain to prove a point.
        let below = world
            .cached_block(x.floor() as i32, (y - 0.1).floor() as i32, z.floor() as i32)
            .map(is_collidable);

        if in_water {
            // Buoyancy legitimately holds a player up and lets them
            // climb, so the flight and hover runs simply don't apply.
            // Speed limits above still do -- water is not a licence to
            // move at 100 blocks a second.
            self.ascent_run = 0.0;
            self.airborne_since = None;
        } else if on_ground {
            // Claiming to stand on something *while climbing*, with air
            // demonstrably underneath, is the signature of a flight cheat
            // that lies about `on_ground` to dodge the ascent check below.
            if self.cfg.verify_ground && dy > 0.05 && below == Some(false) {
                return self.flag(W_FAKE_GROUND, "claimed ground contact while ascending over air");
            }
            self.ascent_run = 0.0;
            self.airborne_since = None;
        } else {
            if dy < -0.05 {
                self.ascent_run = 0.0; // any real fall resets the run
            } else {
                self.ascent_run += dy.max(0.0);
            }
            if self.ascent_run > self.cfg.max_airborne_ascent {
                self.ascent_run = 0.0;
                return self.flag(
                    W_FLIGHT,
                    format!("climbed {:.1} blocks without ground contact", self.cfg.max_airborne_ascent),
                );
            }

            let airborne_since = *self.airborne_since.get_or_insert(now);
            // Hovering: airborne, not losing height, still moving around,
            // and we can see there's nothing underneath. The horizontal
            // requirement is what keeps a player standing on another
            // player's hitbox (legal in this game) from being flagged.
            if now.saturating_duration_since(airborne_since)
                > Duration::from_secs_f32(self.cfg.max_hover_seconds)
                && dy.abs() < 0.02
                && horizontal > 0.05
                && below == Some(false)
            {
                self.airborne_since = Some(now);
                return self.flag(W_HOVER, "hovering in mid-air");
            }
        }

        self.last_pos = Some((x, y, z));
        Verdict::Allow
    }

    pub fn check_block_edit(&mut self, gx: i32, gy: i32, gz: i32, block: BlockId) -> Verdict {
        if !self.cfg.enabled {
            return Verdict::Allow;
        }
        let now = Instant::now();
        self.decay(now);

        if !self.edit_bucket.take(1.0, now) {
            return self.flag(W_RATE_LIMIT, "block edit rate limit exceeded");
        }

        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return self.flag(W_BAD_BLOCK, format!("block edit outside world height (y={gy})"));
        }

        // Air means "break"; anything else must be a block the game
        // actually offers. This is where a client asking to place an
        // invented block id, or water, gets refused.
        if block != BLOCK_AIR && !is_placeable(block) {
            return self.flag(W_BAD_BLOCK, format!("block id {block} is not placeable"));
        }

        if let Some((px, py, pz)) = self.last_pos {
            let ex = px;
            let ey = py + EYE_HEIGHT;
            let ez = pz;
            let dx = (gx as f32 + 0.5) - ex;
            let dy = (gy as f32 + 0.5) - ey;
            let dz = (gz as f32 + 0.5) - ez;
            let distance = (dx * dx + dy * dy + dz * dz).sqrt();
            if distance > self.cfg.max_reach {
                return self.flag(W_REACH, format!("reach of {distance:.1} blocks"));
            }
        }

        Verdict::Allow
    }

    pub fn check_chunk_request(&mut self, pos: ChunkPos) -> Verdict {
        if !self.cfg.enabled {
            return Verdict::Allow;
        }
        let now = Instant::now();
        self.decay(now);

        if !self.chunk_bucket.take(1.0, now) {
            return self.flag(W_RATE_LIMIT, "chunk request rate limit exceeded");
        }

        if let Some((x, _, z)) = self.last_pos {
            let player_chunk = ChunkPos::from_world(x, z);
            // +2 of slack: the player keeps moving while their request is
            // in flight.
            if player_chunk.chebyshev_distance(pos) > self.view_distance_chunks + 2 {
                return self.flag(
                    W_OUT_OF_RANGE_CHUNK,
                    "chunk request outside view distance",
                );
            }
        }
        Verdict::Allow
    }

    pub fn check_chat(&mut self) -> Verdict {
        if !self.cfg.enabled {
            return Verdict::Allow;
        }
        let now = Instant::now();
        self.decay(now);
        if !self.chat_bucket.take(1.0, now) {
            return self.flag(W_RATE_LIMIT, "chat rate limit exceeded");
        }
        Verdict::Allow
    }

    /// Called after a correction is sent, so the next update is measured
    /// against where we put the player rather than where they claimed.
    pub fn reset_to(&mut self, pos: (f32, f32, f32)) {
        self.last_pos = Some(pos);
        self.ascent_run = 0.0;
        self.airborne_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_STONE, BLOCK_WATER};

    fn cfg() -> AntiCheatSettings {
        AntiCheatSettings::default()
    }

    fn empty_world() -> World {
        // No chunks cached => every `cached_block` is None => the
        // world-aware checks stay neutral, which is what we want when
        // testing the purely kinematic rules.
        World::new(1, 64)
    }

    #[test]
    fn normal_walking_is_never_flagged() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let mut x = 0.0f32;
        for seq in 1..40u32 {
            std::thread::sleep(Duration::from_millis(20));
            x += 0.11; // ~5.5 b/s at 50 ms per update
            let verdict = ac.check_transform(x, 30.0, 0.0, true, seq, &world);
            assert!(verdict.is_allowed(), "walking flagged: {verdict:?}");
        }
        assert_eq!(ac.total_violations, 0);
    }

    #[test]
    fn teleporting_across_the_map_is_caught() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let verdict = ac.check_transform(5000.0, 30.0, 5000.0, true, 1, &world);
        assert!(!verdict.is_allowed(), "teleport was allowed");
        match verdict {
            Verdict::Reject { correction, .. } => {
                assert_eq!(correction, Some((0.0, 30.0, 0.0)), "must rubber-band back");
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn sustained_speedhack_exhausts_the_budget() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let mut x = 0.0f32;
        let mut rejected = false;
        for seq in 1..40u32 {
            std::thread::sleep(Duration::from_millis(20));
            x += 2.0; // 100 b/s -- ~18x the legal walk speed
            if !ac.check_transform(x, 30.0, 0.0, true, seq, &world).is_allowed() {
                rejected = true;
                break;
            }
        }
        assert!(rejected, "a 100 b/s speedhack was never flagged");
    }

    #[test]
    fn a_lag_burst_is_tolerated() {
        // Four updates arriving back-to-back after a stall: each covers a
        // legal distance, they just arrive with almost no `dt` between
        // them. A naive distance/dt check would kick here.
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        std::thread::sleep(Duration::from_millis(400));
        let mut x = 0.0f32;
        for seq in 1..5u32 {
            x += 0.5;
            assert!(
                ac.check_transform(x, 30.0, 0.0, true, seq, &world).is_allowed(),
                "honest player punished for a lag burst"
            );
        }
    }

    #[test]
    fn climbing_forever_while_airborne_is_flight() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let mut y = 30.0f32;
        let mut flagged = false;
        for seq in 1..30u32 {
            std::thread::sleep(Duration::from_millis(10));
            y += 0.4;
            if !ac.check_transform(0.0, y, 0.0, false, seq, &world).is_allowed() {
                flagged = true;
                break;
            }
        }
        assert!(flagged, "unbounded ascent was never flagged as flight");
    }

    /// A world with water filling y = 20..=40 in the chunk at the origin.
    ///
    /// The chunk has to be *cached* for `cached_block` to see it -- an
    /// edit alone only goes into the overlay, and the anti-cheat
    /// deliberately never generates terrain just to answer a question.
    fn flooded_world() -> World {
        let world = World::new(1, 64);
        let chunk = world.generate(ChunkPos::new(0, 0));
        world.insert(chunk);
        for y in 20..=40 {
            world.set_block(0, y, 0, BLOCK_WATER);
        }
        world
    }

    #[test]
    fn swimming_upward_is_not_flight() {
        // Regression guard for the whole water feature: buoyancy means a
        // long climb with no ground contact, which is exactly the flight
        // signature. Without the water check, adding swimming would have
        // started kicking anyone who jumped in a lake.
        let world = flooded_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.5, 22.0, 0.5));
        let mut y = 22.0f32;
        for seq in 1..40u32 {
            std::thread::sleep(Duration::from_millis(10));
            y += 0.15; // steady swim upward
            let verdict = ac.check_transform(0.5, y, 0.5, false, seq, &world);
            assert!(verdict.is_allowed(), "swimming flagged at y={y}: {verdict:?}");
        }
        assert_eq!(ac.total_violations, 0);
    }

    #[test]
    fn claiming_to_swim_in_mid_air_is_still_flight() {
        // The world decides, not the client: the same ascent away from
        // the water must still be caught.
        let world = flooded_world();
        let mut ac = AntiCheat::new(cfg(), 8, (60.0, 22.0, 60.0));
        let mut y = 22.0f32;
        let mut flagged = false;
        for seq in 1..40u32 {
            std::thread::sleep(Duration::from_millis(10));
            y += 0.15;
            if !ac.check_transform(60.0, y, 60.0, false, seq, &world).is_allowed() {
                flagged = true;
                break;
            }
        }
        assert!(flagged, "ascent outside water should still be flight");
    }

    #[test]
    fn an_ordinary_jump_is_not_flight() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        // Client physics: jump 8.0 m/s, gravity -22 => apex ~1.45 blocks.
        let arc = [0.35, 0.30, 0.24, 0.16, 0.07, -0.07, -0.16, -0.24, -0.30, -0.35];
        let mut y = 30.0f32;
        for (i, dy) in arc.iter().enumerate() {
            std::thread::sleep(Duration::from_millis(10));
            y += dy;
            let verdict = ac.check_transform(0.0, y, 0.0, false, i as u32 + 1, &world);
            assert!(verdict.is_allowed(), "a normal jump was flagged: {verdict:?}");
        }
    }

    #[test]
    fn reach_is_measured_from_the_last_known_position() {
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        assert!(ac.check_block_edit(2, 30, 0, BLOCK_STONE).is_allowed());
        let far = ac.check_block_edit(400, 30, 0, BLOCK_STONE);
        assert!(!far.is_allowed(), "a 400-block reach was allowed");
    }

    #[test]
    fn placing_an_impossible_block_is_refused() {
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        assert!(!ac.check_block_edit(1, 30, 0, 60000).is_allowed(), "unknown id");
        assert!(!ac.check_block_edit(1, 30, 0, BLOCK_WATER).is_allowed(), "water");
        // Breaking (placing air) stays legal.
        assert!(ac.check_block_edit(1, 30, 0, BLOCK_AIR).is_allowed());
    }

    #[test]
    fn chunk_requests_far_from_the_player_are_refused() {
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        assert!(ac.check_chunk_request(ChunkPos::new(3, 3)).is_allowed());
        assert!(!ac.check_chunk_request(ChunkPos::new(9999, 0)).is_allowed());
    }

    #[test]
    fn flooding_block_edits_hits_the_rate_limit_then_the_kick_threshold() {
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let mut kicked = false;
        for _ in 0..200 {
            if let Verdict::Kick(_) = ac.check_block_edit(1, 30, 0, BLOCK_STONE) {
                kicked = true;
                break;
            }
        }
        assert!(kicked, "an unlimited edit flood never resulted in a kick");
    }

    #[test]
    fn violations_decay_so_a_bad_second_is_survivable() {
        let mut settings = cfg();
        settings.violation_decay_per_sec = 100.0;
        let mut ac = AntiCheat::new(settings, 8, (0.0, 30.0, 0.0));
        let world = empty_world();
        let _ = ac.check_transform(500.0, 30.0, 0.0, true, 1, &world); // teleport
        assert!(ac.score() > 0.0);
        std::thread::sleep(Duration::from_millis(120));
        let _ = ac.check_message();
        assert_eq!(ac.score(), 0.0, "score should have decayed back to zero");
    }

    #[test]
    fn disabling_the_anticheat_allows_everything() {
        let mut settings = cfg();
        settings.enabled = false;
        let mut ac = AntiCheat::new(settings, 8, (0.0, 30.0, 0.0));
        let world = empty_world();
        assert!(ac.check_transform(99999.0, 30.0, 0.0, false, 1, &world).is_allowed());
        assert!(ac.check_block_edit(9999, 30, 0, BLOCK_STONE).is_allowed());
    }
}
