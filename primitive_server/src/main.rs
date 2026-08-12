//! The standalone `primitive_server` binary.
//!
//! Deliberately thin. Everything the server does lives in the library
//! next to this file, because the game client runs the same code
//! in-process for singleplayer -- see the crate docs for why that is one
//! implementation rather than two.

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = ServerSettings::load_or_default();
    primitive_server::run(settings, RunOptions::standalone()).await
}
