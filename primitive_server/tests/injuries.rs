//! Wounds and their dressings -- end to end over a real socket.
//!
//! The rules are `primitive_shared::injury`'s and the body's are
//! `survival`'s, and both have their own tests. These cover the **wiring**,
//! which is where the failures a player would actually meet live:
//!
//! - a wound the server has and never sends, so the mannequin stays clean
//!   while the health bar drains;
//! - a bandage the server accepts and never takes out of the pack, which is
//!   an infinite bandage;
//! - a bandage the server *refuses* and takes anyway, which is the worse
//!   bug of the two: the player did the wrong thing, and was charged for it
//!   without being told.
//!
//! Every test is the whole path: a real server, a real socket, the real
//! gesture, and an assertion on what came back.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::injury::{Injuries, Kind, Part};
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
use primitive_shared::types::{BlockId, BLOCK_BANDAGE, BLOCK_SPLINT};
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
}

impl Client {
    async fn connect(address: &str) -> Self {
        let mut socket = TcpStream::connect(address).await.expect("connect");
        write_message(
            &mut socket,
            &ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                username: "wounded".to_string(),
            },
        )
        .await
        .expect("hello");
        match read_message::<_, ServerMessage>(&mut socket).await.expect("welcome") {
            ServerMessage::Welcome { .. } => {}
            other => panic!("expected Welcome, got {other:?}"),
        }
        Self { socket }
    }

    async fn send(&mut self, message: ClientMessage) {
        write_message(&mut self.socket, &message).await.expect("send");
    }

    /// Reads until a message the predicate likes shows up. Chunks,
    /// snapshots and keepalives share the socket, so the message under test
    /// is never simply the next one -- see `tests/body.rs` for the bug
    /// waiting for "the next one" caught.
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
    /// server's side, so a `give` or an `injure` has somebody to land on.
    async fn settled(&mut self) {
        self.wait_for(10, |m| matches!(m, ServerMessage::InventoryState { .. }).then_some(()))
            .await
            .expect("the server never sent an opening inventory");
    }

    /// The slot the next inventory holding `block` has it in.
    async fn slot_of(&mut self, block: BlockId) -> usize {
        let inventory = self
            .wait_for(10, |m| match m {
                ServerMessage::InventoryState { inventory } if inventory.count(block) > 0 => {
                    Some(inventory.clone())
                }
                _ => None,
            })
            .await
            .expect("the server never sent an inventory holding the item");
        (0..primitive_shared::inventory::SLOTS)
            .find(|&s| inventory.block_in(s) == Some(block))
            .expect("counted but in no slot")
    }

    /// The next body the server describes that the predicate likes.
    async fn injuries(&mut self, want: impl Fn(&Injuries) -> bool) -> Option<Injuries> {
        self.wait_for(10, |m| match m {
            ServerMessage::Injuries { injuries } if want(injuries) => Some(*injuries),
            _ => None,
        })
        .await
    }
}

#[tokio::test]
async fn a_player_is_told_what_is_wrong_with_them_on_arrival_and_when_it_changes() {
    // The join half, and then the change half. A mannequin drawn from a
    // body the client was never sent is a mannequin drawn from a guess.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let mut client = Client::connect(&server.address().to_string()).await;

    client
        .injuries(Injuries::is_whole)
        .await
        .expect("a fresh player was never told they were whole");
    client.settled().await;

    server.injure(Part::RightLeg, Kind::Fracture, 1.0);
    let broken = client
        .injuries(|body| body.leg_broken())
        .await
        .expect("a broken leg was never sent");
    assert!(broken.wound(Part::RightLeg, Kind::Fracture).is_open());
    server.stop().await;
}

#[tokio::test]
async fn a_bandage_dropped_on_a_cut_is_spent_and_the_cut_stops_bleeding() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let mut client = Client::connect(&server.address().to_string()).await;
    client.settled().await;

    server.injure(Part::LeftArm, Kind::Cut, 0.8);
    client
        .injuries(|body| body.is_bleeding())
        .await
        .expect("the cut was never sent");
    assert_eq!(server.give(BLOCK_BANDAGE, 2), 0, "the bandages did not fit");
    let slot = client.slot_of(BLOCK_BANDAGE).await;

    client
        .send(ClientMessage::TreatInjury {
            slot: slot as u8,
            part: Part::LeftArm.index() as u8,
        })
        .await;

    // **Both answers in one wait**, because `wait_for` drops what it is not
    // looking for, and which of the two arrives first is the tick's choice.
    let (mut spent, mut dressed) = (false, false);
    let done = client
        .wait_for(10, |m| {
            match m {
                ServerMessage::InventoryState { inventory } => {
                    spent = inventory.count(BLOCK_BANDAGE) == 1;
                }
                ServerMessage::Injuries { injuries } => {
                    dressed = injuries.wound(Part::LeftArm, Kind::Cut).is_dressed();
                }
                _ => {}
            }
            (spent && dressed).then_some(())
        })
        .await;
    assert!(done.is_some(), "the bandage: spent {spent}, on the arm {dressed}");
    assert!(
        !server.player_injuries().expect("a player").is_bleeding(),
        "a bandaged cut is still bleeding on the server"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_bandage_dropped_on_a_leg_with_no_cut_is_refused_and_kept() {
    // **Refused, kept and said.** The server is the one that decides a
    // bandage does not suit a leg, and the refusal has to be total: the
    // bandage stays in the pack, the arm that *is* cut stays undressed, and
    // the player is told why -- a silent nothing reads as the drop having
    // missed, and they try the same thing again.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let mut client = Client::connect(&server.address().to_string()).await;
    client.settled().await;

    server.injure(Part::LeftArm, Kind::Cut, 0.8);
    assert_eq!(server.give(BLOCK_BANDAGE, 1), 0, "the bandage did not fit");
    let slot = client.slot_of(BLOCK_BANDAGE).await;

    client
        .send(ClientMessage::TreatInjury {
            slot: slot as u8,
            part: Part::LeftLeg.index() as u8,
        })
        .await;
    let said = client
        .wait_for(10, |m| match m {
            ServerMessage::Error(text) if text.contains("bandage") => Some(text.clone()),
            _ => None,
        })
        .await;
    assert!(said.is_some(), "a refused bandage was refused in silence");
    assert_eq!(
        server.player_inventory().expect("a player").count(BLOCK_BANDAGE),
        1,
        "a refused bandage was spent"
    );
    let body = server.player_injuries().expect("a player");
    assert!(body.is_bleeding(), "the cut arm was dressed by a bandage dropped on a leg");

    // ...and a splint on the cut is the same refusal from the other side.
    assert_eq!(server.give(BLOCK_SPLINT, 1), 0, "the splint did not fit");
    let splint = client.slot_of(BLOCK_SPLINT).await;
    client
        .send(ClientMessage::TreatInjury {
            slot: splint as u8,
            part: Part::LeftArm.index() as u8,
        })
        .await;
    let said = client
        .wait_for(10, |m| match m {
            ServerMessage::Error(text) if text.contains("splint") => Some(()),
            _ => None,
        })
        .await;
    assert!(said.is_some(), "a splint on a cut was not refused");
    assert_eq!(server.player_inventory().expect("a player").count(BLOCK_SPLINT), 1);
    server.stop().await;
}
