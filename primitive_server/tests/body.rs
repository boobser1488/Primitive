//! Warmth, water, hides and what a person wears -- end to end over a
//! real socket.
//!
//! The unit tests in `primitive_shared::body`, `equipment` and
//! `primitive_server::climate` cover the *rules*. These cover the
//! **wiring**, which is where the interesting failures are and which no
//! unit test can see:
//!
//! - a temperature that is worked out correctly and never reaches the
//!   client, because the message is queued behind a chunk;
//! - a jug that fills in the server's copy of the pack and is never
//!   pushed back, so the player is holding an empty one;
//! - a garment the server accepts and never announces, so the equipment
//!   screen stays blank while the player's speed changes;
//! - a rack that cures a hide into a map entry nobody can take out.
//!
//! Every test here is the *whole path*: a real server, a real socket, a
//! real client message, and an assertion on what came back.
//!
//! ## Why the test world
//!
//! Every test runs on the `Test` preset. Its climate is one value
//! everywhere by construction (see `showcase::climate`), which is what
//! makes a test of "does a fire warm you" a test of the fire rather than
//! a test of where the noise happened to put a tundra.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::body::Comfort;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
use primitive_shared::types::{
    BLOCK_CAMPFIRE_LIT, BLOCK_DRYING_RACK, BLOCK_HIDE, BLOCK_JUG, BLOCK_JUG_WATER, BLOCK_LEATHER,
    BLOCK_LEATHER_TUNIC, BLOCK_STONE, BLOCK_WATER,
};
use tokio::net::TcpStream;

fn test_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "test".to_string(),
        // Nowhere to save to: nothing a test does may land on disk
        // beside somebody's real world.
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        stats_interval_secs: 0.0,
        // The flat world with everything on it. See the module note.
        world_preset: primitive_shared::worldgen::Preset::Test,
        // These tests put players and blocks where they want them; the
        // movement validator has its own tests.
        anticheat: primitive_server::settings::AntiCheatSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    }
}

struct Client {
    socket: TcpStream,
    /// Who the server says this is: what another client finds this one
    /// by in a snapshot.
    id: primitive_shared::protocol::PlayerId,
    spawn: (f32, f32, f32),
    sequence: u32,
}

impl Client {
    async fn connect(address: &str) -> Self {
        Self::connect_as(address, "body").await
    }

    /// The same join under a name of its own, for a test with two people
    /// in it: two sessions under one name are one person logging in twice.
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
        let (id, spawn) = match read_message::<_, ServerMessage>(&mut socket)
            .await
            .expect("welcome")
        {
            ServerMessage::Welcome { your_id, spawn, .. } => (your_id, spawn),
            other => panic!("expected Welcome, got {other:?}"),
        };
        Self {
            socket,
            id,
            spawn: primitive_shared::geometry::narrow(spawn),
            sequence: 0,
        }
    }

    async fn send(&mut self, message: ClientMessage) {
        write_message(&mut self.socket, &message)
            .await
            .expect("send");
    }

    /// Stands somewhere, which is what the climate sampler reads.
    async fn stand_at(&mut self, x: f32, y: f32, z: f32) {
        self.sequence += 1;
        self.send(ClientMessage::UpdateTransform {
            x: f64::from(x),
            y: f64::from(y),
            z: f64::from(z),
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            sequence: self.sequence,
        })
        .await;
    }

    /// Reads until a message the predicate likes shows up.
    ///
    /// Filtering rather than reading one message is not optional: chunk
    /// data, snapshots and keepalives all share this socket, so the
    /// message under test is never the next one to arrive.
    async fn wait_for<T>(
        &mut self,
        seconds: u64,
        mut want: impl FnMut(&ServerMessage) -> Option<T>,
    ) -> Option<T> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
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

    /// The next inventory snapshot that holds `block`, and which slot it
    /// is in.
    ///
    /// **Not "the next inventory"**, and the difference is a real bug
    /// this caught: a fresh player is sent an inventory the moment they
    /// join, so a test that asked for "the next one" after a `/give` got
    /// the *empty* one that was already in flight and concluded the give
    /// had failed. Waiting for the snapshot that actually contains the
    /// thing is the only assertion that means what it says.
    async fn holding(
        &mut self,
        block: primitive_shared::types::BlockId,
    ) -> (primitive_shared::inventory::Inventory, usize) {
        let inventory = self
            .wait_for(10, |m| match m {
                ServerMessage::InventoryState { inventory } if inventory.count(block) > 0 => {
                    Some(inventory.clone())
                }
                _ => None,
            })
            .await
            .unwrap_or_else(|| {
                panic!(
                    "the server never sent an inventory holding {}",
                    primitive_shared::types::block_name(block)
                )
            });
        let slot = (0..primitive_shared::inventory::SLOTS)
            .find(|&s| inventory.block_in(s) == Some(block))
            .expect("counted but in no slot");
        (inventory, slot)
    }

    /// Waits until the player exists on the server's side.
    ///
    /// A `/give` typed before the join has finished goes to nobody, and
    /// the failure looks exactly like a broken give. The opening
    /// inventory is the signal that the handle is in the registry.
    async fn settled(&mut self) {
        self.wait_for(10, |m| matches!(m, ServerMessage::InventoryState { .. }).then_some(()))
            .await
            .expect("the server never sent an opening inventory");
    }

    /// The next equipment snapshot.
    async fn equipment(&mut self) -> primitive_shared::inventory::Equipment {
        self.wait_for(10, |m| match m {
            ServerMessage::EquipmentState { equipment } => Some(equipment.clone()),
            _ => None,
        })
        .await
        .expect("the server never sent an equipment state")
    }

    /// The next warmth-and-water reading.
    async fn body(&mut self, seconds: u64) -> Option<(f32, Comfort, f32)> {
        self.wait_for(seconds, |m| match m {
            ServerMessage::Body {
                temperature_c,
                comfort,
                hydration,
                // Tiredness rides on the same message and has its own
                // tests; this helper is about warmth and water. (A
                // broken leg used to ride here too, as a flag; it is one
                // wound in `ServerMessage::Injuries` now -- see
                // `tests/injuries.rs`.)
                fatigue: _,
                recovery: _,
                // ...and so do the three the pack screen's health page
                // reads. Same reason: this helper is about warmth and
                // water.
                wetness: _,
                grime: _,
                diet_groups: _,
            } => Some((*temperature_c, *comfort, *hydration)),
            _ => None,
        })
        .await
    }
}

// ---------------------------------------------------------------- water

#[tokio::test]
async fn a_player_is_told_how_warm_and_how_watered_they_are_on_arrival() {
    // The join half. A gauge the client has never been told about is a
    // gauge drawn from whatever the client guessed, and the point of
    // these being server-owned is that the client does not guess.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let mut client = Client::connect(&server.address().to_string()).await;

    let (temperature, comfort, hydration) = client.body(10).await.expect("no body reading");
    assert_eq!(comfort, Comfort::Comfortable, "a fresh player was not comfortable");
    assert!(
        (temperature - primitive_shared::body::NEUTRAL_C).abs() < 1.0,
        "a fresh player started at {temperature}"
    );
    assert!(hydration > 0.99, "a fresh player started thirsty: {hydration}");
    server.stop().await;
}

