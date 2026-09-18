//! **A real world at speed, walked through by one player.**
//!
//! Every other test here builds the corner of the world it is about -- a
//! bed in the test field, a raft on a pond -- and asks one question of it.
//! What none of them can see is what happens when everything that has landed
//! runs *together*, on the terrain the game actually makes: gulls over a real
//! coast, fish in a real sea, frogs and injuries and growth and weather all
//! ticking at once while the clock races. That is where a NaN is born in one
//! system and read by another, where a spawner that is capped per species is
//! uncapped in sum, and where a tick loop that is fine with a meadow stops
//! answering in a swamp.
//!
//! So this walks a player from the spawn to a beach, out over open sea and
//! into a swamp -- places found by asking the generator, not built -- with a
//! day twenty seconds long, and holds three things against everything the
//! server says on the way:
//!
//! * **every number is a number**: no entity, animal facing, or player in a
//!   snapshot is ever NaN or infinite;
//! * **the server keeps talking**: the connection stays up and messages keep
//!   arriving at every stop. Messages, not snapshots: a snapshot carries the
//!   *other* players in view and is not sent at all to a player alone, which
//!   the first version of this test mistook for a server gone quiet;
//! * **the living world stays bounded**: no single entity update carries more
//!   than a world could sensibly hold round one player.
//!
//! Six seconds a stop by default, so it earns its place in the ordinary run;
//! `PRIMITIVE_SOAK_SECONDS=<n>` walks longer.

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use primitive_server::settings::{AntiCheatSettings, ServerSettings};
use primitive_server::RunOptions;
use primitive_shared::net::{read_message, write_message};
use primitive_shared::protocol::{ClientMessage, EntityKind, ServerMessage, PROTOCOL_VERSION};
use primitive_shared::types::ChunkPos;
use primitive_shared::worldgen::{Biome, WorldGen, SEA_LEVEL};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const SEED: u32 = 1337;

/// More entities than this in one update, round one player, is a spawner
/// that has lost count. Generous on purpose: the test is for a runaway, and
/// the ordinary numbers are printed.
const MOST_ENTITIES_ROUND_ONE_PLAYER: usize = 1500;

fn soak_settings() -> ServerSettings {
    ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "soak".to_string(),
        world_seed: SEED,
        // Nowhere to save to, and nothing loaded from beside a real world.
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        stats_interval_secs: 0.0,
        pregenerate_radius_chunks: 0,
        // A day in twenty seconds: the walk crosses dawn and dusk, when the
        // animals change what they are doing, more than once.
        day_length_seconds: 20.0,
        // The walk jumps between stops a long way apart; the movement
        // validator has its own tests.
        anticheat: AntiCheatSettings { enabled: false, ..Default::default() },
        ..Default::default()
    }
}

/// The nearest column the generator answers `wanted` for, on rings out from
/// the origin.
fn find(gen: &WorldGen, wanted: impl Fn(Biome, i32) -> bool) -> Option<(i32, i32)> {
    for ring in 1..60 {
        let radius = ring as f32 * 32.0;
        let samples = ring * 6;
        for step in 0..samples {
            let angle = step as f32 / samples as f32 * std::f32::consts::TAU;
            let (x, z) = ((angle.cos() * radius) as i32, (angle.sin() * radius) as i32);
            if wanted(gen.biome_at(x, z), gen.height_at(x, z)) {
                return Some((x, z));
            }
        }
    }
    None
}

/// Everything wrong seen so far, and what was counted on the way.
#[derive(Default)]
struct Watch {
    wrong: Vec<String>,
    most_entities: usize,
    most_of_a_kind: BTreeMap<String, usize>,
    ids: HashSet<u64>,
    snapshots: usize,
    entity_updates: usize,
    deaths: usize,
    /// Every message of any kind, which is what says the server is alive.
    heard: usize,
}

impl Watch {
    fn not_a_number(&mut self, what: String) {
        if self.wrong.len() < 12 {
            self.wrong.push(what);
        }
    }

