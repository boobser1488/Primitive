//! Survival, end to end over a real socket.
//!
//! The unit tests in `survival` cover the rules; these cover the wiring,
//! which is where the interesting failures are. A fall that hurts nobody
//! because the transform handler never reaches the tracker, or a death
//! that never reaches the client because the message is queued behind a
//! chunk, are both invisible to a unit test and obvious here.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::survival::MAX_HEALTH;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
use tokio::net::TcpStream;

fn test_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "test".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        stats_interval_secs: 0.0,
        // The anti-cheat would otherwise reject the teleport-sized jumps
        // these tests use to stage a fall. What is under test here is
        // the survival wiring, not the movement validator, which has its
        // own tests.
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
    sequence: u32,
    /// This connection's player id, which is what another client has to
    /// name to swing at it.
    id: primitive_shared::protocol::PlayerId,
}

impl Client {
    async fn connect(address: &str) -> Self {
        Self::connect_as(address, "faller").await
    }

    async fn connect_as(address: &str, username: &str) -> Self {
        let mut socket = TcpStream::connect(address).await.expect("connect");
        write_message(
            &mut socket,
            &ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                username: username.to_string(),
            },
        )
        .await
        .expect("hello");

        let (spawn, id) = match read_message::<_, ServerMessage>(&mut socket)
            .await
            .expect("welcome")
        {
            ServerMessage::Welcome { spawn, your_id, .. } => (spawn, your_id),
            other => panic!("expected Welcome, got {other:?}"),
        };

        Self {
            socket,
            spawn,
            sequence: 0,
            id,
        }
    }

    async fn send(&mut self, message: ClientMessage) {
        write_message(&mut self.socket, &message)
            .await
            .expect("send");
    }

    /// Reports a position, which is what drives the fall tracker.
    async fn move_to(&mut self, y: f32, on_ground: bool) {
        self.sequence += 1;
        let (x, _, z) = self.spawn;
        self.send(ClientMessage::UpdateTransform {
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
            on_ground,
            sequence: self.sequence,
        })
        .await;
    }

    /// Reads until a message the predicate likes shows up, or gives up.
    ///
    /// Filtering rather than reading one message is not optional: chunk
    /// data, snapshots and keepalives all share this socket, so the
    /// message under test is never the next one to arrive.
    async fn wait_for<T>(&mut self, mut want: impl FnMut(&ServerMessage) -> Option<T>) -> Option<T> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let message =
                match tokio::time::timeout(remaining, read_message::<_, ServerMessage>(&mut self.socket))
                    .await
                {
                    Ok(Ok(message)) => message,
                    _ => return None,
                };
            if let Some(found) = want(&message) {
                return Some(found);
            }
        }
    }
}

#[tokio::test]
async fn a_new_player_is_told_their_health() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    let health = client
        .wait_for(|m| match m {
            ServerMessage::Health { current, max } => Some((*current, *max)),
            _ => None,
        })
        .await
        .expect("the server never sent a health message");

    assert_eq!(health, (MAX_HEALTH, MAX_HEALTH));
    server.stop().await;
}

#[tokio::test]
async fn a_long_fall_costs_health() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    // Consume the opening health message so the one we assert on is the
    // one the fall caused.
    client
        .wait_for(|m| matches!(m, ServerMessage::Health { .. }).then_some(()))
        .await
        .expect("opening health");

    let ground = client.spawn.1;
    client.move_to(ground + 30.0, false).await;
    client.move_to(ground + 15.0, false).await;
    client.move_to(ground, true).await;

    let after = client
        .wait_for(|m| match m {
            ServerMessage::Health { current, .. } => Some(*current),
            _ => None,
        })
        .await
        .expect("the fall produced no health update");

    assert!(
        after < MAX_HEALTH,
        "a thirty block fall left health at {after}"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_short_hop_costs_nothing() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    client
        .wait_for(|m| matches!(m, ServerMessage::Health { .. }).then_some(()))
        .await
        .expect("opening health");

    let ground = client.spawn.1;
    for _ in 0..5 {
        client.move_to(ground + 1.5, false).await;
        client.move_to(ground, true).await;
    }

    // Nothing should follow. Give the server real time to be wrong.
    let unexpected = tokio::time::timeout(
        Duration::from_millis(600),
        client.wait_for(|m| match m {
            ServerMessage::Health { current, .. } => Some(*current),
            _ => None,
        }),
    )
    .await;

    if let Ok(Some(health)) = unexpected {
        assert_eq!(health, MAX_HEALTH, "hopping hurt the player");
    }
    server.stop().await;
}

