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
    is_collidable, is_known_block, is_placeable, BlockId, ChunkPos, BLOCK_AIR,
    CHUNK_SIZE_Y,
};

use crate::settings::AntiCheatSettings;
use crate::logic::world::World;

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
        correction: Option<(f64, f64, f64)>,
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

    last_pos: Option<(f64, f64, f64)>,
    last_update: Instant,
    last_sequence: Option<u32>,

    /// Distance budget in blocks (see module docs).
    move_budget: TokenBucket,
    /// Height gained since the last time the player was on the ground or
    /// descended.
    ascent_run: f32,
    airborne_since: Option<Instant>,

    /// This player has been *granted* flight, so the checks that measure
    /// motion against what a body on the ground can do do not apply.
    ///
    /// **Not the same thing as turning the anti-cheat off, and the
    /// difference is the whole point.** Sanity, the rate limits, the
    /// replay check, reach on block edits and the block table are all
    /// still enforced -- a flying player who starts editing cells six
    /// hundred metres away is caught by exactly the code that would
    /// catch a walking one. What is suspended is the ascent run, the
    /// hover timer and the two speed limits, because those measure
    /// motion against a set of rules this player has been told not to
    /// follow.
    ///
    /// Set only by the server, from `set_flight`. Nothing a client
    /// sends can reach it.
    flying: bool,

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

/// Is there anything solid under the player's *feet*, as opposed to
/// under the single point at the middle of them?
///
/// `None` when the chunk is not cached: the caller gives the player the
/// benefit of the doubt rather than generating terrain to prove a point.
///
/// **The footprint rather than the centre, and that is a 1.5 fix.** The
/// probe used to be one sample under the middle of the player, which was
/// exactly right while every solid block in the world filled its cell:
/// there was nothing short enough to stand on the edge of. Half-height
/// blocks -- a campfire, a dropped pack -- changed that, and they broke
/// it in the most visible way possible.
///
/// Stepping onto one, the client raises the player before it moves them
/// horizontally into the block's own column. For a frame or two they are
/// therefore *above* the ground they are leaving, rising, and honestly
/// reporting that they are standing on something -- which is the exact
/// signature of the flight cheat this check exists to catch. Every
/// player who tried to stand on a campfire was kicked.
///
/// Four corners of the collider, so what counts as ground is what the
/// collider actually rests on.
fn ground_under(world: &crate::logic::world::World, x: f64, y: f64, z: f64) -> Option<bool> {
    use primitive_shared::geometry::PLAYER_HALF_WIDTH;

    let foot = (y - 0.1).floor() as i32;
    let mut known = false;
    for (dx, dz) in [
        (-PLAYER_HALF_WIDTH, -PLAYER_HALF_WIDTH),
        (PLAYER_HALF_WIDTH, -PLAYER_HALF_WIDTH),
        (-PLAYER_HALF_WIDTH, PLAYER_HALF_WIDTH),
        (PLAYER_HALF_WIDTH, PLAYER_HALF_WIDTH),
    ] {
        match world.cached_block((x + f64::from(dx)).floor() as i32, foot, (z + f64::from(dz)).floor() as i32) {
            // Anything solid under any corner is ground. A player with
            // one foot on a ledge is standing on it.
            //
            // **...and any piece of a tree.** A twig's row is not
            // collidable (that line is about the cell, `types::is_collidable`)
            // and its wood is stood on since branches were given their
            // shape (`branch`): a player on a sapling's limb, told there was
            // air under them, would be a hover cheat for standing still.
            // ...and the point of a stalagmite, which is stood on for the
            // same reason (`dripstone::body`). Not a stalactite: its box is at
            // the top of its cell, and a player whose feet are level with one
            // is beside it in the air.
            Some(block)
                if is_collidable(block)
                    || primitive_shared::types::is_branch(block)
                    || primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_STALAGMITE =>
            {
                return Some(true)
            }
            Some(_) => known = true,
            None => {}
        }
    }
    known.then_some(false)
}