    fn read(&mut self, message: &ServerMessage) {
        self.heard += 1;
        match message {
            ServerMessage::Entities { states, .. } => {
                self.entity_updates += 1;
                self.most_entities = self.most_entities.max(states.len());
                let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                for s in states {
                    self.ids.insert(s.id);
                    if ![s.x, s.y, s.z].iter().all(|v| v.is_finite()) {
                        self.not_a_number(format!("entity {} ({:?}) at ({}, {}, {})", s.id, s.kind, s.x, s.y, s.z));
                    }
                    if let EntityKind::Animal { species, yaw, hurt, .. } = s.kind {
                        if !yaw.is_finite() || !hurt.is_finite() {
                            self.not_a_number(format!("{species:?} {} faces {yaw} hurt {hurt}", s.id));
                        }
                        *kinds.entry(format!("{species:?}")).or_default() += 1;
                    }
                }
                for (kind, count) in kinds {
                    let most = self.most_of_a_kind.entry(kind).or_default();
                    *most = (*most).max(count);
                }
            }
            ServerMessage::Snapshot { states, .. } => {
                self.snapshots += 1;
                for p in states {
                    if ![p.x, p.y, p.z, f64::from(p.yaw), f64::from(p.pitch)].iter().all(|v| v.is_finite()) {
                        self.not_a_number(format!("player {} at ({}, {}, {}) looking {} {}", p.id, p.x, p.y, p.z, p.yaw, p.pitch));
                    }
                }
            }
            ServerMessage::Died { .. } => self.deaths += 1,
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_real_world_at_speed_never_says_a_number_that_is_not_one_and_never_stops_talking() {
    let gen = WorldGen::new(SEED);
    let mut stops: Vec<(&str, (i32, i32))> = Vec::new();
    for (name, wanted) in [
        ("beach", Box::new(|b: Biome, _h: i32| b == Biome::Beach) as Box<dyn Fn(Biome, i32) -> bool>),
        ("open sea", Box::new(|b: Biome, h: i32| b == Biome::Ocean && h < SEA_LEVEL - 8)),
        ("swamp", Box::new(|b: Biome, _h: i32| b == Biome::Swamp)),
    ] {
        match find(&gen, wanted) {
            Some(at) => stops.push((name, at)),
            None => println!("soak: no {name} within two thousand blocks of seed {SEED}; walking past it"),
        }
    }

    let server = primitive_server::start(soak_settings(), RunOptions::embedded()).await.expect("start");
    let mut socket = TcpStream::connect(server.address()).await.expect("connect");
    write_message(&mut socket, &ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, username: "walker".to_string() })
        .await
        .expect("hello");
    let spawn = match read_message::<_, ServerMessage>(&mut socket).await.expect("welcome") {
        ServerMessage::Welcome { spawn, .. } => spawn,
        other => panic!("expected Welcome, got {other:?}"),
    };
    stops.insert(0, ("spawn", (spawn.0 as i32, spawn.2 as i32)));

    // **Reading on its own task.** A timeout around `read_message` can cut a
    // frame in half and leave the next read starting in the middle of one;
    // a channel's receive is safe to give up on.
    let (mut reading, mut writing) = socket.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();
    let reader = tokio::spawn(async move {
        while let Ok(message) = read_message::<_, ServerMessage>(&mut reading).await {
            if tx.send(message).is_err() {
                break;
            }
        }
    });

    let per_stop = Duration::from_secs_f32(
        std::env::var("PRIMITIVE_SOAK_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(6.0),
    );
    let mut watch = Watch::default();
    let mut sequence = 0u32;
    let mut closed = false;

    for (name, (sx, sz)) in &stops {
        let height = gen.height_at(*sx, *sz);
        // At the surface over water, on the ground elsewhere.
        let (floor, on_ground) =
            if height < SEA_LEVEL { (SEA_LEVEL as f32 + 0.2, false) } else { (height as f32 + 1.0, true) };
        let centre = ChunkPos::from_world(*sx as f32, *sz as f32);
        for dz in -3..=3 {
            for dx in -3..=3 {
                let pos = ChunkPos::new(centre.x + dx, centre.z + dz);
                if write_message(&mut writing, &ClientMessage::RequestChunk(pos)).await.is_err() {
                    closed = true;
                }
            }
        }
        let (snapshots_before, heard_before, updates_before) = (watch.snapshots, watch.heard, watch.entity_updates);
        let started = Instant::now();
        while started.elapsed() < per_stop && !closed {
            // A slow circle round the stop, so the player is a moving thing
            // animals notice and flee from.
            let turn = started.elapsed().as_secs_f32() * 0.6;
            sequence += 1;
            let step = ClientMessage::UpdateTransform {
                x: f64::from(*sx as f32 + 0.5 + turn.cos() * 3.0),
                y: f64::from(floor),
                z: f64::from(*sz as f32 + 0.5 + turn.sin() * 3.0),
                yaw: turn,
                pitch: 0.0,
                on_ground,
                sequence,
            };
            if write_message(&mut writing, &step).await.is_err() {
                closed = true;
                break;
            }
            loop {
                match rx.try_recv() {
                    Ok(message) => watch.read(&message),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        closed = true;
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let heard = watch.heard - heard_before;
        println!(
            "soak: {name} at ({sx}, {sz}), ground {height}: {heard} messages, {} entity updates, {} snapshots, most entities in one update so far {}",
            watch.entity_updates - updates_before,
            watch.snapshots - snapshots_before,
            watch.most_entities
        );
        assert!(!closed, "the server closed the connection at the {name}");
        assert!(heard > 0, "the server said nothing at all at the {name}");
    }

    println!(
        "soak: {} entity updates, {} distinct entities, most at once {}, deaths {}, most of each kind in one update {:?}",
        watch.entity_updates,
        watch.ids.len(),
        watch.most_entities,
        watch.deaths,
        watch.most_of_a_kind
    );
    assert!(watch.wrong.is_empty(), "numbers that were not numbers:\n{}", watch.wrong.join("\n"));
    assert!(
        watch.most_entities <= MOST_ENTITIES_ROUND_ONE_PLAYER,
        "{} entities in one update round one player: {:?}",
        watch.most_entities,
        watch.most_of_a_kind
    );

    reader.abort();
    server.stop().await;
}
