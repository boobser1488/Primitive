//! The pit kiln, end to end over a real socket, built the way a player
//! builds one: «Выкапывается 1 блок земли в глубину. Кладутся в низ
//! предметы для обжога и кладётся 8 сена а сверху 8 брёвен. После
//! поджигается и горит 1 час реального времени.»
//!
//! The rules are unit-tested in `primitive_shared::pit` and
//! `primitive_server::logic::pits`. What only this can see is the wiring:
//! a right click that reaches the server as a use and not as a placement,
//! pottery that leaves the pack when it goes into the pit, a refusal that
//! reaches the player in words, a kiln that is lit on the server and not
//! only in a map, and pots that come back out of the ground fired.
//!
//! **An hour is not a test.** The burn is run on by `Server::advance_pits`,
//! the tick's own step with the hour handed in, so what is checked at the
//! end is what a player would find at the end of the real one.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::pit::{Stage, PIT_KILN_SECONDS};
use primitive_shared::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
use primitive_shared::types::{
    BlockId, BLOCK_AIR, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_FLINT, BLOCK_FIBER, BLOCK_LOG,
    BLOCK_VESSEL, BLOCK_VESSEL_RAW,
};
use tokio::net::TcpStream;

fn test_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "test".to_string(),
        // Nowhere to save to: nothing a test does may land on disk beside
        // somebody's real world.
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        stats_interval_secs: 0.0,
        world_preset: primitive_shared::worldgen::Preset::Test,
        anticheat: primitive_server::settings::AntiCheatSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    }
}

struct Client {
    socket: TcpStream,
    spawn: (f32, f32, f32),
}

impl Client {
    async fn connect(address: &str) -> Self {
        let mut socket = TcpStream::connect(address).await.expect("connect");
        write_message(
            &mut socket,
            &ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                username: "potter".to_string(),
            },
        )
        .await
        .expect("hello");
        let spawn = match read_message::<_, ServerMessage>(&mut socket).await.expect("welcome") {
            ServerMessage::Welcome { spawn, .. } => spawn,
            other => panic!("expected Welcome, got {other:?}"),
        };
        Self { socket, spawn: primitive_shared::geometry::narrow(spawn) }
    }

    async fn send(&mut self, message: ClientMessage) {
        write_message(&mut self.socket, &message).await.expect("send");
    }

    /// Reads until a message the predicate likes shows up. Everything else
    /// -- chunks, snapshots, keepalives -- shares the socket.
    async fn wait_for<T>(&mut self, seconds: u64, mut want: impl FnMut(&ServerMessage) -> Option<T>) -> Option<T> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let message = match tokio::time::timeout(remaining, read_message::<_, ServerMessage>(&mut self.socket)).await {
                Ok(Ok(message)) => message,
                _ => return None,
            };
            if let Some(found) = want(&message) {
                return Some(found);
            }
        }
    }

    /// Holds whatever is in `slot`, and uses the cell at `at` with it.
    async fn use_with(&mut self, slot: usize, at: (i32, i32, i32)) {
        self.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;
        self.send(ClientMessage::UseBlock {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
        })
        .await;
    }
}

