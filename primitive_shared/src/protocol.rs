//! Wire protocol.
//!
//! Shape of this version (v2), and why:
//!
//! * **Versioned handshake.** The very first thing a client sends is
//!   `Hello`; the server answers `Welcome` or `Rejected`. Anything else
//!   before the handshake is a protocol error. This is what lets an old
//!   client be told "you're out of date" instead of silently desyncing
//!   once the message layout changes.
//! * **Batching.** `RequestChunks`, `BlockUpdates` and `Snapshot` carry
//!   many items per message. At 1 player it makes no difference; at 500
//!   it's the difference between 20 relayed messages per player per tick
//!   and 1.
//! * **Tick-based snapshots.** Player movement is no longer relayed
//!   message-for-message. The server samples every player once per tick
//!   and sends each of them one `Snapshot` containing only the players
//!   inside their interest radius -- O(nearby), not O(all players).
//! * **Server-authoritative correction.** `PositionCorrection` exists so
//!   the anti-cheat can reject a move instead of only logging it.

use serde::{Deserialize, Serialize};

use crate::types::{BlockId, Chunk, ChunkPos};

pub type PlayerId = u64;

/// Bumped on any incompatible change to the messages below. The server
/// refuses a client whose version doesn't match.
pub const PROTOCOL_VERSION: u32 = 2;

pub const MAX_USERNAME_LEN: usize = 24;
pub const MAX_CHAT_LEN: usize = 256;

/// One player's state as sampled by the server on a given tick.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlayerState {
    pub id: PlayerId,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

pub type EntityId = u64;

/// What an entity is. Kept as an enum rather than a free id so the
/// client can't be asked to render something it doesn't understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    /// A block in mid-fall. `block` is what it will become when it
    /// lands, and what it's drawn as.
    FallingBlock { block: BlockId },
}

/// One entity as sampled by the server on a given tick.
///
/// Entities are replicated the same way players are: a per-tick
/// snapshot of everything near the recipient, with no explicit despawn
/// message. A client drops anything it stops hearing about, which means
/// a lost despawn can't leave a permanent ghost -- the failure mode is
/// an entity lingering for a fraction of a second.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EntityState {
    pub id: EntityId,
    pub kind: EntityKind,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A single block change, for batched updates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BlockChange {
    pub global_x: i32,
    pub global_y: i32,
    pub global_z: i32,
    pub block_id: BlockId,
}

/// Why the server is disconnecting someone. Kept as an enum rather than a
/// free string so a client (or a future admin UI) can react per-reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DisconnectReason {
    ProtocolMismatch { server_version: u32 },
    ServerFull,
    Banned,
    Timeout,
    AntiCheat(String),
    RateLimited,
    ServerShutdown,
    Other(String),
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisconnectReason::ProtocolMismatch { server_version } => {
                write!(f, "protocol mismatch (server speaks v{server_version})")
            }
            DisconnectReason::ServerFull => write!(f, "server is full"),
            DisconnectReason::Banned => write!(f, "banned"),
            DisconnectReason::Timeout => write!(f, "timed out"),
            DisconnectReason::AntiCheat(d) => write!(f, "anti-cheat: {d}"),
            DisconnectReason::RateLimited => write!(f, "too many requests"),
            DisconnectReason::ServerShutdown => write!(f, "server shutting down"),
            DisconnectReason::Other(d) => write!(f, "{d}"),
        }
    }
}

/// Messages the client sends to the server. The server is the sole source
/// of truth (авторитативный сервер) -- the client only ever *requests*
/// things, it never asserts world state directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Must be the first message on the connection.
    Hello {
        protocol_version: u32,
        username: String,
    },
    RequestChunk(ChunkPos),
    /// Batched form of `RequestChunk` -- one message for a whole new
    /// render-distance ring instead of ~50.
    RequestChunks(Vec<ChunkPos>),
    SetBlock {
        global_x: i32,
        global_y: i32,
        global_z: i32,
        block_id: BlockId,
    },
    /// "My feet are here, looking this way." Rate-limited client-side and
    /// re-validated server-side (see the server's `anticheat` module --
    /// this is exactly the message a cheat client would lie in).
    UpdateTransform {
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
        /// Monotonic per-client counter, so the server can detect
        /// reordering/replay and measure the real update rate.
        sequence: u32,
    },
    Chat(String),
    /// Reply to `ServerMessage::Ping`, echoing the nonce back.
    Pong {
        nonce: u64,
    },
    Disconnect,
}