#[tokio::test]
async fn a_fatal_fall_kills_and_respawn_restores() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    // Straight down from the top of the world.
    client.move_to(63.0, false).await;
    client.move_to(30.0, false).await;
    client.move_to(0.0, true).await;

    let cause = client
        .wait_for(|m| match m {
            ServerMessage::Died { cause } => Some(cause.clone()),
            _ => None,
        })
        .await
        .expect("a fall from the top of the world should be fatal");
    assert!(!cause.is_empty(), "the death screen would have nothing to say");

    client.send(ClientMessage::Respawn).await;

    let spawn = client.spawn;
    let respawned = client
        .wait_for(|m| match m {
            ServerMessage::Respawned { x, y, z } => Some((*x, *y, *z)),
            _ => None,
        })
        .await
        .expect("the server never respawned the player");
    assert_eq!(respawned, spawn, "respawned somewhere other than spawn");

    let health = client
        .wait_for(|m| match m {
            ServerMessage::Health { current, .. } => Some(*current),
            _ => None,
        })
        .await
        .expect("no health after respawning");
    assert_eq!(health, MAX_HEALTH, "respawned at {health} health");

    server.stop().await;
}

#[tokio::test]
async fn respawning_while_alive_is_ignored() {
    // Otherwise it is a free teleport to spawn from anywhere, which is
    // exactly the sort of thing a modified client would send.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    client
        .wait_for(|m| matches!(m, ServerMessage::Health { .. }).then_some(()))
        .await
        .expect("opening health");

    client.send(ClientMessage::Respawn).await;

    let answered = tokio::time::timeout(
        Duration::from_millis(600),
        client.wait_for(|m| matches!(m, ServerMessage::Respawned { .. }).then_some(())),
    )
    .await;
    assert!(
        !matches!(answered, Ok(Some(()))),
        "a living player was teleported home by asking to respawn"
    );

    server.stop().await;
}

// ---- being hit ----
//
// The first damage in the game that is not the ground, and the first
// message one player sends that changes another player's state. Every
// check that makes it safe is on the server, so every one of them is
// only observable from out here.

impl Client {
    /// Reads the next health figure, or gives up after a moment.
    ///
    /// The "gives up" is what half of these tests are actually asserting
    /// on: a swing that should not have landed produces no message at
    /// all, and the only way to see that is to wait for one.
    async fn health_within(&mut self, millis: u64) -> Option<f32> {
        tokio::time::timeout(
            Duration::from_millis(millis),
            self.wait_for(|m| match m {
                ServerMessage::Health { current, .. } => Some(*current),
                _ => None,
            }),
        )
        .await
        .ok()
        .flatten()
    }
}

/// Two clients standing on the same spawn point, both settled enough
/// that the server has a position for each.
async fn two_players(address: &str) -> (Client, Client) {
    let mut victim = Client::connect_as(address, "victim").await;
    let mut attacker = Client::connect_as(address, "attacker").await;
    let ground = victim.spawn.1;
    victim.move_to(ground, true).await;
    attacker.move_to(ground, true).await;
    // The opening health message, so what follows is what a swing did.
    victim.health_within(4000).await.expect("opening health");
    attacker.health_within(4000).await.expect("opening health");
    (victim, attacker)
}

#[tokio::test]
async fn a_punch_costs_the_other_player_health() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut victim, mut attacker) = two_players(&address).await;

    attacker
        .send(ClientMessage::Attack { target: victim.id })
        .await;

    let after = victim
        .health_within(4000)
        .await
        .expect("the punch produced no health update");
    assert_eq!(
        after,
        MAX_HEALTH - primitive_shared::combat::MELEE_DAMAGE,
        "a bare-handed hit is worth exactly what the shared rules say"
    );
    server.stop().await;
}