/// Waits until the server's own world holds what `want` accepts at `at`.
///
/// Polled rather than heard: a block change goes out on the chunk
/// subscriptions, which a test client that has asked for no chunks is not
/// on -- see `a_loaded_rack_looks_loaded_from_across_the_camp`.
async fn world_reaches(
    server: &primitive_server::Server,
    at: (i32, i32, i32),
    want: impl Fn(Option<BlockId>) -> bool,
) -> Option<BlockId> {
    for _ in 0..500 {
        let here = server.block_at(at.0, at.1, at.2);
        if want(here) {
            return here;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

#[tokio::test]
async fn a_pit_kiln_is_dug_filled_lit_and_gives_back_fired_pottery_over_the_wire() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded()).await.expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client
        .wait_for(10, |m| matches!(m, ServerMessage::InventoryState { .. }).then_some(()))
        .await
        .expect("the server never sent an opening inventory");

    // «Выкапывается 1 блок земли в глубину»: a hole two paces from where
    // the player stands, earth under it and on all four sides, and a roof
    // two cells over it so the test world's weather cannot put it out.
    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    let pit = (bx + 2, by - 1, bz);
    let floor = (pit.0, pit.1 - 1, pit.2);
    server.place_block(floor.0, floor.1, floor.2, BLOCK_DIRT);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        server.place_block(pit.0 + dx, pit.1, pit.2 + dz, BLOCK_DIRT);
    }
    server.place_block(pit.0, pit.1, pit.2, BLOCK_AIR);
    server.place_block(pit.0, pit.1 + 1, pit.2, BLOCK_AIR);
    server.place_block(pit.0, pit.1 + 2, pit.2, BLOCK_COBBLESTONE);

    for (block, count) in [(BLOCK_VESSEL_RAW, 1), (BLOCK_FIBER, 8), (BLOCK_LOG, 8), (BLOCK_FLINT, 1)] {
        assert_eq!(server.give(block, count), 0, "the pack had no room for block {block}");
    }
    let pack = client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_FLINT) > 0 => Some(inventory.clone()),
            _ => None,
        })
        .await
        .expect("the gifts never reached the pack");
    let slot_of = |block: BlockId| {
        (0..primitive_shared::inventory::SLOTS)
            .find(|&slot| pack.block_in(slot) == Some(block))
            .expect("given but in no slot")
    };
    let (pottery, fibre, logs, flint) = (slot_of(BLOCK_VESSEL_RAW), slot_of(BLOCK_FIBER), slot_of(BLOCK_LOG), slot_of(BLOCK_FLINT));

    // «Кладутся в низ предметы для обжога»: the pot, at the pit's floor.
    client.use_with(pottery, floor).await;
    world_reaches(&server, pit, |b| b.and_then(Stage::of) == Some(Stage::Pottery { pieces: 1, fired: false }))
        .await
        .expect("the pot never went into the pit");

    // «и кладётся 8 сена»
    for _ in 0..8 {
        client.use_with(fibre, pit).await;
    }
    world_reaches(&server, pit, |b| b.and_then(Stage::of) == Some(Stage::Fibre(8)))
        .await
        .expect("eight fibre never lay in the pit");

    // «а сверху 8 брёвен» -- seven first, and a strike that is refused in
    // words that say how many.
    for _ in 0..7 {
        client.use_with(logs, pit).await;
    }
    world_reaches(&server, pit, |b| b.and_then(Stage::of) == Some(Stage::Logs(7)))
        .await
        .expect("seven logs never lay on the fibre");
    client.use_with(flint, pit).await;
    let refusal = client
        .wait_for(10, |m| match m {
            ServerMessage::Error(text) if text.contains("logs") => Some(text.clone()),
            _ => None,
        })
        .await
        .expect("a kiln struck with seven logs said nothing");
    assert!(refusal.contains('7'), "the refusal did not say how many logs there were: {refusal}");
    assert_eq!(server.block_at(pit.0, pit.1, pit.2).and_then(Stage::of), Some(Stage::Logs(7)), "seven logs caught");

    client.use_with(logs, pit).await;
    world_reaches(&server, pit, |b| b.and_then(Stage::of) == Some(Stage::Logs(8)))
        .await
        .expect("the eighth log never went on");

    // «После поджигается»
    client.use_with(flint, pit).await;
    world_reaches(&server, pit, |b| b.and_then(Stage::of) == Some(Stage::Burning))
        .await
        .expect("the full kiln never caught");

    // «и горит 1 час реального времени» -- and not a minute less.
    server.advance_pits(PIT_KILN_SECONDS - 60.0);
    assert_eq!(
        server.block_at(pit.0, pit.1, pit.2).and_then(Stage::of),
        Some(Stage::Burning),
        "the kiln finished a minute early"
    );
    server.advance_pits(61.0);
    world_reaches(&server, pit, |b| b.and_then(Stage::of) == Some(Stage::Pottery { pieces: 1, fired: true }))
        .await
        .expect("the hour went by and nothing was fired");

    // The fired vessel, out of the pit by hand. The pottery slot is empty
    // now, which is what an empty hand is.
    client.use_with(pottery, pit).await;
    client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_VESSEL) == 1 => Some(()),
            _ => None,
        })
        .await
        .expect("the fired vessel never came back into the pack");
    world_reaches(&server, pit, |b| b == Some(BLOCK_AIR))
        .await
        .expect("the emptied pit is not a hole again");
    server.stop().await;
}
