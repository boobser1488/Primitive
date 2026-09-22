//! Sand falling, as a client actually sees it.
//!
//! The unit tests in `logic::falling` cover the rules: a column comes
//! down, nothing is created or destroyed, a block that cannot land
//! waits rather than being deleted. None of them can see the thing a
//! player complained about, because the complaint was not about the
//! rules at all -- "sand falls instantly, or after a pause with no
//! animation, or vanishes altogether" is a description of what arrived
//! over the socket.
//!
//! So these run a real server, connect a real client, and read what the
//! server says while a block is on its way down. Two properties, and
//! the bug broke the second one for every falling block in the game:
//!
//! * The block is reported in the air, moving, for several ticks --
//!   which is the animation. A fall with no snapshots between leaving
//!   and landing is a teleport whatever the server believed.
//! * **No two entities in one snapshot share an id.** The client keys
//!   its entity table on the id alone, so two that collide are one row,
//!   and the falling block -- appended first -- was always the one
//!   overwritten. See `protocol::EntitySource`.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{
    ClientMessage, EntityKind, EntityState, ServerMessage, PROTOCOL_VERSION,
};
use primitive_shared::types::{BLOCK_AIR, BLOCK_SAND, BLOCK_STONE};
use tokio::net::TcpStream;

fn test_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "falling".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        stats_interval_secs: 0.0,
        ..Default::default()
    }
}

/// Connects, and returns the socket plus where the server put the
/// player -- the falling block has to be built near that, or interest
/// filtering will quite correctly say nothing about it.
async fn connect(address: &str) -> (TcpStream, (f32, f32, f32)) {
    let mut socket = TcpStream::connect(address).await.expect("connect");
    write_message(
        &mut socket,
        &ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            username: "watcher".to_string(),
        },
    )
    .await
    .expect("hello");
    let spawn = match read_message::<_, ServerMessage>(&mut socket)
        .await
        .expect("welcome")
    {
        ServerMessage::Welcome { spawn, .. } => spawn,
        other => panic!("expected Welcome, got {other:?}"),
    };
    (socket, primitive_shared::geometry::narrow(spawn))
}

/// Every entity snapshot that arrives in the next `seconds`.
///
/// Whole snapshots rather than only the falling blocks in them: the id
/// property is about what shares one message, so the other entities are
/// the evidence and not noise.
async fn entity_snapshots(socket: &mut TcpStream, seconds: f32) -> Vec<Vec<EntityState>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs_f32(seconds);
    let mut out = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return out;
        }
        match tokio::time::timeout(remaining, read_message::<_, ServerMessage>(socket)).await {
            Ok(Ok(ServerMessage::Entities { states, .. })) => out.push(states),
            Ok(Ok(_)) => {}
            _ => return out,
        }
    }
}

/// A floor, clear air above it, and one block of sand five cells up.
///
/// Five because that is the number in the report: one block of drop
/// lands inside a tick or two and looks the same either way, and it is
/// the longer falls that had nothing to show.
fn build_the_drop(server: &primitive_server::Server, at: (i32, i32, i32)) {
    let (x, y, z) = at;
    server.place_block(x, y, z, BLOCK_STONE);
    for above in 1..=8 {
        server.place_block(x, y + above, z, BLOCK_AIR);
    }
    server.place_block(x, y + 6, z, BLOCK_SAND);
}

#[tokio::test]
async fn sand_falling_five_blocks_is_reported_in_the_air_the_whole_way_down() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut socket, spawn) = connect(&address).await;

    let at = (spawn.0 as i32 + 2, spawn.1 as i32, spawn.2 as i32 + 2);
    build_the_drop(&server, at);

    let snapshots = entity_snapshots(&mut socket, 3.0).await;
    let heights: Vec<f32> = snapshots
        .iter()
        .flatten()
        .filter(|state| matches!(state.kind, EntityKind::FallingBlock { .. }))
        .map(|state| state.y as f32)
        .collect();

    assert!(
        heights.len() >= 3,
        "the block was reported in the air {} time(s); a fall nobody is told about \
         is a teleport, however carefully the server integrated it",
        heights.len()
    );
    let highest = heights.iter().cloned().fold(f32::MIN, f32::max);
    let lowest = heights.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        highest - lowest > 1.0,
        "it was only ever seen between y={lowest} and y={highest}, which is not a fall"
    );

    server.request_shutdown();
}

#[tokio::test]
async fn nothing_in_a_snapshot_shares_an_id_with_anything_else_in_it() {
    // The reproduction. Three simulations feed one snapshot -- falling
    // blocks, dropped stacks and animals -- and each of them used to
    // count its own entities from one, so the first sand to fall in a
    // world was number one and so was the first deer. The client keeps
    // one row per id: the deer, appended last, won every tick, and the
    // sand was drawn nowhere at all on its way down.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut socket, spawn) = connect(&address).await;

    let at = (spawn.0 as i32 + 2, spawn.1 as i32, spawn.2 as i32 + 2);
    build_the_drop(&server, at);
    // Reeds on top of the sand, which is a thing that grows on sand and
    // nothing else needed staging for. The moment the sand leaves the
    // grid the reeds have nothing to stand on, the tick loop breaks
    // them (see `collapse_unsupported`) and the stack lands in the
    // world as a dropped item -- so a falling block and a dropped stack
    // are in the air together, which is all the collision ever needed.
    server.place_block(
        at.0,
        at.1 + 7,
        at.2,
        primitive_shared::types::BLOCK_REEDS,
    );

    let snapshots = entity_snapshots(&mut socket, 3.0).await;
    assert!(!snapshots.is_empty(), "no entity snapshot ever arrived");
    let mut saw_both = false;
    for states in &snapshots {
        let mut seen = std::collections::HashMap::new();
        for state in states {
            if let Some(other) = seen.insert(state.id, state.kind) {
                panic!(
                    "id {} is both {:?} and {:?} in one snapshot",
                    state.id, other, state.kind
                );
            }
        }
        saw_both |= states
            .iter()
            .any(|s| matches!(s.kind, EntityKind::FallingBlock { .. }))
            && states.iter().any(|s| matches!(s.kind, EntityKind::Item { .. }));
    }
    assert!(
        saw_both,
        "no snapshot ever carried a falling block and a dropped stack at once, \
         so nothing here could have collided and the test proved nothing"
    );

    server.request_shutdown();
}
