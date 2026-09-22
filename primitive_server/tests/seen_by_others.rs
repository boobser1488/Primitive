//! What one player does, seen by another -- end to end over real sockets.
//!
//! Written for "частицы тоже не синхронизированы" and "не видно как игрок
//! ломает": a player breaking a block was a figure standing still until the
//! block vanished, and a player struck by another bled only on their own
//! screen. The rules each have unit tests; these are the **wiring**, and the
//! failures they are here for are the ones no unit test can see:
//!
//! - an event the server decides and sends only to the player who caused it,
//!   so everybody else's picture of the world is quieter than the world;
//! - a gesture the server accepts and never puts in the snapshot, so the
//!   figure on the other screen never moves its arm;
//! - blood sent for something that is not a wound -- the fish that became a
//!   fountain.
//!
//! **Nothing here is predicted and echoed**: the player who struck, broke or
//! ate is told by the same message everybody else is, and the tests check the
//! actor receives it as well as the witness. See `ServerMessage::Blood` for why
//! that is the arrangement and not prediction plus an echo to everyone else.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{
    Action, ClientMessage, PlayerId, PlayerState, ServerMessage, PROTOCOL_VERSION,
};
use primitive_shared::types::{ChunkPos, BLOCK_AIR, BLOCK_DIRT, BLOCK_RAW_FISH};
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
    id: PlayerId,
    spawn: (f32, f32, f32),
}

impl Client {
    /// A join under a name of its own: two sessions under one name are one
    /// person logging in twice.
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
        let (id, spawn) = match read_message::<_, ServerMessage>(&mut socket).await.expect("welcome") {
            ServerMessage::Welcome { your_id, spawn, .. } => (your_id, spawn),
            other => panic!("expected Welcome, got {other:?}"),
        };
        Self { socket, id, spawn: primitive_shared::geometry::narrow(spawn) }
    }

    async fn send(&mut self, message: ClientMessage) {
        write_message(&mut self.socket, &message).await.expect("send");
    }

    /// Reads until a message the predicate likes shows up. Chunks, snapshots
    /// and keepalives share the socket, so the message under test is never
    /// simply the next one -- see `tests/body.rs` for the bug that caught.
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

    /// The opening inventory: the signal that the player exists on the
    /// server's side.
    async fn settled(&mut self) {
        self.wait_for(10, |m| matches!(m, ServerMessage::InventoryState { .. }).then_some(()))
            .await
            .expect("the server never sent an opening inventory");
    }

    /// Asks for a chunk and waits for it, which is what puts this player on
    /// the list of people told about changes inside it.
    async fn load_chunk(&mut self, pos: ChunkPos) {
        self.send(ClientMessage::RequestChunk(pos)).await;
        self.wait_for(10, |m| match m {
            ServerMessage::ChunkData(chunk) if chunk.pos == pos => Some(()),
            _ => None,
        })
        .await
        .expect("the chunk never came");
    }

    /// The next snapshot in which `who` satisfies `want`.
    async fn sees(&mut self, who: PlayerId, seconds: u64, want: impl Fn(&PlayerState) -> bool) -> Option<PlayerState> {
        self.wait_for(seconds, |m| match m {
            ServerMessage::Snapshot { states, .. } => {
                states.iter().find(|state| state.id == who && want(state)).copied()
            }
            _ => None,
        })
        .await
    }
}

fn cell_of(at: (f32, f32, f32)) -> (i32, i32, i32) {
    (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32)
}

