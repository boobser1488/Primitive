//! Flight, over a real socket, against a *running* server.
//!
//! The last link that unit tests cannot reach. `tests/flight.rs` proves
//! the mod loads and answers its commands across the C ABI, and the
//! client's own tests prove a player who has been granted flight flies —
//! but the wire between them is exercised nowhere else: a `Chat` going
//! out, a mod deciding, and `ServerMessage::Flight` coming back.
//!
//! Run the server first, then:
//!
//! ```text
//! cargo run -p primitive_server --example verify_flight
//! cargo run -p primitive_server --example verify_flight -- 127.0.0.1:7879
//! ```
//!
//! ## What it checks, and what it cannot
//!
//! With the flight mod loaded and `operators_only` at its default, a
//! fresh player is **not** an operator, so `/fly` comes back refused.
//! That is a pass, and it is worth more than it looks: the command
//! crossed the socket, reached the mod, ran its permission check and
//! sent an answer back — which is every part of the path except the
//! grant itself.
//!
//! Set `operators_only: Bool(false)` in `mods/flight/mod.ron` (or `/op`
//! this player from the server console) and run it again: the same
//! command then comes back as a `Flight` message, and the example says
//! so. Both outcomes are reported rather than one being an error,
//! because which one you get is a fact about the server's configuration
//! rather than about whether the code works.
//!
//! It fails if `/fly` produces **neither** — that is the mod not loaded,
//! not subscribed, or not answering.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};

fn server_address() -> String {
    std::env::args()
        .nth(1)
        .or_else(|| std::env::var("PRIMITIVE_SERVER").ok())
        .unwrap_or_else(|| "127.0.0.1:7878".to_string())
}

const RECV_TIMEOUT: Duration = Duration::from_secs(10);

/// What came back from `/fly`.
#[derive(Debug)]
enum Answer {
    /// The server granted it. The whole path worked.
    Granted { speed: f32 },
    /// The mod answered and said no. The whole path worked too — see the
    /// module note.
    Refused(String),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let address = server_address();
    println!("connecting to {address}");

    let mut socket = TcpStream::connect(&address).await?;
    write_message(
        &mut socket,
        &ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            username: "flyer".to_string(),
        },
    )
    .await?;

    let id = match read_message::<_, ServerMessage>(&mut socket).await? {
        ServerMessage::Welcome {
            your_id,
            protocol_version,
            server_name,
            ..
        } => {
            anyhow::ensure!(
                protocol_version == PROTOCOL_VERSION,
                "server speaks v{protocol_version}, this build v{PROTOCOL_VERSION}"
            );
            println!("  welcomed by \"{server_name}\" as player {your_id}");
            your_id
        }
        other => anyhow::bail!("expected Welcome, got {other:?}"),
    };
    let _ = id;

    println!("sending /fly");
    write_message(&mut socket, &ClientMessage::Chat("/fly".to_string())).await?;

    // Read until one of the two answers turns up, keeping the keepalive
    // going. A running server sends snapshots twenty times a second, so
    // the bound has to be on the whole wait rather than per message --
    // otherwise a failure presents as the example simply stopping.
    let deadline = tokio::time::Instant::now() + RECV_TIMEOUT;
    let answer = loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        anyhow::ensure!(!left.is_zero(), "nothing answered /fly within ten seconds");

        let msg = match timeout(left, read_message::<_, ServerMessage>(&mut socket)).await {
            Ok(msg) => msg?,
            Err(_) => anyhow::bail!("nothing answered /fly within ten seconds"),
        };
        match msg {
            ServerMessage::Ping { nonce } => {
                write_message(&mut socket, &ClientMessage::Pong { nonce }).await?;
            }
            ServerMessage::Flight { enabled, speed } => {
                anyhow::ensure!(enabled, "/fly answered by switching flight off");
                break Answer::Granted { speed };
            }
            // The mod talks to one player through `NetworkApi::tell`,
            // which arrives as a server-authored chat line.
            ServerMessage::Chat { from: None, text, .. } => {
                let lower = text.to_ascii_lowercase();
                if lower.contains("operator") {
                    break Answer::Refused(text);
                }
                // Anything else the server says in its own name -- a
                // join notice, the greeter -- is not an answer to this.
                println!("  (server said: {text})");
            }
            _ => {}
        }
    };

    match answer {
        Answer::Granted { speed } => {
            println!("  granted: {speed:.0} blocks a second");
            println!("\nthe whole path works: chat -> mod -> Flight -> the wire.");
        }
        Answer::Refused(text) => {
            println!("  refused: {text}");
            println!(
                "\nthe whole path works except the grant, which this player is not\n\
                 allowed. that is `operators_only` doing its job -- see the note at\n\
                 the top of this file for how to see the other half."
            );
        }
    }

    write_message(&mut socket, &ClientMessage::Disconnect).await?;
    Ok(())
}