#[tokio::test]
async fn swinging_as_fast_as_the_socket_allows_still_lands_one_hit() {
    // The cooldown is a server rule, not a client courtesy: a modified
    // client that removed its own would otherwise hit as fast as it
    // could write to the socket.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut victim, mut attacker) = two_players(&address).await;

    for _ in 0..20 {
        attacker
            .send(ClientMessage::Attack { target: victim.id })
            .await;
    }

    let first = victim.health_within(4000).await.expect("no hit landed at all");
    assert_eq!(first, MAX_HEALTH - primitive_shared::combat::MELEE_DAMAGE);
    // ...and nothing else, for as long as the cooldown lasts.
    assert!(
        victim.health_within(400).await.is_none(),
        "twenty swings in an instant landed more than one"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_swing_from_across_the_world_lands_on_nobody() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut victim, mut attacker) = two_players(&address).await;

    // Far enough that no tolerance covers it, and reported through the
    // ordinary transform message -- which is exactly what a reach hack
    // cannot avoid doing.
    attacker.move_to(victim.spawn.1 + 400.0, false).await;
    attacker
        .send(ClientMessage::Attack { target: victim.id })
        .await;

    assert!(
        victim.health_within(600).await.is_none(),
        "a swing from four hundred blocks away landed"
    );
    server.stop().await;
}

#[tokio::test]
async fn swinging_at_nobody_in_particular_is_ignored() {
    // Both of these arrive from a client that is making things up: a
    // player id nobody has, and its own.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut victim, mut attacker) = two_players(&address).await;

    attacker
        .send(ClientMessage::Attack { target: 999_999 })
        .await;
    attacker
        .send(ClientMessage::Attack {
            target: attacker.id,
        })
        .await;

    assert!(
        attacker.health_within(600).await.is_none(),
        "a player punched themselves"
    );
    assert!(
        victim.health_within(200).await.is_none(),
        "a swing at nobody hit somebody"
    );
    server.stop().await;
}

// ---- inventory, drops and crafting ----
//
// The inventory moved to the server, which means the whole loop --
// break a block, a drop appears, walking over it fills a slot, placing
// spends one -- now crosses the wire in both directions. These check it
// end to end, because every individual piece can be right while the
// wiring between them is not.

use primitive_shared::inventory::Inventory;
use primitive_shared::types::{block_drop, BLOCK_AIR};

impl Client {
    /// Waits for the next inventory snapshot.
    async fn wait_for_inventory(&mut self) -> Option<Inventory> {
        self.wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } => Some(inventory.clone()),
            _ => None,
        })
        .await
    }

    /// Finds a solid block near the spawn point to break.
    async fn break_block(&mut self, at: (i32, i32, i32)) {
        self.send(ClientMessage::SetBlock {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
            block_id: BLOCK_AIR,
        })
        .await;
    }
}

#[tokio::test]
async fn a_new_player_is_sent_an_empty_inventory() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    let inventory = client
        .wait_for_inventory()
        .await
        .expect("the server never sent an inventory");
    assert!(inventory.is_empty(), "a fresh player started with something");
    assert_eq!(inventory.slots().len(), primitive_shared::inventory::SLOTS);

    server.stop().await;
}

#[tokio::test]
async fn breaking_a_block_drops_it_and_walking_over_it_picks_it_up() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.wait_for_inventory().await.expect("opening inventory");

    // The block the player is standing on. Spawn is one above the
    // surface, so this is solid ground.
    let (x, y, z) = client.spawn;
    let cell = (x.floor() as i32, y.floor() as i32 - 1, z.floor() as i32);

    // Ask for the chunk first: the server only edits blocks it has, and
    // only sends drops for what was really there.
    client
        .send(ClientMessage::RequestChunk(
            primitive_shared::types::ChunkPos::from_global(cell.0, cell.2).0,
        ))
        .await;
    client
        .wait_for(|m| matches!(m, ServerMessage::ChunkData(_)).then_some(()))
        .await
        .expect("chunk");

    client.break_block(cell).await;

    // Stand in the hole that just appeared, which is where a real
    // player ends up, and keep reporting it: pickup runs on the tick
    // loop against the last position the server accepted.
    for _ in 0..40 {
        client.move_to(y - 1.0, true).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let inventory = client
        .wait_for_inventory()
        .await
        .expect("breaking a block produced no inventory change");
    assert!(
        !inventory.is_empty(),
        "the drop was never picked up: {:?}",
        inventory.slots()
    );

    server.stop().await;
}

