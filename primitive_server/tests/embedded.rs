//! The embedded-server path, which is what singleplayer runs on.
//!
//! These are end-to-end: a real listener, a real socket, a real
//! handshake. That is the point -- the interesting failures here are not
//! in any one function but in the seams between binding, spawning and
//! shutting down, and a unit test of `start` in isolation would not
//! notice any of them.

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
use tokio::net::TcpStream;

/// Settings for a throwaway world: loopback, an OS-chosen port, and no
/// persistence, so the tests neither collide with each other nor leave
/// anything behind.
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

async fn handshake(address: &str) -> anyhow::Result<ServerMessage> {
    let mut socket = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(address))
        .await
        .map_err(|_| anyhow::anyhow!("timed out connecting"))??;
    write_message(
        &mut socket,
        &ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            username: "tester".to_string(),
        },
    )
    .await?;
    let reply = tokio::time::timeout(
        Duration::from_secs(5),
        read_message::<_, ServerMessage>(&mut socket),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for Welcome"))??;
    Ok(reply)
}

#[tokio::test]
async fn an_embedded_server_reports_the_port_it_actually_got() {
    // Singleplayer binds port 0 so two copies of the game can run side
    // by side. The client then has to be told which port that was --
    // without this it would have nothing to connect to.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("should start");
    let address = server.address();
    assert!(address.ip().is_loopback(), "bound to {address}");
    assert_ne!(address.port(), 0, "the resolved port was not reported");
    server.stop().await;
}

#[tokio::test]
async fn two_embedded_servers_can_run_at_once() {
    // Two copies of the game on one machine. A hard-coded port would
    // make the second one fail to start.
    let first = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("first should start");
    let second = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("second should start");
    assert_ne!(first.address().port(), second.address().port());
    first.stop().await;
    second.stop().await;
}

#[tokio::test]
async fn a_client_can_complete_the_handshake_with_an_embedded_server() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("should start");
    let address = server.address().to_string();

    match handshake(&address).await.expect("handshake should succeed") {
        ServerMessage::Welcome {
            protocol_version,
            server_name,
            spawn,
            ..
        } => {
            assert_eq!(protocol_version, PROTOCOL_VERSION);
            assert_eq!(server_name, "test");
            assert!(spawn.1 > 0.0, "should spawn above the void, got {spawn:?}");
        }
        other => panic!("expected Welcome, got {other:?}"),
    }

    server.stop().await;
}

#[tokio::test]
async fn stopping_an_embedded_server_frees_its_port() {
    // The failure this guards against is a leaked accept loop: leaving
    // a world and starting a new one would then pile up servers for the
    // rest of the session.
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("should start");
    let address = server.address();
    server.stop().await;

    // Bind the very same port. This only succeeds if the listener is
    // really gone.
    let rebound = tokio::net::TcpListener::bind(address).await;
    assert!(rebound.is_ok(), "port {address} was still held after stop()");
}

#[tokio::test]
async fn an_embedded_server_refuses_a_client_speaking_another_protocol() {
    let server = primitive_server::start(test_settings(), RunOptions::embedded())
        .await
        .expect("should start");
    let address = server.address().to_string();

    let mut socket = TcpStream::connect(&address).await.expect("connect");
    write_message(
        &mut socket,
        &ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION + 100,
            username: "old client".to_string(),
        },
    )
    .await
    .expect("send");

    let reply = read_message::<_, ServerMessage>(&mut socket)
        .await
        .expect("read");
    assert!(
        matches!(reply, ServerMessage::Rejected(_)),
        "expected a rejection, got {reply:?}"
    );

    server.stop().await;
}

#[tokio::test]
async fn a_port_already_in_use_is_an_error_rather_than_a_panic() {
    // The client shows this on the failure screen, so it has to come
    // back as a `Result` from `start` -- not from a task nobody awaits.
    let occupier = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let taken = occupier.local_addr().expect("addr");

    let settings = ServerSettings {
        bind_addr: taken.to_string(),
        ..test_settings()
    };
    let result = primitive_server::start(settings, RunOptions::embedded()).await;
    assert!(result.is_err(), "binding a taken port should fail");
    let message = result.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.contains(&taken.to_string()),
        "the error should name the address: {message}"
    );
}

#[cfg(feature = "plugins")]
#[tokio::test]
async fn an_embedded_server_loads_no_plugins_even_when_some_are_there() {
    // Singleplayer builds the client without the scripting engine at
    // all. This is the runtime half of the same guarantee: pointed at a
    // directory full of plugins, an embedded server ignores it, while a
    // standalone one does not.
    let plugin_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("plugins");
    assert!(plugin_dir.is_dir(), "the repo's plugins/ folder should exist");

    let settings = || ServerSettings {
        plugin_dir: plugin_dir.display().to_string(),
        ..test_settings()
    };

    let embedded = primitive_server::start(settings(), RunOptions::embedded())
        .await
        .expect("should start");
    assert_eq!(embedded.plugin_count(), 0, "singleplayer must run no plugins");
    embedded.stop().await;

    let hosted = primitive_server::start(
        settings(),
        RunOptions {
            console: false,
            logging: false,
            ..RunOptions::standalone()
        },
    )
    .await
    .expect("should start");
    assert!(
        hosted.plugin_count() > 0,
        "a hosted server should still load the same directory"
    );
    hosted.stop().await;
}
