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
///
/// v3 added survival: `Health`, `Died`, `Respawned` and `Respawn`.
/// v4 added `CarriedWeight`, which fed fall damage.
/// v5 moved the inventory to the server: `InventoryState` replaces it,
/// along with `SwapSlots`, `DropSlot`, `Craft` and `SelectSlot`, and
/// `EntityKind::Item` carries dropped stacks in the world.
/// v6 gave the inventory screen the rest of its gestures: `SwapSlots`
/// became `MoveSlots` (which merges rather than only swapping), and
/// `SplitSlot`, `QuickMoveSlot` and `SortInventory` joined it. `Craft`
/// gained `times`.
/// v7 changed nothing about the messages, and is a bump anyway: fibre
/// is a new block id, pulling up grass yields it instead of the tuft,
/// and the recipe table gained two entries while `thatch` changed what
/// it costs. `Craft` names a recipe by its index into that table, so an
/// older client asking for recipe 7 would be spending the wrong
/// ingredients -- which the message format cannot express, and only a
/// version check can catch.
/// v8 added loose stones: a new block id, a tenth recipe, and a third
/// block shape. Same reason as v7 -- `Craft` names a recipe by its index
/// into a table both sides have to agree on, and an older client would
/// be spending the wrong ingredients.
/// v9 changed the rules rather than the messages: rock and standing
/// timber cannot be broken by hand, dressed stone is no longer
/// placeable, and recipe 2 now splits cobble into stones instead of
/// pressing stones into a block. A recipe is named by its index, so two
/// sides disagreeing about what index 2 means is two sides spending
/// different ingredients.
/// v10 added `Attack`, and with it the first damage in the game that is
/// not the ground. Flint is a new block id and a twelfth recipe, and
/// the terrain generator was rebuilt -- the seed is on the wire, so two
/// sides disagreeing about what a seed means is a client drawing a
/// world the server does not have.
/// v11 added chests: `OpenChest`, `CloseChest`, `ChestMove`,
/// `ChestQuickMove` and `ChestState`, and a block id to go with them.
/// The chest is also the first block whose *contents* are server state,
/// which is why the gestures name a side rather than a slot index --
/// see `Side`.
/// v12 also added `ChestBulkMove`: storing forty slots one message at a
/// time is what the rate limit is for, and a transfer that half
/// happened because a message was dropped is worse than one that did
/// not. It changed what a block id *means* rather than adding a message:
/// loose material carries a depth in the same field a log carries its
/// axis in, so a cell of sand may be any of eight heights. Nothing on
/// the wire is a different shape, and that is exactly why this had to
/// be a version bump -- an older client would take a three-eighths
/// drift of snow for a whole block, draw it as one, walk on top of it,
/// and aim at a metre of empty air above it. A refused handshake says
/// so; a world that is quietly the wrong shape does not.
/// v14 added the backpack: a new block id, a second container, and a new
/// thing that happens when you die -- the pack goes into a block at the
/// death site instead of staying on the corpse. No message changed
/// shape, and that is once again exactly why the version had to move. An
/// older client would meet an id it has no row for, draw a dead player's
/// belongings as the unknown-block placeholder, and refuse to open the
/// one thing in the world worth opening; a newer client against an older
/// server would keep expecting to respawn with its pack. Neither is
/// something the message layout can express, and a refused handshake is
/// the only place either can be caught.
/// v15 added ore, metal and tools: thirteen block ids, eight recipes at
/// the end of the table, and a rule about mining that did not exist
/// before -- what you are *holding* now decides whether a block gives
/// way at all.
///
/// No message changed shape, and as with v12 and v14 that is precisely
/// why the number had to move. The disagreements an older peer would
/// have are all invisible ones. An older client would meet copper ore
/// and draw the unknown-block placeholder into the middle of a hillside;
/// worse, it computes its own mining progress, so against this server it
/// would fill a progress bar on a rock it has no tool for and have every
/// swing refused, which reads as the game being broken rather than as a
/// rule. A newer client against an older server would ask for recipes by
/// indices that server does not have. A refused handshake is the only
/// place any of that can be caught.
/// v16 took the metal picks out and put a stone age in: three flint
/// tools instead of a four-rung pick ladder, seven new block ids for the
/// parts they are assembled from, and eight recipes **in the middle of
/// the table** where the one-click flint pick used to be.
///
/// That last part is why this bump is not optional and not cosmetic.
/// `Craft` names a recipe by its index, and every index from the flint
/// pick onwards now means something else -- an older client asking for
/// what it thinks is "copper ingot" would be spending a player's flint
/// on a knife head. Nor could an older client survive the block ids: it
/// would find a knife in a pack it has no row for and draw the unknown
/// placeholder, and it computes its own mining progress, so it would
/// fill a bar on a standing tree it believes nothing can fell and have
/// every swing refused. A newer client against an older server is the
/// mirror image, asking for recipes past the end of that server's
/// table. None of it changes the shape of a message; all of it is
/// caught by the handshake and nowhere else.
pub const PROTOCOL_VERSION: u32 = 16;

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
    /// A dropped stack lying in the world, waiting to be picked up.
    /// Drawn as a small copy of the block it is.
    Item { block: BlockId, count: u32 },
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

