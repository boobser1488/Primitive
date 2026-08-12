//! One connected client: handshake, read loop, writer task, chunk pump.
//!
//! Task layout per client (three tasks, not one):
//! - **reader** — parses client messages, runs them past the anti-cheat,
//!   applies the survivors. Never writes to the socket.
//! - **writer** — drains the bounded outgoing queue onto the socket. A
//!   client that stops reading backs this up; the queue is bounded, so
//!   the pressure shows up as dropped messages and eventually a kick,
//!   never as unbounded server memory.
//! - **chunk pump** — serves that client's chunk requests at a fixed
//!   budget per tick. Terrain generation is CPU-bound, so it runs on
//!   `spawn_blocking` rather than on an async worker: generating a chunk
//!   inline would block a runtime thread that dozens of other players'
//!   sockets are sharing.
//!
//! Splitting them is what stops one player's problem from becoming
//! everyone's: a huge chunk backlog can't delay movement snapshots, and a
//! stalled socket can't delay the world.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use primitive_shared::net::{read_message, write_message, NetError};
use primitive_shared::protocol::{
    sanitize_chat, sanitize_username, BlockChange, ClientMessage, DisconnectReason, PlayerId,
    ServerMessage, PROTOCOL_VERSION,
};
use primitive_shared::types::{ChunkPos, BLOCK_AIR};

use crate::anticheat::{AntiCheat, Verdict};
use crate::players::{AdmissionError, PlayerHandle};
use crate::Context;

/// Upper bound on a single batched chunk request, so a malicious client
/// can't hand us a million-entry vector to iterate.
const MAX_CHUNK_REQUEST_BATCH: usize = 2048;

pub async fn handle_connection(ctx: Arc<Context>, socket: TcpStream, addr: SocketAddr) {
    // Small writes, sent immediately: this is an interactive game, not a
    // bulk transfer, and Nagle's algorithm would add tens of milliseconds
    // to every input.
    let _ = socket.set_nodelay(true);

    if let Err(e) = ctx.registry.admit(addr) {
        let reason = match e {
            AdmissionError::ServerFull => DisconnectReason::ServerFull,
            AdmissionError::TooManyConnectionsFromIp => DisconnectReason::RateLimited,
        };
        let mut socket = socket;
        let _ = write_message(&mut socket, &ServerMessage::Rejected(reason)).await;
        return;
    }

    let outcome = run_connection(Arc::clone(&ctx), socket, addr).await;
    ctx.registry.release(addr);

    match outcome {
        Ok(Some(id)) => {
            if let Some(handle) = ctx.registry.remove(id) {
                crate::fire_plugin_hook(
                    &ctx,
                    "on_leave",
                    vec![
                        crate::plugins::Value::Int(id as i64),
                        crate::plugins::Value::Text(handle.username.clone()),
                    ],
                    None,
                );
                if ctx.options.logging {
                    let (sent, dropped) = handle.stats();
                    println!(
                        "[net] {} (#{id}, {addr}) disconnected after {:.0}s -- sent {sent}, dropped {dropped}",
                        handle.username,
                        handle.joined_at.elapsed().as_secs_f32()
                    );
                }
                ctx.registry
                    .broadcast(ServerMessage::PlayerLeft { id });
            }
        }
        Ok(None) => {}
        Err(e) => {
            if ctx.options.logging {
                eprintln!("[net] {addr} connection error: {e}")
            }
        }
    }
}