#[tokio::test]
async fn placing_without_the_block_is_refused() {
    // The point of the inventory being server-side: an empty-handed
    // player cannot build.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.wait_for_inventory().await.expect("opening inventory");

    let (x, y, z) = client.spawn;
    client
        .send(ClientMessage::SetBlock {
            global_x: x.floor() as i32,
            global_y: y.floor() as i32 + 3,
            global_z: z.floor() as i32,
            block_id: primitive_shared::types::BLOCK_STONE,
        })
        .await;

    let refused = client
        .wait_for(|m| match m {
            ServerMessage::Error(text) => Some(text.clone()),
            _ => None,
        })
        .await
        .expect("the server accepted a placement from an empty inventory");
    assert!(!refused.is_empty());

    server.stop().await;
}

#[tokio::test]
async fn an_unknown_recipe_is_refused_rather_than_crashing_the_server() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.wait_for_inventory().await.expect("opening inventory");

    client
        .send(ClientMessage::Craft {
            index: 9_999,
            times: 1,
        })
        .await;
    client
        .wait_for(|m| matches!(m, ServerMessage::Error(_)).then_some(()))
        .await
        .expect("no answer to a nonsense recipe");

    // ...and the connection is still usable afterwards.
    client.send(ClientMessage::MoveSlots { from: 0, to: 200 }).await;
    client
        .wait_for_inventory()
        .await
        .expect("the server stopped talking after a bad request");

    server.stop().await;
}

