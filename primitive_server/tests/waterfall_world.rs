//! Builds the save a waterfall bug is photographed in, without a person
//! at the keyboard.
//!
//! The test world's water plot (`showcase::water`, plot `(-1, 1)`,
//! middle `(-8, 24)`) is a pool with a tank standing over it and a
//! cobblestone plug in the tank's wall. Pulling that plug is the whole
//! demonstration -- and it is also the only way to get a waterfall in
//! this game without a player breaking a block by hand, which is
//! exactly the "hand-run experiment nobody can repeat" the project
//! refuses.
//!
//! So: start an embedded server on the world directory named by
//! `PRIMITIVE_WORLD_DIR`, take the plug out through `place_block`, let
//! the flow settle on the server's own tick, and save. What comes out
//! is a directory `PRIMITIVE_AUTOSTART` can open.
//!
//! ```text
//! PRIMITIVE_WORLD_DIR=/tmp/repro/saves/waterfall \
//!     cargo test -p primitive_server --test waterfall_world -- --ignored --nocapture
//! ```

use std::time::Duration;

use primitive_server::settings::ServerSettings;
use primitive_server::RunOptions;
use primitive_shared::types::BLOCK_AIR;

/// Where the plug is: `showcase::water` sets it at the middle of the
/// tank wall, one course above the pool's rim.
const PLUG: (i32, i32, i32) = (-8, primitive_shared::showcase::GROUND_Y + 1, 21);

/// A fall with **nothing behind it**, cut into the empty field far from
/// any plot.
///
/// The plot's own waterfall has a cobblestone channel, a tank and the
/// tank's own water surface directly behind it, and everything behind a
/// sheet of water is visible through it -- so a patch seen on the plot's
/// fall cannot be told from a patch seen *through* it. This one has a
/// pool, one falling column and open sky on every side.
/// Deliberately in the middle of chunk (3, 3), whose cells are 48..63
/// in both axes: everything a ray from the camera crosses is then in
/// one chunk, and one chunk is exactly what a mesher fixture can
/// reproduce quad for quad.
const LONE: (i32, i32) = (56, 56);

#[tokio::test]
#[ignore = "a tool: writes a world to disk where PRIMITIVE_WORLD_DIR says"]
async fn a_test_world_with_the_plug_pulled() {
    let dir = std::env::var("PRIMITIVE_WORLD_DIR").expect("PRIMITIVE_WORLD_DIR");
    std::fs::create_dir_all(&dir).expect("world directory");

    let settings = ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "waterfall".to_string(),
        world_dir: dir.clone(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        stats_interval_secs: 0.0,
        world_seed: 1337,
        world_preset: primitive_shared::worldgen::Preset::Test,
        ..Default::default()
    };
    let server = primitive_server::start(settings, RunOptions::embedded())
        .await
        .expect("server");

    server.place_block(PLUG.0, PLUG.1, PLUG.2, BLOCK_AIR);

    // ...and, out in the field, the same fall with nothing behind it.
    let g = primitive_shared::showcase::GROUND_Y;
    for x in LONE.0 - 4..=LONE.0 + 4 {
        for z in LONE.1 - 4..=LONE.1 + 4 {
            for y in g - 3..=g {
                server.place_block(x, y, z, BLOCK_AIR);
            }
            server.place_block(x, g - 4, z, primitive_shared::types::BLOCK_CLAY);
        }
    }
    for x in LONE.0 - 4..=LONE.0 + 4 {
        for z in LONE.1 - 4..=LONE.1 + 4 {
            for y in g - 3..=g - 1 {
                server.place_block(x, y, z, primitive_shared::types::BLOCK_WATER);
            }
        }
    }
    // One source hanging six blocks over the pool: it never runs dry,
    // and what falls out of it is a column of flowing water in open air.
    server.place_block(LONE.0, g + 6, LONE.1, primitive_shared::types::BLOCK_WATER);
    // Water moves on the server's tick, and a spill of this size is
    // over in well under a second; five is slack, not a measurement.
    tokio::time::sleep(Duration::from_secs(5)).await;

    for y in (20..=31).rev() {
        for z in LONE.1 - 5..=LONE.1 + 5 {
            let mut row = String::new();
            for x in LONE.0 - 5..=LONE.0 + 5 {
                row.push(match server.block_at(x, y, z) {
                    Some(primitive_shared::types::BLOCK_AIR) | None => '.',
                    Some(b) if primitive_shared::types::is_liquid(b) => 'W',
                    Some(_) => '#',
                });
            }
            println!("lone y={y:>2} z={z:>2}  {row}");
        }
        println!();
    }

    // What the flow actually built, cell by cell: the numbers any
    // claim about the waterfall has to be checked against.
    for y in (20..=27).rev() {
        for z in 19..=28 {
            let mut row = String::new();
            for x in -11..=-5 {
                row.push(match server.block_at(x, y, z) {
                    Some(primitive_shared::types::BLOCK_AIR) | None => '.',
                    Some(b) if primitive_shared::types::is_liquid(b) => 'W',
                    Some(_) => '#',
                });
            }
            println!("y={y:>2} z={z:>2}  {row}");
        }
        println!();
    }

    for line in server.console_command("/save") {
        println!("[save] {line}");
    }
    server.request_shutdown();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The world's own metadata, which is what the client's world list
    // reads: without it the directory is not a world at all.
    std::fs::write(
        std::path::Path::new(&dir).join("world.toml"),
        "name = \"waterfall\"\nseed = 1337\npreset = \"test\"\nlast_played = 0\n",
    )
    .expect("world.toml");
}
