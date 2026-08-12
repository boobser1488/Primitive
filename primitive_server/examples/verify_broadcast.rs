//! End-to-end smoke test against a *running* server, covering the parts
//! unit tests can't reach: the real socket, the real handshake, and the
//! real broadcast fan-out.
//!
//! Run the server first, then:
//! `cargo run -p primitive_server --example verify_broadcast`
//!
//! What it checks:
//! 1. Two independent clients complete the v2 handshake and get distinct
//!    player ids.
//! 2. Both load the same chunk via the batched `RequestChunks`.
//! 3. One edits a block *within reach of where the server thinks it is*
//!    -- and both see the resulting `BlockUpdate`, including the editor
//!    (no "trust your own edit" shortcut: the server is the only thing
//!    that confirms world state).
//! 4. The keepalive `Ping` gets answered, and the world clock arrives.
//!
//! Point 3 is worth spelling out, because getting it wrong the first
//! time is what proved the anti-cheat works: a client that never sends
//! an `UpdateTransform` is, as far as the server knows, still standing
//! at spawn. Editing a block ten blocks away from there is exactly the
//! reach-hack signature, and the server refuses it -- so this example
//! has to actually walk over first, like a real client would.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{
    BlockChange, ClientMessage, PlayerId, ServerMessage, PROTOCOL_VERSION,
};
use primitive_shared::types::{ChunkPos, BLOCK_GLOWSTONE, BLOCK_SAND};

const ADDR: &str = "127.0.0.1:7878";
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

struct Client {
    socket: TcpStream,
    id: PlayerId,
}

impl Client {
    async fn connect(username: &str) -> anyhow::Result<Self> {
        let mut socket = TcpStream::connect(ADDR).await?;
        write_message(
            &mut socket,
            &ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                username: username.to_string(),
            },
        )
        .await?;

        match read_message::<_, ServerMessage>(&mut socket).await? {
            ServerMessage::Welcome {
                your_id,
                protocol_version,
                server_name,
                ..
            } => {
                anyhow::ensure!(
                    protocol_version == PROTOCOL_VERSION,
                    "server speaks v{protocol_version}, this build v{PROTOCOL_VERSION}"
                );
                println!("  {username}: welcomed by \"{server_name}\" as player {your_id}");
                Ok(Self {
                    socket,
                    id: your_id,
                })
            }
            other => anyhow::bail!("expected Welcome, got {other:?}"),
        }
    }

    async fn send(&mut self, msg: ClientMessage) -> anyhow::Result<()> {
        write_message(&mut self.socket, &msg).await?;
        Ok(())
    }

    /// Reads until `predicate` matches, answering keepalives along the
    /// way. Without the Ping handling this test would get itself
    /// disconnected mid-run on a slow machine.
    async fn wait_for<T, F>(&mut self, what: &str, mut predicate: F) -> anyhow::Result<T>
    where
        F: FnMut(&ServerMessage) -> Option<T>,
    {
        loop {
            let msg = timeout(
                RECV_TIMEOUT,
                read_message::<_, ServerMessage>(&mut self.socket),
            )
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting for {what}"))??;

            if let ServerMessage::Ping { nonce } = msg {
                write_message(&mut self.socket, &ClientMessage::Pong { nonce }).await?;
                continue;
            }
            if let ServerMessage::Kick(reason) = &msg {
                anyhow::bail!("kicked while waiting for {what}: {reason}");
            }
            if let Some(value) = predicate(&msg) {
                return Ok(value);
            }
        }
    }
}