#[tokio::test]
async fn a_returning_player_is_put_back_where_they_left() {
    // The point of profiles: a name resolves to a UUID, and the UUID
    // owns a pack and a place of exit. Checked end to end, because the
    // pieces can each be right while the wiring at the handshake is not
    // -- which is exactly what "I logged in at spawn with nothing"
    // looks like.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();

    let mut client = Client::connect_as(&address, "homebody").await;
    let spawn = client.spawn;
    client.wait_for_inventory().await.expect("opening inventory");

    // Wander off and stop somewhere.
    let away = spawn.1 + 6.0;
    for _ in 0..6 {
        client.move_to(away, true).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    drop(client);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let again = Client::connect_as(&address, "homebody").await;
    assert!(
        (again.spawn.1 - away).abs() < 0.01,
        "came back at {:?} instead of where they left ({away})",
        again.spawn
    );

    // ...and someone else is still a stranger, starting at spawn.
    let newcomer = Client::connect_as(&address, "stranger").await;
    assert!(
        (newcomer.spawn.1 - spawn.1).abs() < 0.01,
        "a first-time player started at someone else's place of exit"
    );

    server.stop().await;
}

#[tokio::test]
async fn nonsense_slot_indices_are_ignored_rather_than_believed() {
    // Every one of these carries a slot number straight off the wire, so
    // every one of them is a chance to index past the end of a vector on
    // a message a modified client can send for free.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.wait_for_inventory().await.expect("opening inventory");

    for message in [
        ClientMessage::MoveSlots { from: 255, to: 254 },
        ClientMessage::SplitSlot { from: 255, to: 0 },
        ClientMessage::QuickMoveSlot { slot: 255 },
        ClientMessage::DropSlot {
            slot: 255,
            whole_stack: true,
        },
        ClientMessage::SelectSlot { slot: 255 },
        ClientMessage::SortInventory,
    ] {
        client.send(message).await;
    }

    // Still alive, still answering, and still holding nothing: none of
    // that could have created anything either.
    client.send(ClientMessage::MoveSlots { from: 0, to: 1 }).await;
    let inventory = client
        .wait_for_inventory()
        .await
        .expect("the server stopped talking after out-of-range slots");
    assert!(inventory.is_empty(), "a nonsense request conjured something up");

    server.stop().await;
}

#[tokio::test]
async fn every_block_that_drops_something_drops_something_usable() {
    // Cheap, but it is the contract the whole loop rests on: if a block
    // yielded something you could neither place nor spend, mining it
    // would fill a slot with a dead item.
    //
    // Items are the one exception, and only because a recipe wants
    // them: fibre cannot be placed, but three of it is a tuft of grass
    // again. `types` checks that side of it; here we only need to know
    // that mining cannot produce a block with nothing to do.
    for &(id, name) in primitive_shared::types::ALL_BLOCK_IDS {
        if let Some(drop) = block_drop(id) {
            assert!(
                primitive_shared::types::is_placeable(drop)
                    || primitive_shared::types::is_item(drop),
                "{name} drops something that can neither be placed nor used"
            );
        }
    }
}

// ---- chests ----
//
// The first block whose contents are server state, and the first screen
// two players can be inside at once. Everything that makes it safe is on
// the server -- the reach check, whose chest is whose, what a slot index
// is allowed to be -- so all of it is only observable from out here.

use primitive_shared::protocol::Side;
use primitive_shared::types::{BLOCK_CHEST, BLOCK_DIRT};

impl Client {
    /// Waits for the next chest snapshot.
    async fn wait_for_chest(&mut self) -> Option<Inventory> {
        tokio::time::timeout(
            Duration::from_millis(4000),
            self.wait_for(|m| match m {
                ServerMessage::ChestState { inventory, .. } => Some(inventory.clone()),
                _ => None,
            }),
        )
        .await
        .ok()
        .flatten()
    }

    /// Breaks the ground under the spawn point and waits for the drop to
    /// arrive in the pack, then says which slot it landed in.
    ///
    /// Terrain rather than a fixture, so it depends on what the world
    /// generator put there -- hence the `Option`: a spawn standing on
    /// something that yields nothing is not this test's business.
    async fn dig_something_up(&mut self, at: (i32, i32, i32)) -> Option<usize> {
        self.break_block(at).await;
        for _ in 0..8 {
            let pack = self.wait_for_inventory().await?;
            if let Some(slot) =
                (0..primitive_shared::inventory::SLOTS).find(|&slot| pack.count_in(slot) > 0)
            {
                return Some(slot);
            }
        }
        None
    }
}

/// A client at the spawn point with a chest standing in front of it.
///
/// The chest goes two blocks above the feet: within arm's reach, and not
/// in the cell the player is standing in. It is placed straight into the
/// world rather than through `SetBlock`, because a *player* placing one
/// has to be carrying one, and stocking a pack is not what any of these
/// tests are about.
async fn client_with_a_chest(
    server: &primitive_server::Server,
    address: &str,
) -> (Client, (i32, i32, i32)) {
    let mut client = Client::connect_as(address, "hoarder").await;
    client.wait_for_inventory().await.expect("opening inventory");
    let spawn = client.spawn;
    let at = (
        spawn.0.floor() as i32,
        spawn.1.floor() as i32 + 2,
        spawn.2.floor() as i32,
    );
    client.move_to(spawn.1, true).await;
    server.place_block(at.0, at.1, at.2, BLOCK_CHEST);
    (client, at)
}

#[tokio::test]
async fn what_goes_into_a_chest_stays_there() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut client, at) = client_with_a_chest(&server, &address).await;

    let Some(slot) = client.dig_something_up((at.0, at.1 - 3, at.2)).await else {
        server.stop().await;
        return; // nothing to store; there is no assertion to make
    };

    client
        .send(ClientMessage::OpenChest {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
        })
        .await;
    let opened = client.wait_for_chest().await.expect("the chest never opened");
    assert!(opened.is_empty(), "a fresh chest came with something in it");

    client
        .send(ClientMessage::ChestQuickMove {
            side: Side::Pack,
            slot: slot as u8,
        })
        .await;
    let contents = client.wait_for_chest().await.expect("no chest update");
    assert!(!contents.is_empty(), "the stack never arrived in the chest");
    let stored = contents.total_items();

    // Shut it and open it again: what is in a chest is not a property of
    // the screen being up.
    client.send(ClientMessage::CloseChest).await;
    client
        .send(ClientMessage::OpenChest {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
        })
        .await;
    let reopened = client.wait_for_chest().await.expect("it did not open again");
    assert_eq!(
        reopened.total_items(),
        stored,
        "the chest forgot what was in it"
    );

    server.stop().await;
}

