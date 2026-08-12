//! Socket <-> game-loop bridge.
//!
//! Нюанс from the plan: `tokio::sync::mpsc` decouples the socket from the
//! render loop, so a slow server can never stall a frame.
//!
//! New this pass: the **handshake happens before the game starts**. The
//! client sends `Hello` and waits for `Welcome` (or `Rejected`) up front,
//! so a version mismatch or a full server produces a clear message at
//! startup instead of a window that opens onto an empty void.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::mpsc;

use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{
    ClientMessage, PlayerId, ServerMessage, PROTOCOL_VERSION,
};

/// What the server told us about itself. The client configures itself
/// from this rather than assuming its own settings match the server's.
#[derive(Debug, Clone)]
pub struct WelcomeInfo {
    pub your_id: PlayerId,
    pub server_name: String,
    pub tick_rate_hz: f32,
    pub server_view_distance: i32,
    pub world_seed: u32,
    pub spawn: (f32, f32, f32),
    pub time_of_day: f32,
    pub day_length_seconds: f32,
}

pub struct NetworkHandle {
    pub from_game: mpsc::Sender<ClientMessage>,
    pub to_game: mpsc::Receiver<ServerMessage>,
}

impl NetworkHandle {
    /// Fire-and-forget send. Returns false if the queue is full, which
    /// for movement updates is fine (the next one supersedes it anyway).
    pub fn send(&self, msg: ClientMessage) -> bool {
        self.from_game.try_send(msg).is_ok()
    }
}

pub struct Connection {
    pub handle: NetworkHandle,
    pub welcome: WelcomeInfo,
}

pub async fn connect(addr: &str, username: &str) -> anyhow::Result<Connection> {
    let mut socket = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(addr))
        .await
        .map_err(|_| anyhow::anyhow!("timed out connecting to {addr}"))??;
    let _ = socket.set_nodelay(true);

    write_message(
        &mut socket,
        &ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            username: username.to_string(),
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("handshake send failed: {e}"))?;

    let reply: ServerMessage = tokio::time::timeout(
        Duration::from_secs(10),
        read_message::<_, ServerMessage>(&mut socket),
    )
    .await
    .map_err(|_| anyhow::anyhow!("server did not answer the handshake"))?
    .map_err(|e| anyhow::anyhow!("handshake read failed: {e}"))?;

    let welcome = match reply {
        ServerMessage::Welcome {
            your_id,
            protocol_version,
            server_name,
            tick_rate_hz,
            view_distance_chunks,
            world_seed,
            spawn,
            time_of_day,
            day_length_seconds,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                anyhow::bail!(
                    "protocol mismatch: server speaks v{protocol_version}, this client v{PROTOCOL_VERSION}"
                );
            }
            WelcomeInfo {
                your_id,
                server_name,
                tick_rate_hz,
                server_view_distance: view_distance_chunks,
                world_seed,
                spawn,
                time_of_day,
                day_length_seconds,
            }
        }
        ServerMessage::Rejected(reason) => anyhow::bail!("server refused the connection: {reason}"),
        other => anyhow::bail!("unexpected handshake reply: {other:?}"),
    };

    // FIX (kept from the earlier version): split into owned read/write
    // halves so the read and write loops each hold their own `&mut`
    // instead of fighting over one shared socket.
    let (mut read_half, mut write_half) = socket.into_split();

    let (to_game_tx, to_game_rx) = mpsc::channel::<ServerMessage>(1024);
    let (from_game_tx, mut from_game_rx) = mpsc::channel::<ClientMessage>(256);

    // Outgoing: game loop -> server.
    tokio::spawn(async move {
        while let Some(msg) = from_game_rx.recv().await {
            let is_disconnect = matches!(msg, ClientMessage::Disconnect);
            if write_message(&mut write_half, &msg).await.is_err() || is_disconnect {
                break;
            }
        }
    });

    // Incoming: server -> game loop.
    tokio::spawn(async move {
        loop {
            match read_message::<_, ServerMessage>(&mut read_half).await {
                Ok(msg) => {
                    if to_game_tx.send(msg).await.is_err() {
                        break; // game loop went away
                    }
                }
                Err(_) => break, // connection closed or desynced
            }
        }
    });

    Ok(Connection {
        handle: NetworkHandle {
            from_game: from_game_tx,
            to_game: to_game_rx,
        },
        welcome,
    })
}