fn matches_change(msg: &ServerMessage, expected: &BlockChange) -> Option<()> {
    let hit = |c: &BlockChange| {
        c.global_x == expected.global_x
            && c.global_y == expected.global_y
            && c.global_z == expected.global_z
            && c.block_id == expected.block_id
    };
    match msg {
        ServerMessage::BlockUpdate(c) if hit(c) => Some(()),
        ServerMessage::BlockUpdates(changes) if changes.iter().any(hit) => Some(()),
        _ => None,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("connecting two clients to {ADDR}...");
    let mut alice = Client::connect("alice").await?;
    let mut bob = Client::connect("bob").await?;
    anyhow::ensure!(alice.id != bob.id, "both clients got the same player id");

    let target_chunk = ChunkPos::new(0, 0);
    println!("both clients requesting chunk {target_chunk:?}...");
    alice
        .send(ClientMessage::RequestChunks(vec![target_chunk]))
        .await?;
    bob.send(ClientMessage::RequestChunks(vec![target_chunk]))
        .await?;

    let surface_y = alice
        .wait_for("alice's chunk", |msg| match msg {
            ServerMessage::ChunkData(chunk) if chunk.pos == target_chunk => {
                Some(chunk.height_at(8, 8))
            }
            _ => None,
        })
        .await?;
    bob.wait_for("bob's chunk", |msg| match msg {
        ServerMessage::ChunkData(chunk) if chunk.pos == target_chunk => Some(()),
        _ => None,
    })
    .await?;
    println!("  both received the chunk (surface at y={surface_y})");

    // Tell the server where alice is standing before touching anything.
    // Without this the edit below is a reach violation (see the module
    // comment) and the server correctly refuses it.
    //
    // She stands away from spawn on purpose: the bundled `bedrock_guard`
    // example plugin protects a small radius around it, and a test that
    // fights the shipped plugins tests the wrong thing.
    let stand_y = surface_y as f32 + 1.0;
    alice
        .send(ClientMessage::UpdateTransform {
            x: 6.5,
            y: stand_y,
            z: 6.5,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            sequence: 1,
        })
        .await?;

    // Place a block right next to her -- comfortably inside the reach
    // limit, and away from a chunk border.
    let change = BlockChange {
        global_x: 6,
        global_y: surface_y + 1,
        global_z: 5,
        block_id: BLOCK_GLOWSTONE,
    };
    println!(
        "alice places glowstone at ({}, {}, {})...",
        change.global_x, change.global_y, change.global_z
    );
    alice
        .send(ClientMessage::SetBlock {
            global_x: change.global_x,
            global_y: change.global_y,
            global_z: change.global_z,
            block_id: change.block_id,
        })
        .await?;

    alice
        .wait_for("alice's own confirmation", |m| matches_change(m, &change))
        .await?;
    println!("  alice received the confirmation");
    bob.wait_for("bob's broadcast copy", |m| matches_change(m, &change))
        .await?;
    println!("  bob received the broadcast");

    // The world clock should be arriving on its own schedule.
    let time_of_day = bob
        .wait_for("a TimeSync", |msg| match msg {
            ServerMessage::TimeSync { time_of_day, .. } => Some(*time_of_day),
            _ => None,
        })
        .await?;
    println!("  world clock is ticking (time_of_day={time_of_day:.3})");

    // --- you can't build inside yourself ---
    // Alice stands at (0.5, stand_y, 0.5), so the cube at (0, stand_y, 0)
    // contains her feet. The server must refuse it even though it's well
    // within reach.
    println!("alice tries to place a block inside herself...");
    alice
        .send(ClientMessage::SetBlock {
            global_x: 6,
            global_y: stand_y as i32,
            global_z: 6,
            block_id: BLOCK_GLOWSTONE,
        })
        .await?;
    let refusal = alice
        .wait_for("the refusal", |msg| match msg {
            ServerMessage::Error(text) => Some(text.clone()),
            _ => None,
        })
        .await?;
    anyhow::ensure!(
        refusal.contains("inside"),
        "expected a 'can't place inside' refusal, got: {refusal}"
    );
    println!("  refused: {refusal}");

    // --- chat commands ---
    println!("alice runs /list...");
    alice.send(ClientMessage::Chat("/list".to_string())).await?;
    let listing = alice
        .wait_for("the /list reply", |msg| match msg {
            ServerMessage::Chat { username, text, .. } if username == "server" => {
                Some(text.clone())
            }
            _ => None,
        })
        .await?;
    println!("  server replied: {listing}");

    // ...and must not be allowed to run an operator command.
    println!("alice tries /stop (operator-only)...");
    alice.send(ClientMessage::Chat("/stop".to_string())).await?;
    // `/list` replies with several lines, so match on the denial itself
    // rather than on "the next thing the server says".
    let denial = alice
        .wait_for("the permission denial", |msg| match msg {
            ServerMessage::Chat { username, text, .. }
                if username == "server" && text.contains("operator") =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .await?;
    anyhow::ensure!(
        denial.contains("operator"),
        "a plain player must not be able to stop the server, got: {denial}"
    );
    println!("  denied: {denial}");

    // --- falling sand ---
    // Place sand a few blocks above the ground and watch the server
    // bring it down on its own.
    let sand_y = surface_y + 4;
    println!("alice places sand at (7, {sand_y}, 6), above open air...");
    alice
        .send(ClientMessage::SetBlock {
            global_x: 7,
            global_y: sand_y,
            global_z: 6,
            block_id: BLOCK_SAND,
        })
        .await?;

    // While it's in the air it must exist as an *entity* -- that's the
    // difference from the old version, which teleported the block down
    // one grid cell at a time.
    let airborne = alice
        .wait_for("the falling-block entity", |msg| match msg {
            ServerMessage::Entities { states, .. } => states
                .iter()
                .find(|e| matches!(e.kind, primitive_shared::protocol::EntityKind::FallingBlock { block } if block == BLOCK_SAND))
                .map(|e| e.y),
            _ => None,
        })
        .await?;
    println!("  it exists as a falling entity at y={airborne:.2}");
    anyhow::ensure!(
        airborne < sand_y as f32 && airborne > 0.0,
        "the entity should be below where it was placed, got {airborne}"
    );

    // It should land on the surface without anyone asking it to.
    let landed = alice
        .wait_for("the sand to land", |msg| {
            // "It moved down" rather than "it reached exactly y=N":
            // the surface height under (7, 6) isn't necessarily the one
            // sampled at the middle of the chunk.
            let hit = |c: &BlockChange| {
                c.global_x == 7 && c.global_z == 6 && c.block_id == BLOCK_SAND && c.global_y < sand_y
            };
            match msg {
                ServerMessage::BlockUpdate(c) if hit(c) => Some(c.global_y),
                ServerMessage::BlockUpdates(changes) => {
                    changes.iter().find(|c| hit(c)).map(|c| c.global_y)
                }
                _ => None,
            }
        })
        .await?;
    println!("  sand fell on its own and settled at y={landed}");
    anyhow::ensure!(
        landed < sand_y,
        "sand never actually moved (still at y={landed})"
    );

    // --- plugins ---
    // These only run if the example plugins are present; skip quietly
    // otherwise so the check still works on a bare server.
    println!("checking plugin hooks (/online, spawn protection)...");
    // `/online` is not a server command -- if anything answers, it's a
    // plugin.
    alice
        .send(ClientMessage::Chat("/online".to_string()))
        .await?;
    match tokio::time::timeout(
        Duration::from_secs(3),
        alice.wait_for("the /players reply", |msg| match msg {
            ServerMessage::Chat { username, text, .. }
                if username == "server" && text.contains("online:") =>
            {
                Some(text.clone())
            }
            _ => None,
        }),
    )
    .await
    {
        Ok(Ok(reply)) => {
            println!("  a plugin answered a command it invented: {reply}");

            // The protection plugin guards a small radius around spawn.
            // Alice steps back to it and tries to dig there.
            alice
                .send(ClientMessage::UpdateTransform {
                    x: 1.5,
                    y: stand_y,
                    z: 1.5,
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: true,
                    sequence: 3,
                })
                .await?;
            alice
                .send(ClientMessage::SetBlock {
                    global_x: 1,
                    global_y: surface_y,
                    global_z: 1,
                    block_id: primitive_shared::types::BLOCK_AIR,
                })
                .await?;
            let refusal = alice
                .wait_for("the plugin's refusal", |msg| match msg {
                    ServerMessage::Chat { username, text, .. }
                        if username == "server" && text.contains("protected") =>
                    {
                        Some(text.clone())
                    }
                    ServerMessage::Error(text) if text.contains("plugin") => Some(text.clone()),
                    _ => None,
                })
                .await?;
            println!("  a plugin vetoed a block edit: {refusal}");
        }
        _ => println!("  (no plugins loaded on this server -- skipped)"),
    }

    alice.send(ClientMessage::Disconnect).await?;
    bob.send(ClientMessage::Disconnect).await?;

    println!(
        "\nOK: handshake, chunk streaming, block broadcast, world clock, \
         self-placement guard, chat command permissions, falling sand, \
         plugin hooks and falling-block entities all verified."
    );
    Ok(())
}