/// Returns the player id if the handshake got far enough to register one.
async fn run_connection(
    ctx: Arc<Context>,
    socket: TcpStream,
    addr: SocketAddr,
) -> Result<Option<PlayerId>, NetError> {
    let (mut read_half, mut write_half) = socket.into_split();

    // ---- handshake ----
    // Bounded in time: a connection that opens and then says nothing must
    // not hold a slot indefinitely.
    let hello = tokio::time::timeout(
        Duration::from_secs_f32(ctx.settings.handshake_timeout_secs),
        read_message::<_, ClientMessage>(&mut read_half),
    )
    .await;

    let (username, protocol_version) = match hello {
        Err(_) => {
            let _ = write_message(
                &mut write_half,
                &ServerMessage::Rejected(DisconnectReason::Timeout),
            )
            .await;
            return Ok(None);
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(ClientMessage::Hello {
            protocol_version,
            username,
        })) => (username, protocol_version),
        Ok(Ok(_)) => {
            // Anything before Hello is a protocol violation, not a
            // message to be helpfully interpreted.
            let _ = write_message(
                &mut write_half,
                &ServerMessage::Rejected(DisconnectReason::Other(
                    "expected Hello as the first message".to_string(),
                )),
            )
            .await;
            return Ok(None);
        }
    };

    if protocol_version != PROTOCOL_VERSION {
        let _ = write_message(
            &mut write_half,
            &ServerMessage::Rejected(DisconnectReason::ProtocolMismatch {
                server_version: PROTOCOL_VERSION,
            }),
        )
        .await;
        return Ok(None);
    }

    let username = sanitize_username(&username);
    let id = ctx.registry.allocate_id();
    let spawn = ctx.world.spawn_point();

    let (tx, mut rx) = mpsc::channel::<ServerMessage>(ctx.settings.outgoing_queue_capacity);
    let (chunk_tx, chunk_rx) = mpsc::channel::<ChunkPos>(ctx.settings.chunk_queue_capacity);

    let handle = Arc::new(PlayerHandle::new(
        id,
        username.clone(),
        addr,
        tx,
        chunk_tx,
        ctx.settings.slow_client_drop_threshold,
        spawn,
        AntiCheat::new(
            ctx.settings.anticheat.clone(),
            ctx.settings.view_distance_chunks,
            spawn,
        ),
    ));
    ctx.registry.insert(Arc::clone(&handle));

    handle.send(ServerMessage::Welcome {
        your_id: id,
        protocol_version: PROTOCOL_VERSION,
        server_name: ctx.settings.server_name.clone(),
        tick_rate_hz: ctx.settings.tick_rate_hz,
        view_distance_chunks: ctx.settings.view_distance_chunks,
        world_seed: ctx.world.seed(),
        spawn,
        time_of_day: ctx.clock.time_of_day(),
        day_length_seconds: ctx.settings.day_length_seconds,
    });

    if ctx.options.logging {
        println!(
            "[net] {username} (#{id}) connected from {addr} ({} online)",
            ctx.registry.len()
        );
    }
    ctx.registry.broadcast_except(
        id,
        ServerMessage::PlayerJoined {
            id,
            username: username.clone(),
        },
    );
    crate::fire_plugin_hook(
        &ctx,
        "on_join",
        vec![
            crate::plugins::Value::Int(id as i64),
            crate::plugins::Value::Text(username.clone()),
        ],
        None,
    );

    // ---- writer ----
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write_message(&mut write_half, &msg).await.is_err() {
                break;
            }
        }
        // Best-effort: let the peer see the close rather than a reset.
        let _ = write_half.shutdown().await;
    });

    // ---- chunk pump ----
    let pump = tokio::spawn(chunk_pump(
        Arc::clone(&ctx),
        Arc::clone(&handle),
        chunk_rx,
    ));

    // ---- reader, racing against any kick request ----
    let read_result = tokio::select! {
        result = read_loop(Arc::clone(&ctx), Arc::clone(&handle), &mut read_half) => result,
        reason = handle.kicked() => {
            if ctx.options.logging {
                println!("[net] kicking {username} (#{id}): {reason}");
            }
            ctx.metrics.kicks.fetch_add(1, Ordering::Relaxed);
            // Try to tell them why before the socket closes. The queue may
            // be full (that's often *why* they're being kicked), so this
            // is genuinely best-effort.
            handle.send(ServerMessage::Kick(reason));
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(())
        }
    };

    pump.abort();
    drop(handle); // release our clone so the writer's queue can close
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;

    read_result.map(|()| Some(id))
}