#[tokio::test]
async fn drinking_from_a_jug_empties_it_and_gives_the_jug_back() {
    // **The whole point of a jug**, and three things have to be true at
    // once: the water goes in the player, the full jug is spent, and the
    // empty one comes back. A version that got two of the three would be
    // an item that vanishes or an infinite drink.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    // Thirsty enough for a drink to do something -- a jug emptied at
    // full hydration is an item destroyed, which the server refuses.
    client.settled().await;
    assert_eq!(server.give(BLOCK_JUG_WATER, 1), 0, "the jug did not fit");
    let (_, slot) = client.holding(BLOCK_JUG_WATER).await;

    // Wait until thirst has actually taken something, or the drink is
    // refused as pointless. The tick loop bills a fraction of a unit
    // every tick, and `Vitals::drink` refuses only within a sip of full.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    client.send(ClientMessage::Eat { slot: slot as u8 }).await;

    let after = client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory }
                if inventory.count(BLOCK_JUG_WATER) == 0 =>
            {
                Some(inventory.clone())
            }
            _ => None,
        })
        .await
        .expect("the full jug was never spent");
    assert_eq!(
        after.count(BLOCK_JUG),
        1,
        "the jug did not come back empty -- it stopped existing"
    );
    server.stop().await;
}

#[tokio::test]
async fn filling_a_jug_needs_water_within_reach() {
    // The server reads the block from its *own* copy of the world, so a
    // client that names a cell it has decided is a lake gets nothing.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    client.settled().await;
    assert_eq!(server.give(BLOCK_JUG, 1), 0, "the jug did not fit");
    let (_, slot) = client.holding(BLOCK_JUG).await;
    client
        .send(ClientMessage::SelectSlot { slot: slot as u8 })
        .await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);

    // A cell of stone beside them: not water, so nothing happens.
    server.place_block(bx + 1, by, bz, BLOCK_STONE);
    client
        .send(ClientMessage::UseBlock {
            global_x: bx + 1,
            global_y: by,
            global_z: bz,
        })
        .await;

    // ...then water in the same place, and it fills.
    server.place_block(bx + 1, by, bz, BLOCK_WATER);
    client
        .send(ClientMessage::UseBlock {
            global_x: bx + 1,
            global_y: by,
            global_z: bz,
        })
        .await;

    let filled = client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory }
                if inventory.count(BLOCK_JUG_WATER) == 1 =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    assert!(filled.is_some(), "an empty jug at a water block stayed empty");
    server.stop().await;
}

#[tokio::test]
async fn a_jug_poured_into_a_barrel_can_be_dipped_back_out_of_it() {
    // **Both halves through the real server**, because each half alone
    // proves nothing about the barrel: a pour that emptied the jug and
    // wrote nothing would pass a test of the pour, and only dipping the
    // same water back out says the barrel actually held it.
    use primitive_shared::types::BLOCK_BARREL;

    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    client.settled().await;
    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    server.place_block(bx + 1, by, bz, BLOCK_BARREL);
    let use_barrel = ClientMessage::UseBlock {
        global_x: bx + 1,
        global_y: by,
        global_z: bz,
    };

    // An empty jug at an empty barrel: nothing to dip, and said so.
    assert_eq!(server.give(BLOCK_JUG, 1), 0, "the jug did not fit");
    let (_, slot) = client.holding(BLOCK_JUG).await;
    client.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;
    client.send(use_barrel.clone()).await;
    let refused = client
        .wait_for(10, |m| match m {
            ServerMessage::Notice { what: primitive_shared::notice::Notice::BarrelEmpty } => Some(()),
            _ => None,
        })
        .await;
    assert!(refused.is_some(), "an empty barrel filled a jug, or said nothing");

    // Fill the jug somewhere else: pour a full one in instead.
    assert_eq!(server.give(BLOCK_JUG_WATER, 1), 0, "the full jug did not fit");
    let (_, full_slot) = client.holding(BLOCK_JUG_WATER).await;
    client.send(ClientMessage::SelectSlot { slot: full_slot as u8 }).await;
    client.send(use_barrel.clone()).await;
    let poured = client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory }
                if inventory.count(BLOCK_JUG_WATER) == 0 && inventory.count(BLOCK_JUG) == 2 =>
            {
                Some(inventory.clone())
            }
            _ => None,
        })
        .await
        .expect("the full jug was not poured into the barrel and given back empty");

    // ...and dip it straight back out. The slot is read off the snapshot
    // the pour produced rather than waited for: `holding` waits for the
    // *next* inventory, and that one has already been read -- nothing else
    // will come until the next gesture.
    let empty_slot = (0..primitive_shared::inventory::SLOTS)
        .find(|&s| poured.block_in(s) == Some(BLOCK_JUG))
        .expect("counted but in no slot");
    client.send(ClientMessage::SelectSlot { slot: empty_slot as u8 }).await;
    client.send(use_barrel).await;
    let dipped = client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory }
                if inventory.count(BLOCK_JUG_WATER) == 1 =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    assert!(dipped.is_some(), "the water poured into the barrel could not be dipped back out");
    server.stop().await;
}

#[tokio::test]
async fn an_empty_hand_at_a_barrel_drinks_and_the_barrel_goes_down_by_one_drink() {
    // **Through the real server and the real gesture**, because the rule
    // is in three places that each pass alone: the client has to send the
    // use for an empty hand (`use_gesture`), the server has to turn an
    // empty hand at a barrel into a drink rather than silence
    // (`use_barrel`), and the drink has to cost the level
    // (`types::barrel_after_drinking`). Each of those has a unit test; only
    // this one says they are the same drink.
    use primitive_shared::body::Water;
    use primitive_shared::types::{barrel_of, BARREL_DRINK_JUGS};

    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.settled().await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    const JUGS: u8 = 3;
    server.place_block(bx + 1, by, bz, barrel_of(Water::Fresh, JUGS));

    // Thirsty enough to take a drink at all -- the same wait the jug test
    // makes, for the same reason: a full player is refused, and the
    // refusal must leave the barrel as it was.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    client
        .send(ClientMessage::UseBlock {
            global_x: bx + 1,
            global_y: by,
            global_z: bz,
        })
        .await;

    let lowered = barrel_of(Water::Fresh, JUGS - BARREL_DRINK_JUGS);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while server.block_at(bx + 1, by, bz) != Some(lowered) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "a bare hand at a barrel left it at {:?}, not one drink lower",
            server.block_at(bx + 1, by, bz)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    server.stop().await;
}

// ------------------------------------------------------------- warmth

#[tokio::test]
async fn a_fire_is_warmer_than_a_lake() {
    // The two ends of what the world does to a body, through the whole
    // path: a block placed, a position reported, a sample taken, a
    // message sent. Both directions in one test, because what is being
    // asserted is the *difference* -- an absolute temperature would be a
    // test of the constants rather than of the wiring.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);

    // A lit fire two cells one way and a pool two cells the other.
    server.place_block(bx + 2, by, bz, BLOCK_CAMPFIRE_LIT);
    server.console_command("/weather clear");
    server.place_block(bx - 2, by, bz, BLOCK_WATER);

    // The ambient sample is what these assert on, and it is taken twice
    // a second -- so standing still for a moment is enough.
    client
        .stand_at(bx as f32 + 2.9, by as f32, bz as f32 + 0.5)
        .await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    let by_the_fire = current_ambient(&server);

    client
        .stand_at(bx as f32 - 1.5, by as f32, bz as f32 + 0.5)
        .await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    let in_the_water = current_ambient(&server);

    assert!(
        by_the_fire > in_the_water + 10.0,
        "a fire ({by_the_fire}) was not much warmer than a lake ({in_the_water})"
    );
    server.stop().await;
}

/// What the server currently thinks the ambient temperature is where the
/// one connected player is standing.
///
/// Read off the server's own state rather than out of a message, because
/// the *body* temperature drifts over minutes -- far too slowly for a
/// test -- while the ambient is resampled twice a second. What the
/// message carries is checked by the join test above; what this checks is
/// that the sampler is reading the world.
fn current_ambient(server: &primitive_server::Server) -> f32 {
    // `/where` is the only command that reaches a player's runtime, and
    // it does not report this -- so the reading is taken through the
    // stats the console already exposes. Nothing else in the server
    // publishes an ambient, which is why this helper exists at all.
    let reply = server.console_command("/list").join(" ");
    assert!(reply.contains("player"), "nobody is online: {reply}");
    server.player_ambient().expect("no player to sample")
}

