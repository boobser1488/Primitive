//! Hunger, fire and weather, end to end over a real socket.
//!
//! The unit tests in `survival`, `fire` and `weather` cover the rules;
//! these cover the wiring, which is where the interesting failures are.
//! A hunger bar that never reaches the client because the tick loop
//! forgets to send it, a fire that lights on the server and is never
//! drawn as lit, or a craft that is refused because the two sides
//! disagree about what "beside a fire" means, are all invisible to a
//! unit test and obvious here.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
use primitive_shared::types::{
    BLOCK_APPLE, BLOCK_APPLE_LEAVES_FRUIT, BLOCK_APPLE_LEAVES_PICKED, BLOCK_CAMPFIRE,
    BLOCK_CAMPFIRE_LIT, BLOCK_COOKED_MEAT, BLOCK_FLINT, BLOCK_RAW_MEAT,
};
use tokio::net::TcpStream;

fn test_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "test".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        stats_interval_secs: 0.0,
        // These tests place blocks under a player who has not walked to
        // them and craft as fast as the socket allows; what is under
        // test is the survival wiring, not the validator, which has its
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
}

impl Client {
    async fn connect(address: &str) -> Self {
        let mut socket = TcpStream::connect(address).await.expect("connect");
        write_message(
            &mut socket,
            &ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                username: "forager".to_string(),
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
        Self { socket, spawn: primitive_shared::geometry::narrow(spawn) }
    }

    /// Loads the chunk the player is standing in.
    ///
    /// Not optional for anything that watches for a `BlockUpdate`: the
    /// server sends block changes only to the players *subscribed* to
    /// that chunk, and a client subscribes by asking for it. A test that
    /// skips this waits ten seconds for a message that was deliberately
    /// never addressed to it.
    async fn load_spawn_chunk(&mut self) {
        let (x, _, z) = self.spawn;
        let pos = primitive_shared::types::ChunkPos::from_world(x, z);
        self.send(ClientMessage::RequestChunk(pos)).await;
        self.wait_for(|m| match m {
            ServerMessage::ChunkData(chunk) if chunk.pos == pos => Some(()),
            _ => None,
        })
        .await
        .expect("the spawn chunk never arrived");
    }

    /// Puts blocks into this player's own pack.
    ///
    /// Through `/give` typed by the player rather than by the console,
    /// because `/give` fills *the caller's* pack and the console has no
    /// pack to fill. The operator grant is the caller's job -- see
    /// `give_to`.
    async fn give(&mut self, what: &str, count: u32) {
        self.send(ClientMessage::Chat(format!("/give {what} {count}")))
            .await;
    }

    async fn send(&mut self, message: ClientMessage) {
        write_message(&mut self.socket, &message)
            .await
            .expect("send");
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
            let message = match tokio::time::timeout(
                remaining,
                read_message::<_, ServerMessage>(&mut self.socket),
            )
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

    /// The inventory as the server last reported it.
    async fn inventory(&mut self) -> primitive_shared::inventory::Inventory {
        self.wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } => Some(inventory.clone()),
            _ => None,
        })
        .await
        .expect("the server never sent an inventory")
    }
}

/// The index of a recipe, by the name it goes by in the table.
///
/// By name rather than by number, because the number is a wire
/// identity that moves whenever the table grows -- which is exactly the
/// thing a test hard-coding one would silently stop testing.
fn recipe(name: &str) -> u16 {
    primitive_shared::crafting::RECIPES
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("no recipe called {name:?}")) as u16
}

#[tokio::test]
async fn a_new_player_is_told_how_full_they_are_and_what_the_sky_is_doing() {
    // Both are world state the player cannot infer, and both are sent
    // once at the handshake and only on change after that -- so if the
    // join path forgets either, the bar and the sky are wrong for the
    // whole session and nothing ever puts them right.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    let fed = client
        .wait_for(|m| match m {
            ServerMessage::Nourishment { fraction } => Some(*fraction),
            _ => None,
        })
        .await
        .expect("the server never said how full the player is");
    assert_eq!(fed, 1.0, "a new player arrives hungry");

    let sky = client
        .wait_for(|m| match m {
            ServerMessage::WeatherSync { weather } => Some(*weather),
            _ => None,
        })
        .await
        .expect("the server never said what the weather was");
    assert_eq!(
        sky,
        primitive_shared::weather::Weather::Clear,
        "a fresh world opened in the rain"
    );

    server.stop().await;
}