#[tokio::test]
async fn a_chest_across_the_map_cannot_be_opened() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut client, at) = client_with_a_chest(&server, &address).await;

    // The same chest, asked for from four hundred blocks up.
    client.move_to(client.spawn.1 + 400.0, false).await;
    client
        .send(ClientMessage::OpenChest {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
        })
        .await;
    assert!(
        client.wait_for_chest().await.is_none(),
        "a chest opened from four hundred blocks away"
    );

    server.stop().await;
}

#[tokio::test]
async fn a_gesture_without_an_open_chest_is_ignored() {
    // Every one of these is a message a modified client can send for
    // free, and the player's own pack is on the other end of all of them.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.wait_for_inventory().await.expect("opening inventory");

    for message in [
        ClientMessage::ChestMove {
            from: (Side::Pack, 0),
            to: (Side::Chest, 0),
            half: false,
        },
        ClientMessage::ChestQuickMove {
            side: Side::Chest,
            slot: 255,
        },
        ClientMessage::ChestMove {
            from: (Side::Chest, 255),
            to: (Side::Pack, 255),
            half: true,
        },
        ClientMessage::CloseChest,
    ] {
        client.send(message).await;
    }

    // Still answering, and still holding nothing: none of that could
    // have created anything either.
    client.send(ClientMessage::MoveSlots { from: 0, to: 1 }).await;
    let inventory = client
        .wait_for_inventory()
        .await
        .expect("the server stopped talking after nonsense chest gestures");
    assert!(
        inventory.is_empty(),
        "a chest gesture with no chest conjured something up"
    );

    server.stop().await;
}

#[tokio::test]
async fn breaking_a_chest_shuts_the_screen_over_it() {
    // A client left looking at a chest that no longer exists is a client
    // putting things into nothing.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let (mut client, at) = client_with_a_chest(&server, &address).await;

    client
        .send(ClientMessage::OpenChest {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
        })
        .await;
    client.wait_for_chest().await.expect("the chest never opened");

    client.break_block(at).await;
    let closed = tokio::time::timeout(
        Duration::from_millis(4000),
        client.wait_for(|m| matches!(m, ServerMessage::ChestClosed).then_some(())),
    )
    .await;
    assert!(
        matches!(closed, Ok(Some(()))),
        "the screen was left open over a broken chest"
    );

    server.stop().await;
}

#[test]
fn a_chest_is_worth_making_and_can_be_taken_home_again() {
    // The loop from the block table's point of view: something makes
    // one, it can be put down, and breaking it gives it back rather
    // than yielding planks or nothing.
    assert!(primitive_shared::types::is_placeable(BLOCK_CHEST));
    assert_eq!(block_drop(BLOCK_CHEST), Some(BLOCK_CHEST));
    assert!(
        primitive_shared::crafting::RECIPES
            .iter()
            .any(|r| r.output.0 == BLOCK_CHEST),
        "nothing makes a chest"
    );
    // ...and it is a container, which is what makes a right click open
    // it rather than build against it.
    assert!(primitive_shared::types::is_container(BLOCK_CHEST));
    assert!(!primitive_shared::types::is_container(BLOCK_DIRT));
}

// ---- what a death leaves behind ----
//
// A backpack is a chest with a different picture on it, put down by the
// server rather than by a player. Everything here is the wiring: the
// block appearing, the pack emptying, and the two agreeing about what
// went where. All of it is only observable from out here, because the
// only thing the client ever hears about a container is where it is.

use primitive_shared::types::BLOCK_BACKPACK;