async fn read_loop(
    ctx: Arc<Context>,
    handle: Arc<PlayerHandle>,
    read_half: &mut tokio::net::tcp::OwnedReadHalf,
) -> Result<(), NetError> {
    loop {
        let msg: ClientMessage = match read_message(read_half).await {
            Ok(m) => m,
            Err(NetError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        ctx.metrics.messages_in.fetch_add(1, Ordering::Relaxed);
        handle.touch();

        // Global rate limit first: it's the cheapest check, and it's the
        // one that has to hold when everything else is under attack.
        {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Verdict::Kick(reason) = state.anticheat.check_message() {
                drop(state);
                handle.request_kick(DisconnectReason::AntiCheat(reason));
                return Ok(());
            }
        }

        match msg {
            ClientMessage::Hello { .. } => {
                // A second Hello mid-session is nonsense; treat it as a
                // protocol error rather than re-running the handshake.
                handle.request_kick(DisconnectReason::Other(
                    "duplicate handshake".to_string(),
                ));
                return Ok(());
            }

            ClientMessage::RequestChunk(pos) => {
                request_chunk(&ctx, &handle, pos);
            }

            ClientMessage::RequestChunks(list) => {
                if list.len() > MAX_CHUNK_REQUEST_BATCH {
                    handle.request_kick(DisconnectReason::AntiCheat(format!(
                        "chunk request batch of {} entries",
                        list.len()
                    )));
                    return Ok(());
                }
                for pos in list {
                    if !request_chunk(&ctx, &handle, pos) {
                        break; // kicked or rate-limited; stop early
                    }
                }
            }

            ClientMessage::SetBlock {
                global_x,
                global_y,
                global_z,
                block_id,
            } => {
                let verdict = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state
                        .anticheat
                        .check_block_edit(global_x, global_y, global_z, block_id)
                };
                match verdict {
                    Verdict::Allow => {}
                    Verdict::Reject { reason, .. } => {
                        ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
                        handle.send(ServerMessage::Error(format!("edit refused: {reason}")));
                        // Tell the client what's actually there, so a
                        // refused edit doesn't leave it desynced.
                        if let Some(actual) = ctx.world.cached_block(global_x, global_y, global_z) {
                            handle.send(ServerMessage::BlockUpdate(BlockChange {
                                global_x,
                                global_y,
                                global_z,
                                block_id: actual,
                            }));
                        }
                        continue;
                    }
                    Verdict::Kick(reason) => {
                        ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
                        handle.request_kick(DisconnectReason::AntiCheat(reason));
                        return Ok(());
                    }
                }

                // Plugin veto. Placement and breaking are separate
                // hooks because protection plugins almost always want
                // to treat them differently.
                let hook = if block_id == BLOCK_AIR {
                    "on_block_break"
                } else {
                    "on_block_place"
                };
                let mut hook_args = vec![
                    crate::plugins::Value::Int(handle.id as i64),
                    crate::plugins::Value::Int(global_x as i64),
                    crate::plugins::Value::Int(global_y as i64),
                    crate::plugins::Value::Int(global_z as i64),
                ];
                if block_id != BLOCK_AIR {
                    hook_args.push(crate::plugins::Value::Int(block_id as i64));
                }
                if !crate::fire_plugin_hook(
                    &ctx,
                    hook,
                    hook_args,
                    Some(vec![(global_x, global_y, global_z)]),
                ) {
                    handle.send(ServerMessage::Error(
                        "a plugin refused that change".to_string(),
                    ));
                    if let Some(actual) = ctx.world.cached_block(global_x, global_y, global_z) {
                        handle.send(ServerMessage::BlockUpdate(BlockChange {
                            global_x,
                            global_y,
                            global_z,
                            block_id: actual,
                        }));
                    }
                    continue;
                }

                // Authoritative "no building inside people" check. The
                // client refuses this locally too, but a modified client
                // could suffocate someone (or trap themselves in a way
                // the server would then have to fix), so the rule lives
                // here as well.
                if block_id != BLOCK_AIR {
                    if let Some(occupant) =
                        ctx.registry.player_occupying_block(global_x, global_y, global_z)
                    {
                        let who = if occupant.id == handle.id {
                            "yourself".to_string()
                        } else {
                            occupant.username.clone()
                        };
                        handle.send(ServerMessage::Error(format!(
                            "can't place a block inside {who}"
                        )));
                        if let Some(actual) = ctx.world.cached_block(global_x, global_y, global_z) {
                            handle.send(ServerMessage::BlockUpdate(BlockChange {
                                global_x,
                                global_y,
                                global_z,
                                block_id: actual,
                            }));
                        }
                        continue;
                    }
                }

                if !ctx.world.set_block(global_x, global_y, global_z, block_id) {
                    handle.send(ServerMessage::Error("block edit out of bounds".to_string()));
                    continue;
                }
                ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);

                // Sand above or at this cell may now be unsupported.
                {
                    let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                    sim.on_block_changed(global_x, global_y, global_z);
                }

                let (chunk_pos, _, _) = ChunkPos::from_global(global_x, global_z);
                let change = BlockChange {
                    global_x,
                    global_y,
                    global_z,
                    block_id,
                };
                // Only players who actually have this chunk loaded, via
                // the reverse index -- not a scan of every player.
                for subscriber in ctx.registry.subscribers(chunk_pos) {
                    subscriber.send(ServerMessage::BlockUpdate(change));
                }
            }

            ClientMessage::UpdateTransform {
                x,
                y,
                z,
                yaw,
                pitch,
                on_ground,
                sequence,
            } => {
                let verdict = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let verdict = state
                        .anticheat
                        .check_transform(x, y, z, on_ground, sequence, &ctx.world);
                    if verdict.is_allowed() {
                        state.position = (x, y, z);
                        state.yaw = yaw;
                        state.pitch = pitch;
                        state.on_ground = on_ground;
                    }
                    verdict
                };

                match verdict {
                    Verdict::Allow => {}
                    Verdict::Reject { reason, correction } => {
                        ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
                        if let Some(pos) = correction {
                            let mut state =
                                handle.state.lock().unwrap_or_else(|e| e.into_inner());
                            state.position = pos;
                            state.anticheat.reset_to(pos);
                            drop(state);
                            handle.send(ServerMessage::PositionCorrection {
                                x: pos.0,
                                y: pos.1,
                                z: pos.2,
                                reason: reason.clone(),
                            });
                        }
                        if ctx.options.logging {
                            println!("[anticheat] {} (#{}) {}", handle.username, handle.id, reason);
                        }
                    }
                    Verdict::Kick(reason) => {
                        ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
                        handle.request_kick(DisconnectReason::AntiCheat(reason));
                        return Ok(());
                    }
                }
            }

            ClientMessage::Chat(text) => {
                let verdict = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.anticheat.check_chat()
                };
                match verdict {
                    Verdict::Kick(reason) => {
                        handle.request_kick(DisconnectReason::AntiCheat(reason));
                        return Ok(());
                    }
                    Verdict::Reject { .. } => continue,
                    Verdict::Allow => {}
                }
                let text = sanitize_chat(&text);
                if text.is_empty() {
                    continue;
                }

                // Plugins get a veto on chat: that's how a mute or a
                // word filter is written.
                if !crate::fire_plugin_hook(
                    &ctx,
                    "on_chat",
                    vec![
                        crate::plugins::Value::Int(handle.id as i64),
                        crate::plugins::Value::Text(text.clone()),
                    ],
                    None,
                ) {
                    continue;
                }

                // A chat line starting with '/' is a command, not chat --
                // same parser and same permission check as the console,
                // just at player level.
                if text.starts_with('/') {
                    if ctx.options.logging {
                        println!("[command] <{}> {text}", handle.username);
                    }
                    for reply in crate::run_command(
                        &ctx,
                        &text,
                        crate::commands::Permission::Player,
                        Some(handle.id),
                    ) {
                        handle.send(ServerMessage::Chat {
                            from: None,
                            username: "server".to_string(),
                            text: reply,
                        });
                    }
                    continue;
                }
                if ctx.options.logging {
                    println!("[chat] <{}> {text}", handle.username);
                }
                ctx.registry.broadcast(ServerMessage::Chat {
                    from: Some(handle.id),
                    username: handle.username.clone(),
                    text,
                });
            }

            ClientMessage::Pong { .. } => {
                // `touch()` above already did the work; the nonce is only
                // useful once we start measuring per-client RTT.
            }

            ClientMessage::Disconnect => return Ok(()),
        }
    }
}