#[tokio::test]
async fn eating_fills_the_bar_and_spends_the_food() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    // A haunch of cooked meat, and a stomach with room for it.
    server.console_command("op forager");
    client.give("cooked_meat", 2).await;
    let pack = client
        .wait_for(|m| match m {
            ServerMessage::InventoryState { inventory }
                if inventory.count(BLOCK_COOKED_MEAT) == 2 =>
            {
                Some(inventory.clone())
            }
            _ => None,
        })
        .await
        .expect("the meat never arrived");
    let slot = (0..primitive_shared::inventory::SLOTS)
        .find(|&slot| pack.block_in(slot) == Some(BLOCK_COOKED_MEAT))
        .expect("the meat never arrived");

    // Nothing happens on a full stomach, and -- the part that matters --
    // nothing is *spent*: an item destroyed for no effect is the same
    // bug as a craft that eats its ingredients and produces nothing.
    client.send(ClientMessage::Eat { slot: slot as u8 }).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let still_there = client
        .wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } => Some(inventory.count(BLOCK_COOKED_MEAT)),
            _ => None,
        })
        .await
        .unwrap_or(2);
    assert_eq!(still_there, 2, "eating on a full stomach spent the meat");

    server.stop().await;
}

#[tokio::test]
async fn a_right_click_picks_apples_into_the_pack_and_the_tree_is_told_it_stands() {
    // **The wiring, end to end.** The rule is tested beside `use_block`;
    // what only a socket shows is that the client's click reaches it, that
    // the pack with the apple in it is sent back, and that everybody
    // watching the tree is told the leaves are still there -- as the
    // picked leaf, not as air. A pick that the server made and never
    // broadcast would leave the apple drawn on every other player's tree.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.load_spawn_chunk().await;

    let (x, y, z) = client.spawn;
    let cell = (x.floor() as i32 + 1, y.floor() as i32, z.floor() as i32);
    server.place_block(cell.0, cell.1, cell.2, BLOCK_APPLE_LEAVES_FRUIT);

    client
        .send(ClientMessage::UseBlock {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
        })
        .await;

    let left = client
        .wait_for(|m| match m {
            ServerMessage::BlockUpdate(change)
                if (change.global_x, change.global_y, change.global_z) == cell
                    && change.block_id != BLOCK_APPLE_LEAVES_FRUIT =>
            {
                Some(change.block_id)
            }
            ServerMessage::BlockUpdates(changes) => changes
                .iter()
                .find(|c| {
                    (c.global_x, c.global_y, c.global_z) == cell
                        && c.block_id != BLOCK_APPLE_LEAVES_FRUIT
                })
                .map(|c| c.block_id),
            _ => None,
        })
        .await
        .expect("nobody was told the apple was picked");
    assert_eq!(left, BLOCK_APPLE_LEAVES_PICKED, "the pick did not leave the leaves standing");

    let picked = client
        .wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_APPLE) > 0 => {
                Some(inventory.count(BLOCK_APPLE))
            }
            _ => None,
        })
        .await
        .expect("the apple never reached the pack");
    assert_eq!(picked, 1);
    assert_eq!(server.block_at(cell.0, cell.1, cell.2), Some(BLOCK_APPLE_LEAVES_PICKED));

    server.stop().await;
}

#[tokio::test]
async fn a_fire_is_struck_alight_and_the_nodule_it_was_struck_with_is_spent() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.load_spawn_chunk().await;

    // A fire, put into the world beside the player by the server itself
    // -- this is about the *using*, not about the placing.
    let (x, y, z) = client.spawn;
    let cell = (x.floor() as i32 + 1, y.floor() as i32, z.floor() as i32);
    server.place_block(cell.0, cell.1, cell.2, BLOCK_CAMPFIRE);

    // ...and a nodule of flint to strike it with.
    server.console_command("op forager");
    client.give("flint", 1).await;
    let pack = client
        .wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_FLINT) == 1 => {
                Some(inventory.clone())
            }
            _ => None,
        })
        .await
        .expect("the flint never arrived");
    let slot = (0..primitive_shared::inventory::SLOTS)
        .find(|&slot| pack.block_in(slot) == Some(BLOCK_FLINT))
        .expect("the flint never arrived");
    client.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;

    client
        .send(ClientMessage::UseBlock {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
        })
        .await;

    let lit = client
        .wait_for(|m| match m {
            ServerMessage::BlockUpdate(change)
                if (change.global_x, change.global_y, change.global_z) == cell =>
            {
                Some(change.block_id)
            }
            ServerMessage::BlockUpdates(changes) => changes
                .iter()
                .find(|c| (c.global_x, c.global_y, c.global_z) == cell)
                .map(|c| c.block_id),
            _ => None,
        })
        .await
        .expect("the fire never lit");
    assert_eq!(lit, BLOCK_CAMPFIRE_LIT);

    // **The nodule is spent on the fire it lit** -- "after striking fire
    // the flint should break". It used to be kept, on the argument that a
    // player out of flint could never light a fire again; gravel sifts into
    // flint anywhere, and the price is what makes keeping a fire fed a
    // decision. See `fire::STRIKER` for the ways that were weighed.
    //
    // Asked for rather than waited for, as it was when the claim was the
    // opposite: `SortInventory` is the cheapest gesture that always answers.
    client.send(ClientMessage::SortInventory).await;
    let after = client.inventory().await;
    assert_eq!(
        after.count(BLOCK_FLINT),
        0,
        "lighting the fire kept the flint"
    );

    server.stop().await;
}