/// Messages the server sends to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Handshake accepted. Carries everything the client needs to
    /// configure itself against *this* server rather than guessing:
    /// where to spawn, how far the server will actually stream, what the
    /// tick rate is, and the current time of day for lighting.
    Welcome {
        your_id: PlayerId,
        protocol_version: u32,
        server_name: String,
        tick_rate_hz: f32,
        view_distance_chunks: i32,
        world_seed: u32,
        spawn: (f32, f32, f32),
        time_of_day: f32,
        /// Real seconds per in-game day, so the client can keep the sun
        /// moving smoothly between `TimeSync` messages instead of
        /// stepping it every two seconds.
        day_length_seconds: f32,
    },
    /// Handshake refused; the connection closes right after.
    Rejected(DisconnectReason),
    ChunkData(Chunk),
    BlockUpdate(BlockChange),
    /// Batched block changes (bulk edits, or several edits landing in the
    /// same tick).
    BlockUpdates(Vec<BlockChange>),
    /// All players near the recipient, as of `tick`. Replaces per-message
    /// relaying of movement.
    Snapshot {
        tick: u64,
        states: Vec<PlayerState>,
    },
    /// All entities near the recipient, as of `tick`. Sent only when
    /// there are any, so an idle world costs nothing.
    Entities {
        tick: u64,
        states: Vec<EntityState>,
    },
    PlayerJoined {
        id: PlayerId,
        username: String,
    },
    PlayerLeft {
        id: PlayerId,
    },
    Chat {
        from: Option<PlayerId>,
        username: String,
        text: String,
    },
    /// World clock for the day/night cycle. `time_of_day` is 0.0..1.0,
    /// where 0.0 = midnight, 0.5 = noon. Sun direction and sky/fog colour
    /// are derived from this on the client, so every player sees the same
    /// sky at the same moment.
    TimeSync {
        tick: u64,
        time_of_day: f32,
    },
    /// Anti-cheat rubber-band: the server rejected the client's reported
    /// position and is telling it where it actually is.
    PositionCorrection {
        x: f32,
        y: f32,
        z: f32,
        reason: String,
    },
    /// Keepalive. The client must answer with `ClientMessage::Pong`;
    /// silence past the configured timeout is a disconnect.
    Ping {
        nonce: u64,
    },
    Kick(DisconnectReason),
    Error(String),
}

/// Trims/sanitises a username before it's shown to other players or
/// written to a log. Runs on the *server* -- a client can send anything.
pub fn sanitize_username(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_USERNAME_LEN)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "player".to_string()
    } else {
        cleaned
    }
}

pub fn sanitize_chat(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX_CHAT_LEN)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_sanitising_is_defensive() {
        assert_eq!(sanitize_username("  Shamkhan \n"), "Shamkhan");
        assert_eq!(sanitize_username(""), "player");
        assert_eq!(sanitize_username("\u{0}\u{1}"), "player");
        assert_eq!(sanitize_username(&"x".repeat(200)).len(), MAX_USERNAME_LEN);
    }

    #[test]
    fn chat_is_length_capped() {
        assert!(sanitize_chat(&"y".repeat(1000)).len() <= MAX_CHAT_LEN);
    }

    #[test]
    fn messages_roundtrip_through_bincode() {
        let msg = ServerMessage::Snapshot {
            tick: 7,
            states: vec![PlayerState {
                id: 1,
                x: 1.0,
                y: 2.0,
                z: 3.0,
                yaw: 0.5,
                pitch: -0.2,
                on_ground: true,
            }],
        };
        let bytes = bincode::serialize(&msg).unwrap();
        let back: ServerMessage = bincode::deserialize(&bytes).unwrap();
        match back {
            ServerMessage::Snapshot { tick, states } => {
                assert_eq!(tick, 7);
                assert_eq!(states.len(), 1);
                assert_eq!(states[0].id, 1);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}

#[cfg(test)]
mod entity_tests {
    use super::*;
    use crate::types::BLOCK_SAND;

    #[test]
    fn entity_snapshots_survive_a_round_trip() {
        let states = vec![EntityState {
            id: 7,
            kind: EntityKind::FallingBlock { block: BLOCK_SAND },
            x: 1.5,
            y: 40.25,
            z: -3.5,
        }];
        let message = ServerMessage::Entities { tick: 99, states };
        let bytes = bincode::serialize(&message).unwrap();
        let decoded: ServerMessage = bincode::deserialize(&bytes).unwrap();

        match decoded {
            ServerMessage::Entities { tick, states } => {
                assert_eq!(tick, 99);
                assert_eq!(states.len(), 1);
                assert_eq!(states[0].id, 7);
                assert_eq!(states[0].kind, EntityKind::FallingBlock { block: BLOCK_SAND });
                assert_eq!(states[0].y, 40.25);
            }
            other => panic!("wrong message: {other:?}"),
        }
    }
}