/// Which of the two open inventories a chest gesture is about.
///
/// A side and a slot rather than one number over both, because the two
/// inventories are two different things on the server -- one belongs to
/// the player and one to a place in the world -- and a single index
/// space would mean a client could name a chest slot where a pack slot
/// was expected by getting the arithmetic wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    /// The player's own pack, hotbar included.
    Pack,
    /// The chest they currently have open.
    Chest,
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
    /// "Move the stack in `from` onto `to`."
    ///
    /// Merged if the two hold the same block, swapped otherwise. It was
    /// a plain swap in v5, which meant two part-stacks of stone could
    /// never be made into one.
    MoveSlots {
        from: u8,
        to: u8,
    },
    /// "Put half of `from` in `to`." `to` has to be empty or hold the
    /// same block.
    SplitSlot {
        from: u8,
        to: u8,
    },
    /// "Send this stack between the bar and the pile behind it" -- the
    /// shift-click. Which way round is decided by where the slot is, so
    /// there is nothing here to get wrong.
    QuickMoveSlot {
        slot: u8,
    },
    /// "Tidy the storage rows." The hotbar is left alone: where things
    /// sit on the bar is an arrangement the player made.
    SortInventory,
    /// "Throw this out." `whole_stack` is the shift-click: all of it
    /// rather than one.
    DropSlot {
        slot: u8,
        whole_stack: bool,
    },
    /// "Make recipe number `index`, up to `times` of it." An index
    /// rather than a description of the recipe, so a client can only ask
    /// for one the server also has -- it looks the index up in its own
    /// table. The server stops early when the ingredients or the room
    /// run out, so `times` is an ask rather than a promise.
    Craft {
        index: u16,
        times: u8,
    },
    /// Which hotbar slot is selected, so the server knows what a
    /// placement should spend.
    SelectSlot {
        slot: u8,
    },
    /// "Open the chest at that cell."
    ///
    /// The server checks it is within reach and that the cell really
    /// holds a chest, then answers with `ChestState` and remembers which
    /// chest this player has open. Every gesture below is against *that*
    /// chest and carries no position of its own, so a client cannot
    /// reach into a chest across the map by naming it.
    OpenChest {
        global_x: i32,
        global_y: i32,
        global_z: i32,
    },
    /// "I am done with it." Also sent when the screen closes for any
    /// other reason, so the server stops sending updates for it.
    CloseChest,
    /// "Move this slot onto that one." `half` is the right-click: half
    /// the stack rather than all of it.
    ///
    /// Both sides may be the pack or the chest, so this one message is
    /// also how things are rearranged *within* an open chest.
    ChestMove {
        from: (Side, u8),
        to: (Side, u8),
        half: bool,
    },
    /// "Send this slot to the other side" -- the shift-click. Which way
    /// round is decided by which side it is on, so there is nothing here
    /// to get wrong.
    /// Everything that fits, in one gesture.
    ///
    /// One message rather than forty `ChestQuickMove`s, for two
    /// reasons: a burst of forty is exactly what the message rate limit
    /// exists to stop, and a bulk transfer that is *partly* applied
    /// because the fortieth was dropped is worse than one that is not
    /// applied at all.
    ChestBulkMove {
        /// True to send the pack into the chest, false to empty the
        /// chest into the pack.
        to_chest: bool,
    },
    ChestQuickMove {
        side: Side,
        slot: u8,
    },
    /// "I swung at that player."
    ///
    /// Deliberately the whole of the message. No damage figure, no
    /// position, no direction: the server has its own copy of where
    /// everyone is and its own idea of what a punch is worth, and the
    /// only thing it cannot work out for itself is who was aimed at.
    /// See `primitive_shared::combat` for what it checks.
    Attack {
        target: PlayerId,
    },
    /// "I have read the death screen, put me back in the world."
    ///
    /// Respawning is a request rather than something the server does on
    /// its own timer, so a player who died is not dropped back into the
    /// world before they have seen why.
    Respawn,
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
    /// One chunk of the world.
    ///
    /// Behind an `Arc` because a chunk is 32 KB and the server hands the
    /// same one to every player who can see it. Sending it used to deep
    /// copy that array per player per chunk -- megabytes a second of
    /// pure memcpy while terrain streams, and in singleplayer it is the
    /// game's own process paying it. `serde`'s `rc` feature serialises
    /// an `Arc<T>` as plain `T`, so the bytes on the wire are exactly
    /// what they were.
    ChunkData(std::sync::Arc<Chunk>),
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
    /// The player's whole inventory.
    ///
    /// Sent as a snapshot rather than as deltas, and only when something
    /// changes. Forty slots of `Option<(u16, u32)>` is under half a
    /// kilobyte -- far less than one chunk -- and a snapshot cannot
    /// drift out of step with the server the way a stream of deltas can
    /// after a single dropped message.
    InventoryState {
        inventory: crate::inventory::Inventory,
    },
    /// What is in the chest the player has open.
    ///
    /// A snapshot, for the same reason the inventory is one: forty slots
    /// is under half a kilobyte, and a snapshot cannot drift out of step
    /// with the server the way a stream of deltas can after one dropped
    /// message. Sent when the chest is opened and after every change to
    /// it -- including changes another player made, so two people at one
    /// chest see the same thing.
    ChestState {
        global_x: i32,
        global_y: i32,
        global_z: i32,
        inventory: crate::inventory::Inventory,
    },
    /// The open chest is gone -- broken, or out of range. The client
    /// shuts the screen; anything else would leave it showing a chest
    /// that no longer exists.
    ChestClosed,
    /// Current and maximum health.
    ///
    /// Sent only when the value actually changes, not every tick: health
    /// is static for most of a session, and a per-tick broadcast would
    /// cost more bandwidth than player movement does.
    /// How much air is left, 0..1, and only while it is running out.
    ///
    /// Its own message rather than a field on `Health`, because the two
    /// change on completely different schedules: health changes when
    /// something happens, and breath changes every tick of a dive and
    /// never again. Sent only when it is *not* full, so a player who
    /// never puts their head under water never receives one.
    Breath {
        fraction: f32,
    },
    Health {
        current: f32,
        max: f32,
    },
    /// The player's health reached zero. The client shows this and asks
    /// for `ClientMessage::Respawn` when the player is ready.
    ///
    /// Carries its own text because the server is the only side that
    /// knows *why* -- the client cannot tell a fall from a drowning, and
    /// "you died" with no cause is the kind of thing players reload a
    /// save over.
    Died {
        cause: String,
    },
    /// Health restored and the player put back at the spawn point.
    Respawned {
        x: f32,
        y: f32,
        z: f32,
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