impl Client {
    /// Steps off the top of the world onto the ground. Returns the last
    /// position the server was told about, which is where the pack
    /// should end up.
    ///
    /// Deliberately does *not* wait for anything. A death produces an
    /// inventory snapshot, a block update and `Died` in that order, and
    /// a helper that read as far as `Died` would swallow the first two
    /// -- which is exactly what the caller is here to look at.
    async fn step_off_the_world(&mut self) -> (f32, f32, f32) {
        let (x, _, z) = self.spawn;
        let ground = self.spawn.1.floor();
        self.move_to(63.0, false).await;
        self.move_to(30.0, false).await;
        self.move_to(ground, true).await;
        (x, ground, z)
    }
}

#[tokio::test]
async fn dying_with_something_on_you_leaves_it_in_a_backpack() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect_as(&address, "unlucky").await;
    client.wait_for_inventory().await.expect("opening inventory");

    // Something to lose. The ground under the spawn point, dug up and
    // walked over, which is the only way to fill a pack from out here.
    let (x, y, z) = client.spawn;
    let cell = (x.floor() as i32, y.floor() as i32 - 1, z.floor() as i32);
    client
        .send(ClientMessage::RequestChunk(
            primitive_shared::types::ChunkPos::from_global(cell.0, cell.2).0,
        ))
        .await;
    client
        .wait_for(|m| matches!(m, ServerMessage::ChunkData(_)).then_some(()))
        .await
        .expect("chunk");
    client.break_block(cell).await;
    for _ in 0..40 {
        client.move_to(y - 1.0, true).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut carried_items = 0;
    for _ in 0..8 {
        let pack = client.wait_for_inventory().await.expect("no inventory");
        if !pack.is_empty() {
            carried_items = pack.total_items();
            break;
        }
    }
    if carried_items == 0 {
        // Nothing to lose means nothing to assert. The spawn point's
        // ground is terrain, not a fixture.
        server.stop().await;
        return;
    }

    let died_at = client.step_off_the_world().await;

    // One pass over everything the death produced, rather than one wait
    // per thing: the snapshot, the block and `Died` arrive in that
    // order down one socket, and reading for any of them on its own
    // throws the others away.
    let mut emptied = false;
    let mut placed = None;
    client
        .wait_for(|m| {
            match m {
                ServerMessage::InventoryState { inventory } if inventory.is_empty() => {
                    emptied = true
                }
                ServerMessage::BlockUpdate(change) if change.block_id == BLOCK_BACKPACK => {
                    placed = Some((change.global_x, change.global_y, change.global_z))
                }
                _ => {}
            }
            (emptied && placed.is_some()).then_some(())
        })
        .await
        .expect("dying neither emptied the pack nor put a bag in the world");
    let placed = placed.expect("checked above");

    let cell = (
        died_at.0.floor() as i32,
        died_at.1.floor() as i32,
        died_at.2.floor() as i32,
    );
    assert_eq!(placed.0, cell.0, "the bag is in the wrong column");
    assert_eq!(placed.2, cell.2, "the bag is in the wrong column");
    assert!(
        (placed.1 - cell.1).abs() <= 3,
        "the bag is at y={} and the body fell at y={}",
        placed.1,
        cell.1
    );

    // ...and opening it gives back exactly what was carried.
    client.send(ClientMessage::Respawn).await;
    client
        .wait_for(|m| matches!(m, ServerMessage::Respawned { .. }).then_some(()))
        .await
        .expect("never respawned");
    client
        .send(ClientMessage::OpenChest {
            global_x: placed.0,
            global_y: placed.1,
            global_z: placed.2,
        })
        .await;
    let inside = client
        .wait_for_chest()
        .await
        .expect("the backpack would not open");
    assert_eq!(
        inside.total_items(),
        carried_items,
        "the bag holds {} of the {carried_items} things that went into it",
        inside.total_items()
    );

    server.stop().await;
}