/// Returns false if the caller should stop processing further requests
/// (rate-limited or kicked).
fn request_chunk(ctx: &Arc<Context>, handle: &Arc<PlayerHandle>, pos: ChunkPos) -> bool {
    let verdict = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.anticheat.check_chunk_request(pos)
    };
    match verdict {
        Verdict::Allow => {
            handle.queue_chunk(pos);
            true
        }
        Verdict::Reject { .. } => {
            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
            false
        }
        Verdict::Kick(reason) => {
            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
            handle.request_kick(DisconnectReason::AntiCheat(reason));
            false
        }
    }
}

/// Serves one player's chunk requests at a fixed budget per tick, and
/// prunes their subscriptions as they walk away from old chunks.
async fn chunk_pump(
    ctx: Arc<Context>,
    handle: Arc<PlayerHandle>,
    mut chunk_rx: mpsc::Receiver<ChunkPos>,
) {
    let mut ticker = tokio::time::interval(ctx.settings.tick_duration());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_pruned_chunk: Option<ChunkPos> = None;

    loop {
        ticker.tick().await;

        for _ in 0..ctx.settings.chunk_send_budget_per_tick {
            let pos = match chunk_rx.try_recv() {
                Ok(pos) => pos,
                Err(_) => break,
            };

            // Already cached: no generation needed, so stay on the async
            // worker and skip the thread hop entirely.
            let chunk = match ctx.world.cached(pos) {
                Some(chunk) => chunk,
                None => {
                    let world = Arc::clone(&ctx.world);
                    // Terrain generation is pure CPU work (noise over
                    // 16x64x16 cells). On an async worker it would block
                    // every other socket that worker is driving.
                    let generated =
                        match tokio::task::spawn_blocking(move || world.generate(pos)).await {
                            Ok(chunk) => chunk,
                            Err(_) => return, // runtime shutting down
                        };
                    ctx.world.insert(generated)
                }
            };

            {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.loaded_chunks.insert(pos);
            }
            ctx.registry.subscribe(handle.id, pos);

            if !handle.send(ServerMessage::ChunkData((*chunk).clone())) {
                // Queue full: the client is behind. Drop it and let the
                // client's own retry timer ask again.
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.loaded_chunks.remove(&pos);
                drop(state);
                ctx.registry.unsubscribe(handle.id, pos);
                break;
            }
            ctx.metrics.chunks_sent.fetch_add(1, Ordering::Relaxed);
        }

        // Prune only when the player actually changes chunk -- otherwise
        // this walks a few hundred entries per player per tick for nothing.
        let (px, _, pz) = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            state.position
        };
        let player_chunk = ChunkPos::from_world(px, pz);
        if last_pruned_chunk != Some(player_chunk) {
            last_pruned_chunk = Some(player_chunk);
            let keep = ctx.settings.view_distance_chunks + 2;
            let stale: Vec<ChunkPos> = {
                let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state
                    .loaded_chunks
                    .iter()
                    .filter(|pos| player_chunk.chebyshev_distance(**pos) > keep)
                    .copied()
                    .collect()
            };
            if !stale.is_empty() {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                for pos in &stale {
                    state.loaded_chunks.remove(pos);
                }
                drop(state);
                for pos in stale {
                    ctx.registry.unsubscribe(handle.id, pos);
                }
            }
        }
    }
}
