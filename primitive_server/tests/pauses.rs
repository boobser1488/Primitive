//! A pause is not a disconnection.
//!
//! "ошибка internal server error на Android при сворачивании или на ПК при
//! сне": the player minimised the game, or the computer slept, and came
//! back to a failure screen. Three different pauses, and the rule each is
//! held to:
//!
//! * **the whole process froze** (a computer asleep, a debugger): the
//!   server was not listening, so it forgives the silence -- see
//!   `PAUSE_GAP` in the crate root;
//! * **the embedded server was told to hold still** (the activity went to
//!   the background): nothing ticks, nobody is timed out;
//! * **the client froze while the server ran** (a phone in a pocket on a
//!   real server): timed out after `client_timeout_secs` like any dead
//!   connection -- and a pause shorter than that is not.
//!
//! Over real sockets with the timeouts shortened to their floor (a
//! keepalive a second, a timeout of two), so a "long" pause is seconds.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, DisconnectReason, ServerMessage, PROTOCOL_VERSION};
use tokio::net::TcpStream;

fn impatient_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "test".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        stats_interval_secs: 0.0,
        keepalive_interval_secs: 1.0,
        client_timeout_secs: 2.0,
        ..Default::default()
    }
}

async fn join(address: &str, username: &str) -> TcpStream {
    let mut socket = TcpStream::connect(address).await.expect("connect");
    write_message(
        &mut socket,
        &ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, username: username.to_string() },
    )
    .await
    .expect("hello");
    let reply = tokio::time::timeout(Duration::from_secs(5), read_message::<_, ServerMessage>(&mut socket))
        .await
        .expect("no welcome")
        .expect("welcome");
    assert!(matches!(reply, ServerMessage::Welcome { .. }), "refused: {reply:?}");
    socket
}

/// A live client for `how_long`: reads everything and answers every ping.
/// Returns why the server ended the session, if it did.
async fn stay_awake(socket: &mut TcpStream, how_long: Duration) -> Option<String> {
    let deadline = tokio::time::Instant::now() + how_long;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return None;
        }
        match tokio::time::timeout(left, read_message::<_, ServerMessage>(socket)).await {
            Err(_) => return None,
            Ok(Err(e)) => return Some(format!("the connection closed: {e}")),
            Ok(Ok(ServerMessage::Kick(reason))) => return Some(reason.to_string()),
            Ok(Ok(ServerMessage::Ping { nonce })) => {
                // A ping read out of the buffer can be older than the
                // server's decision to hang up: the answer then goes into a
                // socket that is already shut, and on Linux that is a broken
                // pipe. That *is* the session ending, not the test failing --
                // `expect` here made the timeout test red on a runner that
                // happened to schedule the kick between the two.
                if let Err(e) = write_message(socket, &ClientMessage::Pong { nonce }).await {
                    return Some(format!("the connection closed: {e}"));
                }
            }
            Ok(Ok(_)) => {}
        }
    }
}

#[tokio::test]
async fn a_server_that_was_asleep_times_nobody_out_for_the_night() {
    // The computer sleeps: the tick loop's thread stops for longer than the
    // timeout, and so does the client. On waking, the first keepalive tick
    // used to find the player silent for the whole sleep and kick them
    // before they had had a chance to say a word.
    let server = primitive_server::start(impatient_settings(), RunOptions::embedded()).await.expect("start");
    let address = server.address().to_string();
    let mut socket = join(&address, "sleeper").await;
    assert_eq!(stay_awake(&mut socket, Duration::from_millis(500)).await, None);

    server.stall_tick_loop_for(Duration::from_secs(4));
    // The client sleeps too, in the same process: not a word while the
    // server is frozen, and then awake again.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let ended = stay_awake(&mut socket, Duration::from_secs(3)).await;
    assert_eq!(ended, None, "a player was thrown out for the time the server itself was asleep");
    assert!(server.position_of("sleeper").is_some(), "the sleeper is no longer on the server");
    server.stop().await;
}

#[tokio::test]
async fn a_held_embedded_server_neither_ticks_nor_times_anybody_out() {
    // The Android activity goes to the background: the client's loop stops
    // and says nothing, and the world is held still for it.
    let server = primitive_server::start(impatient_settings(), RunOptions::embedded()).await.expect("start");
    let address = server.address().to_string();
    let mut socket = join(&address, "pocket").await;
    assert_eq!(stay_awake(&mut socket, Duration::from_millis(500)).await, None);

    server.set_paused(true);
    // One tick may already have been under way when the flag went up.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let held_at = server.ticks();
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(
        server.ticks() <= held_at + 1,
        "the world went on without its only player: {} ticks while held",
        server.ticks() - held_at
    );
    server.set_paused(false);

    let ended = stay_awake(&mut socket, Duration::from_secs(3)).await;
    assert_eq!(ended, None, "a player was timed out for a pause the server was told about");
    assert!(server.ticks() > held_at + 10, "the world did not start again");
    server.stop().await;
}

#[tokio::test]
async fn a_client_that_stops_answering_on_a_running_server_is_timed_out() {
    // The other half of the rule. A real server does not wait for a phone
    // in a pocket: silence longer than the timeout, while the server was
    // running, is a dead connection -- and the client reconnects when it
    // wakes.
    let server = primitive_server::start(impatient_settings(), RunOptions::embedded()).await.expect("start");
    let address = server.address().to_string();
    let mut socket = join(&address, "ghost").await;

    tokio::time::sleep(Duration::from_secs(4)).await;
    let ended = stay_awake(&mut socket, Duration::from_secs(3)).await;
    let timed_out = DisconnectReason::Timeout.to_string();
    assert!(
        ended.as_deref().is_some_and(|why| why == timed_out || why.starts_with("the connection closed")),
        "a client silent for twice the timeout was kept: {ended:?}"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_client_that_pauses_for_less_than_the_timeout_is_kept() {
    let server = primitive_server::start(impatient_settings(), RunOptions::embedded()).await.expect("start");
    let address = server.address().to_string();
    let mut socket = join(&address, "blinker").await;
    assert_eq!(stay_awake(&mut socket, Duration::from_millis(500)).await, None);

    tokio::time::sleep(Duration::from_millis(1200)).await;
    let ended = stay_awake(&mut socket, Duration::from_secs(3)).await;
    assert_eq!(ended, None, "a pause shorter than the timeout ended the session");
    server.stop().await;
}