#[tokio::test]
async fn a_block_one_player_breaks_comes_apart_on_the_other_players_screen_too() {
    // **The break is the event both screens draw chips from** (`burst_for` in
    // the client reads the block the change replaced). So what has to be true
    // is that the witness is sent the change at all -- and so is the player
    // who broke it, because nothing on their side predicted the chips either.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut breaker = Client::connect_as(&address, "breaker").await;
    let mut witness = Client::connect_as(&address, "witness").await;
    breaker.settled().await;
    witness.settled().await;

    let (bx, by, bz) = cell_of(breaker.spawn);
    let target = (bx + 2, by, bz);
    let (chunk, _, _) = ChunkPos::from_global(target.0, target.2);
    breaker.load_chunk(chunk).await;
    witness.load_chunk(chunk).await;
    server.place_block(target.0, target.1, target.2, BLOCK_DIRT);

    breaker
        .send(ClientMessage::SetBlock {
            global_x: target.0,
            global_y: target.1,
            global_z: target.2,
            block_id: BLOCK_AIR,
        })
        .await;
    let broken = |m: &ServerMessage| match m {
        ServerMessage::BlockUpdate(change)
            if (change.global_x, change.global_y, change.global_z) == target
                && change.block_id == BLOCK_AIR =>
        {
            Some(())
        }
        _ => None,
    };
    witness
        .wait_for(10, broken)
        .await
        .expect("the other player was never told the block came apart");
    breaker
        .wait_for(10, broken)
        .await
        .expect("the player who broke it was never told, and draws no chips");
    server.stop().await;
}

#[tokio::test]
async fn a_player_digging_is_seen_digging_by_somebody_else_and_stops() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut digger = Client::connect_as(&address, "digger").await;
    let mut witness = Client::connect_as(&address, "witness").await;
    digger.settled().await;
    witness.settled().await;

    digger.send(ClientMessage::Digging { digging: true }).await;
    witness
        .sees(digger.id, 10, |state| state.gesture.digging)
        .await
        .expect("somebody digging was never seen digging");

    digger.send(ClientMessage::Digging { digging: false }).await;
    witness
        .sees(digger.id, 10, |state| !state.gesture.digging)
        .await
        .expect("somebody who stopped digging went on swinging on the other screen");
    server.stop().await;
}

#[tokio::test]
async fn a_blow_between_two_players_is_seen_struck_and_bleeds_on_both_screens() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut striker = Client::connect_as(&address, "striker").await;
    let mut struck = Client::connect_as(&address, "struck").await;
    striker.settled().await;
    struck.settled().await;
    let before = struck
        .sees(striker.id, 10, |_| true)
        .await
        .expect("the two players never saw each other")
        .gesture;

    striker.send(ClientMessage::Attack { target: struck.id }).await;

    // **Both off the one socket, in whichever order they come.** The blow's
    // snapshot and its blood leave on the same tick, and a reader that waited
    // for the snapshot first threw the blood away with the chunks and the
    // keepalives -- which is how this test first failed, against a server
    // that had sent both.
    let spawn = struck.spawn;
    let near = move |at: (f32, f32, f32)| {
        let (dx, dy, dz) = (at.0 - spawn.0, at.1 - spawn.1, at.2 - spawn.2);
        dx * dx + dy * dy + dz * dz < 9.0
    };
    let striker_id = striker.id;
    let (mut seen, mut bled) = (None, None);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while (seen.is_none() || bled.is_none()) && tokio::time::Instant::now() < deadline {
        let next = struck
            .wait_for(1, |m| match m {
                ServerMessage::Snapshot { states, .. } => states
                    .iter()
                    .find(|state| state.id == striker_id && state.gesture.count != before.count)
                    .map(|state| Ok(state.gesture)),
                ServerMessage::Blood { at, drops } if *drops > 0 && near(primitive_shared::geometry::narrow(*at)) => Some(Err(*drops)),
                _ => None,
            })
            .await;
        match next {
            Some(Ok(gesture)) => seen = Some(gesture),
            Some(Err(drops)) => bled = Some(drops),
            None => {}
        }
    }
    let after = seen.expect("a blow was struck and never seen");
    assert_eq!(after.last, Action::Strike, "the blow was seen as {:?}", after.last);
    assert!(bled.is_some(), "the player struck never saw their own blood");
    striker
        .wait_for(10, |m| match m {
            ServerMessage::Blood { at, drops } if *drops > 0 && near(primitive_shared::geometry::narrow(*at)) => Some(()),
            _ => None,
        })
        .await
        .expect("the player who struck the blow never saw it bleed");
    server.stop().await;
}