impl AntiCheat {
    pub fn new(cfg: AntiCheatSettings, view_distance_chunks: i32, spawn: (f64, f64, f64)) -> Self {
        let now = Instant::now();
        Self {
            flying: false,
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

    pub fn known_position(&self) -> Option<(f64, f64, f64)> {
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
        x: f64,
        y: f64,
        z: f64,
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
        if x.abs() > f64::from(border) || z.abs() > f64::from(border) || y < -256.0 || y > (CHUNK_SIZE_Y as f64 + 256.0) {
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

        // Granted flight: everything from here down measures movement
        // against what a body on the ground can do, and this player has
        // been told they are not one. See the `flying` field for what is
        // *still* checked.
        if self.flying {
            self.move_budget.refill(now);
            self.ascent_run = 0.0;
            self.airborne_since = None;
            self.last_pos = Some((x, y, z));
            return Verdict::Allow;
        }

        let Some((px, py, pz)) = self.last_pos else {
            self.last_pos = Some((x, y, z));
            return Verdict::Allow;
        };

        // The move in `f64` and only then narrowed: two positions a million
        // blocks out differ by a stride, and each alone has sixteenths
        // between its neighbours in `f32`.
        let dx = (x - px) as f32;
        let dy = (y - py) as f32;
        let dz = (z - pz) as f32;
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
        //
        // How deep the water is, not merely whether the cell holds
        // some: a cell can hold an eighth now, and an eighth-deep film
        // is not something anybody swims in. Asking `is_liquid` would
        // hand the flight exemption to a player standing on the last
        // wet patch of a drained puddle, which is a place a cheat can
        // arrange to be.
        //
        // The cell above comes along for the same reason the drowning
        // check takes it: a full cell of water stops `SURFACE_DROP`
        // short of its ceiling, so the top twelfth of a *submerged* cell
        // would otherwise read as air. Here that meant a swimmer whose
        // sample points all happened to land in those bands losing the
        // buoyancy exemption for a tick and being read as flying, which
        // is the one thing this exemption exists to prevent.
        let in_water = [0.0f32, PLAYER_HEIGHT * 0.5, PLAYER_HEIGHT * 0.9]
            .iter()
            .any(|offset| {
                let height = y + f64::from(*offset);
                let (cx, cy, cz) = (x.floor() as i32, height.floor() as i32, z.floor() as i32);
                world.cached_block(cx, cy, cz).is_some_and(|id| {
                    let above = world
                        .cached_block(cx, cy + 1, cz)
                        .unwrap_or(primitive_shared::types::BLOCK_AIR);
                    primitive_shared::fluid::covers_with_above(
                        id,
                        above,
                        (height - height.floor()) as f32,
                    )
                })
            });

        // --- flight / hover ---
        // `below` is None when we simply don't have that chunk cached; in
        // that case we give the player the benefit of the doubt rather
        // than generating terrain to prove a point.
        let below = ground_under(world, x, y, z);

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
                // ...and the hover clock with it. **A player holding jump
                // touches the ground for one frame a hop**, and the client
                // sends a transform every few frames, so the landing is
                // seldom the frame that is sent: the clock ran on from the
                // first hop, and a few seconds into a run of hops any
                // sample taken at the top of one -- where a jump stops
                // rising and has not begun to fall -- read as "hovering in
                // mid-air" and put the player back. A body that has just
                // dropped is not hovering, whatever it claimed about ground.
                self.airborne_since = None;
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
        //
        // `is_known_block` as well as `is_placeable`, because the two
        // ask different things now that an id carries an orientation.
        // Placeable is about the *kind* -- a log may be placed -- and
        // the client picks the direction bits itself, so those have to
        // be checked separately: an axis of 3 means nothing, and a
        // rotated cobblestone would be a second id for a block that has
        // only one.
        // **A slice off a block is neither a break nor a placement**, and
        // `is_placeable` is the wrong question to ask about one: it says
        // whether a player may *put this down*, and nobody puts down three
        // quarters of a granite block -- an ore that cannot be placed at
        // all would have made every swing at a vein an anti-cheat flag.
        // What still has to hold is that the id is a real one, which is
        // where an invented face or a bite on something nobody quarries is
        // refused (`is_known_block`); the reach and the rate above are
        // asked of a dig exactly as of any other edit, which is the whole
        // reason a dig comes through here at all.
        if primitive_shared::dig::is_dug(block) {
            if !is_known_block(block) {
                return self.flag(W_BAD_BLOCK, format!("block id {block} is not a dig"));
            }
        } else if block != BLOCK_AIR && (!is_placeable(block) || !is_known_block(block)) {
            return self.flag(W_BAD_BLOCK, format!("block id {block} is not placeable"));
        }

        // ...and one variant bit that is legal on a block the *server*
        // writes and never on one a client asks for. A rack carries
        // "there is a skin on me" in the spare bit of the same field its
        // facing lives in (`types::RACK_LOADED`), and what is actually
        // on a rack is server state keyed by position -- so a placement
        // claiming a hide would draw one the player never had.
        if primitive_shared::types::rack_is_loaded(block) {
            // The condition above is true exactly when the client's
            // placement *claims a hide*, so the reason has to say that --
            // this string reaches the player verbatim (`edit refused:
            // {reason}` in `net::connection`) and used to claim the
            // opposite of what was refused, which told whoever read it
            // the wrong story about their own rejected edit.
            return self.flag(W_BAD_BLOCK, "a rack is placed already loaded".to_string());
        }
        // ...and the same bit on a bed means "the head half", which the
        // server writes behind the foot a client asks for. A client that
        // placed a head itself would be placing half a bed, or a second
        // one on top of the half the server is about to write.
        if primitive_shared::types::is_bed_head(block) {
            return self.flag(W_BAD_BLOCK, "a bed is placed by its foot".to_string());
        }

        if let Some((px, py, pz)) = self.last_pos {
            let ex = px;
            let ey = py + f64::from(EYE_HEIGHT);
            let ez = pz;
            let dx = (f64::from(gx) + 0.5 - ex) as f32;
            let dy = (f64::from(gy) + 0.5 - ey) as f32;
            let dz = (f64::from(gz) + 0.5 - ez) as f32;
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
    /// Grants or withdraws the exemption. See the [`flying`](Self::flying)
    /// field.
    ///
    /// Withdrawing resets the budgets rather than leaving them as flight
    /// left them: a player who has just been dropped out of the air is
    /// falling fast and through no fault of their own, and a stale
    /// ascent run or a spent movement budget would flag them for it.
    pub fn set_flying(&mut self, flying: bool) {
        self.flying = flying;
        self.ascent_run = 0.0;
        self.airborne_since = None;
        if !flying {
            self.move_budget.refill(Instant::now());
        }
    }

    pub fn is_flying(&self) -> bool {
        self.flying
    }

    pub fn reset_to(&mut self, pos: (f64, f64, f64)) {
        self.last_pos = Some(pos);
        self.ascent_run = 0.0;
        self.airborne_since = None;
    }
}

#[cfg(test)]
mod tests {
    // Cobblestone rather than stone: dressed stone stopped being
    // placeable when it stopped being breakable by hand, and these
    // tests need a block a player can actually put down.
    use super::*;
    use primitive_shared::types::{BLOCK_COBBLESTONE, BLOCK_WATER};

    fn cfg() -> AntiCheatSettings {
        AntiCheatSettings::default()
    }

    fn empty_world() -> World {
        // No chunks cached => every `cached_block` is None => the
        // world-aware checks stay neutral, which is what we want when
        // testing the purely kinematic rules.
        World::new(1, 64)
    }

    /// A world with a floor of cobble at y = 19 and one half-height
    /// block -- a campfire -- standing on it at x = 1.
    fn world_with_a_campfire() -> World {
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_CAMPFIRE, CHUNK_VOLUME};
        let world = World::new(1, 64);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..16 {
            for x in 0..16 {
                blocks[Chunk::index(x, 19, z)] = BLOCK_COBBLESTONE;
            }
        }
        blocks[Chunk::index(1, 20, 1)] = BLOCK_CAMPFIRE;
        world.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks,
        });
        world
    }

    #[test]
    fn stepping_onto_a_campfire_is_not_a_flight_cheat() {
        // **The bug this test exists for.** Half-height blocks arrived in
        // 1.5 and with them, for the first time, something a player can
        // step *onto*. The client raises the player before it moves them
        // horizontally into the block's column -- so for a frame they are
        // above the ground they are leaving, rising, and honestly
        // reporting that they are standing on something. That is the
        // exact signature of the flight cheat this check hunts, and
        // every player who tried to stand on their own campfire was
        // kicked for it.
        //
        // The player's centre is deliberately still over the *empty*
        // column here: that is the frame that did it.
        let world = world_with_a_campfire();
        let mut ac = AntiCheat::new(cfg(), 8, (1.45, 20.0, 1.5));
        std::thread::sleep(Duration::from_millis(20));
        let verdict = ac.check_transform(1.45, 20.5, 1.5, true, 1, &world);
        assert!(
            verdict.is_allowed(),
            "stepping onto a campfire was refused: {verdict:?}"
        );
        assert_eq!(ac.total_violations, 0);
    }

    #[test]
    fn hovering_over_nothing_at_all_is_still_caught() {
        // The other half: widening the probe to the whole footprint must
        // not blind it. A player rising with air under every corner is
        // the cheat, and is still flagged.
        let world = world_with_a_campfire();
        let mut ac = AntiCheat::new(cfg(), 8, (8.5, 24.0, 8.5));
        std::thread::sleep(Duration::from_millis(20));
        let verdict = ac.check_transform(8.5, 24.4, 8.5, true, 1, &world);
        assert!(!verdict.is_allowed(), "a flight cheat was allowed");
    }

    #[test]
    fn normal_walking_is_never_flagged() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let mut x = 0.0f32;
        for seq in 1..40u32 {
            std::thread::sleep(Duration::from_millis(20));
            x += 0.11; // ~5.5 b/s at 50 ms per update
            let verdict = ac.check_transform(f64::from(x), 30.0, 0.0, true, seq, &world);
            assert!(verdict.is_allowed(), "walking flagged: {verdict:?}");
        }
        assert_eq!(ac.total_violations, 0);
    }

    /// A granted flyer climbing steadily is doing what they were told
    /// they may. Without the exemption this is the exact signature of
    /// the cheat the module exists to catch, so the first thing a
    /// flight mod would do is get its own players kicked.
    #[test]
    fn a_granted_flyer_is_not_flagged_for_flying() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 64.0, 0.0));
        ac.set_flying(true);
        let mut y = 64.0;
        for sequence in 1..60 {
            y += 0.5;
            let verdict = ac.check_transform(0.0, y, 0.0, false, sequence, &world);
            assert!(
                verdict.is_allowed(),
                "flagged at y={y}: {verdict:?}"
            );
        }
        assert_eq!(ac.score(), 0.0, "a granted flyer collected violations");
    }

    /// ...and the exemption is withdrawn with the grant, rather than
    /// leaving the player permanently unwatched.
    #[test]
    fn withdrawing_flight_puts_the_checks_back() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 64.0, 0.0));
        ac.set_flying(true);
        let mut y = 64.0;
        for sequence in 1..20 {
            y += 0.5;
            ac.check_transform(0.0, y, 0.0, false, sequence, &world);
        }
        ac.set_flying(false);
        assert!(!ac.is_flying());

        // Now climb far enough to trip the ascent run.
        let mut flagged = false;
        for sequence in 20..200 {
            y += 0.5;
            if !ac.check_transform(0.0, y, 0.0, false, sequence, &world).is_allowed() {
                flagged = true;
                break;
            }
        }
        assert!(flagged, "climbing was still unwatched after flight ended");
    }

    /// The exemption is not "the anti-cheat off". A position that is not
    /// a number is a kick whoever sent it.
    #[test]
    fn a_flyer_is_still_held_to_the_things_that_are_never_negotiable() {
        let world = empty_world();
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 64.0, 0.0));
        ac.set_flying(true);
        assert!(matches!(
            ac.check_transform(f64::from(f32::NAN), 64.0, 0.0, false, 1, &world),
            Verdict::Kick(_)
        ));
        assert!(matches!(
            ac.check_transform(0.0, 1.0e9, 0.0, false, 2, &world),
            Verdict::Kick(_)
        ));
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
        // **Paced on the clock rather than on the sleep.** This moved two
        // blocks per twenty-millisecond sleep and called that 100 b/s --
        // which it is only while the sleep is twenty milliseconds. On a
        // machine busy compiling, each sleep stretched towards 150 ms,
        // the real speed fell to thirteen blocks a second against a
        // legal twelve, the budget took fourteen seconds to drain and
        // thirty-nine updates were six: the test went red with the rule
        // working. Each step is now what 100 b/s covers in the time that
        // actually passed, kept under the teleport distance so a long
        // stall is still measured as speed and not flagged as a jump.
        let mut last = std::time::Instant::now();
        for seq in 1..400u32 {
            std::thread::sleep(Duration::from_millis(20));
            let now = std::time::Instant::now();
            x += (100.0 * now.duration_since(last).as_secs_f32()).min(20.0);
            last = now;
            if !ac.check_transform(f64::from(x), 30.0, 0.0, true, seq, &world).is_allowed() {
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
                ac.check_transform(f64::from(x), 30.0, 0.0, true, seq, &world).is_allowed(),
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
            if !ac.check_transform(0.0, f64::from(y), 0.0, false, seq, &world).is_allowed() {
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
            let verdict = ac.check_transform(0.5, f64::from(y), 0.5, false, seq, &world);
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
            if !ac.check_transform(60.0, f64::from(y), 60.0, false, seq, &world).is_allowed() {
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
            let verdict = ac.check_transform(0.0, f64::from(y), 0.0, false, i as u32 + 1, &world);
            assert!(verdict.is_allowed(), "a normal jump was flagged: {verdict:?}");
        }
    }

    #[test]
    fn reach_is_measured_from_the_last_known_position() {
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        assert!(ac.check_block_edit(2, 30, 0, BLOCK_COBBLESTONE).is_allowed());
        let far = ac.check_block_edit(400, 30, 0, BLOCK_COBBLESTONE);
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
    fn a_chest_may_be_put_down_facing_any_of_the_four_ways() {
        // The client turns a chest toward whoever placed it, so the id
        // that arrives carries a facing three times out of four. This
        // refused all three of those, and each refusal was a quarter of
        // a kick.
        use primitive_shared::types::{faced, Facing, BLOCK_CHEST, BLOCK_KILN};
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            for kind in [BLOCK_CHEST, BLOCK_KILN] {
                let verdict = ac.check_block_edit(1, 30, 0, faced(kind, facing));
                assert!(
                    verdict.is_allowed(),
                    "a {kind} facing {facing:?} was refused: {verdict:?}"
                );
            }
        }
    }

    #[test]
    fn a_rack_cannot_be_placed_with_a_hide_already_on_it() {
        // What is on a rack is server state keyed by position; the bit
        // in the id only says how to draw it. A client that set it would
        // be drawing a skin it never had.
        use primitive_shared::types::{faced, rack_with_hide, Facing, BLOCK_DRYING_RACK};
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let empty = faced(BLOCK_DRYING_RACK, Facing::South);
        assert!(ac.check_block_edit(1, 30, 0, empty).is_allowed());
        let loaded = rack_with_hide(empty, true);
        assert!(!ac.check_block_edit(1, 30, 0, loaded).is_allowed());
    }

    #[test]
    fn the_rack_refusal_names_the_fault_that_actually_happened() {
        // `edit refused: {reason}` is what a player and an operator both
        // read (see `net::connection`, which sends this string straight
        // back over the wire). The check fires when the placed rack
        // *claims a hide*, so a reason claiming the opposite -- "placed
        // empty" -- tells whoever reads it the wrong story: it says the
        // rack is missing something the client actually over-claimed.
        use primitive_shared::types::{faced, rack_with_hide, Facing, BLOCK_DRYING_RACK};
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let loaded = rack_with_hide(faced(BLOCK_DRYING_RACK, Facing::South), true);
        let Verdict::Reject { reason, .. } = ac.check_block_edit(1, 30, 0, loaded) else {
            panic!("a loaded rack placement was not refused at all");
        };
        assert!(
            !reason.contains("empty"),
            "the rack was refused for claiming a hide, not for being empty: {reason:?}"
        );
    }

    #[test]
    fn a_slice_off_a_rock_is_let_through_and_an_invented_bite_is_not() {
        // A dig writes a block id into the world on every swing, so it
        // comes through the same door every other edit does -- and the
        // question asked of it cannot be `is_placeable`: nobody puts down
        // three quarters of a granite block, and an ore that cannot be
        // placed at all would have made every swing at a vein a violation.
        // What still has to hold is that the id names a real bite, which
        // is what stops a client claiming the last quarter on the first
        // swing or a bite on a block nobody quarries.
        use primitive_shared::dig;
        let mut ac = AntiCheat::new(cfg(), 8, (0.0, 30.0, 0.0));
        let slice = dig::next_bite(primitive_shared::types::BLOCK_STONE, dig::Side::PosX).unwrap();
        assert!(ac.check_block_edit(1, 30, 0, slice).is_allowed(), "an honest swing was refused");
        // A face of six that names nothing, and a bite on a log.
        assert!(!ac
            .check_block_edit(1, 30, 0, primitive_shared::types::BLOCK_STONE | dig::DUG | (7 << primitive_shared::types::VARIANT_SHIFT))
            .is_allowed());
        assert!(!ac
            .check_block_edit(1, 30, 0, primitive_shared::types::BLOCK_LOG | dig::DUG)
            .is_allowed());
        // ...and a dig across the valley is still a dig across the valley.
        assert!(!ac.check_block_edit(400, 30, 400, slice).is_allowed(), "reach is not asked of a dig");
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
            if let Verdict::Kick(_) = ac.check_block_edit(1, 30, 0, BLOCK_COBBLESTONE) {
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
        assert!(ac.check_block_edit(9999, 30, 0, BLOCK_COBBLESTONE).is_allowed());
    }
}
