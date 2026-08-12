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
use primitive_shared::protocol::{DisconnectReason, PlayerId, PlayerState, ServerMessage};
use primitive_shared::types::ChunkPos;

use crate::anticheat::AntiCheat;

/// Mutable per-player state. Guarded by a short-lived std mutex; nothing
/// in here is ever held across an `.await`.
pub struct PlayerRuntime {
    pub position: (f32, f32, f32),
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub last_activity: Instant,
    pub loaded_chunks: HashSet<ChunkPos>,
    pub anticheat: AntiCheat,
}

pub struct PlayerHandle {
    pub id: PlayerId,
    pub username: String,
    pub addr: SocketAddr,
    pub joined_at: Instant,

    tx: mpsc::Sender<ServerMessage>,
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
        tx: mpsc::Sender<ServerMessage>,
        chunk_tx: mpsc::Sender<ChunkPos>,
        drop_threshold: u64,
        spawn: (f32, f32, f32),
        anticheat: AntiCheat,
    ) -> Self {
        Self {
            id,
            username,
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
        match self.tx.try_send(msg) {
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
            yaw: state.yaw,
            pitch: state.pitch,
            on_ground: state.on_ground,
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

    pub fn insert(&self, handle: Arc<PlayerHandle>) {
        let mut players = self.players.write().unwrap_or_else(|e| e.into_inner());
        players.insert(handle.id, handle);
        let now = players.len() as u64;
        self.peak_players.fetch_max(now, Ordering::Relaxed);
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
    pub fn player_occupying_block(&self, bx: i32, by: i32, bz: i32) -> Option<Arc<PlayerHandle>> {
        self.handles().into_iter().find(|handle| {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            block_overlaps_player(state.position, bx, by, bz)
        })
    }

    pub fn broadcast(&self, msg: ServerMessage) {
        for handle in self.handles() {
            handle.send(msg.clone());
        }
    }

    pub fn broadcast_except(&self, except: PlayerId, msg: ServerMessage) {
        for handle in self.handles() {
            if handle.id != except {
                handle.send(msg.clone());
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
    origin: (f32, f32, f32),
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
fn distance_sq(state: &PlayerState, origin: (f32, f32, f32)) -> f32 {
    let dx = state.x - origin.0;
    let dy = state.y - origin.1;
    let dz = state.z - origin.2;
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
        origin: (f32, f32, f32),
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

#[inline]
fn cell_of(x: f32, z: f32, cell_size: f32) -> (i32, i32) {
    (
        (x / cell_size).floor() as i32,
        (z / cell_size).floor() as i32,
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
                x,
                y: 30.0,
                z,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: true,
            },
        )
    }

    /// The grid must answer exactly what the flat scan answered. This is
    /// the property that matters: it is an optimisation, so any
    /// disagreement is a bug in the optimisation.
    fn agrees(states: &[(PlayerId, PlayerState)], origin: (f32, f32, f32), radius: f32, exclude: PlayerId) {
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
mod tests {
    use super::*;

    fn state(id: PlayerId, x: f32, z: f32) -> (PlayerId, PlayerState) {
        (
            id,
            PlayerState {
                id,
                x,
                y: 30.0,
                z,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: true,
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
        let (tx, _rx) = mpsc::channel::<ServerMessage>(2);
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

    #[test]
    fn subscriptions_are_cleaned_up_on_disconnect() {
        let registry = Registry::new(10, 10);
        let (tx, _rx) = mpsc::channel::<ServerMessage>(8);
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
        registry.insert(Arc::clone(&handle));
        registry.subscribe(1, pos);
        assert_eq!(registry.subscribers(pos).len(), 1);

        registry.remove(1);
        assert_eq!(registry.subscribers(pos).len(), 0);
        assert_eq!(registry.subscription_count(), 0, "index must not leak entries");
    }
}