#[tokio::test]
async fn a_raw_fish_makes_a_player_ill_and_sheds_no_blood() {
    // **The report, end to end**: eat a raw fish, lose health to the
    // illness, and see no blood -- not the spray a blow throws, and not the
    // drip of a cut, because an illness is neither.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let mut eater = Client::connect_as(&server.address().to_string(), "eater").await;
    eater.settled().await;
    // Hungry enough for a fish to be worth eating: a mouthful spent on a
    // full stomach is refused as an item destroyed.
    server.set_player_nourishment(primitive_shared::food::MAX_NOURISHMENT / 2.0);
    assert_eq!(server.give(BLOCK_RAW_FISH, 1), 0, "the fish did not fit");
    let slot = eater
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_RAW_FISH) > 0 => {
                (0..primitive_shared::inventory::SLOTS).find(|&s| inventory.block_in(s) == Some(BLOCK_RAW_FISH))
            }
            _ => None,
        })
        .await
        .expect("the fish never reached the pack");
    eater.send(ClientMessage::Eat { slot: slot as u8 }).await;
    // Gone down, and then the two minutes of digestion skipped: this test
    // is about the illness, not the wait (`body::DIGESTION_SECONDS`).
    eater
        .wait_for(10, |m| match m {
            ServerMessage::InventoryState { inventory } if inventory.count(BLOCK_RAW_FISH) == 0 => Some(()),
            _ => None,
        })
        .await
        .expect("the fish was never eaten");
    server.digest_player_meal();

    // Until the illness has visibly cost something -- and not one drop of
    // blood on the way there, or for a few seconds after.
    let mut ill = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        let seen = eater
            .wait_for(1, |m| match m {
                ServerMessage::Blood { drops, .. } => Some(Err(*drops)),
                ServerMessage::Health { current, max } if current < max => Some(Ok(())),
                _ => None,
            })
            .await;
        match seen {
            Some(Err(drops)) => panic!("eating a raw fish shed {drops} drops of blood"),
            Some(Ok(())) => ill = true,
            None => {}
        }
    }
    assert!(ill, "a raw fish cost no health in eight seconds");
    let injuries = server.player_injuries().expect("a player");
    assert!(injuries.is_whole(), "a raw fish left a wound: {injuries:?}");
    server.stop().await;
}