// -------------------------------------------------------------- hides

#[tokio::test]
async fn a_hide_goes_on_a_rack_and_leather_comes_off_it() {
    // The tannery, whole, through the screen a player actually uses:
    // placing the rack, opening it, shift-clicking a skin onto the
    // frame, finishing it, and shift-clicking the leather back out.
    // Five gestures and five different pieces of wiring, and the only
    // way to know they meet is to do all five.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    let rack = (bx + 1, by, bz);
    // The hide frame: a skin's rack since the two racks split
    // (`rack::Trade`), and one cell, as every rack here was written for.
    server.place_block(rack.0, rack.1, rack.2, primitive_shared::types::BLOCK_HIDE_FRAME);

    client.settled().await;
    assert_eq!(server.give(BLOCK_HIDE, 1), 0, "the hide did not fit");
    let (_, slot) = client.holding(BLOCK_HIDE).await;

    // Opening it is what a right click on a rack now does, and the
    // answer says which screen to draw.
    client
        .send(ClientMessage::OpenChest {
            global_x: rack.0,
            global_y: rack.1,
            global_z: rack.2,
        })
        .await;
    let opened = client
        .wait_for(10, |m| match m {
            ServerMessage::ChestState { kind, rack, .. } => Some((*kind, *rack)),
            _ => None,
        })
        .await;
    let (kind, weather) = opened.expect("the rack never opened");
    assert_eq!(kind, primitive_shared::protocol::ContainerKind::Rack);
    let weather = weather.expect("a rack with no weather on it");
    assert_eq!(weather.progress, 0.0, "an empty frame was part way through something");

    // The skin onto the frame. A shift-click, which is the gesture that
    // has to be *routed* -- see the server's `rack_target`.
    client
        .send(ClientMessage::ChestQuickMove {
            side: primitive_shared::protocol::Side::Pack,
            slot: slot as u8,
        })
        .await;
    //
    // **Both halves in one wait**, because `wait_for` drops what it is
    // not looking for: waiting for the rack and then for the pack throws
    // away whichever of the two answers arrived first, and which one
    // that is depends on the tick.
    let mut on_frame = false;
    let mut out_of_pack = false;
    let laid = client
        .wait_for(10, |m| {
            match m {
                ServerMessage::ChestState { inventory, .. } => {
                    on_frame = inventory.count_in(primitive_shared::rack::HIDE_SLOT) == 1;
                }
                ServerMessage::InventoryState { inventory } => {
                    out_of_pack = inventory.count(BLOCK_HIDE) == 0;
                }
                _ => {}
            }
            (on_frame && out_of_pack).then_some(())
        })
        .await;
    assert!(
        laid.is_some(),
        "the skin did not move: on the frame {on_frame}, out of the pack {out_of_pack}"
    );

    // Curing takes twelve minutes of world time, which is not a test.
    // Pushed to the end directly -- the same door the test world uses to
    // arrive with a nearly-finished hide on one.
    server.finish_drying(rack);
    let cured = client
        .wait_for(10, |m| match m {
            ServerMessage::ChestState { inventory, .. }
                if inventory.count_in(primitive_shared::rack::LEATHER_SLOT) == 1 =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    assert!(cured.is_some(), "the skin never became leather in the tray");

    // ...and out of the tray into the pack, which must not be refused
    // because the tray is a slot nothing may be put *into*.
    client
        .send(ClientMessage::ChestQuickMove {
            side: primitive_shared::protocol::Side::Chest,
            slot: primitive_shared::rack::LEATHER_SLOT as u8,
        })
        .await;
    let taken = client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_LEATHER) == 1 => {
                Some(())
            }
            _ => None,
        })
        .await;
    assert!(taken.is_some(), "a finished rack gave back no leather");
    server.stop().await;
}

