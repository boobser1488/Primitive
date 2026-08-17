//! End-to-end smoke test against a *running* server, covering the parts
//! unit tests can't reach: the real socket, the real handshake, and the
//! real broadcast fan-out.
//!
//! Run the server first, then:
//! `cargo run -p primitive_server --example verify_broadcast`
//!
//! What it checks:
//! 1. Two independent clients complete the handshake and get distinct
//!    player ids.
//! 2. Both load the same chunk via the batched `RequestChunks`.
//! 3. One *walks* to a block, breaks it, and both see the resulting
//!    `BlockUpdate` -- including the breaker (no "trust your own edit"
//!    shortcut: the server is the only thing that confirms world state).
//! 4. What that break dropped is picked up off the ground and turns up
//!    in a `ServerMessage::InventoryState`.
//! 5. It goes back down: first inside herself, which is refused, then as
//!    a single *layer*, which is then thickened for nothing. That last
//!    pair is the whole layer economy end to end -- one item starts a
//!    cell, the rest of it is free, and the depth survives the wire.
//! 6. Chat commands and their permissions, the world clock, and plugin
//!    hooks if any plugins are loaded.
//!
//! Two things here are done the long way round, and both of them are the
//! anti-cheat working:
//!
//! * **She has to walk.** A client that never sends an `UpdateTransform`
//!   is, as far as the server knows, still standing at spawn, and
//!   editing a block ten blocks from there is the reach-hack signature.
//!   Arriving in one message is the *other* signature -- a teleport --
//!   so `walk_to` steps like a player instead.
//! * **She has to earn the block.** The inventory is server-side: a
//!   fresh player carries nothing, and every placement is refused with
//!   "you are not carrying that" until something has been dug up and
//!   walked over. An earlier version of this example placed glowstone
//!   out of thin air, which stopped meaning anything the day inventories
//!   became real -- it had been failing at the first placement and
//!   saying so only by hanging.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{
    BlockChange, ClientMessage, PlayerId, ServerMessage, PROTOCOL_VERSION,
};
use primitive_shared::types::ChunkPos;

/// Where to look for the server.
///
/// Overridable, because the default port is a *default*: a machine can
/// easily have something else on it -- another copy of this game, most
/// likely -- and the failure that produces is a handshake that dies
/// with "unexpected end of file", which says nothing about the actual
/// problem.
///
/// ```text
/// cargo run -p primitive_server --example verify_broadcast -- 127.0.0.1:7879
/// ```
fn server_address() -> String {
    std::env::args()
        .nth(1)
        .or_else(|| std::env::var("PRIMITIVE_SERVER").ok())
        .unwrap_or_else(|| "127.0.0.1:7878".to_string())
}
const RECV_TIMEOUT: Duration = Duration::from_secs(15);

struct Client {
    socket: TcpStream,
    id: PlayerId,
    /// The last inventory the server sent.
    ///
    /// Kept rather than waited for, because it arrives when the *server*
    /// decides: a drop is picked up on the tick loop, and the message
    /// saying so can easily land while this example is waiting for
    /// something else -- at which point waiting for another one waits
    /// for ever. That is precisely how this test used to hang.
    pack: primitive_shared::inventory::Inventory,
    /// Where the server put this player. The first `UpdateTransform` is
    /// measured against it, so a walk has to start from here.
    at: (f32, f32, f32),
}

impl Client {
    async fn connect(username: &str) -> anyhow::Result<Self> {
        let mut socket = TcpStream::connect(server_address()).await?;
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
                spawn,
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
                    pack: primitive_shared::inventory::Inventory::new(),
                    at: spawn,
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
        // Bounded on the *whole wait*, not per message. A running server
        // sends snapshots twenty times a second, so a per-message
        // timeout never fires however wrong things have gone -- which is
        // how a failure here used to present as the example simply
        // stopping, with nothing said and nothing to read.
        let deadline = tokio::time::Instant::now() + RECV_TIMEOUT;
        loop {
            let msg = timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                read_message::<_, ServerMessage>(&mut self.socket),
            )
            .await
            .map_err(|_| anyhow::anyhow!("gave up waiting for {what}"))??;

            if let ServerMessage::Ping { nonce } = msg {
                write_message(&mut self.socket, &ClientMessage::Pong { nonce }).await?;
                continue;
            }
            if let ServerMessage::Kick(reason) = &msg {
                anyhow::bail!("kicked while waiting for {what}: {reason}");
            }
            // Say so out loud rather than waiting for a timeout to
            // explain it. Every hang this example has ever had was the
            // server refusing something and nobody listening: the
            // refusal arrives immediately, the timeout arrives ten
            // seconds later, and only one of them says why.
            if let ServerMessage::Error(text) = &msg {
                println!("    [server refused something: {text}]");
            }
            if let ServerMessage::InventoryState { inventory } = &msg {
                self.pack = inventory.clone();
            }
            if let Some(value) = predicate(&msg) {
                return Ok(value);
            }
        }
    }
}