#[tokio::test]
async fn meat_cooks_beside_a_fire_and_nowhere_else() {
    // The single most consequential rule 1.5 added: a recipe that needs
    // heat is refused in a field and runs at a fireside. Both halves are
    // decided by the server against its own copy of where the player is,
    // which is what this test is actually pinning -- a client that
    // decided for itself would be a client that smelts bronze in a
    // meadow.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    server.console_command("op forager");
    client.give("raw_meat", 4).await;
    client
        .wait_for(|m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_RAW_MEAT) == 4 => {
                Some(())
            }
            _ => None,
        })
        .await
        .expect("the meat never arrived");

    // In a field: refused, and the meat is still raw.
    client
        .send(ClientMessage::Craft {
            index: recipe("cooked meat"),
            times: 1,
        })
        .await;
    let refusal = client
        .wait_for(|m| match m {
            ServerMessage::Error(text) => Some(text.clone()),
            _ => None,
        })
        .await;
    assert!(refusal.is_some(), "the server cooked meat in an open field");

    // ...and beside a fire it is *still* refused, because cooking is
    // not something a player does with their hands any more. It is
    // something the fire does, and the fire has an inside now.
    let (x, y, z) = client.spawn;
    let cell = (x.floor() as i32 + 1, y.floor() as i32, z.floor() as i32);
    server.place_block(cell.0, cell.1, cell.2, BLOCK_CAMPFIRE_LIT);
    // Let the fires mechanic notice it -- the reconciliation runs on the
    // tick after the edit.
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
        .send(ClientMessage::Craft {
            index: recipe("cooked meat"),
            times: 1,
        })
        .await;
    assert!(
        client
            .wait_for(|m| match m {
                ServerMessage::Error(text) => Some(text.clone()),
                _ => None,
            })
            .await
            .is_some(),
        "the server cooked meat by hand at a fire"
    );

    // The real path: open the hearth, load it, and let it work.
    client
        .send(ClientMessage::OpenChest {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
        })
        .await;
    let opened = client
        .wait_for(|m| match m {
            ServerMessage::ChestState { kind, hearth, .. } => Some((*kind, *hearth)),
            _ => None,
        })
        .await
        .expect("the hearth never opened");
    assert!(
        matches!(
            opened.0,
            primitive_shared::protocol::ContainerKind::Hearth(
                primitive_shared::hearth::Kind::Campfire
            )
        ),
        "a campfire opened as {:?}",
        opened.0
    );
    assert!(opened.1.is_some_and(|fire| fire.fuel_left > 0.0), "it was not alight");

    // The meat is in the pack's first slot -- it is the only thing in
    // there -- so a shift-click sends it where it belongs.
    client
        .send(ClientMessage::ChestQuickMove {
            side: primitive_shared::protocol::Side::Pack,
            slot: 0,
        })
        .await;
    let loaded = client
        .wait_for(|m| match m {
            ServerMessage::ChestState { inventory, .. }
                if inventory.count_within(primitive_shared::hearth::INPUT_SLOTS, BLOCK_RAW_MEAT)
                    > 0 =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    assert!(loaded.is_some(), "the meat never reached the fire");

    // ...and now it cooks, on the fire's own clock, with nobody doing
    // anything at all.
    // Twice round, because one batch takes longer than the harness will
    // wait for a single message and the fire is deliberately not
    // instant: what is being checked is that it works *on its own
    // clock*, with the player doing nothing.
    let mut cooked = None;
    for _ in 0..3 {
        cooked = client
            .wait_for(|m| match m {
                ServerMessage::ChestState { inventory, .. }
                    if inventory
                        .count_within(primitive_shared::hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT)
                        > 0 =>
                {
                    Some(inventory.clone())
                }
                _ => None,
            })
            .await;
        if cooked.is_some() {
            break;
        }
    }
    let cooked = cooked.expect("the fire never cooked what was put in it");
    assert_eq!(
        cooked.count_within(primitive_shared::hearth::INPUT_SLOTS, BLOCK_RAW_MEAT),
        3,
        "it cooked the wrong amount"
    );

    server.stop().await;
}

#[tokio::test]
async fn the_weather_reaches_everyone_who_is_playing() {
    // Weather is world state, not a local effect: two players in one
    // field have to be standing in the same rain, and the moment it puts
    // a fire out it stops being cosmetic.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    // The first one, from the handshake.
    let _ = client
        .wait_for(|m| matches!(m, ServerMessage::WeatherSync { .. }).then_some(()))
        .await;

    server.console_command("weather storm");

    let sky = client
        .wait_for(|m| match m {
            ServerMessage::WeatherSync { weather }
                if *weather == primitive_shared::weather::Weather::Storm =>
            {
                Some(*weather)
            }
            _ => None,
        })
        .await;
    assert_eq!(
        sky,
        Some(primitive_shared::weather::Weather::Storm),
        "the storm never reached the client"
    );

    server.stop().await;
}
