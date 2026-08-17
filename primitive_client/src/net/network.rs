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
        // Not a `while let`: the error arm logs and breaks, which is a
        // third thing to do rather than a pattern that failed to match.
        #[allow(clippy::while_let_loop)]
        loop {
            match read_message::<_, ServerMessage>(&mut read_half).await {
                Ok(msg) => {
                    // Structure before the game loop ever sees it. See
                    // `well_formed`.
                    if let Err(complaint) = well_formed(&msg) {
                        eprintln!("[net] discarding a malformed message: {complaint}");
                        continue;
                    }
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

/// Is this message shaped the way the rest of the client assumes every
/// message is shaped? `Err` carries what was wrong with it, for the log.
///
/// **Deserialising is not validating.** bincode will happily hand back a
/// `ChunkData` whose block array is one element short of a chunk: it was
/// asked for a `Vec`, and a short `Vec` is a perfectly good `Vec`. Every
/// accessor on `Chunk` then indexes straight into it, so the first read
/// of a block near the top of that column panics -- and a panic here is
/// the game closing, mid-session, on a packet.
///
/// It has to happen before the queue, not at the point of use. The game
/// loop takes chunks off this channel and hands them to the mesher, the
/// lighting and the collider, and "check it wherever you touch it" is
/// three places to remember instead of one -- which is exactly the shape
/// the inventory messages already avoid by calling `sanitize` on arrival.
///
/// Dropped rather than treated as a disconnect. A server that sends one
/// bad chunk is far more likely to be a version drift or a bug than an
/// attack, and closing the session over it would turn a missing chunk
/// into a lost afternoon; the chunk manager will ask for it again. A
/// server that sends nothing but bad chunks produces a client that logs
/// and waits, which is a diagnosable state rather than a crash report.
fn well_formed(msg: &ServerMessage) -> Result<(), String> {
    match msg {
        ServerMessage::ChunkData(chunk) if !chunk.is_well_formed() => Err(format!(
            "chunk ({}, {}) carried {} blocks, not {}",
            chunk.pos.x,
            chunk.pos.z,
            chunk.blocks.len(),
            primitive_shared::types::CHUNK_VOLUME
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, CHUNK_VOLUME};

    fn chunk_of(blocks: usize) -> ServerMessage {
        ServerMessage::ChunkData(std::sync::Arc::new(Chunk {
            pos: ChunkPos::new(3, -4),
            blocks: vec![BLOCK_STONE; blocks],
        }))
    }

    #[test]
    fn a_chunk_of_the_right_size_goes_through() {
        assert!(well_formed(&chunk_of(CHUNK_VOLUME)).is_ok());
    }

    #[test]
    fn a_chunk_of_the_wrong_size_is_refused_rather_than_indexed_into() {
        // Short, long and empty: all three used to reach the mesher, and
        // the first read past the end of the array closed the game.
        for count in [0, CHUNK_VOLUME - 1, CHUNK_VOLUME + 1] {
            let complaint = well_formed(&chunk_of(count))
                .expect_err("a chunk with the wrong number of blocks was accepted");
            // The log has to name the chunk, or an operator has a crash
            // that stopped happening and nothing to look at.
            assert!(complaint.contains("(3, -4)"), "{complaint}");
        }
    }

    #[test]
    fn nothing_else_is_held_up_by_the_check() {
        // A guard on one message must not become a guard on all of
        // them: this runs on every packet the client ever receives.
        assert!(well_formed(&ServerMessage::Ping { nonce: 1 }).is_ok());
        assert!(well_formed(&ServerMessage::ChestClosed).is_ok());
        assert!(well_formed(&ServerMessage::BlockUpdates(Vec::new())).is_ok());
        let _ = BLOCK_AIR;
    }
}
