//! A raft, end to end over real sockets.
//!
//! The unit tests in `primitive_shared::raft` cover the rules and the ones in
//! `primitive_server::rafts` the store. These cover the **wiring**, which is
//! where a raft goes wrong in a way nobody can see from inside one module:
//!
//! - a raft the server moves and never puts in anybody's entity snapshot;
//! - a rider the server carries on its own raft while every other screen
//!   places them from the snapshots of a different tick, so they slide about
//!   a deck going in a straight line;
//! - a player who joins after the raft has left the shore and is shown it
//!   where it was launched.
//!
//! On the `Test` preset, with a pond dug into it by the test: the flat field
//! is the same every run, and a pond sunk into the ground keeps its water
//! rather than spilling across the plaza while the raft is on it.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{
    ClientMessage, EntityId, EntityKind, EntityState, PlayerId, PlayerState, Posture, ServerMessage,
    PROTOCOL_VERSION,
};
use primitive_shared::raft::{self, Body};
use primitive_shared::types::{BLOCK_AIR, BLOCK_RAFT, BLOCK_WATER};
use tokio::net::TcpStream;

fn test_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "test".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        stats_interval_secs: 0.0,
        world_preset: primitive_shared::worldgen::Preset::Test,
        // These tests put players where they need them to be; the movement
        // validator has its own tests, and a deck transform does not pass
        // through it (see `rafts::deck`).
        anticheat: primitive_server::settings::AntiCheatSettings { enabled: false, ..Default::default() },
        ..Default::default()
    }
}

struct Client {
    socket: TcpStream,
    id: PlayerId,
    spawn: (f32, f32, f32),
    sequence: u32,
    /// Everything read so far that a test may still want to look at.
    heard: Vec<ServerMessage>,
}

impl Client {
    async fn connect(address: &str, username: &str) -> Self {
        let mut socket = TcpStream::connect(address).await.expect("connect");
        write_message(&mut socket, &ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, username: username.to_string() })
            .await
            .expect("hello");
        let (id, spawn) = match read_message::<_, ServerMessage>(&mut socket).await.expect("welcome") {
            ServerMessage::Welcome { your_id, spawn, .. } => (your_id, spawn),
            other => panic!("expected Welcome, got {other:?}"),
        };
        Self { socket, id, spawn: primitive_shared::geometry::narrow(spawn), sequence: 0, heard: Vec::new() }
    }

    async fn send(&mut self, message: ClientMessage) {
        write_message(&mut self.socket, &message).await.expect("send");
    }

    async fn stand_at(&mut self, x: f32, y: f32, z: f32) {
        self.sequence += 1;
        self.send(ClientMessage::UpdateTransform { x: f64::from(x), y: f64::from(y), z: f64::from(z), yaw: 0.0, pitch: 0.0, on_ground: true, sequence: self.sequence })
            .await;
    }

    async fn on_deck(&mut self, raft: EntityId, local: [f32; 3]) {
        self.sequence += 1;
        self.send(ClientMessage::Deck {
            raft,
            x: f64::from(local[0]),
            y: f64::from(local[1]),
            z: f64::from(local[2]),
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            sequence: self.sequence,
        })
        .await;
    }

    /// Reads for `millis`, keeping what came.
    async fn listen(&mut self, millis: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(millis);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return;
            }
            match tokio::time::timeout(remaining, read_message::<_, ServerMessage>(&mut self.socket)).await {
                Ok(Ok(message)) => self.heard.push(message),
                _ => return,
            }
        }
    }

    /// Reads until something matches, for up to ten seconds.
    async fn wait_for<T>(&mut self, mut want: impl FnMut(&ServerMessage) -> Option<T>) -> Option<T> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let message = match tokio::time::timeout(remaining, read_message::<_, ServerMessage>(&mut self.socket)).await {
                Ok(Ok(message)) => message,
                _ => return None,
            };
            let found = want(&message);
            self.heard.push(message);
            if found.is_some() {
                return found;
            }
        }
    }

    /// The latest raft this client has been shown, with the tick it was on.
    fn latest_raft(&self) -> Option<(u64, EntityState)> {
        self.heard.iter().rev().find_map(|m| match m {
            ServerMessage::Entities { tick, states } => states
                .iter()
                .find(|s| matches!(s.kind, EntityKind::Raft { .. }))
                .map(|s| (*tick, *s)),
            _ => None,
        })
    }

    /// Every tick this client was shown both a raft and `player` on, paired.
    fn raft_and_player_by_tick(&self, player: PlayerId) -> Vec<(Body, PlayerState)> {
        let rafts: std::collections::HashMap<u64, Body> = self
            .heard
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Entities { tick, states } => states.iter().find_map(body).map(|b| (*tick, b)),
                _ => None,
            })
            .collect();
        self.heard
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Snapshot { tick, states } => {
                    let state = states.iter().find(|s| s.id == player)?;
                    Some((*rafts.get(tick)?, *state))
                }
                _ => None,
            })
            .collect()
    }
}