impl Client {
    /// Waits until the pack holds at least `count` of `kind`, counting
    /// whatever has already arrived.
    async fn pack_holding(&mut self, kind: primitive_shared::types::BlockId, count: u32)
        -> anyhow::Result<u32>
    {
        if self.pack.count(kind) >= count {
            return Ok(self.pack.count(kind));
        }
        let held_before = self.pack.count(kind);
        self.wait_for("the drops to reach the pack", |msg| match msg {
            ServerMessage::InventoryState { inventory } => {
                let held = inventory.count(kind);
                (held >= count).then_some(held)
            }
            _ => None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e} (wanted {count}, last saw {held_before})"))
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
    println!("connecting two clients to {}...", server_address());
    // A fresh pair of names every run, because a returning player is
    // restored *where they logged out* -- and this example only knows
    // the world spawn, which is where the walk below starts from. Run
    // twice under one name and the second run walks from somewhere the
    // server does not think it is, which the anti-cheat correctly reads
    // as a teleport and quietly undoes.
    let tag = std::process::id() % 10_000;
    let mut alice = Client::connect(&format!("alice{tag}")).await?;
    let mut bob = Client::connect(&format!("bob{tag}")).await?;
    anyhow::ensure!(alice.id != bob.id, "both clients got the same player id");

    // A few chunks rather than one, and searched in order until a spot
    // turns up. Terrain is terrain -- and this example *digs*, so a
    // server it has already run against has holes in it exactly where
    // it looked last time.
    let candidates: Vec<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    println!("both clients requesting {} chunks around the origin...", candidates.len());
    alice
        .send(ClientMessage::RequestChunks(candidates.clone()))
        .await?;
    bob.send(ClientMessage::RequestChunks(candidates.clone()))
        .await?;

    let mut spot = None;
    let mut chunk_of_spot = None;
    for wanted in &candidates {
        let chunk = alice
            .wait_for("a chunk", |msg| match msg {
                ServerMessage::ChunkData(chunk) if chunk.pos == *wanted => Some(chunk.clone()),
                _ => None,
            })
            .await?;
        bob.wait_for("bob's copy of it", |msg| match msg {
            ServerMessage::ChunkData(chunk) if chunk.pos == *wanted => Some(()),
            _ => None,
        })
        .await?;
        if spot.is_none() {
            if let Some(found) = find_a_spot(&chunk) {
                spot = Some(found);
                chunk_of_spot = Some(chunk);
            }
        }
    }
    let (Some((dig, spare, stand)), Some(chunk)) = (spot, chunk_of_spot) else {
        anyhow::bail!("no diggable pair of columns anywhere around the origin");
    };
    let ground = chunk.height_at(
        (stand.0 - chunk.pos.x * 16) as usize,
        (stand.1 - chunk.pos.z * 16) as usize,
    );
    println!(
        "  both received the chunk; digging ({}, {}, {}) from ({}, {})",
        dig.0, ground, dig.1, stand.0, stand.1
    );

    // --- walking over ---
    let feet = (
        stand.0 as f32 + 0.5,
        ground as f32 + 1.0,
        stand.1 as f32 + 0.5,
    );
    println!(
        "alice walks to ({:.1}, {:.1}, {:.1})...",
        feet.0, feet.1, feet.2
    );
    let mut sequence = 1u32;
    walk_to(&mut alice, feet, &mut sequence).await?;

    // --- breaking a block, and both clients hearing about it ---
    let surface = chunk.get(
        (dig.0 - chunk.pos.x * 16) as usize,
        ground as usize,
        (dig.1 - chunk.pos.z * 16) as usize,
    );
    let material = primitive_shared::types::block_drop(surface)
        .ok_or_else(|| anyhow::anyhow!("the block we picked yields nothing"))?;
    let kind = primitive_shared::types::block_kind(material);

    let broken = BlockChange {
        global_x: dig.0,
        global_y: ground,
        global_z: dig.1,
        block_id: primitive_shared::types::BLOCK_AIR,
    };
    println!(
        "alice breaks the {} at ({}, {}, {})...",
        primitive_shared::types::block_name(surface),
        dig.0,
        ground,
        dig.1
    );
    alice
        .send(ClientMessage::SetBlock {
            global_x: broken.global_x,
            global_y: broken.global_y,
            global_z: broken.global_z,
            block_id: broken.block_id,
        })
        .await?;
    alice
        .wait_for("alice's own confirmation", |m| matches_change(m, &broken))
        .await?;
    println!("  alice received the confirmation");
    bob.wait_for("bob's broadcast copy", |m| matches_change(m, &broken))
        .await?;
    println!("  bob received the broadcast");

    // A second one, from the column beside it: two of the same
    // material, one to spend on the first layer and one to prove the
    // second is free.
    let second = BlockChange {
        global_x: spare.0,
        global_z: spare.1,
        ..broken
    };
    alice
        .send(ClientMessage::SetBlock {
            global_x: second.global_x,
            global_y: second.global_y,
            global_z: second.global_z,
            block_id: second.block_id,
        })
        .await?;
    alice
        .wait_for("the second block to break", |m| matches_change(m, &second))
        .await?;

    // --- and picking up what they dropped ---
    //
    // She walks *onto* each hole rather than waiting beside it. Pickup
    // is measured to the player's body and runs on the tick loop
    // against the last position the server accepted, so standing on the
    // drop -- which is what a player does -- is both the realistic
    // gesture and the one that does not depend on a range check to the
    // decimal place. A drop also has to arm before it can be taken (see
    // `logic::items`), and the walk itself is long enough for that.
    println!("alice walks over both holes to pick the drops up...");
    walk_to(
        &mut alice,
        (dig.0 as f32 + 0.5, ground as f32, dig.1 as f32 + 0.5),
        &mut sequence,
    )
    .await?;
    walk_to(
        &mut alice,
        (spare.0 as f32 + 0.5, ground as f32, spare.1 as f32 + 0.5),
        &mut sequence,
    )
    .await?;
    // ...and stands there a moment. A drop cannot be taken until it has
    // been in the world half a second (see `items::PICKUP_ARM_DELAY`),
    // which is longer than it takes to step one cell -- so walking over
    // a fresh drop and moving straight on can genuinely miss it, and a
    // check that only passes when the walk is slow enough is a check
    // that fails on a fast machine.
    linger(&mut alice, 24, &mut sequence).await?;
    let carried = alice.pack_holding(kind, 2).await?;
    println!(
        "  picked up {carried} x {}",
        primitive_shared::types::block_name(kind)
    );

    // Which slot it landed in matters: a placement spends *the selected
    // slot*, and the server does not take the client's word for what is
    // in it.
    let slot = (0..primitive_shared::inventory::SLOTS)
        .find(|&s| alice.pack.block_in(s) == Some(kind))
        .ok_or_else(|| anyhow::anyhow!("the pack holds it but no slot admits to it"))?;
    alice
        .send(ClientMessage::SelectSlot { slot: slot as u8 })
        .await?;

    // --- you cannot build inside yourself ---
    //
    // The cell her feet are in, which after walking over the drops is
    // the second hole rather than where she started -- a check aimed at
    // where the player *was* is a check that passes by accident.
    println!("alice tries to put it inside herself...");
    alice
        .send(ClientMessage::SetBlock {
            global_x: spare.0,
            global_y: ground,
            global_z: spare.1,
            block_id: kind,
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

    // --- one layer, then another, and the second one is free ---
    anyhow::ensure!(
        primitive_shared::types::is_loose(kind),
        "the spot search picked a material that does not come in layers"
    );
    // What she is carrying *now*, read from the message that came back
    // with the refusal above rather than from the count taken when the
    // drops arrived. The two can differ: a server that has been running
    // a while has other people's leavings lying about, and anything she
    // strays within reach of is hers.
    let before = alice.pack.count(kind);
    let one_layer = BlockChange {
        block_id: primitive_shared::types::with_layers(kind, 1),
        ..broken
    };
    println!("alice lays a single layer of it back down...");
    alice
        .send(ClientMessage::SetBlock {
            global_x: one_layer.global_x,
            global_y: one_layer.global_y,
            global_z: one_layer.global_z,
            block_id: one_layer.block_id,
        })
        .await?;
    alice
        .wait_for("alice's copy of the layer", |m| matches_change(m, &one_layer))
        .await?;
    bob.wait_for("bob's copy of the layer", |m| matches_change(m, &one_layer))
        .await?;
    println!("  bob sees one eighth of a block rather than a whole one");

    let two_layers = BlockChange {
        block_id: primitive_shared::types::with_layers(kind, 2),
        ..broken
    };
    alice
        .send(ClientMessage::SetBlock {
            global_x: two_layers.global_x,
            global_y: two_layers.global_y,
            global_z: two_layers.global_z,
            block_id: two_layers.block_id,
        })
        .await?;
    // Alice first, and not only for symmetry: reading her own socket is
    // what brings the pack up to date, and the pack is what the sum
    // below is about. She has not read a word since the placement, and
    // what the *server* thinks she is carrying is only knowable from
    // messages she has actually taken off the wire.
    alice
        .wait_for("alice's copy of the second layer", |m| {
            matches_change(m, &two_layers)
        })
        .await?;
    bob.wait_for("bob's copy of the second layer", |m| {
        matches_change(m, &two_layers)
    })
    .await?;
    // Nothing new to wait for: the second layer costs nothing, so the
    // last word the server said about the pack is the word after both.
    let left = alice.pack.count(kind);
    println!("  thickened to two eighths for one item in total ({left} left of {before})");
    anyhow::ensure!(
        left + 1 == before,
        "a cell costs one item however thinly it is spread: {before} -> {left}"
    );

    // The world clock should be arriving on its own schedule.
    let time_of_day = bob
        .wait_for("a TimeSync", |msg| match msg {
            ServerMessage::TimeSync { time_of_day, .. } => Some(*time_of_day),
            _ => None,
        })
        .await?;
    println!("  world clock is ticking (time_of_day={time_of_day:.3})");

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

    // --- plugins ---
    // These only run if the example plugins are present; skip quietly
    // otherwise so the check still works on a bare server.
    println!("checking plugin hooks (/online, spawn protection)...");
    // `/online` is not a server command -- if anything answers, it is a
    // plugin.
    alice
        .send(ClientMessage::Chat("/online".to_string()))
        .await?;
    match tokio::time::timeout(
        Duration::from_secs(3),
        alice.wait_for("the /online reply", |msg| match msg {
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

            // The protection plugin guards a small radius around spawn,
            // so digging *here* is the thing it must refuse -- and this
            // is the one edit in the example that is expected to be
            // vetoed rather than applied.
            alice
                .send(ClientMessage::SetBlock {
                    global_x: spare.0,
                    global_y: ground - 1,
                    global_z: spare.1,
                    block_id: primitive_shared::types::BLOCK_AIR,
                })
                .await?;
            match tokio::time::timeout(
                Duration::from_secs(3),
                alice.wait_for("the plugin's refusal", |msg| match msg {
                    ServerMessage::Chat { username, text, .. }
                        if username == "server" && text.contains("protected") =>
                    {
                        Some(text.clone())
                    }
                    ServerMessage::Error(text) if text.contains("plugin") => Some(text.clone()),
                    _ => None,
                }),
            )
            .await
            {
                Ok(Ok(refusal)) => println!("  a plugin vetoed a block edit: {refusal}"),
                _ => println!("  (nothing guards this spot -- veto not exercised)"),
            }
        }
        _ => println!("  (no plugins loaded on this server -- skipped)"),
    }

    alice.send(ClientMessage::Disconnect).await?;
    bob.send(ClientMessage::Disconnect).await?;

    println!(
        "\nOK: handshake, chunk streaming, walking, breaking, drops and pickup, \
         the layer economy over the wire, the self-placement guard, the world \
         clock, chat command permissions and plugin hooks all verified."
    );
    Ok(())
}

/// Two neighbouring columns of loose surface material, level with each
/// other, well inside the chunk, with more of the same underneath.
///
/// Read out of the chunk the server sent rather than assumed, because
/// terrain is terrain: a fixed pair of coordinates digs at whatever
/// happens to be there, which is a test that passes or fails by seed.
/// Two columns to dig and one to stand on, in global coordinates.
type DigSite = ((i32, i32), (i32, i32), (i32, i32));

fn find_a_spot(chunk: &primitive_shared::types::Chunk) -> Option<DigSite> {
    use primitive_shared::types::{block_drop, block_kind, is_loose};
    let origin = (chunk.pos.x * 16, chunk.pos.z * 16);
    let global = |(lx, lz): (i32, i32)| (origin.0 + lx, origin.1 + lz);
    for lz in 1..14i32 {
        for lx in 1..14i32 {
            let (first, second, stand) = ((lx, lz), (lx, lz + 1), (lx + 1, lz));
            let top = chunk.height_at(first.0 as usize, first.1 as usize);
            if top < 4
                || top != chunk.height_at(second.0 as usize, second.1 as usize)
                || top != chunk.height_at(stand.0 as usize, stand.1 as usize)
            {
                continue;
            }
            // Nothing standing on any of them, and nothing above them:
            // a tuft of grass or an overhang makes "the surface" an
            // argument rather than a fact.
            if (top + 1..(top + 3).min(63))
                .any(|y| chunk.get(first.0 as usize, y as usize, first.1 as usize)
                    != primitive_shared::types::BLOCK_AIR
                    || chunk.get(second.0 as usize, y as usize, second.1 as usize)
                        != primitive_shared::types::BLOCK_AIR
                    || chunk.get(stand.0 as usize, y as usize, stand.1 as usize)
                        != primitive_shared::types::BLOCK_AIR)
            {
                continue;
            }
            // **Side by side, not stacked.** Digging one hole and then
            // its floor drops both items to the bottom of it, and a
            // pickup is measured to the player's body: standing at the
            // rim, an item two blocks down is out of reach, and the
            // example waits for a pack that never fills.
            let a = chunk.get(first.0 as usize, top as usize, first.1 as usize);
            let b = chunk.get(second.0 as usize, top as usize, second.1 as usize);
            let (Some(a), Some(b)) = (block_drop(a), block_drop(b)) else {
                continue;
            };
            if block_kind(a) == block_kind(b) && is_loose(a) {
                return Some((global(first), global(second), global(stand)));
            }
        }
    }
    None
}

/// Stands still, saying so, for `ticks` updates at twenty a second.
///
/// Pickup runs on the server's tick loop against the last position it
/// accepted, so standing still is a thing a client has to *keep saying*
/// for it to mean anything.
async fn linger(client: &mut Client, ticks: u32, sequence: &mut u32) -> anyhow::Result<()> {
    let at = client.at;
    for _ in 0..ticks {
        *sequence += 1;
        client
            .send(ClientMessage::UpdateTransform {
                x: at.0,
                y: at.1,
                z: at.2,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: true,
                sequence: *sequence,
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

/// Sends the run of `UpdateTransform`s a player walking there would.
///
/// The anti-cheat measures a distance budget that refills at walking
/// pace and a hard cap on vertical speed, so a client that jumps
/// straight to where it wants to be is rubber-banded back to where the
/// server last believed it was -- and every edit it sends afterwards is
/// out of reach of *that*. Stepping is not politeness here; it is the
/// only way to arrive.
///
/// The first update is measured against the spawn the server chose, so
/// the walk starts from there -- the client is told where that is in
/// its `Welcome`.
async fn walk_to(
    client: &mut Client,
    to: (f32, f32, f32),
    sequence: &mut u32,
) -> anyhow::Result<()> {
    // Where the server put us, from the handshake. A snapshot would
    // not do: it carries the players *near* the recipient, and a lone
    // player is not near anybody.
    let from = client.at;
    let (dx, dy, dz) = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
    let horizontal = (dx * dx + dz * dz).sqrt();
    // A fifth of a block per step along the ground and one block down,
    // twenty steps a second: 4 b/s horizontally and 20 b/s vertically,
    // both comfortably inside what the server allows.
    let steps = ((horizontal / 0.2).max(dy.abs()).ceil() as u32).clamp(1, 600);
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        *sequence += 1;
        client
            .send(ClientMessage::UpdateTransform {
                x: from.0 + dx * t,
                y: from.1 + dy * t,
                z: from.2 + dz * t,
                yaw: 0.0,
                pitch: 0.0,
                // On the ground the whole way, because that is what
                // this is: a walk down a hillside. Reporting a descent
                // as *falling* -- which an earlier version did, on the
                // theory that only the last step had landed -- bills
                // the drop as a fall, and nineteen blocks of hillside
                // is a fatal one. She died on arrival and the example
                // then waited for a corpse to pick something up.
                //
                // Claiming ground while *climbing* is a different
                // matter and is checked: see the anti-cheat's
                // `verify_ground`. This never climbs.
                on_ground: dy <= 0.0,
                sequence: *sequence,
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    client.at = to;
    Ok(())
}