#[tokio::test]
async fn a_player_who_dies_is_seen_fallen_and_their_body_wears_what_they_wore() {
    // **The report, end to end**: "при смерти тело игрока всё ещё стоит, у
    // трупа нет текстуры игрока". A dead player was a standing figure in every
    // snapshot, and the body their death left told nobody what was on it. So
    // the witness has to see two things over the wire: the dead player's
    // posture change, and -- when it asks for the chunk the body lies in --
    // what that body is wearing, right behind the chunk.
    use primitive_shared::protocol::Posture;
    use primitive_shared::types::BLOCK_IRON_HELM;
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut victim = Client::connect_as(&address, "victim").await;
    victim.settled().await;
    // Given before the witness joins: `give` hands to the one player there is.
    assert_eq!(server.give(BLOCK_IRON_HELM, 1), 0);
    let slot = server
        .player_inventory()
        .and_then(|pack| (0..primitive_shared::inventory::SLOTS).find(|&s| pack.block_in(s) == Some(BLOCK_IRON_HELM)))
        .expect("the helmet went nowhere");
    victim.send(ClientMessage::Equip { slot: slot as u8 }).await;
    victim
        .wait_for(10, |m| match m {
            ServerMessage::EquipmentState { equipment } if !equipment.is_empty() => Some(()),
            _ => None,
        })
        .await
        .expect("the helmet was never put on");

    let mut witness = Client::connect_as(&address, "witness").await;
    witness.settled().await;
    witness
        .sees(victim.id, 10, |state| state.posture == Posture::Standing)
        .await
        .expect("the two players never saw each other");

    // Off the top of the world and down onto the spawn.
    let (x, y, z) = victim.spawn;
    let top = primitive_shared::types::CHUNK_SIZE_Y as f32 - 1.0;
    for (sequence, (height, on_ground)) in [(top, false), (top * 0.5, false), (y, true)].into_iter().enumerate() {
        victim
            .send(ClientMessage::UpdateTransform { x: f64::from(x), y: f64::from(height), z: f64::from(z), yaw: 0.0, pitch: 0.0, on_ground, sequence: sequence as u32 + 1 })
            .await;
    }
    victim
        .wait_for(10, |m| matches!(m, ServerMessage::Died { .. }).then_some(()))
        .await
        .expect("the fall was not fatal");
    witness
        .sees(victim.id, 10, |state| state.posture == Posture::Fallen)
        .await
        .expect("a dead player went on standing on the other screen");

    let (chunk, _, _) = ChunkPos::from_global(x.floor() as i32, z.floor() as i32);
    witness.send(ClientMessage::RequestChunk(chunk)).await;
    let worn = witness
        .wait_for(10, |m| match m {
            ServerMessage::BodyWorn { x: bx, z: bz, worn, .. } if (*bx, *bz) == (x.floor() as i32, z.floor() as i32) => {
                Some(*worn)
            }
            _ => None,
        })
        .await
        .expect("the body's clothes never came with its chunk");
    assert_eq!(worn[primitive_shared::equipment::Slot::Head.index()], BLOCK_IRON_HELM, "the body is bare: {worn:?}");
    server.stop().await;
}

/// Sends a transform with the next sequence number of its own.
async fn stand_at(client: &mut Client, sequence: &mut u32, (x, y, z): (f64, f64, f64)) {
    *sequence += 1;
    client
        .send(ClientMessage::UpdateTransform { x, y, z, yaw: 0.0, pitch: 0.0, on_ground: true, sequence: *sequence })
        .await;
}

/// **Somebody walking a long way from zero is seen where they are.** Every
/// position on the wire was an `f32`, and so was the server's copy of the
/// player: a million blocks out a stride of a centimetre and a bit was
/// relayed as nothing or as a sixteenth, and ten million out as nothing or a
/// whole block -- a player walking beside you was drawn standing still and
/// then teleporting. Each step has to arrive on the other screen to the bit.
#[tokio::test]
async fn a_player_walking_far_from_zero_is_seen_exactly_where_they_are() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut walker = Client::connect_as(&address, "walker").await;
    let mut witness = Client::connect_as(&address, "witness").await;
    walker.settled().await;
    witness.settled().await;
    let y = f64::from(walker.spawn.1);
    let (mut walked, mut watched) = (0, 0);
    for far in [1_000_000.0f64, -10_000_000.0] {
        stand_at(&mut witness, &mut watched, (far + 3.0, y, far + 0.5)).await;
        for step in 0..8 {
            let x = far + 0.3 + 0.013 * f64::from(step);
            stand_at(&mut walker, &mut walked, (x, y, far + 0.5)).await;
            witness
                .sees(walker.id, 10, |state| state.x == x && state.z == far + 0.5)
                .await
                .unwrap_or_else(|| panic!("{far} out, step {step}: the walker was never seen at x = {x}"));
        }
    }
    server.stop().await;
}

/// **A newcomer learns the names of everybody already there.** A name used to
/// travel only in `PlayerJoined`, sent to everybody else as a player arrived,
/// so whoever came last knew nobody -- and when one of them left, the chat of
/// the newcomer said nothing, because a leaver with no name cannot be named.
#[tokio::test]
async fn somebody_who_joins_is_told_who_is_already_here() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut first = Client::connect_as(&address, "first").await;
    first.settled().await;
    let mut second = Client::connect_as(&address, "second").await;
    let first_id = first.id;
    second
        .wait_for(10, |m| match m {
            ServerMessage::PlayerPresent { id, username } if *id == first_id => Some(username.clone()),
            _ => None,
        })
        .await
        .map(|name| assert_eq!(name, "first"))
        .expect("the newcomer was never told who was already here");
    // ...and the one already here is told somebody *joined*, as before.
    let second_id = second.id;
    first
        .wait_for(10, |m| matches!(m, ServerMessage::PlayerJoined { id, .. } if *id == second_id).then_some(()))
        .await
        .expect("the player already here was never told of the newcomer");
    server.stop().await;
}