fn body(state: &EntityState) -> Option<Body> {
    match state.kind {
        EntityKind::Raft { yaw, vx, vz, spin, sail, sail_angle, .. } => {
            Some(Body { x: state.x, y: state.y as f32, z: state.z, yaw, vx, vz, spin, sail, sail_angle })
        }
        _ => None,
    }
}

#[tokio::test]
async fn two_players_ride_one_raft_and_everyone_sees_them_where_they_stand_on_its_deck() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded()).await.expect("start");
    let address = server.address().to_string();
    let mut rower = Client::connect(&address, "rower").await;
    rower
        .wait_for(|m| matches!(m, ServerMessage::InventoryState { .. }).then_some(()))
        .await
        .expect("the server never sent an opening inventory");

    // A pond sunk into the field east of the spawn: water a cell deep where
    // the ground was, and the air above it cleared of whatever the test world
    // stood there.
    let (sx, sy, sz) = rower.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    for x in bx + 2..=bx + 16 {
        for z in bz - 5..=bz + 5 {
            server.place_block(x, by - 1, z, BLOCK_WATER);
            for y in by..=by + 3 {
                server.place_block(x, y, z, BLOCK_AIR);
            }
        }
    }

    // The raft, out of the pack and onto the pond.
    assert_eq!(server.give(BLOCK_RAFT, 1), 0, "the raft did not fit in the pack");
    let slot = rower
        .wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_RAFT) > 0 => {
                (0..primitive_shared::inventory::SLOTS).find(|&s| inventory.block_in(s) == Some(BLOCK_RAFT))
            }
            _ => None,
        })
        .await
        .expect("the server never said the raft was in the pack");
    rower.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;
    rower.stand_at(sx, sy, sz).await;
    rower.listen(150).await;
    rower.send(ClientMessage::UseBlock { global_x: bx + 3, global_y: by - 1, global_z: bz }).await;
    let launched = rower
        .wait_for(|m| match m {
            ServerMessage::Entities { states, .. } => states.iter().find_map(|s| body(s).map(|b| (s.id, b))),
            _ => None,
        })
        .await;
    let (id, at_launch) = launched.expect("a raft right-clicked onto water never appeared");
    assert!(
        rower.heard.iter().rev().any(|m| matches!(m, ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_RAFT) == 0)),
        "the raft went onto the water and stayed in the pack"
    );

    // Aboard at the stern, and at the oars.
    let stern = at_launch.world_of([-1.0, 0.0, 0.0]);
    rower.stand_at((stern[0]) as f32, (stern[1]) as f32, (stern[2]) as f32).await;
    rower.listen(100).await;
    rower.on_deck(id, [-1.0, 0.0, 0.0]).await;
    rower.listen(100).await;
    rower.send(ClientMessage::UseRaft { raft: id }).await;
    rower
        .wait_for(|m| matches!(m, ServerMessage::Oars { raft: Some(raft) } if *raft == id).then_some(()))
        .await
        .expect("using a raft from its deck did not give the oars");

    // Rowing, while somebody else arrives.
    let mut passenger = Client::connect(&address, "passenger").await;
    for _ in 0..8 {
        rower.send(ClientMessage::Row { raft: id, stroke: 1.0, turn: 0.0 }).await;
        rower.listen(100).await;
        passenger.listen(25).await;
    }

    // **Joined late: shown the raft where it is**, not where it was launched.
    passenger.listen(300).await;
    let (_, seen) = passenger.latest_raft().expect("a player who joined later was never shown the raft");
    let seen = body(&seen).expect("a raft");
    assert!(seen.x > at_launch.x + 0.5, "the late joiner was shown the raft at {} when it was launched at {}", seen.x, at_launch.x);

    // They step aboard, forward of the mast.
    let place = [0.9, 0.0, 0.4];
    let deck = seen.world_of(place);
    passenger.stand_at((deck[0]) as f32, (deck[1]) as f32, (deck[2]) as f32).await;
    passenger.listen(100).await;
    passenger.on_deck(id, place).await;

    for _ in 0..12 {
        rower.send(ClientMessage::Row { raft: id, stroke: 1.0, turn: 0.3 }).await;
        rower.listen(80).await;
        passenger.listen(40).await;
    }

    // **Everyone sees the rider where they stand on the deck**: in every tick
    // that carried both, the rider is at their place on *that tick's* raft.
    let rower_as_seen = passenger.raft_and_player_by_tick(rower.id);
    assert!(rower_as_seen.len() > 5, "the passenger was hardly shown the rower and the raft together");
    for (raft, state) in rower_as_seen.iter().rev().take(10) {
        let seat = raft.world_of(raft::SEAT);
        let off = (state.x - seat[0]).hypot(state.z - seat[2]);
        assert!(off < 0.02, "the rower was drawn {off} blocks off the stern of the raft in the same tick");
        assert_eq!(state.posture, Posture::Sitting, "the rower is shown standing at the oars");
    }
    let passenger_as_seen = rower.raft_and_player_by_tick(passenger.id);
    let recent: Vec<_> = passenger_as_seen.iter().rev().take(8).collect();
    assert!(!recent.is_empty(), "the rower was never shown the passenger and the raft together");
    for (raft, state) in recent {
        let standing = raft.world_of(place);
        let off = (state.x - standing[0]).hypot(state.z - standing[2]);
        assert!(off < 0.02, "the passenger was drawn {off} blocks from where they stand on the deck");
        assert!((state.y - f64::from(raft.deck_top())).abs() < 0.02, "the passenger is not standing on the deck");
    }
    let (_, last) = rower.latest_raft().expect("raft");
    let last = body(&last).expect("a raft");
    assert!(last.x > seen.x, "the raft stopped when a passenger got on");

    server.stop().await;
}

