//! Connected-player bookkeeping.
//!
//! Two things here are specifically about surviving a lot of players:
//!
//! * **Bounded outgoing queues with an explicit drop policy.** The old
//!   version used an unbounded channel, which converts a slow client into
//!   unbounded server memory growth -- one stalled TCP connection can
//!   take the process down. Every queue here is bounded; a full queue
//!   drops the message and increments a counter, and a client that has
//!   dropped too many is disconnected. Losing a laggy client is a much
//!   better failure mode than losing the server.
//!
//! * **A chunk -> subscribers index.** Broadcasting a block edit used to
//!   walk every connected player and test whether they had that chunk
//!   loaded: O(players) per edit. With the reverse index it's
//!   O(players who can actually see it), which at 500 players spread over
//!   a map is a completely different number.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use tokio::sync::{mpsc, Notify};

use primitive_shared::geometry::block_overlaps_player;
use primitive_shared::protocol::{
    DisconnectReason, EntityState, PlayerId, PlayerState, ServerMessage,
};
use primitive_shared::types::{BlockId, ChunkPos};

use crate::logic::anticheat::AntiCheat;

/// What a player flies at unless whatever granted it says otherwise.
///
/// Twice a walk and a little under a sprint-and-a-half: fast enough that
/// flying somewhere is quicker than walking, slow enough that the chunk
/// streamer keeps up. A grant that names its own speed overrides it.
pub const DEFAULT_FLY_SPEED: f32 = 12.0;
use crate::logic::survival::Vitals;
use primitive_shared::inventory::Inventory;

/// One entry in a player's outgoing queue.
///
/// Two shapes because there are two kinds of message. Something composed
/// for one recipient -- a snapshot, an inventory, a chunk -- is queued as
/// the message itself and serialised by that player's writer task. But a
/// broadcast is the *same* bytes for everyone, and serialising a chat
/// line once per recipient was O(players) identical bincode runs per
/// message; those are serialised once, up front, and every queue gets an
/// `Arc` over the one buffer.
///
/// `Raw` carries the complete frame -- length prefix included -- built by
/// `primitive_shared::net::frame_message`, the same function the
/// `Message` path writes through. One framing function, so the two paths
/// cannot drift.
pub enum Outgoing {
    Message(ServerMessage),
    Raw(Arc<[u8]>),
}

/// A broadcastable message as ready-to-send bytes, shared by `Arc`.
///
/// `None` only if serialisation failed, which for our own protocol types
/// means a bug rather than a condition to handle; the callers drop the
/// message, which is also what the writer task did with one it could not
/// serialise.
pub fn frame(msg: &ServerMessage) -> Option<Arc<[u8]>> {
    primitive_shared::net::frame_message(msg)
        .ok()
        .map(Arc::from)
}