/// **What is in somebody's hand changes on the other screen when it changes
/// in theirs**, and goes when their hand is empty again: the held block is read
/// off the selected slot every tick (`players::outfit_of`), and this is the
/// wiring from the key to the other screen.
#[tokio::test]
async fn a_player_who_takes_something_in_hand_is_seen_holding_it_and_then_not() {
    use primitive_shared::types::BLOCK_IRON_HELM;
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut holder = Client::connect_as(&address, "holder").await;
    holder.settled().await;
    assert_eq!(server.give(BLOCK_IRON_HELM, 1), 0);
    let slot = server
        .player_inventory()
        .and_then(|pack| (0..primitive_shared::inventory::SLOTS).find(|&s| pack.block_in(s) == Some(BLOCK_IRON_HELM)))
        .expect("the helmet went nowhere");
    let empty = server
        .player_inventory()
        .and_then(|pack| (0..9).find(|&s| pack.block_in(s).is_none()))
        .expect("a hotbar with no empty slot");
    let mut witness = Client::connect_as(&address, "witness").await;
    witness.settled().await;
    holder.send(ClientMessage::SelectSlot { slot: slot as u8 }).await;
    witness
        .sees(holder.id, 10, |state| state.outfit.holding == BLOCK_IRON_HELM)
        .await
        .expect("the other player never saw what was taken in hand");
    holder.send(ClientMessage::SelectSlot { slot: empty as u8 }).await;
    witness
        .sees(holder.id, 10, |state| state.outfit.holding == BLOCK_AIR)
        .await
        .expect("the other player went on seeing a hand that had been emptied");
    server.stop().await;
}

/// **A swimmer is seen swimming, and seen standing again on the bank.** See
/// `Posture::Swimming`: in water past the waist the figure on every other
/// screen stood upright on the bed of the lake and strode through it.
#[tokio::test]
async fn a_player_in_deep_water_is_seen_swimming_and_out_of_it_standing() {
    use primitive_shared::protocol::Posture;
    use primitive_shared::types::{BLOCK_STONE, BLOCK_WATER};
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();
    let mut swimmer = Client::connect_as(&address, "swimmer").await;
    let mut witness = Client::connect_as(&address, "witness").await;
    swimmer.settled().await;
    witness.settled().await;
    let (x, y, z) = cell_of(swimmer.spawn);
    let feet = (f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5);
    let mut sequence = 0;
    stand_at(&mut swimmer, &mut sequence, feet).await;
    witness
        .sees(swimmer.id, 10, |state| state.posture == Posture::Standing)
        .await
        .expect("the two players never saw each other standing");
    // A well two cells deep with the player in it: stone round it so the
    // water stays, and water over the waist.
    for dy in 0..2 {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            server.place_block(x + dx, y + dy, z + dz, BLOCK_STONE);
        }
        server.place_block(x, y + dy, z, BLOCK_WATER);
    }
    stand_at(&mut swimmer, &mut sequence, feet).await;
    witness
        .sees(swimmer.id, 10, |state| state.posture == Posture::Swimming)
        .await
        .expect("a player in water past the waist was never seen swimming");
    for dy in 0..2 {
        server.place_block(x, y + dy, z, BLOCK_AIR);
    }
    witness
        .sees(swimmer.id, 10, |state| state.posture == Posture::Standing)
        .await
        .expect("a player whose water was gone went on being seen swimming");
    server.stop().await;
}