#[tokio::test]
async fn only_a_hand_that_can_reach_the_sheets_braces_the_yard() {
    // The wiring the unit tests cannot see: `ClientMessage::Trim` arriving
    // from three different players standing in three different places, and
    // the angle coming back out in everybody's entity snapshot.
    //
    // The one that matters is the refusal. A client that could name any raft
    // by its id and set its sail could brace the sail of a raft it is not on
    // -- from the bank, from across the lake -- and sail somebody else's
    // raft out from under them.
    let server = primitive_server::start(test_settings(), RunOptions::embedded()).await.expect("start");
    let address = server.address().to_string();
    let mut rower = Client::connect(&address, "rower").await;
    rower
        .wait_for(|m| matches!(m, ServerMessage::InventoryState { .. }).then_some(()))
        .await
        .expect("the server never sent an opening inventory");

    let (sx, sy, sz) = rower.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    for x in bx + 2..=bx + 16 {
        for z in bz - 5..=bz + 5 {
            server.place_block(x, by - 1, z, BLOCK_WATER);
            for y in by..=by + 3 {
                server.place_block(x, y, z, BLOCK_AIR);
            }
        }
    }
    assert_eq!(server.give(BLOCK_RAFT, 1), 0, "the raft did not fit in the pack");
    let slot = rower
        .wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_RAFT) > 0 => {
                (0..primitive_shared::inventory::SLOTS).find(|&s| inventory.block_in(s) == Some(BLOCK_RAFT))
            }
            _ => None,
        })
        .await
        .expect("the server never said the raft was in the pack");
    rower.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;
    rower.stand_at(sx, sy, sz).await;
    rower.listen(150).await;
    rower.send(ClientMessage::UseBlock { global_x: bx + 3, global_y: by - 1, global_z: bz }).await;
    let (id, afloat) = rower
        .wait_for(|m| match m {
            ServerMessage::Entities { states, .. } => states.iter().find_map(|s| body(s).map(|b| (s.id, b))),
            _ => None,
        })
        .await
        .expect("a raft right-clicked onto water never appeared");

    /// The yard's angle as the latest snapshot this client was sent has it.
    fn yard(client: &Client) -> f32 {
        client.latest_raft().and_then(|(_, state)| body(&state)).expect("a raft").sail_angle
    }

    // At the oars, and the sail up: the second use of a raft from its own
    // oars is what raises it.
    let stern = afloat.world_of([-1.0, 0.0, 0.0]);
    rower.stand_at((stern[0]) as f32, (stern[1]) as f32, (stern[2]) as f32).await;
    rower.listen(100).await;
    rower.on_deck(id, [-1.0, 0.0, 0.0]).await;
    rower.listen(100).await;
    rower.send(ClientMessage::UseRaft { raft: id }).await;
    rower
        .wait_for(|m| matches!(m, ServerMessage::Oars { raft: Some(raft) } if *raft == id).then_some(()))
        .await
        .expect("using a raft from its deck did not give the oars");
    rower.send(ClientMessage::UseRaft { raft: id }).await;
    rower.listen(200).await;
    assert_eq!(yard(&rower), 0.0, "a raft was launched with its yard already braced round");

    // **The rower braces it, and the angle comes back to them.**
    rower.send(ClientMessage::Trim { raft: id, angle: 0.6 }).await;
    rower.listen(250).await;
    assert!((yard(&rower) - 0.6).abs() < 1e-4, "the rower's trim did not take: the yard is at {}", yard(&rower));

    // **...and never through its own mast**, whatever a client claims.
    rower.send(ClientMessage::Trim { raft: id, angle: 40.0 }).await;
    rower.listen(250).await;
    assert!(
        (yard(&rower) - raft::SAIL_MAX_ANGLE).abs() < 1e-4,
        "an angle of forty radians off a socket braced the yard to {}",
        yard(&rower)
    );

    // **A player on the bank cannot touch it.** They know the raft's id --
    // it is in every snapshot they are sent -- and that is all a dishonest
    // client would need.
    let mut ashore = Client::connect(&address, "ashore").await;
    ashore.listen(400).await;
    let seen = ashore.latest_raft().map(|(_, state)| state.id).expect("the bystander was never shown the raft");
    assert_eq!(seen, id, "the bystander was shown a different raft");
    ashore.stand_at(sx, sy, sz).await;
    ashore.listen(100).await;
    ashore.send(ClientMessage::Trim { raft: id, angle: -1.0 }).await;
    ashore.listen(300).await;
    rower.listen(250).await;
    assert!(
        (yard(&rower) - raft::SAIL_MAX_ANGLE).abs() < 1e-4,
        "somebody standing on dry land braced the sail of a raft they had never been on: {}",
        yard(&rower)
    );

    // **A passenger standing at the mast can.** That is what a second person
    // aboard is for: one at the oars, one on the sheets.
    let (_, drawn) = rower.latest_raft().expect("raft");
    let drawn = body(&drawn).expect("a raft");
    let place = [raft::MAST_ALONG, 0.0, 0.3];
    let at_the_mast = drawn.world_of(place);
    ashore.stand_at((at_the_mast[0]) as f32, (at_the_mast[1]) as f32, (at_the_mast[2]) as f32).await;
    ashore.listen(150).await;
    ashore.on_deck(id, place).await;
    ashore.listen(150).await;
    ashore.send(ClientMessage::Trim { raft: id, angle: -0.4 }).await;
    ashore.listen(300).await;
    // The rower has to be listening for their own snapshots to be worth
    // reading: a client that was not reading its socket while somebody else
    // moved the world still holds the world as it was, and an assertion
    // against that would pass whatever happened.
    rower.listen(250).await;
    assert!(
        (yard(&rower) + 0.4).abs() < 1e-4,
        "a passenger standing at the mast could not brace the yard: it is at {}",
        yard(&rower)
    );

    server.stop().await;
}