/// Mutable per-player state. Guarded by a short-lived std mutex; nothing
/// in here is ever held across an `.await`.
pub struct PlayerRuntime {
    pub position: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub last_activity: Instant,
    pub loaded_chunks: HashSet<ChunkPos>,
    pub anticheat: AntiCheat,
    /// Health, fall tracking and death. Server-owned: the client is only
    /// ever told what it is.
    pub vitals: Vitals,
    /// What the player is carrying. Server-owned for the same reason
    /// health is: it decides fall damage, what a placement spends and
    /// what a break yields, and none of those can be left to the client.
    pub inventory: Inventory,
    /// Which hotbar slot is selected, so a placement knows what to
    /// spend. The client tells us when it changes.
    pub selected_slot: usize,
    /// Set whenever the inventory changes, cleared once the client has
    /// been sent the new state.
    pub inventory_dirty: bool,
    /// The chest this player has open, if any.
    ///
    /// Server-side because it is what makes a chest gesture safe: the
    /// messages that move things carry a slot and a side, never a
    /// position, so a client can only ever reach into the chest the
    /// server watched it open. It is also who to send an update to when
    /// somebody else changes that chest.
    pub open_chest: Option<crate::logic::containers::ChestPos>,
    /// The anvil or potter's wheel this player has open, and the run on it.
    ///
    /// Server-side for the chest's reason and one more: the *moment* the run
    /// was allowed to begin lives in here, and it is what the client's claimed
    /// timings are checked against (`minigame`, rule 4). A clock a client
    /// could name would be a clock a client could stop.
    pub station: Option<crate::StationSeat>,
    /// The last air reading this player was sent.
    ///
    /// Kept so the meter can be told when it goes *back to full*. It was
    /// not, and the bug was visible: the server sent readings while the
    /// head was under water and stopped the moment it came up, so the
    /// last thing the client ever heard was "nearly out of air" -- and
    /// it drew that bar for the rest of the session, including after
    /// drowning and respawning.
    pub breath_reported: f32,
    /// The last smoke thickness this player was sent, for `breath_reported`'s
    /// reason: a fog of smoke that was never told it had lifted would hang
    /// over the player for the rest of the session.
    pub smoke_reported: f32,
    /// The last `shelter::Reading` this player was sent, for the same
    /// reason; `None` until the first, so a new player is always told.
    pub shelter_reported: Option<primitive_shared::shelter::Reading>,
    /// When this player last threw a punch that the server accepted.
    ///
    /// `None` until they throw one. Server-side because the cooldown is
    /// a rule rather than a courtesy: a client that removed its own
    /// would otherwise hit as fast as it could send.
    pub last_swing: Option<Instant>,
    /// When this player last broke or placed a block that the server
    /// accepted.
    ///
    /// What "was this player working" means for hunger. Measured from
    /// edits rather than from a "digging" flag the client would have to
    /// send, for the reason every other survival number is measured
    /// server-side: a flag a client sets is a flag a client can leave
    /// unset, and a player who never gets hungry is the easiest cheat in
    /// the game to write.
    ///
    /// It undercounts, and deliberately so. A swing at a block of iron
    /// ore is thirteen seconds of work and one edit at the end of it, so
    /// a miner is billed for the moments they finish something rather
    /// than for the whole of the effort. The error is in the player's
    /// favour, which is the right direction for it -- see the same
    /// argument about fall distance in `survival`.
    pub last_edit: Option<Instant>,
    /// What turning earth last told this player, and when -- so the same
    /// news about the same field is said once rather than once a
    /// spadeful. See `field_note` in `lib.rs`.
    pub field_note: Option<(&'static str, Instant)>,
    /// What the player has on. Server-owned for the reason the pack is:
    /// it decides what a blow costs, how cold they get and how fast they
    /// move, and a client that owned it would be a client in
    /// indestructible armour.
    pub equipment: primitive_shared::inventory::Equipment,
    /// Set whenever the worn set changes, cleared once the client has
    /// been told. Same contract as `inventory_dirty`.
    pub equipment_dirty: bool,
    /// What the world around them is doing, resampled on an interval
    /// rather than every tick. See `logic::climate`.
    pub ambient: crate::logic::climate::Ambient,
    /// Seconds since that sample. When it passes
    /// `climate::SAMPLE_INTERVAL_SECS` the world is looked at again.
    pub since_ambient: f32,
    /// Whether this player has been granted flight, and how fast.
    ///
    /// Server-owned like everything else about a body. The client is
    /// *told* -- see `protocol::ServerMessage::Flight` -- and told
    /// again whenever it changes; it never decides. Cleared by dying,
    /// because coming back at the spawn point still in the air is a
    /// state nobody asked for and one that outlives whatever granted
    /// it.
    pub flying: bool,
    pub fly_speed: f32,

    /// The last warmth-and-water reading this player was sent.
    ///
    /// Kept for the reason `breath_reported` is: both numbers move
    /// continuously, so the only way to send them on change rather than
    /// every tick is to remember what was said last.
    pub body_reported: (f32, f32, f32),
    /// ...and the stamina multiplier comfort is worth, on the same contract.
    /// Its own field rather than a fourth in that tuple, which half the tick
    /// loop and its tests already destructure. See `ServerMessage::Body`.
    pub recovery_reported: f32,
    /// What the last look at this player's surroundings found, and how long
    /// ago. Kept between looks for the reason `ambient` is: a room does not
    /// change in a tick, and comfort settles over tens of seconds anyway.
    /// See `primitive_shared::comfort::SURVEY_SECONDS`.
    pub surroundings: primitive_shared::comfort::Surroundings,
    pub since_survey: f32,
    /// ...and the wounds this player was last shown, on the same contract.
    /// A whole `Injuries` rather than a fingerprint, because what decides
    /// whether a new one is worth a message is a comparison against it --
    /// see `Injuries::worth_reporting`.
    pub injuries_reported: primitive_shared::injury::Injuries,

    /// The bed this player is asleep in, if they are.
    ///
    /// **The cell rather than a bare flag**, because a sleeper has to be
    /// woken by the world as well as by themselves: the bed being broken
    /// out from under them is the case that made this a coordinate. The
    /// server checks it each tick -- a player whose bed is no longer a
    /// bed is a player lying in a hole, and they wake.
    ///
    /// While it is set the server ignores this player's transforms. See
    /// `ServerMessage::Asleep` for why the client is told rather than
    /// simply corrected.
    ///
    /// **In bed, not necessarily asleep.** Morning leaves a sleeper lying
    /// here awake until they choose to get up; whether their eyes are
    /// closed is `asleep_since`.
    pub sleeping_in: Option<(i32, i32, i32)>,
    /// When this player fell asleep, while they are; `None` awake, and
    /// `None` again for a sleeper the morning has woken in their bed.
    ///
    /// **A moment rather than a flag, because the night waits on it.** The
    /// clock is not wound to dawn until everybody has been asleep for
    /// `body::NIGHT_PASSES_AFTER_SECONDS` -- the time their screens take to
    /// go dark -- and a flag cannot say how long. Kept apart from
    /// `sleeping_in` so that a player lying in bed at dawn does not sleep
    /// the next day through as well on the following tick.
    pub asleep_since: Option<std::time::Instant>,
    /// What was last said about that, so the message is sent on change
    /// only -- the same contract as `body_reported`.
    pub asleep_reported: bool,
    /// The stool this player sat down on, if they are still on it.
    ///
    /// **Sitting is not a lock and never became one.** It is a claim
    /// the server checks each tick -- are they still within a step of
    /// that stool -- and drops the moment they walk away. That is why
    /// it needs no protocol message and no client support: nothing is
    /// taken away from the player, so there is nothing for their client
    /// to be told about. See `body::SITTING_RECOVERY_PER_SECOND`.
    pub sitting_on: Option<(i32, i32, i32)>,
    /// The way the chair under a sitter faces, or `None` on a stool.
    ///
    /// **Read only while `sitting_on` is set, and written every time it
    /// is**, so a stale turn can never reach a snapshot. It exists because
    /// a sitter's transforms are still taken (sitting is not a lock) and
    /// carry the camera's yaw: without it the snapshot of a player in a
    /// chair would turn with their mouse, and everyone else would see a
    /// figure spinning on a seat that has a back. See `sit_down`.
    pub seat_facing: Option<f32>,
    /// Every kind of block this player has held. What the recipe book is
    /// drawn from; see `primitive_shared::discovery`. Noted in
    /// `send_inventory`, the one path every change to the pack leaves by.
    pub discovered: primitive_shared::discovery::Discovered,
    /// The bags this player died away from and has not emptied, oldest
    /// first. See `remember_bag` in `lib.rs` for why there is more than
    /// one and why there are at most four.
    pub bags: Vec<(i32, i32, i32)>,
    /// The raft this player is standing on, and where on its deck, in the
    /// deck's own frame (`raft::Body::local_of`).
    ///
    /// **The place on the deck is the truth and the world position is worked
    /// out from it every tick** (`carry_riders` in `lib.rs`). The other way
    /// round -- keep the world position and nudge it by however far the raft
    /// moved -- is the same arithmetic until the raft turns, and then a rider
    /// at the bow is swung through an arc the nudge does not know about and
    /// ends up in the water beside the deck they were standing on.
    ///
    /// Set by `ClientMessage::Deck` and cleared by any `UpdateTransform`,
    /// which is a client saying its feet are on something else.
    pub aboard: Option<(primitive_shared::protocol::EntityId, [f32; 3])>,
    /// The raft whose oars this player has, if any. Always a raft they are
    /// `aboard`, on its seat (`raft::SEAT`).
    pub rowing: Option<primitive_shared::protocol::EntityId>,
    /// The horse this player is riding, if any. Their body is its saddle
    /// every tick (`horses::tick`), and their own transforms are read by the
    /// anti-cheat and move nothing.
    pub riding: Option<primitive_shared::protocol::EntityId>,
    /// When the rider was last told their horse's wind (`ServerMessage::Mounted`).
    pub mount_told: Option<Instant>,
    /// The horse whose saddlebags this player has open, if any: the chest's
    /// `open_chest`, for a container that walks (`horses::with_open_bags`).
    pub open_bags: Option<primitive_shared::protocol::EntityId>,
    /// In water past the waist, as the server's world has it this tick. Only
    /// for everybody else's picture of this player: see `Posture::Swimming`.
    pub swimming: bool,
    /// What this player's hands last did that happens once, and how many
    /// times, for the snapshot (`protocol::Gesture`). Its `digging` is not
    /// kept here: that is `digging_until`, read when a snapshot is taken.
    pub gesture: primitive_shared::protocol::Gesture,
    /// Until when this player is swinging at a block, as their client last
    /// said. A moment rather than a flag, so it lapses on its own -- see
    /// `DIGGING_LAPSES_AFTER`.
    pub digging_until: Option<Instant>,
}

/// How long a `ClientMessage::Digging` holds without being said again.
///
/// Three times the half second a client repeats it at, so one lost message is
/// not a swing that stops and starts on everybody else's screen -- and short
/// enough that a client that dropped mid-swing does not leave a figure
/// hammering at nothing for longer than a breath.
pub const DIGGING_LAPSES_AFTER: std::time::Duration = std::time::Duration::from_millis(1500);

pub struct PlayerHandle {
    pub id: PlayerId,
    pub username: String,
    /// Who this is, across sessions. `None` only for handles built by
    /// tests, which have no profile behind them.
    ///
    /// The numeric `id` above is a *connection* number, handed out fresh
    /// on every join and reused once it is free; this is the identity
    /// their pack and their place of exit are filed under.
    pub uuid: Option<crate::logic::profiles::Uuid>,
    pub addr: SocketAddr,
    pub joined_at: Instant,

