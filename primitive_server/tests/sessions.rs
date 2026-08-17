//! One name, one session.
//!
//! Over real sockets, because the thing being tested is a decision made
//! in the middle of the handshake and everything interesting about it is
//! in the ordering: two clients racing to claim the same identity, and a
//! name that has to become free again the moment its owner leaves.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
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
        ..Default::default()
    }
}

/// Says hello and reports what came back, keeping the socket alive --
/// which is the whole point here: a session that has hung up is not a
/// session anybody is colliding with.
async fn say_hello(address: &str, username: &str) -> (TcpStream, ServerMessage) {
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
    let reply = tokio::time::timeout(
        Duration::from_secs(5),
        read_message::<_, ServerMessage>(&mut socket),
    )
    .await
    .expect("timed out waiting for a reply")
    .expect("reply");
    (socket, reply)
}

#[tokio::test]
async fn one_name_cannot_be_logged_in_twice() {
    // **The duplication bug.** Admission counted connections -- against
    // the player cap, and against a per-address limit that defaults to
    // eight -- and the player map is keyed by a connection number handed
    // out fresh on every join. Nothing asked whether the *person* was
    // already playing, and every session was handed its own clone of
    // that person's pack out of the profile. Eight copies of the game
    // under one name were eight copies of one rucksack; tip each into a
    // chest and the world has eight times the things it did.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();

    let (_first, welcome) = say_hello(&address, "twin").await;
    assert!(
        matches!(welcome, ServerMessage::Welcome { .. }),
        "the first login was refused: {welcome:?}"
    );

    let (_second, refusal) = say_hello(&address, "twin").await;
    assert!(
        matches!(refusal, ServerMessage::Rejected(_)),
        "a second session under one name was allowed in: {refusal:?}"
    );

    server.stop().await;
}

#[tokio::test]
async fn the_capitalisation_of_a_name_is_not_a_different_person() {
    // Identity is the UUID, and the UUID is derived from the name
    // case-insensitively -- otherwise "Twin" would be a free second copy
    // of "twin" and the check above would be decoration.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();

    let (_first, welcome) = say_hello(&address, "shouty").await;
    assert!(matches!(welcome, ServerMessage::Welcome { .. }), "{welcome:?}");

    let (_second, refusal) = say_hello(&address, "SHOUTY").await;
    assert!(
        matches!(refusal, ServerMessage::Rejected(_)),
        "the same player got in twice by holding down shift: {refusal:?}"
    );

    server.stop().await;
}

#[tokio::test]
async fn two_different_people_are_still_two_people() {
    // The check must be about identity and not about "somebody is
    // already playing", which would be a rather shorter server.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();

    let (_alice, first) = say_hello(&address, "alice").await;
    let (_bob, second) = say_hello(&address, "bob").await;
    assert!(matches!(first, ServerMessage::Welcome { .. }), "{first:?}");
    assert!(matches!(second, ServerMessage::Welcome { .. }), "{second:?}");

    server.stop().await;
}

#[tokio::test]
async fn a_name_is_free_again_once_its_owner_leaves() {
    // The cost of refusing rather than kicking, and the reason it is
    // affordable: leaving has to give the name straight back, or a
    // player who alt-F4s is locked out of their own world.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("start");
    let address = server.address().to_string();

    let (first, welcome) = say_hello(&address, "revenant").await;
    assert!(matches!(welcome, ServerMessage::Welcome { .. }), "{welcome:?}");
    drop(first);

    // The server notices the closed socket on its own schedule, so this
    // waits rather than asserting on the first attempt -- but the budget
    // is deliberately tight. Departure used to happen behind the writer
    // task's two-second drain timeout, and with one session per name
    // that turned into two seconds of being locked out of your own
    // world every time the game crashed.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        let (_socket, reply) = say_hello(&address, "revenant").await;
        if matches!(reply, ServerMessage::Welcome { .. }) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the name never came free after its owner disconnected"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    server.stop().await;
}