#[tokio::test]
async fn a_loaded_rack_looks_loaded_from_across_the_camp() {
    // **The one thing about a container that anybody can see without
    // opening it.** A rack is drawn as a frame with a skin stretched in
    // it or as a bare frame, and which of the two is a bit on the block
    // itself -- so a tannery reads as a tannery rather than as a row of
    // identical frames. The bit is set by the server, from the
    // container's own contents, on every gesture that can change them.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    let rack = (bx + 1, by, bz);
    server.place_block(rack.0, rack.1, rack.2, primitive_shared::types::BLOCK_HIDE_FRAME);

    client.settled().await;
    assert_eq!(server.give(BLOCK_HIDE, 1), 0, "the hide did not fit");
    let (_, slot) = client.holding(BLOCK_HIDE).await;
    client
        .send(ClientMessage::OpenChest {
            global_x: rack.0,
            global_y: rack.1,
            global_z: rack.2,
        })
        .await;
    client
        .wait_for(10, |m| matches!(m, ServerMessage::ChestState { .. }).then_some(()))
        .await
        .expect("the rack never opened");
    assert!(
        !server
            .block_at(rack.0, rack.1, rack.2)
            .is_some_and(primitive_shared::types::rack_is_loaded),
        "an empty frame was already wearing a skin"
    );

    client
        .send(ClientMessage::ChestQuickMove {
            side: primitive_shared::protocol::Side::Pack,
            slot: slot as u8,
        })
        .await;
    // The screen's answer is the acknowledgement -- the block change
    // goes out through the chunk subscriptions, which a test client that
    // has asked for no chunks is not on.
    client
        .wait_for(10, |m| match m {
            ServerMessage::ChestState { inventory, .. }
                if inventory.count_in(primitive_shared::rack::HIDE_SLOT) == 1 =>
            {
                Some(())
            }
            _ => None,
        })
        .await
        .expect("the skin never reached the frame");
    let dressed = server.block_at(rack.0, rack.1, rack.2);
    assert!(
        dressed.is_some_and(primitive_shared::types::rack_is_loaded),
        "the frame did not put the skin on: {dressed:?}"
    );
    // ...and the block is still a rack, which is what the whole
    // spare-bit trick rests on.
    assert_eq!(
        dressed.map(primitive_shared::types::block_kind),
        Some(primitive_shared::types::BLOCK_HIDE_FRAME)
    );

    // Take it back and the frame goes bare again.
    client
        .send(ClientMessage::ChestQuickMove {
            side: primitive_shared::protocol::Side::Chest,
            slot: primitive_shared::rack::HIDE_SLOT as u8,
        })
        .await;
    client
        .wait_for(10, |m| match m {
            ServerMessage::ChestState { inventory, .. }
                if inventory.count_in(primitive_shared::rack::HIDE_SLOT) == 0 =>
            {
                Some(())
            }
            _ => None,
        })
        .await
        .expect("the skin never came off the frame");
    let bare = server.block_at(rack.0, rack.1, rack.2);
    assert!(
        bare.is_some_and(|b| !primitive_shared::types::rack_is_loaded(b)),
        "the frame kept the skin after it was taken off: {bare:?}"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_rack_takes_nothing_but_a_skin() {
    // The rule the client draws and the server owns. A rack with a rock
    // wedged in its frame is a rack that never cures anything again, and
    // "the client would not offer it" is not a defence: the client is a
    // request.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    let rack = (bx + 1, by, bz);
    server.place_block(rack.0, rack.1, rack.2, primitive_shared::types::BLOCK_HIDE_FRAME);

    client.settled().await;
    assert_eq!(server.give(BLOCK_STONE, 4), 0, "the stone did not fit");
    let (_, slot) = client.holding(BLOCK_STONE).await;
    client
        .send(ClientMessage::OpenChest {
            global_x: rack.0,
            global_y: rack.1,
            global_z: rack.2,
        })
        .await;
    client
        .wait_for(10, |m| matches!(m, ServerMessage::ChestState { .. }).then_some(()))
        .await
        .expect("the rack never opened");

    // Both routes in: the shift-click, and a drag straight onto the
    // frame. The first is refused by the routing and the second by the
    // slot rules, and they are different code.
    client
        .send(ClientMessage::ChestQuickMove {
            side: primitive_shared::protocol::Side::Pack,
            slot: slot as u8,
        })
        .await;
    client
        .send(ClientMessage::ChestMove {
            from: (primitive_shared::protocol::Side::Pack, slot as u8),
            to: (
                primitive_shared::protocol::Side::Chest,
                primitive_shared::rack::HIDE_SLOT as u8,
            ),
            half: false,
        })
        .await;
    // Nothing to wait *for*: a refused gesture is silence, so the only
    // honest test is to give the server a round trip's worth of time and
    // then ask what it thinks is on the frame.
    client
        .send(ClientMessage::OpenChest {
            global_x: rack.0,
            global_y: rack.1,
            global_z: rack.2,
        })
        .await;
    let contents = client
        .wait_for(10, |m| match m {
            ServerMessage::ChestState { inventory, .. } => Some(inventory.clone()),
            _ => None,
        })
        .await
        .expect("the rack never answered");
    assert_eq!(
        contents.count_in(primitive_shared::rack::HIDE_SLOT),
        0,
        "a stone went onto the frame"
    );
    server.stop().await;
}

// ---------------------------------------------------------- equipment

#[tokio::test]
async fn a_garment_goes_on_and_comes_off_again() {
    // Both directions, because the failure mode of one direction is
    // invisible: a tunic that goes on and cannot come off is a tunic a
    // player is stuck in, and the pack it came out of has a hole where
    // it was.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    client.settled().await;
    assert_eq!(server.give(BLOCK_LEATHER_TUNIC, 1), 0, "the tunic did not fit");
    let (_, slot) = client.holding(BLOCK_LEATHER_TUNIC).await;

    client
        .send(ClientMessage::Equip { slot: slot as u8 })
        .await;
    let worn = client
        .wait_for(10, |m| match m {
            ServerMessage::EquipmentState { equipment }
                if equipment
                    .in_slot(primitive_shared::equipment::Slot::Chest)
                    .is_some() =>
            {
                Some(equipment.clone())
            }
            _ => None,
        })
        .await
        .expect("the tunic never went on");
    assert_eq!(
        worn.in_slot(primitive_shared::equipment::Slot::Chest)
            .map(|s| s.block),
        Some(BLOCK_LEATHER_TUNIC)
    );
    assert!(worn.worn().insulation > 0.0, "a worn tunic was worth nothing");

    // ...and off again, back into the pack.
    client
        .send(ClientMessage::Unequip {
            slot: primitive_shared::equipment::Slot::Chest as u8,
        })
        .await;
    let back = client
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory }
                if inventory.count(BLOCK_LEATHER_TUNIC) == 1 =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    assert!(back.is_some(), "the tunic came off and stopped existing");
    let empty = client.equipment().await;
    assert!(
        empty
            .in_slot(primitive_shared::equipment::Slot::Chest)
            .is_none(),
        "the tunic came off and is still on"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_lump_of_dirt_cannot_be_worn() {
    // The server decides what fits where, not the client -- and the
    // refusal has to be *silent and total*, not "it goes on and does
    // nothing", or a player ends up with a helmet slot full of soil.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;

    client.settled().await;
    assert_eq!(server.give(primitive_shared::types::BLOCK_DIRT, 4), 0);
    let (_, slot) = client.holding(primitive_shared::types::BLOCK_DIRT).await;

    client
        .send(ClientMessage::Equip { slot: slot as u8 })
        .await;
    // Nothing should come back at all. Give it a moment and then check
    // the server's own copy, which is the thing that would be wrong.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        server.player_equipment().is_some_and(|e| e.is_empty()),
        "a lump of dirt was accepted as a garment"
    );
    server.stop().await;
}

// ------------------------------------------------------- the test world

#[tokio::test]
async fn the_test_world_arrives_with_its_tannery_and_its_wardrobe_stocked() {
    // **The first-run pass, which is the one thing about the test world
    // that is not a pure function of a chunk position.** Racks, chests
    // and lit hearths are all *state* the server writes when it notices
    // a `Test` world that has never been stocked -- three separate
    // passes in `build_context`, each easy to leave out and none of them
    // visible to the generator's own tests.
    //
    // Needs somewhere to save to, because "has this world been stocked
    // before" is answered by whether the stores are empty, and a world
    // with no directory has empty stores by construction.
    let dir = std::env::temp_dir().join(format!(
        "primitive_testworld_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let settings = ServerSettings {
        world_dir: dir.display().to_string(),
        ..test_settings()
    };
    let server = primitive_server::start(settings, RunOptions::embedded())
        .await
        .expect("start");

    // The plaza chests, and the four in the wardrobe.
    let stocked = primitive_shared::showcase::chest_stock();
    assert!(stocked.len() > 4, "the test world stocks nothing");
    let wardrobe = stocked
        .iter()
        .rev()
        .take(4)
        .flat_map(|(_, inventory)| inventory.slots().iter().flatten().map(|s| s.block))
        .collect::<Vec<_>>();
    assert!(
        wardrobe.contains(&BLOCK_LEATHER_TUNIC),
        "the wardrobe has no leather in it: {wardrobe:?}"
    );
    assert!(
        wardrobe.contains(&primitive_shared::types::BLOCK_IRON_CUIRASS),
        "the wardrobe has no iron in it"
    );

    // ...and the two hides already curing, at two different points.
    let racks = primitive_shared::showcase::rack_stock();
    // Two hides on the tannery's frames, and a fish on the bog's rack:
    // peat is laid on the ground to dry now, not hung.
    assert_eq!(racks.len(), 3, "the tannery or the bog arrived empty");
    assert_eq!(racks.iter().filter(|(_, raw, _)| *raw == BLOCK_HIDE).count(), 2);
    assert!(racks.iter().any(|(_, raw, _)| *raw == primitive_shared::types::BLOCK_RAW_FISH));
    for (at, raw, _) in &racks {
        // The frame or rack really is one in the generated world, or what
        // is on it is filed against a cell nothing will ever find it at.
        let holder = if *raw == BLOCK_HIDE { primitive_shared::types::BLOCK_HIDE_FRAME } else { BLOCK_DRYING_RACK };
        server.place_block(at.0, at.1, at.2, holder);
    }
    // One of them is nearly done, so a player can watch one finish
    // rather than waiting twelve minutes for the first.
    assert!(
        racks.iter().any(|&(_, _, progress)| progress > 0.5),
        "every rack in the test world started from nothing"
    );
    assert!(
        racks.iter().any(|&(_, _, progress)| progress == 0.0),
        "no rack in the test world starts from the beginning"
    );

    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------- water

/// How many cells of water there are in a box.
fn water_in(
    server: &primitive_server::Server,
    from: (i32, i32, i32),
    to: (i32, i32, i32),
) -> usize {
    let mut found = 0;
    for y in from.1..=to.1 {
        for z in from.2..=to.2 {
            for x in from.0..=to.0 {
                if server
                    .block_at(x, y, z)
                    .is_some_and(primitive_shared::types::is_liquid)
                {
                    found += 1;
                }
            }
        }
    }
    found
}

#[tokio::test]
async fn cutting_a_ponds_wall_sends_water_out_over_the_ground_and_the_pond_stays() {
    // **The wiring half of the model in `primitive_shared::fluid`**,
    // which is where the explanation is. A pond is a field of *sources*,
    // and a source is not a bucket: cut the wall and water runs out over
    // the ground outside for as long as the pond is there, which is for
    // ever. The pond does not go down.
    //
    // This test used to assert the opposite -- that the pond drained to
    // nothing, because the old model conserved water and a pool you cut
    // was a pool that emptied. That model is gone; see the note at the
    // top of `fluid` for what conservation bought, what it cost, and why
    // the trade went the other way.
    //
    // None of it is something a unit test of `fluid` can check -- the
    // rules there have no world in them. This is the wiring: an edit
    // reaches the mechanic, the mechanic steps on the tick loop, and the
    // water actually moves.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");

    let ground = primitive_shared::showcase::GROUND_Y;
    // A basin: a floor, four walls, and water in the middle. Away from
    // spawn so nothing the test world built is in the way.
    let (ox, oz) = (200, 200);
    for z in 0..5 {
        for x in 0..5 {
            server.place_block(ox + x, ground, oz + z, BLOCK_STONE);
        }
    }
    for n in 0..5 {
        server.place_block(ox + n, ground + 1, oz, BLOCK_STONE);
        server.place_block(ox + n, ground + 1, oz + 4, BLOCK_STONE);
        server.place_block(ox, ground + 1, oz + n, BLOCK_STONE);
        server.place_block(ox + 4, ground + 1, oz + n, BLOCK_STONE);
    }
    for z in 1..4 {
        for x in 1..4 {
            server.place_block(ox + x, ground + 1, oz + z, BLOCK_WATER);
        }
    }
    let box_from = (ox - 12, ground, oz - 12);
    let box_to = (ox + 16, ground + 2, oz + 16);

    // It settles into a still pond and stays one.
    tokio::time::sleep(Duration::from_millis(900)).await;
    let held = water_in(&server, box_from, box_to);
    assert!(held >= 6, "the pond emptied itself before it was cut: {held}");

    // Now cut the wall and dig the ground away outside it, so there is
    // somewhere for the water to go.
    for z in 1..4 {
        server.place_block(ox + 4, ground + 1, oz + z, primitive_shared::types::BLOCK_AIR);
        for x in 5..12 {
            server.place_block(ox + x, ground, oz + z, primitive_shared::types::BLOCK_AIR);
        }
    }

    // Give it time. Water is not instant and is not meant to be: the
    // flow runs on its own interval, slower than the tick (see
    // `water::FLOW_INTERVAL`), and a channel that filled in one frame
    // would look like the water being teleported rather than running.
    let mut outside = 0;
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        outside = water_in(
            &server,
            (ox + 5, ground, oz + 1),
            (ox + 11, ground, oz + 3),
        );
        if outside > 0 {
            break;
        }
    }
    assert!(
        outside > 0,
        "nothing came out of the cut in the wall after the ground outside was dug away"
    );

    // ...and the pond is exactly the pond it was. Every cell of it is a
    // source, and nothing that runs out of a source takes anything from
    // it.
    let inside = water_in(
        &server,
        (ox + 1, ground + 1, oz + 1),
        (ox + 3, ground + 1, oz + 3),
    );
    assert_eq!(inside, 9, "the pond went down while it was running out: {inside}");
    server.stop().await;
}

// ------------------------------------------------------------- trees

#[tokio::test]
async fn a_tree_that_lands_on_you_kills_you() {
    // **The other half of felling**, and the one a player finds out
    // about the hard way: a trunk is five metres of wood, and standing
    // in the lane it is going down is fatal.
    //
    // Staged rather than hoped for: the tree is built so that the cell
    // the player is standing in is the only place it can go, which is
    // exactly the case the tie-break in `felling` deliberately does not
    // save them from.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.settled().await;

    let ground = primitive_shared::showcase::GROUND_Y;
    let (tx, tz) = (340, 340);
    const TRUNK: i32 = 6;
    for n in 0..TRUNK {
        server.place_block(tx, ground + 1 + n, tz, primitive_shared::types::BLOCK_LOG);
    }
    // Walled in on three sides at the height the trunk will lie, so
    // north is the only lane left -- and the player is standing in it.
    for &(dx, dz) in &[(1, 0), (-1, 0), (0, 1)] {
        server.place_block(tx + dx, ground + 1, tz + dz, BLOCK_STONE);
    }

    client
        .stand_at(tx as f32 + 0.5, (ground + 1) as f32, tz as f32 - 0.5)
        .await;
    // The server has to have *seen* them standing there, or the crush
    // is measured against wherever they spawned. A sleep rather than a
    // message to wait for: a transform is not acknowledged, which is the
    // whole reason it is cheap.
    tokio::time::sleep(Duration::from_millis(400)).await;

    server.place_block(tx, ground + 1, tz, primitive_shared::types::BLOCK_AIR);
    server.fell(tx, ground + 1, tz);

    let died = client
        .wait_for(10, |m| match m {
            ServerMessage::Died { cause } => Some(cause.clone()),
            _ => None,
        })
        .await;
    assert_eq!(
        died.as_deref(),
        Some("was crushed by a falling tree"),
        "the tree landed on the player and nothing happened"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_tree_that_lands_beside_you_does_not() {
    // The other side of the same rule, and the reason the box test is
    // not enough: a crush that caught the whole clearing would make
    // felling unplayable rather than dangerous.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.settled().await;

    let ground = primitive_shared::showcase::GROUND_Y;
    let (tx, tz) = (360, 360);
    for n in 0..6 {
        server.place_block(tx, ground + 1 + n, tz, primitive_shared::types::BLOCK_LOG);
    }
    // Two cells east of the stump, with the tree going north.
    client
        .stand_at(tx as f32 + 2.5, (ground + 1) as f32, tz as f32 + 0.5)
        .await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    server.place_block(tx, ground + 1, tz, primitive_shared::types::BLOCK_AIR);
    server.fell(tx, ground + 1, tz);

    let died = client
        .wait_for(2, |m| match m {
            ServerMessage::Died { cause } => Some(cause.clone()),
            _ => None,
        })
        .await;
    assert_eq!(died, None, "a tree two cells away killed somebody");
    server.stop().await;
}

#[tokio::test]
async fn cutting_the_base_of_a_tree_brings_it_down() {
    // **The wiring half of `logic::felling`**, which is a pure function
    // and cannot see any of this: that a *player's* break reaches it, that
    // what it decides is written into the world, and that the timber is
    // where a player can pick it up.
    //
    // Built rather than found, because a tree in the test world is at a
    // coordinate that moves the day the showcase is rearranged.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.settled().await;

    let ground = primitive_shared::showcase::GROUND_Y;
    let (tx, tz) = (300, 300);
    const TRUNK: i32 = 6;
    for n in 0..TRUNK {
        server.place_block(tx, ground + 1 + n, tz, primitive_shared::types::BLOCK_LOG);
    }
    // A canopy, so the leaves half is exercised too.
    for lz in -2..=2 {
        for lx in -2..=2 {
            server.place_block(
                tx + lx,
                ground + TRUNK,
                tz + lz,
                primitive_shared::types::BLOCK_LEAVES,
            );
        }
    }

    // The base, taken out the way the world takes one out.
    server.place_block(tx, ground + 1, tz, primitive_shared::types::BLOCK_AIR);
    server.fell(tx, ground + 1, tz);

    // Nothing of the trunk is left standing.
    for n in 1..TRUNK {
        let left = server.block_at(tx, ground + 1 + n, tz);
        assert!(
            left.is_some_and(primitive_shared::types::is_air),
            "the log at +{n} was left standing in the air: {left:?}"
        );
    }
    // ...and nothing of its canopy is left floating.
    let floating = (-2..=2)
        .flat_map(|lx| (-2..=2).map(move |lz| (lx, lz)))
        .filter(|&(lx, lz)| {
            server
                .block_at(tx + lx, ground + TRUNK, tz + lz)
                .is_some_and(|b| !primitive_shared::types::is_air(b))
        })
        .count();
    assert_eq!(floating, 0, "{floating} cells of canopy were left hanging");

    // What landed is the tree's own timber, lying down. **Its own**: a fall
    // does not take the bark off, so an oak on the ground is still an oak.
    // How many of the five lie rather than drop at the stump depends on the
    // room round it (a tuft in a lane stops the trunk); that every one of
    // the five comes back is `logic::felling`'s to say, and says.
    //
    // Counted by cell: the stump's own cell is on all four lanes, and was
    // counted four times when a butt landed in it.
    let mut laid = std::collections::HashSet::new();
    for step in -8..=8i32 {
        for &(dx, dz) in &[(1, 0), (0, 1), (-1, 0), (0, -1)] {
            let at = (tx + dx * step, ground + 1, tz + dz * step);
            if let Some(block) = server.block_at(at.0, at.1, at.2) {
                if primitive_shared::types::block_kind(block)
                    == primitive_shared::types::BLOCK_LOG
                {
                    laid.insert(at);
                    assert_ne!(
                        primitive_shared::types::block_axis(block),
                        primitive_shared::types::Axis::Y,
                        "a felled log landed standing up"
                    );
                }
            }
        }
    }
    assert!(!laid.is_empty(), "the tree came down and left no timber");
    assert!(
        (laid.len() as i32) < TRUNK,
        "the fall laid {} logs from the {} that stood over the cut",
        laid.len(),
        TRUNK - 1
    );
    server.stop().await;
}

#[tokio::test]
async fn a_bed_is_put_down_as_two_cells_and_broken_as_one() {
    // **Through the real placement**, because the rule lives in the
    // connection's edit path and nowhere a unit test reaches: the client
    // sends one cell, the foot, and the server has to find the second cell
    // behind it, check it, spend one bed and write both -- and a swing at
    // either half has to take both away.
    use primitive_shared::types::{bed_half, bed_partner, Facing, BLOCK_AIR, BLOCK_BED};

    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.settled().await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    // Facing the player, who stands to the west: the head is the cell
    // beyond the foot, further east. Floor and air laid down so whatever the
    // test world keeps by the spawn cannot decide the test.
    let foot = bed_half(Facing::West, false);
    let foot_at = (bx + 2, by, bz);
    let (head_at, head) = bed_partner(foot_at, foot).expect("a bed has a partner");
    for (x, z) in [(foot_at.0, foot_at.2), (head_at.0, head_at.2)] {
        server.place_block(x, by - 1, z, BLOCK_STONE);
        server.place_block(x, by, z, BLOCK_AIR);
        server.place_block(x, by + 1, z, BLOCK_AIR);
    }

    assert_eq!(server.give(BLOCK_BED, 1), 0, "the bed did not fit");
    let (_, slot) = client.holding(BLOCK_BED).await;
    client.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;
    client
        .send(ClientMessage::SetBlock {
            global_x: foot_at.0,
            global_y: foot_at.1,
            global_z: foot_at.2,
            block_id: foot,
        })
        .await;
    // The world is read off the server rather than waited for as block
    // updates: this client has asked for no chunks, so nobody sends it any.
    // Whatever the server refused with is kept for the failure message --
    // a bed that did not go down for a *reason* is a different bug from one
    // that went down as half a bed.
    let mut refusals = Vec::new();
    let settled = |want: [((i32, i32, i32), primitive_shared::types::BlockId); 2]| {
        want.iter()
            .all(|&(at, block)| server.block_at(at.0, at.1, at.2) == Some(block))
    };
    // A second at a time, read off the socket: long enough for a tick to
    // have written the bed, and whatever arrives meanwhile is searched for
    // a refusal.
    for _ in 0..10 {
        if settled([(foot_at, foot), (head_at, head)]) {
            break;
        }
        if let Some(text) = client
            .wait_for(1, |m| match m {
                ServerMessage::Error(text) => Some(text.clone()),
                _ => None,
            })
            .await
        {
            refusals.push(text);
        }
    }
    assert_eq!(
        (server.block_at(foot_at.0, foot_at.1, foot_at.2), server.block_at(head_at.0, head_at.1, head_at.2)),
        (Some(foot), Some(head)),
        "placing the foot did not put a whole bed down; the server said {refusals:?}"
    );

    // A swing at the head takes the foot as well.
    client
        .send(ClientMessage::SetBlock {
            global_x: head_at.0,
            global_y: head_at.1,
            global_z: head_at.2,
            block_id: BLOCK_AIR,
        })
        .await;
    for _ in 0..50 {
        if settled([(foot_at, BLOCK_AIR), (head_at, BLOCK_AIR)]) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        server.block_at(foot_at.0, foot_at.1, foot_at.2),
        Some(BLOCK_AIR),
        "breaking the head of a bed left its foot standing"
    );
    assert_eq!(server.block_at(head_at.0, head_at.1, head_at.2), Some(BLOCK_AIR));
    server.stop().await;
}

#[tokio::test]
async fn a_door_is_hung_as_two_cells_swung_as_one_and_broken_as_one() {
    // **Through the real placement and the real use**, for the bed's reason:
    // the rules live in the connection's edit path and in `use_block`. The
    // client sends the lower half; the server refuses it with nothing over it
    // for the top and with nothing under it to stand on, writes both halves
    // when there is room, swings both on a right click at either half, and
    // takes both away for a swing at the top -- giving one door.
    use primitive_shared::types::{door_partner, door_swung, faced, Facing, BLOCK_AIR, BLOCK_DOOR};

    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut client = Client::connect(&address).await;
    client.settled().await;

    let (sx, sy, sz) = client.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    let lower = faced(BLOCK_DOOR, Facing::West);
    let at = (bx + 2, by, bz);
    let (top_at, top) = door_partner(at, lower).expect("a door has a top");
    server.place_block(at.0, by - 1, at.2, BLOCK_STONE);
    server.place_block(at.0, by, at.2, BLOCK_AIR);
    // Something in the way of the top half first: a lintel, on two jambs,
    // because a stone with nothing under it does not stay up to be in the
    // way of anything.
    for dz in [-1, 1] {
        for y in by..=top_at.1 {
            server.place_block(at.0, y, at.2 + dz, BLOCK_STONE);
        }
    }
    server.place_block(top_at.0, top_at.1, top_at.2, BLOCK_STONE);

    assert_eq!(server.give(BLOCK_DOOR, 1), 0, "the door did not fit");
    let (_, slot) = client.holding(BLOCK_DOOR).await;
    client.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;
    let hang = ClientMessage::SetBlock { global_x: at.0, global_y: at.1, global_z: at.2, block_id: lower };
    client.send(hang.clone()).await;
    // Thirty seconds, not five: under a full test run the refusal is queued
    // behind every chunk and snapshot this socket carries.
    let refusal = client
        .wait_for(30, |m| match m {
            ServerMessage::Error(text) => Some(text.clone()),
            _ => None,
        })
        .await;
    assert!(
        refusal.as_deref().is_some_and(|text| text.contains("door")),
        "a door under a stone was not refused for its top half: {refusal:?}"
    );
    assert_eq!(server.block_at(at.0, at.1, at.2), Some(BLOCK_AIR), "half a door was hung under a stone");

    // ...and nothing to stand on is refused as well.
    server.place_block(top_at.0, top_at.1, top_at.2, BLOCK_AIR);
    server.place_block(at.0, by - 1, at.2, BLOCK_AIR);
    client.send(hang.clone()).await;
    let refusal = client
        .wait_for(30, |m| match m {
            ServerMessage::Error(text) => Some(text.clone()),
            _ => None,
        })
        .await;
    assert!(refusal.is_some(), "a door was hung on air without a word");
    assert_eq!(server.block_at(at.0, at.1, at.2), Some(BLOCK_AIR), "a door was hung on air");

    // With a floor and a free cell over it, both halves go down.
    server.place_block(at.0, by - 1, at.2, BLOCK_STONE);
    client.send(hang).await;
    let settled = |want: [((i32, i32, i32), primitive_shared::types::BlockId); 2]| {
        want.iter().all(|&(cell, block)| server.block_at(cell.0, cell.1, cell.2) == Some(block))
    };
    for _ in 0..50 {
        if settled([(at, lower), (top_at, top)]) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        (server.block_at(at.0, at.1, at.2), server.block_at(top_at.0, top_at.1, top_at.2)),
        (Some(lower), Some(top)),
        "hanging the lower half did not hang a whole door"
    );

    // A right click at the top swings both halves; one at the bottom swings
    // them back.
    for (clicked, want) in [(top_at, true), (at, false)] {
        client
            .send(ClientMessage::UseBlock { global_x: clicked.0, global_y: clicked.1, global_z: clicked.2 })
            .await;
        let (want_lower, want_top) = if want { (door_swung(lower), door_swung(top)) } else { (lower, top) };
        for _ in 0..50 {
            if settled([(at, want_lower), (top_at, want_top)]) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(
            (server.block_at(at.0, at.1, at.2), server.block_at(top_at.0, top_at.1, top_at.2)),
            (Some(want_lower), Some(want_top)),
            "a click at {clicked:?} did not swing the whole door {}",
            if want { "open" } else { "shut" }
        );
    }

    // A swing at the top takes the bottom as well.
    client
        .send(ClientMessage::SetBlock { global_x: top_at.0, global_y: top_at.1, global_z: top_at.2, block_id: BLOCK_AIR })
        .await;
    for _ in 0..50 {
        if settled([(at, BLOCK_AIR), (top_at, BLOCK_AIR)]) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(server.block_at(at.0, at.1, at.2), Some(BLOCK_AIR), "breaking the top of a door left its bottom hanging");
    assert_eq!(server.block_at(top_at.0, top_at.1, top_at.2), Some(BLOCK_AIR));
    server.stop().await;
}

#[tokio::test]
async fn a_chair_is_sat_in_facing_its_way_seen_by_everyone_and_let_go_of_by_every_way_out() {
    // **Every party that has to agree about a seat, over the real socket.**
    // The sitter's own client is told where the body went and which way it
    // faces; a player already there and a player who joins afterwards both
    // see the figure seated in the chair, turned the way the chair is, even
    // while the sitter looks round; a second body is refused; and standing
    // up, the chair being broken and dying each let go of it. Each of those
    // was a separate path through the server, and a seat that stayed claimed
    // after any one of them is a chair nobody on the server can use again.
    use primitive_shared::protocol::Posture;
    use primitive_shared::types::{faced, seat_yaw, Facing, BLOCK_AIR, BLOCK_CHAIR};

    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut sitter = Client::connect_as(&address, "sitter").await;
    sitter.settled().await;

    let (sx, sy, sz) = sitter.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    // Two cells east of the spawn, facing west -- back to the east, seat
    // toward the players at the spawn. A floor and headroom are laid so
    // whatever the test world keeps here cannot decide the test.
    let at = (bx + 2, by, bz);
    let chair = faced(BLOCK_CHAIR, Facing::West);
    let facing = seat_yaw(chair).expect("a chair faces a way");
    let put_chair = |server: &primitive_server::Server| {
        server.place_block(at.0, at.1 - 1, at.2, BLOCK_STONE);
        server.place_block(at.0, at.1, at.2, chair);
        server.place_block(at.0, at.1 + 1, at.2, BLOCK_AIR);
        server.place_block(at.0, at.1 + 2, at.2, BLOCK_AIR);
    };
    put_chair(&server);
    let use_chair = ClientMessage::UseBlock {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
    };
    let same_yaw = |a: f32, b: f32| (a - b).rem_euclid(std::f32::consts::TAU).min((b - a).rem_euclid(std::f32::consts::TAU)) < 1e-3;
    let sat = |m: &ServerMessage| match m {
        ServerMessage::Posture { posture: Posture::Sitting, at, yaw } => Some((*at, *yaw)),
        _ => None,
    };
    let stood = |m: &ServerMessage| matches!(m, ServerMessage::Posture { posture: Posture::Standing, .. }).then_some(());

    // ---- sitting down, and what the sitter is told ----
    sitter.send(use_chair.clone()).await;
    let (seat, yaw) = sitter.wait_for(10, sat).await.expect("a click on a chair did not sit the player in it");
    let seat = seat.expect("sitting down said nothing about where the body went");
    assert!(same_yaw(yaw, facing), "sat in a west-facing chair facing {yaw}, not {facing}");
    assert!(
        (seat.0 - f64::from(at.0 as f32 + 0.5)).abs() < 0.2
            && (seat.2 - f64::from(at.2 as f32 + 0.5)).abs() < 0.2
            && (seat.1 - f64::from(at.1 as f32 + 0.5)).abs() < 1e-3,
        "the body was put at {seat:?}, not on the seat of the chair at {at:?}"
    );
    // The sitter looks round. Their transforms are still taken -- sitting
    // is not a lock -- and none of this may turn the figure in the chair.
    sitter
        .send(ClientMessage::UpdateTransform {
            x: seat.0,
            y: seat.1,
            z: seat.2,
            yaw: facing + 1.3,
            pitch: 0.2,
            on_ground: true,
            sequence: 1,
        })
        .await;

    // ---- a player who joins afterwards sees them seated ----
    let mut watcher = Client::connect_as(&address, "watcher").await;
    watcher.settled().await;
    let sitter_id = sitter.id;
    let seen = |posture: Posture| {
        move |m: &ServerMessage| match m {
            ServerMessage::Snapshot { states, .. } => states
                .iter()
                .find(|s| s.id == sitter_id && s.posture == posture)
                .map(|s| (s.x, s.y, s.z, s.yaw)),
            _ => None,
        }
    };
    let (x, y, z, drawn_yaw) = watcher
        .wait_for(10, seen(Posture::Sitting))
        .await
        .expect("a player who joined later never saw the sitter seated");
    assert!(same_yaw(drawn_yaw, facing), "the figure in the chair is drawn facing {drawn_yaw}, not the chair's {facing}");
    assert!(
        (x - seat.0).abs() < 1e-3 && (y - seat.1).abs() < 1e-3 && (z - seat.2).abs() < 1e-3,
        "the seated figure is drawn at ({x}, {y}, {z}), not on the seat at {seat:?}"
    );

    // ---- one body to a chair ----
    watcher.send(use_chair.clone()).await;
    let refusal = watcher
        .wait_for(5, |m| match m {
            ServerMessage::Error(text) => Some(text.clone()),
            ServerMessage::Notice { what } => Some(format!("{what:?}")),
            ServerMessage::Posture { posture: Posture::Sitting, .. } => Some("sat down".to_string()),
            _ => None,
        })
        .await;
    assert!(
        refusal.as_deref() == Some("SeatTaken"),
        "a second player clicking an occupied chair got {refusal:?}"
    );

    // ---- standing up lets go ----
    sitter.send(ClientMessage::StandUp).await;
    sitter.wait_for(10, stood).await.expect("asking to stand up did not stand the sitter up");
    watcher
        .wait_for(10, seen(Posture::Standing))
        .await
        .expect("the watcher still sees the sitter seated after they stood up");

    // ---- the chair broken under them lets go ----
    sitter.send(use_chair.clone()).await;
    sitter.wait_for(10, sat).await.expect("could not sit down again");
    server.place_block(at.0, at.1, at.2, BLOCK_AIR);
    sitter.wait_for(10, stood).await.expect("breaking the chair left its sitter seated on nothing");
    watcher
        .wait_for(10, seen(Posture::Standing))
        .await
        .expect("the watcher still sees a figure seated where the chair was");

    // ---- dying lets go, at the death and not at the respawn ----
    put_chair(&server);
    sitter.send(use_chair.clone()).await;
    sitter.wait_for(10, sat).await.expect("could not sit in the replaced chair");
    let mut dead = false;
    // Fourteen bare-handed blows at the fist's cooldown: see
    // `combat::MELEE_DAMAGE`. A second a swing keeps well clear of it.
    for _ in 0..40 {
        watcher.send(ClientMessage::Attack { target: sitter_id }).await;
        if sitter
            .wait_for(1, |m| matches!(m, ServerMessage::Died { .. }).then_some(()))
            .await
            .is_some()
        {
            dead = true;
            break;
        }
    }
    assert!(dead, "forty blows did not kill the sitter; the rest of this test needs a death");
    // The sitter does not respawn. The chair must be free already -- and
    // the body out of it: a dead player is drawn fallen now
    // (`Posture::Fallen`), where this used to wait for them standing, which
    // was the statue on everybody else's screen.
    watcher
        .wait_for(10, seen(Posture::Fallen))
        .await
        .expect("a dead player is still drawn seated in the chair");
    watcher.send(use_chair).await;
    watcher
        .wait_for(10, sat)
        .await
        .expect("the chair a player died in still refuses everyone else");
    server.stop().await;
}

#[tokio::test]
async fn the_night_passes_behind_a_dark_screen_and_morning_finds_the_sleeper_still_in_bed() {
    // **The report, over the real socket**: "при сне игрок сразу встает и
    // не засыпает". A lone player lay down, the night passed on the next
    // tick and the same function stood them up -- so the client heard
    // "asleep", "awake" and "standing" inside a twentieth of a second, and
    // the sun jumped in front of eyes that had never closed.
    //
    // What has to hold instead, in the order the client needs it: asleep;
    // no new hour until the fade has had its time; the new hour *before*
    // the waking, so the dark lifts on a morning sky; no standing up at
    // dawn; and standing up the moment the player asks. On a straw pallet,
    // so its two cells are exercised by the same lying down.
    use primitive_shared::body::FALLING_ASLEEP_SECONDS;
    use primitive_shared::protocol::Posture;
    use primitive_shared::types::{bed_half_of, bed_partner, Facing, BLOCK_AIR, BLOCK_STRAW_BED};

    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut sleeper = Client::connect(&address).await;
    sleeper.settled().await;
    // Dusk, so there is a night to pass and no ordinary sync can be taken
    // for the morning.
    server.console_command("/time 0.8");

    let (sx, sy, sz) = sleeper.spawn;
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    let foot = bed_half_of(BLOCK_STRAW_BED, Facing::West, false);
    let foot_at = (bx + 2, by, bz);
    let (head_at, head) = bed_partner(foot_at, foot).expect("a pallet has a partner");
    for (at, block) in [(foot_at, foot), (head_at, head)] {
        server.place_block(at.0, by - 1, at.2, BLOCK_STONE);
        server.place_block(at.0, by + 1, at.2, BLOCK_AIR);
        server.place_block(at.0, by + 2, at.2, BLOCK_AIR);
        server.place_block(at.0, at.1, at.2, block);
    }

    sleeper
        .send(ClientMessage::UseBlock {
            global_x: foot_at.0,
            global_y: foot_at.1,
            global_z: foot_at.2,
        })
        .await;
    let mut refused = None;
    sleeper
        .wait_for(10, |m| match m {
            ServerMessage::Asleep { asleep: true } => Some(()),
            ServerMessage::Error(text) => {
                refused = Some(text.clone());
                None
            }
            _ => None,
        })
        .await
        .unwrap_or_else(|| panic!("lying down on the pallet never fell asleep; the server said {refused:?}"));
    let eyes_closed = tokio::time::Instant::now();

    // Everything up to the morning, watching for the two ways it used to go.
    let (mut stood, mut woke) = (false, false);
    let morning = sleeper
        .wait_for(20, |m| match m {
            ServerMessage::Posture { posture: Posture::Standing, .. } => {
                stood = true;
                None
            }
            ServerMessage::Asleep { asleep: false } => {
                woke = true;
                None
            }
            ServerMessage::TimeSync { time_of_day, .. } if (time_of_day - 0.25).abs() < 0.01 => {
                Some(tokio::time::Instant::now())
            }
            _ => None,
        })
        .await
        .expect("the night never passed for a lone sleeper");
    assert!(!stood, "the sleeper was stood up before the morning came");
    assert!(!woke, "the sleeper was woken before the new hour was sent, so the dark lifts on a night sky");
    let dark_for = morning.duration_since(eyes_closed).as_secs_f32();
    assert!(
        dark_for >= FALLING_ASLEEP_SECONDS,
        "the night passed {dark_for:.2} s after the eyes closed, before a {FALLING_ASLEEP_SECONDS} s fade could hide it"
    );

    // The morning: awake, and still lying there.
    let (mut woke, mut stood) = (false, false);
    let _ = sleeper
        .wait_for(2, |m| {
            match m {
                ServerMessage::Asleep { asleep: false } => woke = true,
                ServerMessage::Posture { posture: Posture::Standing, .. } => stood = true,
                _ => {}
            }
            None::<()>
        })
        .await;
    assert!(woke, "morning came and the sleeper was never told they are awake");
    assert!(!stood, "morning stood the sleeper up -- they get up when they choose");

    // ...and getting up is theirs to ask for.
    sleeper.send(ClientMessage::StandUp).await;
    sleeper
        .wait_for(10, |m| matches!(m, ServerMessage::Posture { posture: Posture::Standing, .. }).then_some(()))
        .await
        .expect("asking to stand up in the morning did not get the sleeper out of bed");
    server.stop().await;
}

#[tokio::test]
async fn a_look_that_is_not_a_number_never_reaches_anybody_elses_screen() {
    // The anti-cheat judges where a body is and not which way it faces, so
    // a modified client's NaN yaw was stored as it came and handed to every
    // other player's snapshot: a head with no direction, and the aim of
    // everything that player threw. The bad look comes with a small step,
    // so the watcher can tell the message was applied rather than dropped.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut looker = Client::connect_as(&address, "looker").await;
    looker.settled().await;
    let mut watcher = Client::connect_as(&address, "watcher").await;
    watcher.settled().await;

    let (x, y, z) = looker.spawn;
    let looks = [(0.0, 0.7, 0.2), (0.25, f32::NAN, f32::INFINITY)];
    for (sequence, (step, yaw, pitch)) in looks.into_iter().enumerate() {
        looker
            .send(ClientMessage::UpdateTransform {
                x: f64::from(x + step),
                y: f64::from(y),
                z: f64::from(z),
                yaw,
                pitch,
                on_ground: true,
                sequence: sequence as u32 + 1,
            })
            .await;
    }
    let looker_id = looker.id;
    let (yaw, pitch) = watcher
        .wait_for(10, |m| match m {
            ServerMessage::Snapshot { states, .. } => states
                .iter()
                .find(|s| s.id == looker_id && (s.x - f64::from(x + 0.25)).abs() < 1e-3)
                .map(|s| (s.yaw, s.pitch)),
            _ => None,
        })
        .await
        .expect("the watcher never saw the step that came with the bad look");
    assert!(
        (yaw - 0.7).abs() < 1e-6 && (pitch - 0.2).abs() < 1e-6,
        "a look of NaN and infinity reached another player as yaw {yaw}, pitch {pitch}, not the last real one"
    );
    server.stop().await;
}