    tx: mpsc::Sender<Outgoing>,
    /// Chunk requests waiting to be served by this player's own chunk
    /// pump task. Separate from the outgoing queue so a chunk backlog
    /// can't starve movement snapshots, and vice versa.
    chunk_tx: mpsc::Sender<ChunkPos>,

    pub state: Mutex<PlayerRuntime>,

    sent: AtomicU64,
    dropped: AtomicU64,
    drop_threshold: u64,

    kick: Notify,
    kick_reason: Mutex<Option<DisconnectReason>>,
}

impl PlayerHandle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PlayerId,
        username: String,
        addr: SocketAddr,
        tx: mpsc::Sender<Outgoing>,
        chunk_tx: mpsc::Sender<ChunkPos>,
        drop_threshold: u64,
        spawn: (f64, f64, f64),
        anticheat: AntiCheat,
    ) -> Self {
        Self {
            id,
            username,
            uuid: None,
            addr,
            joined_at: Instant::now(),
            tx,
            chunk_tx,
            state: Mutex::new(PlayerRuntime {
                position: spawn,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: true,
                last_activity: Instant::now(),
                loaded_chunks: HashSet::new(),
                anticheat,
                vitals: Vitals::new(),
                inventory: Inventory::new(),
                selected_slot: 0,
                inventory_dirty: true,
                open_chest: None,
                station: None,
                breath_reported: 1.0,
                smoke_reported: 0.0,
                shelter_reported: None,
                last_swing: None,
                last_edit: None,
                field_note: None,
                equipment: primitive_shared::inventory::Equipment::new(),
                // True, so a fresh join is told what it is wearing --
                // which for a restored profile is not nothing.
                equipment_dirty: true,
                ambient: crate::logic::climate::Ambient::default(),
                // Past the interval, so the very first tick samples the
                // world rather than believing the neutral default for
                // half a second. That half second is invisible in play
                // and is exactly the sort of thing that makes a test of
                // "does standing in a fire warm you" flaky.
                since_ambient: crate::logic::climate::SAMPLE_INTERVAL_SECS,
                flying: false,
                fly_speed: DEFAULT_FLY_SPEED,
                body_reported: (
                    primitive_shared::body::NEUTRAL_C,
                    1.0,
                    0.0,
                ),
                recovery_reported: 1.0,
                surroundings: primitive_shared::comfort::Surroundings::default(),
                // Due at once, on `since_ambient`'s argument.
                since_survey: primitive_shared::comfort::SURVEY_SECONDS,
                // Whole, which is what a fresh client assumes. A restored
                // profile with a wound on it is sent its set on join
                // regardless -- see the connection's handshake.
                injuries_reported: primitive_shared::injury::Injuries::default(),
                // Awake. A player joins standing up, whatever they were
                // doing when they left: a sleeper who logged out and came
                // back still asleep would be a player who cannot move and
                // does not know why.
                sleeping_in: None,
                asleep_since: None,
                asleep_reported: false,
                sitting_on: None,
                seat_facing: None,
                discovered: primitive_shared::discovery::Discovered::new(),
                bags: Vec::new(),
                // On their feet on the ground: nobody joins aboard a raft,
                // because the raft they left from may have been broken up
                // since, and a body placed on a deck that is not there is a
                // body in the lake.
                aboard: None,
                rowing: None,
                riding: None,
                mount_told: None,
                open_bags: None,
                swimming: false,
                gesture: primitive_shared::protocol::Gesture::default(),
                digging_until: None,
            }),
            sent: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            drop_threshold,
            kick: Notify::new(),
            kick_reason: Mutex::new(None),
        }
    }

    /// Non-blocking send. Returns false if the message was dropped.
    ///
    /// Deliberately never `.await`s: this is called from the tick loop
    /// and from other players' request handlers, and one unresponsive
    /// socket must not be able to stall either.
    pub fn send(&self, msg: ServerMessage) -> bool {
        self.enqueue(Outgoing::Message(msg))
    }

    /// Queues bytes that were serialised once for many recipients. Same
    /// contract as `send`, including the drop accounting -- a slow client
    /// is a slow client whichever shape its messages take.
    pub fn send_raw(&self, frame: Arc<[u8]>) -> bool {
        self.enqueue(Outgoing::Raw(frame))
    }

    fn enqueue(&self, out: Outgoing) -> bool {
        match self.tx.try_send(out) {
            Ok(()) => {
                self.sent.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped >= self.drop_threshold {
                    self.request_kick(DisconnectReason::Other(
                        "client cannot keep up with the server".to_string(),
                    ));
                }
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    /// Queues a chunk for this player's chunk pump. Dropping here is safe:
    /// the client re-requests anything still missing (see the client's
    /// `ChunkManager` retry timer).
    pub fn queue_chunk(&self, pos: ChunkPos) -> bool {
        self.chunk_tx.try_send(pos).is_ok()
    }

    pub fn request_kick(&self, reason: DisconnectReason) {
        {
            let mut slot = self.kick_reason.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_none() {
                *slot = Some(reason);
            }
        }
        self.kick.notify_waiters();
        self.kick.notify_one();
    }

    /// Resolves when someone calls `request_kick`.
    pub async fn kicked(&self) -> DisconnectReason {
        loop {
            if let Some(reason) = self
                .kick_reason
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
            {
                return reason;
            }
            self.kick.notified().await;
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (
            self.sent.load(Ordering::Relaxed),
            self.dropped.load(Ordering::Relaxed),
        )
    }

    pub fn player_state(&self) -> PlayerState {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        PlayerState {
            id: self.id,
            x: state.position.0,
            y: state.position.1,
            z: state.position.2,
            // A sitter in a chair faces the chair, whatever their camera is
            // doing -- see `PlayerRuntime::seat_facing`.
            yaw: state
                .seat_facing
                .filter(|_| state.sitting_on.is_some())
                .unwrap_or(state.yaw),
            pitch: state.pitch,
            on_ground: state.on_ground,
            outfit: outfit_of(&state),
            // Dead before everything: a death clears the seat and the bed,
            // and a body is not standing in either. See `Posture::Fallen`
            // for the statue this replaced.
            //
            // Sleeping before sitting: `lie_down` clears the seat, but a
            // tick that saw both would rather draw the bed.
            posture: if state.vitals.is_dead() {
                primitive_shared::protocol::Posture::Fallen
            } else if state.sleeping_in.is_some() {
                primitive_shared::protocol::Posture::Lying
            } else if state.riding.is_some() {
                // Astride, whatever else: see `Posture::Mounted`.
                primitive_shared::protocol::Posture::Mounted
            } else if state.sitting_on.is_some() || state.rowing.is_some() {
                // A rower sits on the stern to pull, and is drawn so.
                primitive_shared::protocol::Posture::Sitting
            } else if state.swimming {
                primitive_shared::protocol::Posture::Swimming
            } else {
                primitive_shared::protocol::Posture::Standing
            },
            gesture: primitive_shared::protocol::Gesture {
                digging: state.digging_until.is_some_and(|until| until > Instant::now()),
                ..state.gesture
            },
            // A byte, quantised here rather than sent as a float, because
            // nothing downstream can use more than a byte of it: it ends as
            // a fraction of a radian on somebody else's hip. See
            // `survival::Vitals::limp`.
            limp: (state.vitals.limp().clamp(0.0, 1.0) * 255.0) as u8,
            // ...and which leg, so the figure favours the one that is broken.
            limp_left: state.vitals.limp_left(),
        }
    }

    pub fn touch(&self) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_activity = Instant::now();
    }

    pub fn idle_for(&self) -> std::time::Duration {
        let last = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_activity;
        Instant::now().saturating_duration_since(last)
    }
}

/// What this player looks like to everybody else, read off the state the
/// server already owns.
///
/// **Nothing here is a second copy of anything.** The worn set is
/// `PlayerRuntime::equipment`, which is the same set combat subtracts
/// from and the body reads its insulation off; the held block is the
/// selected hotbar slot, which is the same slot a placement spends and
/// the same one mining asks about. So a helmet that is protecting
/// somebody is a helmet other people can see, always, with no flag to
/// set and nothing to forget to set -- which is the whole reason this is
/// derived here rather than tracked as appearance state of its own.
///
/// It runs once per player per tick, inside the lock `player_state`
/// already holds, and reads five slots. That is why it is not cached:
/// a cache would need invalidating from every gesture that moves an
/// item, and the thing it would save is five array reads.
fn outfit_of(state: &PlayerRuntime) -> primitive_shared::protocol::Outfit {
    let mut outfit = primitive_shared::protocol::Outfit::BARE;
    for (index, slot) in state.equipment.slots().iter().enumerate() {
        // Guarded rather than trusted: `Equipment::sanitize` keeps the
        // set four long, and a set restored from an older profile has
        // been through it -- but the wire's array is a fixed four and
        // writing past it would be a panic in the tick loop.
        let Some(out) = outfit.worn.get_mut(index) else {
            break;
        };
        if let Some(stack) = slot {
            *out = stack.block;
        }
    }
    outfit.holding = state
        .inventory
        .block_in(state.selected_slot)
        .unwrap_or(primitive_shared::types::BLOCK_AIR);
    outfit
}

#[derive(Debug)]
pub enum AdmissionError {
    ServerFull,
    TooManyConnectionsFromIp,
}

pub struct Registry {
    players: RwLock<HashMap<PlayerId, Arc<PlayerHandle>>>,
    /// chunk -> everyone who currently has it loaded.
    subscriptions: RwLock<HashMap<ChunkPos, HashSet<PlayerId>>>,
    ip_counts: Mutex<HashMap<IpAddr, u32>>,
    next_id: AtomicU64,
    max_players: usize,
    max_per_ip: u32,
    peak_players: AtomicU64,
}

impl Registry {
    pub fn new(max_players: usize, max_per_ip: u32) -> Self {
        Self {
            players: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            ip_counts: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            max_players,
            max_per_ip,
            peak_players: AtomicU64::new(0),
        }
    }

    pub fn allocate_id(&self) -> PlayerId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Reserves a connection slot *before* the handshake, so a flood of
    /// half-open connections from one address can't fill the server.
    pub fn admit(&self, addr: SocketAddr) -> Result<(), AdmissionError> {
        if self.len() >= self.max_players {
            return Err(AdmissionError::ServerFull);
        }
        let mut counts = self.ip_counts.lock().unwrap_or_else(|e| e.into_inner());
        let entry = counts.entry(addr.ip()).or_insert(0);
        if *entry >= self.max_per_ip {
            return Err(AdmissionError::TooManyConnectionsFromIp);
        }
        *entry += 1;
        Ok(())
    }

    pub fn release(&self, addr: SocketAddr) {
        let mut counts = self.ip_counts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = counts.get_mut(&addr.ip()) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                counts.remove(&addr.ip());
            }
        }
    }

    /// Puts a handle in, unless whoever it belongs to is already here.
    /// Answers whether it went in.
    ///
    /// **One identity, one session, and the check happens under the same
    /// lock as the insert.** `admit` counts connections -- against
    /// `max_players`, and against a per-address limit that defaults to
    /// eight -- and this map is keyed by `PlayerId`, which is handed out
    /// fresh on every join. Nothing anywhere asked whether the *person*
    /// was already playing, and `Profiles::restore` hands every caller
    /// its own clone of that person's pack. So eight copies of the game
    /// logged in under one name were eight copies of the same rucksack;
    /// tip each into a chest and the world has eight times the things it
    /// did. That is not a leak at the edges, it is arbitrary item
    /// duplication with no tools and no timing.
    ///
    /// ## Refusing the newcomer rather than kicking the old session
    ///
    /// Kicking the old one is the more familiar behaviour -- it is what
    /// a player expects when their connection drops and they come
    /// straight back -- and it was the first thing tried. It trades this
    /// bug for a worse one. The newcomer's state is restored from the
    /// *profile*, which is only as fresh as the last autosave (two
    /// minutes by default); the displaced session then writes its own,
    /// current state into that profile as it tears down, and the next
    /// autosave overwrites it with the newcomer's stale copy. An hour of
    /// play can go through that gap. Duplication is a bug an operator
    /// can see and roll back; a silent two-minute rollback on every
    /// reconnect is one nobody can even reproduce.
    ///
    /// Doing it properly means handing the live session's state to the
    /// newcomer instead of the profile's, and suppressing the old
    /// session's final write -- a takeover protocol, with a window in
    /// which two sockets both hold what is nominally one player. That is
    /// a real feature and it is not a bug fix.
    ///
    /// What refusing costs is a player whose connection died ungracefully
    /// waiting to get back in, and that cost is already bounded: the tick
    /// loop kicks anyone who has not answered a keepalive within
    /// `client_timeout_secs`, so a genuinely dead session frees the name
    /// by itself. A player who is told "you are already logged in"
    /// understands what happened; a player quietly rolled back does not.
    ///
    /// Handles with no `uuid` -- which is only ever a test fixture, see
    /// `PlayerHandle::uuid` -- have no identity to collide on and always
    /// go in.
    pub fn insert_unique(&self, handle: Arc<PlayerHandle>) -> bool {
        let mut players = self.players.write().unwrap_or_else(|e| e.into_inner());
        if let Some(uuid) = handle.uuid {
            if players.values().any(|other| other.uuid == Some(uuid)) {
                return false;
            }
        }
        players.insert(handle.id, handle);
        let now = players.len() as u64;
        self.peak_players.fetch_max(now, Ordering::Relaxed);
        true
    }

    pub fn remove(&self, id: PlayerId) -> Option<Arc<PlayerHandle>> {
        let handle = {
            let mut players = self.players.write().unwrap_or_else(|e| e.into_inner());
            players.remove(&id)
        };
        if let Some(handle) = &handle {
            let loaded: Vec<ChunkPos> = {
                let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.loaded_chunks.iter().copied().collect()
            };
            let mut subs = self.subscriptions.write().unwrap_or_else(|e| e.into_inner());
            for pos in loaded {
                if let Some(set) = subs.get_mut(&pos) {
                    set.remove(&id);
                    if set.is_empty() {
                        subs.remove(&pos);
                    }
                }
            }
        }
        handle
    }

    pub fn get(&self, id: PlayerId) -> Option<Arc<PlayerHandle>> {
        self.players
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.players
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn peak_players(&self) -> u64 {
        self.peak_players.load(Ordering::Relaxed)
    }

    /// Snapshot of the handle list. Cloning `Arc`s under a read lock and
    /// then releasing it keeps the lock hold time proportional to the
    /// player count rather than to whatever the caller does next.
    pub fn handles(&self) -> Vec<Arc<PlayerHandle>> {
        self.players
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub fn subscribe(&self, id: PlayerId, pos: ChunkPos) {
        let mut subs = self.subscriptions.write().unwrap_or_else(|e| e.into_inner());
        subs.entry(pos).or_default().insert(id);
    }

    pub fn unsubscribe(&self, id: PlayerId, pos: ChunkPos) {
        let mut subs = self.subscriptions.write().unwrap_or_else(|e| e.into_inner());
        if let Some(set) = subs.get_mut(&pos) {
            set.remove(&id);
            if set.is_empty() {
                subs.remove(&pos);
            }
        }
    }

    /// Everyone who should hear about a change in this chunk.
    pub fn subscribers(&self, pos: ChunkPos) -> Vec<Arc<PlayerHandle>> {
        let ids: Vec<PlayerId> = {
            let subs = self.subscriptions.read().unwrap_or_else(|e| e.into_inner());
            match subs.get(&pos) {
                Some(set) => set.iter().copied().collect(),
                None => return Vec::new(),
            }
        };
        let players = self.players.read().unwrap_or_else(|e| e.into_inner());
        ids.iter().filter_map(|id| players.get(id).cloned()).collect()
    }

    /// The first player whose collider contains this block, if any.
    ///
    /// Used to refuse placing a block inside someone -- including inside
    /// *yourself*, which is the common case: a player looking down at
    /// their own feet would otherwise entomb themselves.
    ///
    /// Takes the block being placed, not just the cell, because how much
    /// of the cell it fills is now part of the answer: a layer of soil
    /// laid at your own feet is something you stand on, and a whole
    /// block there is something you are buried in.
    pub fn player_occupying_block(
        &self,
        bx: i32,
        by: i32,
        bz: i32,
        block: BlockId,
    ) -> Option<Arc<PlayerHandle>> {
        self.handles().into_iter().find(|handle| {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            block_overlaps_player(state.position, bx, by, bz, block)
        })
    }

    /// Serialised once, sent to everyone. Cloning the message per player
    /// and letting each writer task bincode identical bytes was
    /// O(players) serialisations per broadcast; now it is one, and each
    /// recipient costs an `Arc` clone.
    pub fn broadcast(&self, msg: ServerMessage) {
        let Some(frame) = frame(&msg) else { return };
        for handle in self.handles() {
            handle.send_raw(Arc::clone(&frame));
        }
    }

    pub fn broadcast_except(&self, except: PlayerId, msg: ServerMessage) {
        let Some(frame) = frame(&msg) else { return };
        for handle in self.handles() {
            if handle.id != except {
                handle.send_raw(Arc::clone(&frame));
            }
        }
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

/// Players within `radius` blocks of `origin`, excluding `exclude`.
///
/// This is the interest-management step: at 500 connected players spread
/// over a world, each snapshot carries the handful of players you can
/// actually see rather than all 499.
pub fn nearby_states(
    states: &[(PlayerId, PlayerState)],
    origin: (f64, f64, f64),
    radius: f32,
    exclude: PlayerId,
) -> Vec<PlayerState> {
    let mut out = Vec::new();
    let radius_sq = radius * radius;
    for (id, state) in states {
        if *id == exclude {
            continue;
        }
        if distance_sq(state, origin) <= radius_sq {
            out.push(*state);
        }
    }
    out
}

#[inline]
fn distance_sq(state: &PlayerState, origin: (f64, f64, f64)) -> f32 {
    // The difference first, in `f64`, and only then narrowed: see
    // `PROTOCOL_VERSION`'s fifty-seven.
    let dx = (state.x - origin.0) as f32;
    let dy = (state.y - origin.1) as f32;
    let dz = (state.z - origin.2) as f32;
    dx * dx + dy * dy + dz * dz
}

/// A uniform grid over one tick's player positions.
///
/// ## Why
///
/// Building snapshots was the one thing in the server that grew with the
/// square of the player count: every player was compared against every
/// other, every tick. The distance check is three subtractions and a dot
/// product, so it stayed cheap for a while and then stopped -- at 256
/// players that is 65,000 comparisons twenty times a second, and all but
/// a handful of them are between players who are nowhere near each other.
///
/// The grid buckets by x/z at the interest radius, so a query reads the
/// 3x3 block of cells around the asker and nothing else. Cost becomes
/// proportional to how many players are actually nearby, which is what
/// the interest radius was always meant to buy.
///
/// ## What it deliberately doesn't do
///
/// It doesn't bucket by y. The world is 64 blocks tall and the interest
/// radius is 160, so a vertical axis would put every player in one layer
/// and cost a dimension of bookkeeping for nothing. The distance test is
/// still fully 3D -- the grid only decides which players are worth
/// testing.
///
/// It is rebuilt from scratch every tick rather than maintained
/// incrementally. Players move constantly, so an incremental version
/// would be doing the same work in a harder-to-follow way.
pub struct InterestGrid {
    states: Vec<(PlayerId, PlayerState)>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    cell_size: f32,
}

impl InterestGrid {
    pub fn build(states: Vec<(PlayerId, PlayerState)>, radius: f32) -> Self {
        // One cell per interest radius: a query then covers 3x3 cells,
        // which is the smallest grid that can answer without either
        // scanning more cells or missing a player at the edge.
        let cell_size = radius.max(1.0);
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (index, (_, state)) in states.iter().enumerate() {
            cells
                .entry(cell_of(state.x, state.z, cell_size))
                .or_default()
                .push(index as u32);
        }
        Self {
            states,
            cells,
            cell_size,
        }
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Everyone within `radius` of `origin`, excluding one id.
    ///
    /// Writes into a caller-owned buffer rather than returning a `Vec`:
    /// this runs once per player per tick, and the allocation it saves is
    /// the only one left in the loop.
    pub fn nearby(
        &self,
        origin: (f64, f64, f64),
        radius: f32,
        exclude: PlayerId,
        out: &mut Vec<PlayerState>,
    ) {
        out.clear();
        let radius_sq = radius * radius;
        let (cx, cz) = cell_of(origin.0, origin.2, self.cell_size);
        // 3x3 is exact as long as the cell is at least the radius: a
        // player further than one cell away is further than the radius.
        for dz in -1..=1 {
            for dx in -1..=1 {
                let Some(bucket) = self.cells.get(&(cx + dx, cz + dz)) else {
                    continue;
                };
                for index in bucket {
                    let (id, state) = &self.states[*index as usize];
                    if *id == exclude {
                        continue;
                    }
                    if distance_sq(state, origin) <= radius_sq {
                        out.push(*state);
                    }
                }
            }
        }
    }
}

/// `InterestGrid`'s shape, over entities instead of players.
///
/// Falling blocks and dropped items go through the same interest
/// filtering as players, and they had the same problem: every entity was
/// distance-tested against every player, every tick, which is O(players
/// x entities) for a test that nearly always says no. Same cure -- bucket
/// by x/z at the interest radius, rebuilt each tick, query the 3x3
/// neighbourhood -- and the same deliberate omissions: no y axis in the
/// grid (the world is far shorter than the radius) while the distance
/// test itself stays fully 3D.
///
/// A separate type rather than a generic one because the two differ in
/// exactly one way that matters: entities have no id to exclude, so the
/// query is simpler, not parameterised.
pub struct EntityGrid {
    states: Vec<EntityState>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    cell_size: f32,
}

impl EntityGrid {
    pub fn build(states: Vec<EntityState>, radius: f32) -> Self {
        let cell_size = radius.max(1.0);
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (index, state) in states.iter().enumerate() {
            cells
                .entry(cell_of(state.x, state.z, cell_size))
                .or_default()
                .push(index as u32);
        }
        Self {
            states,
            cells,
            cell_size,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Every entity within `radius` of `origin`, into a reused buffer --
    /// same allocation-free contract as `InterestGrid::nearby`.
    pub fn nearby(&self, origin: (f64, f64, f64), radius: f32, out: &mut Vec<EntityState>) {
        out.clear();
        let radius_sq = radius * radius;
        let (cx, cz) = cell_of(origin.0, origin.2, self.cell_size);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let Some(bucket) = self.cells.get(&(cx + dx, cz + dz)) else {
                    continue;
                };
                for index in bucket {
                    let state = &self.states[*index as usize];
                    let (dx, dy, dz) = (
                        (state.x - origin.0) as f32,
                        (state.y - origin.1) as f32,
                        (state.z - origin.2) as f32,
                    );
                    if dx * dx + dy * dy + dz * dz <= radius_sq {
                        out.push(*state);
                    }
                }
            }
        }
    }
}

#[inline]
fn cell_of(x: f64, z: f64, cell_size: f32) -> (i32, i32) {
    (
        (x / f64::from(cell_size)).floor() as i32,
        (z / f64::from(cell_size)).floor() as i32,
    )
}

#[cfg(test)]
mod interest_grid_tests {
    use super::*;

    fn at(id: PlayerId, x: f32, z: f32) -> (PlayerId, PlayerState) {
        (
            id,
            PlayerState {
                id,
                x: f64::from(x),
                y: 30.0,
                z: f64::from(z),
                yaw: 0.0,
                pitch: 0.0,
                on_ground: true,
                outfit: primitive_shared::protocol::Outfit::BARE,
                posture: primitive_shared::protocol::Posture::Standing,
                gesture: primitive_shared::protocol::Gesture::default(),
                limp: 0,
                limp_left: false,
            },
        )
    }

    /// The grid must answer exactly what the flat scan answered. This is
    /// the property that matters: it is an optimisation, so any
    /// disagreement is a bug in the optimisation.
    fn agrees(states: &[(PlayerId, PlayerState)], origin: (f64, f64, f64), radius: f32, exclude: PlayerId) {
        let expected = nearby_states(states, origin, radius, exclude);
        let grid = InterestGrid::build(states.to_vec(), radius);
        let mut got = Vec::new();
        grid.nearby(origin, radius, exclude, &mut got);

        let mut expected_ids: Vec<_> = expected.iter().map(|s| s.id).collect();
        let mut got_ids: Vec<_> = got.iter().map(|s| s.id).collect();
        expected_ids.sort();
        got_ids.sort();
        assert_eq!(got_ids, expected_ids, "grid disagreed with the flat scan");
    }

    #[test]
    fn the_grid_agrees_with_the_scan_it_replaced() {
        let states = vec![
            at(1, 0.0, 0.0),
            at(2, 10.0, 10.0),
            at(3, 200.0, 0.0),
            at(4, -200.0, -200.0),
            at(5, 159.0, 0.0),
            at(6, 161.0, 0.0),
        ];
        agrees(&states, (0.0, 30.0, 0.0), 160.0, 1);
    }

    #[test]
    fn it_agrees_across_negative_coordinates_and_cell_seams() {
        // Cell indices come from a floor division, which is where an
        // off-by-one on negative coordinates would hide.
        let mut states = Vec::new();
        let mut id = 0;
        for x in [-330.0f32, -161.0, -160.0, -1.0, 0.0, 1.0, 159.0, 160.0, 330.0] {
            for z in [-330.0f32, -160.0, 0.0, 160.0, 330.0] {
                id += 1;
                states.push(at(id, x, z));
            }
        }
        for origin in [
            (0.0, 30.0, 0.0),
            (-160.0, 30.0, -160.0),
            (159.9, 30.0, -0.1),
            (-0.001, 30.0, 0.001),
        ] {
            agrees(&states, origin, 160.0, 0);
        }
    }

    #[test]
    fn a_player_just_inside_the_radius_is_still_seen() {
        // The 3x3 query window is only exact because a cell is at least
        // one radius across. If that ever changes, this catches it.
        let states = vec![at(1, 0.0, 0.0), at(2, 0.0, 159.9)];
        let grid = InterestGrid::build(states, 160.0);
        let mut got = Vec::new();
        grid.nearby((0.0, 30.0, 0.0), 160.0, 1, &mut got);
        assert_eq!(got.len(), 1, "a player inside the radius was missed");
    }

    #[test]
    fn distance_is_measured_in_three_dimensions_though_the_grid_is_flat() {
        // The grid buckets by x/z only; height still has to count, or a
        // player far above would be reported as nearby.
        let mut high = at(2, 0.0, 0.0);
        high.1.y = 30.0 + 200.0;
        let grid = InterestGrid::build(vec![at(1, 0.0, 0.0), high], 160.0);
        let mut got = Vec::new();
        grid.nearby((0.0, 30.0, 0.0), 160.0, 1, &mut got);
        assert!(got.is_empty(), "height was ignored");
    }

    #[test]
    fn the_asker_is_never_in_their_own_snapshot() {
        let grid = InterestGrid::build(vec![at(1, 0.0, 0.0), at(2, 5.0, 5.0)], 160.0);
        let mut got = Vec::new();
        grid.nearby((0.0, 30.0, 0.0), 160.0, 1, &mut got);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, 2);
    }

    #[test]
    fn the_buffer_is_reused_rather_than_appended_to() {
        // It is passed in to avoid an allocation per player per tick, so
        // it has to be cleared or every snapshot would carry the last
        // player's too.
        let grid = InterestGrid::build(vec![at(1, 0.0, 0.0), at(2, 5.0, 5.0)], 160.0);
        let mut got = vec![at(9, 0.0, 0.0).1];
        grid.nearby((0.0, 30.0, 0.0), 160.0, 1, &mut got);
        assert_eq!(got.len(), 1, "stale entries survived");
    }

    #[test]
    fn an_empty_server_produces_an_empty_grid() {
        let grid = InterestGrid::build(Vec::new(), 160.0);
        assert!(grid.is_empty());
        let mut got = Vec::new();
        grid.nearby((0.0, 0.0, 0.0), 160.0, 1, &mut got);
        assert!(got.is_empty());
    }

    #[test]
    fn a_crowd_in_one_place_is_still_answered_in_full() {
        // The grid is a filter, not a cap: everyone standing on spawn
        // must still see everyone else.
        let states: Vec<_> = (1..=64).map(|i| at(i, 1.0, 1.0)).collect();
        let grid = InterestGrid::build(states, 160.0);
        let mut got = Vec::new();
        grid.nearby((1.0, 30.0, 1.0), 160.0, 1, &mut got);
        assert_eq!(got.len(), 63);
    }

    #[test]
    fn a_scattered_server_reads_far_fewer_players_than_it_holds() {
        // The point of the whole exercise. Spread players a long way
        // apart and check the query touches a handful, not all of them.
        let mut states = Vec::new();
        let mut id = 0;
        for x in 0..16 {
            for z in 0..16 {
                id += 1;
                states.push(at(id, x as f32 * 500.0, z as f32 * 500.0));
            }
        }
        assert_eq!(states.len(), 256);
        let grid = InterestGrid::build(states, 160.0);
        let mut got = Vec::new();
        grid.nearby((0.0, 30.0, 0.0), 160.0, 1, &mut got);
        assert!(got.is_empty(), "nobody is within 160 blocks");
    }
}

#[cfg(test)]
mod entity_grid_tests {
    use super::*;
    use primitive_shared::protocol::EntityKind;

    fn item_at(id: u64, x: f32, y: f32, z: f32) -> EntityState {
        EntityState {
            id,
            kind: EntityKind::Item { block: 1, count: 1 },
            x: f64::from(x),
            y: f64::from(y),
            z: f64::from(z),
        }
    }

    /// Same property the player grid is tested for: the grid is an
    /// optimisation over the flat scan, so any disagreement is a bug in
    /// the optimisation.
    #[test]
    fn the_grid_agrees_with_the_flat_scan_it_replaced() {
        let radius = 160.0f32;
        let mut states = Vec::new();
        let mut id = 0;
        // Across cell seams, negative coordinates, just inside and just
        // outside the radius, and far above it.
        for x in [-330.0f32, -161.0, -160.0, -1.0, 0.0, 1.0, 159.9, 160.1, 330.0] {
            for z in [-330.0f32, -160.0, 0.0, 160.0, 330.0] {
                id += 1;
                states.push(item_at(id, x, 30.0, z));
            }
        }
        id += 1;
        states.push(item_at(id, 0.0, 30.0 + 200.0, 0.0)); // height must count

        let grid = EntityGrid::build(states.clone(), radius);
        for origin in [
            (0.0, 30.0, 0.0),
            (-160.0, 30.0, -160.0),
            (159.9, 30.0, -0.1),
        ] {
            let radius_sq = radius * radius;
            let mut expected: Vec<u64> = states
                .iter()
                .filter(|s| {
                    let (dx, dy, dz) = (s.x - origin.0, s.y - origin.1, s.z - origin.2);
                    dx * dx + dy * dy + dz * dz <= f64::from(radius_sq)
                })
                .map(|s| s.id)
                .collect();
            let mut got = Vec::new();
            grid.nearby(origin, radius, &mut got);
            let mut got: Vec<u64> = got.iter().map(|s| s.id).collect();
            expected.sort_unstable();
            got.sort_unstable();
            assert_eq!(got, expected, "grid disagreed with the flat scan at {origin:?}");
        }
    }

    #[test]
    fn the_buffer_is_reused_rather_than_appended_to() {
        let grid = EntityGrid::build(vec![item_at(1, 0.0, 30.0, 0.0)], 160.0);
        let mut got = vec![item_at(9, 500.0, 30.0, 500.0)];
        grid.nearby((0.0, 30.0, 0.0), 160.0, &mut got);
        assert_eq!(got.len(), 1, "stale entries survived");
        assert_eq!(got[0].id, 1);
    }

    #[test]
    fn no_entities_means_an_empty_answer() {
        let grid = EntityGrid::build(Vec::new(), 160.0);
        assert!(grid.is_empty());
        let mut got = Vec::new();
        grid.nearby((0.0, 0.0, 0.0), 160.0, &mut got);
        assert!(got.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: PlayerId, x: f32, z: f32) -> (PlayerId, PlayerState) {
        (
            id,
            PlayerState {
                id,
                x: f64::from(x),
                y: 30.0,
                z: f64::from(z),
                yaw: 0.0,
                pitch: 0.0,
                on_ground: true,
                outfit: primitive_shared::protocol::Outfit::BARE,
                posture: primitive_shared::protocol::Posture::Standing,
                gesture: primitive_shared::protocol::Gesture::default(),
                limp: 0,
                limp_left: false,
            },
        )
    }

    #[test]
    fn interest_filtering_drops_distant_players_and_yourself() {
        let all = vec![
            state(1, 0.0, 0.0),
            state(2, 10.0, 0.0),
            state(3, 500.0, 0.0),
        ];
        let near = nearby_states(&all, (0.0, 30.0, 0.0), 100.0, 1);
        let ids: Vec<PlayerId> = near.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![2], "expected only the nearby other player");
    }

    #[test]
    fn per_ip_connection_cap_is_enforced() {
        let registry = Registry::new(100, 2);
        let addr: SocketAddr = "10.0.0.5:1234".parse().unwrap();
        assert!(registry.admit(addr).is_ok());
        assert!(registry.admit(addr).is_ok());
        assert!(
            matches!(registry.admit(addr), Err(AdmissionError::TooManyConnectionsFromIp)),
            "third connection from one IP should be refused"
        );
        registry.release(addr);
        assert!(registry.admit(addr).is_ok(), "slot should free on disconnect");
    }

    #[test]
    fn ids_are_unique() {
        let registry = Registry::new(10, 10);
        let a = registry.allocate_id();
        let b = registry.allocate_id();
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn a_full_queue_drops_messages_and_eventually_kicks() {
        let (tx, _rx) = mpsc::channel::<Outgoing>(2);
        let (chunk_tx, _chunk_rx) = mpsc::channel::<ChunkPos>(2);
        let handle = PlayerHandle::new(
            1,
            "slowpoke".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            4,
            (0.0, 0.0, 0.0),
            AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                (0.0, 0.0, 0.0),
            ),
        );

        // The receiver never reads, so the queue fills immediately.
        for _ in 0..10 {
            handle.send(ServerMessage::Ping { nonce: 1 });
        }
        let (_sent, dropped) = handle.stats();
        assert!(dropped > 0, "messages should have been dropped, not buffered");

        // ... and the client that caused it gets disconnected rather than
        // being allowed to grow the server's memory forever.
        let reason = handle.kicked().await;
        assert!(matches!(reason, DisconnectReason::Other(_)));
    }

    /// A connected player with an empty pack, nothing on, and a queue
    /// nobody reads -- everything the outfit tests below need and
    /// nothing else.
    fn dressed_up() -> Arc<PlayerHandle> {
        let (tx, _rx) = mpsc::channel::<Outgoing>(64);
        let (chunk_tx, _crx) = mpsc::channel::<ChunkPos>(64);
        Arc::new(PlayerHandle::new(
            1,
            "wearer".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            1000,
            (0.0, 40.0, 0.0),
            AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                (0.0, 40.0, 0.0),
            ),
        ))
    }

    #[test]
    fn a_helmet_is_in_the_very_next_snapshot_the_wearer_appears_in() {
        // **The property the whole feature rests on.** Everything else
        // the server owns about a body is sent on change; this is read
        // fresh out of the authoritative state every time a snapshot is
        // sampled, so there is no "dirty" flag to forget and no window
        // in which somebody is wearing something nobody can see. See
        // `protocol::Outfit` for why that is worth ten bytes a tick.
        use primitive_shared::equipment::Slot;
        use primitive_shared::inventory::Stack;
        use primitive_shared::types::{BLOCK_AIR, BLOCK_IRON_HELM, BLOCK_STONE_PICKAXE};

        let handle = dressed_up();
        assert!(handle.player_state().outfit.is_bare());

        {
            let mut state = handle.state.lock().unwrap();
            state.equipment.wear(Stack::new(BLOCK_IRON_HELM, 1));
            state.inventory.put_in_slot(0, Stack::new(BLOCK_STONE_PICKAXE, 1));
            state.selected_slot = 0;
        }
        let outfit = handle.player_state().outfit;
        assert_eq!(outfit.worn_in(Slot::Head), BLOCK_IRON_HELM);
        assert_eq!(outfit.holding, BLOCK_STONE_PICKAXE);
        // ...and the slots nobody filled stay empty rather than
        // repeating the helmet down the body.
        assert_eq!(outfit.worn_in(Slot::Chest), BLOCK_AIR);
        assert_eq!(outfit.worn_in(Slot::Legs), BLOCK_AIR);
        assert_eq!(outfit.worn_in(Slot::Feet), BLOCK_AIR);

        // Taking it off is seen just as promptly. A snapshot that only
        // ever *added* garments would leave a player who unequipped in
        // a fight visibly armoured to everyone but themselves.
        {
            let mut state = handle.state.lock().unwrap();
            state.equipment.take(Slot::Head);
        }
        let outfit = handle.player_state().outfit;
        assert_eq!(outfit.worn_in(Slot::Head), BLOCK_AIR);
        assert_eq!(outfit.holding, BLOCK_STONE_PICKAXE, "the hand emptied too");
    }

    /// **A dead player is seen lying down until they come back**, and on
    /// their feet again the moment they do. Nothing in the snapshot used to
    /// say so, and the figure stood over its own body on every other screen.
    /// Dead outranks the bed and the seat, which a death does not always
    /// clear before the next snapshot is sampled.
    #[test]
    fn a_dead_player_is_seen_fallen_and_a_respawned_one_standing() {
        use primitive_shared::protocol::Posture;
        let handle = dressed_up();
        assert_eq!(handle.player_state().posture, Posture::Standing);
        {
            let mut state = handle.state.lock().unwrap();
            state.sleeping_in = Some((0, 0, 0));
            state.vitals.hurt(1.0e6, "a test");
        }
        assert_eq!(handle.player_state().posture, Posture::Fallen);
        {
            let mut state = handle.state.lock().unwrap();
            state.sleeping_in = None;
            state.vitals.set_health(50.0);
        }
        assert_eq!(handle.player_state().posture, Posture::Standing);
    }

    #[test]
    fn what_a_player_is_holding_follows_the_hotbar_slot_they_selected() {
        // The held block is the *selected* slot rather than slot zero,
        // and it is the same slot a placement spends and mining asks
        // about -- so a player who switches from a pick to a torch is
        // seen to switch. Reading a fixed slot would have been a model
        // that quietly holds whatever is first in the pack.
        use primitive_shared::inventory::Stack;
        use primitive_shared::types::{BLOCK_AIR, BLOCK_STONE_PICKAXE, BLOCK_TORCH};

        let handle = dressed_up();
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, Stack::new(BLOCK_STONE_PICKAXE, 1));
            state.inventory.put_in_slot(1, Stack::new(BLOCK_TORCH, 4));
            state.selected_slot = 1;
        }
        assert_eq!(handle.player_state().outfit.holding, BLOCK_TORCH);
        {
            handle.state.lock().unwrap().selected_slot = 0;
        }
        assert_eq!(handle.player_state().outfit.holding, BLOCK_STONE_PICKAXE);
        // An empty slot is an empty hand, not the last thing held.
        {
            handle.state.lock().unwrap().selected_slot = 5;
        }
        assert_eq!(handle.player_state().outfit.holding, BLOCK_AIR);
    }

    #[test]
    fn subscriptions_are_cleaned_up_on_disconnect() {
        let registry = Registry::new(10, 10);
        let (tx, _rx) = mpsc::channel::<Outgoing>(8);
        let (chunk_tx, _crx) = mpsc::channel::<ChunkPos>(8);
        let handle = Arc::new(PlayerHandle::new(
            1,
            "a".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            100,
            (0.0, 0.0, 0.0),
            AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                (0.0, 0.0, 0.0),
            ),
        ));
        let pos = ChunkPos::new(4, 4);
        handle
            .state
            .lock()
            .unwrap()
            .loaded_chunks
            .insert(pos);
        assert!(registry.insert_unique(Arc::clone(&handle)));
        registry.subscribe(1, pos);
        assert_eq!(registry.subscribers(pos).len(), 1);

        registry.remove(1);
        assert_eq!(registry.subscribers(pos).len(), 0);
        assert_eq!(registry.subscription_count(), 0, "index must not leak entries");
    }
}