#[tokio::test]
async fn dying_with_nothing_on_you_leaves_nothing_behind() {
    // An empty bag is worse than no bag: a block to walk back to, break,
    // and find empty.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect_as(&address, "pauper").await;
    let opening = client.wait_for_inventory().await.expect("opening inventory");
    assert!(opening.is_empty(), "a fresh player started with something");

    client.step_off_the_world().await;
    client
        .wait_for(|m| matches!(m, ServerMessage::Died { .. }).then_some(()))
        .await
        .expect("a fall from the top of the world should be fatal");

    let littered = tokio::time::timeout(
        Duration::from_millis(1200),
        client.wait_for(|m| match m {
            ServerMessage::BlockUpdate(change) if change.block_id == BLOCK_BACKPACK => Some(()),
            _ => None,
        }),
    )
    .await;
    assert!(
        !matches!(littered, Ok(Some(()))),
        "an empty death left a bag in the world"
    );

    server.stop().await;
}

#[test]
fn a_backpack_is_the_servers_to_place_and_nobody_elses() {
    // The block table's half of the feature, and every one of these is a
    // way the bag could have become a free container.
    assert!(primitive_shared::types::is_container(BLOCK_BACKPACK));
    assert!(
        !primitive_shared::types::is_placeable(BLOCK_BACKPACK),
        "a player who can place one can stamp fake graves across a world"
    );
    assert_eq!(
        block_drop(BLOCK_BACKPACK),
        None,
        "breaking a bag handed back a second bag"
    );
    assert!(
        primitive_shared::types::is_breakable(BLOCK_BACKPACK),
        "a bag you cannot open by breaking is a bag nobody gets back"
    );
    assert!(
        !primitive_shared::crafting::RECIPES
            .iter()
            .any(|r| r.output.0 == BLOCK_BACKPACK),
        "a bag you can make is a chest that costs nothing"
    );
    // ...and it is a real id, so the world can save one and a client can
    // draw it.
    assert!(primitive_shared::types::is_known_block(BLOCK_BACKPACK));
    assert!(primitive_shared::types::ALL_BLOCK_IDS
        .iter()
        .any(|&(id, _)| id == BLOCK_BACKPACK));
}

#[tokio::test]
async fn a_head_deep_under_water_still_runs_out_of_air() {
    // **The twelve per cent.** A full cell of water is drawn stopping
    // `SURFACE_DROP` short of its ceiling -- that is what makes a
    // waterline visible from the shore -- and the drowning check read
    // that same line at every depth. So the top twelfth of *every*
    // submerged cell answered "your head is out of the water": one eye
    // height in eight, at any depth, anywhere in the ocean. A player
    // standing on the sea floor at one of those heights had their breath
    // handed back every tick, the meter jittered, and drowning deep
    // under water was a matter of where you happened to be standing.
    //
    // End to end rather than against `fluid`, because the rule was
    // right in the mesher and wrong here: what was broken was which
    // function the tick loop asked.
    use primitive_shared::fluid::SURFACE_DROP;
    use primitive_shared::geometry::EYE_HEIGHT;
    use primitive_shared::types::BLOCK_WATER;

    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect_as(&address, "diver").await;

    // A column of water over the spawn, deep enough that the cell above
    // the player's eyes is water too -- which is the whole point.
    let (sx, sy, sz) = client.spawn;
    let (x, z) = (sx.floor() as i32, sz.floor() as i32);
    let base = sy.floor() as i32;
    for y in base..base + 6 {
        server.place_block(x, y, z, BLOCK_WATER);
    }

    // Stood so that the eyes land in the band that used to read as air.
    // Asserted rather than assumed: a fixture that drifts out of the
    // band is a test that passes without testing anything.
    let feet = base as f32 + 0.3;
    let eye = feet + EYE_HEIGHT;
    assert!(
        eye.fract() >= 1.0 - SURFACE_DROP,
        "the fixture no longer aims at the band it exists to test ({})",
        eye.fract()
    );

    client
        .wait_for(|m| matches!(m, ServerMessage::Health { .. }).then_some(()))
        .await
        .expect("opening health");
    client.move_to(feet, false).await;

    let breath = client
        .wait_for(|m| match m {
            ServerMessage::Breath { fraction } if *fraction < 0.99 => Some(*fraction),
            _ => None,
        })
        .await;

    assert!(
        breath.is_some(),
        "a player submerged to the eyes never started losing air"
    );
    server.stop().await;
}
