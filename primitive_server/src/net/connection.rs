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

use primitive_shared::net::{read_message, write_frame, write_message, NetError};
use primitive_shared::protocol::{
    sanitize_chat, sanitize_username, BlockChange, ClientMessage, DisconnectReason, ServerMessage,
    PROTOCOL_VERSION,
};
use primitive_shared::types::{ChunkPos, Placement, BLOCK_AIR};

use crate::logic::anticheat::{AntiCheat, Verdict};
use crate::net::players::{AdmissionError, Outgoing, PlayerHandle};
use crate::Context;

/// Upper bound on a single batched chunk request, so a malicious client
/// can't hand us a million-entry vector to iterate.
const MAX_CHUNK_REQUEST_BATCH: usize = 2048;

/// Upper bound on one `Craft` message, for the same reason: the count
/// comes off the wire, and the loop it drives holds the player's lock.
const MAX_CRAFTS_PER_REQUEST: u8 = 64;

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

    if let Err(e) = outcome {
        if ctx.options.logging {
            eprintln!("[net] {addr} connection error: {e}")
        }
    }
}

/// One session, from the first byte to the last.
///
/// Departure -- taking the handle out of the registry, telling the
/// plugins and telling everyone else -- happens *here* rather than in
/// the caller, and that is not tidiness either. It used to be the
/// caller's job, keyed off a player id returned on success, and both
/// halves of that were wrong:
///
/// * A connection that ended in an **error** never came back with an id,
///   so nothing ever removed the handle. The player stayed online for
///   ever: in `/list`, in every broadcast, counted against
///   `max_players`, and -- now that a name may only be used once at a
///   time -- holding their own name against themselves for the life of
///   the server.
/// * Even on the happy path, removal waited behind the **writer's drain
///   timeout**, up to two seconds spent letting a socket that has
///   already closed have its say. Somebody whose game crashed sat
///   locked out of their own world for that long.
///
/// Both disappear once the session ends itself: the removal is on the
/// one path every ending goes through.
async fn run_connection(
    ctx: Arc<Context>,
    socket: TcpStream,
    addr: SocketAddr,
) -> Result<(), NetError> {
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
            return Ok(());
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
            return Ok(());
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
        return Ok(());
    }

    let username = sanitize_username(&username);
    let id = ctx.registry.allocate_id();
    let spawn = ctx.world.spawn_point();

    // Who this is, and what they had when they last left. A first visit
    // gets a fresh record at spawn with an empty pack; a returning
    // player gets their own back, at the place they logged out.
    let restored = {
        let mut profiles = ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
        profiles.restore(&username, spawn, crate::logic::survival::MAX_HEALTH)
    };
    // Where they logged out, if they can still stand there.
    //
    // The world moves under a saved position -- somebody builds on it,
    // sand falls on it, or the generator changes and grows a tree
    // exactly where they were standing -- and coming back *inside* a
    // block is not a cosmetic problem: every direction out of a block
    // you are already in is blocked by that same block, so the player
    // is welded in place and dying does not help. See
    // `World::safe_position`.
    let start = ctx.world.safe_position(restored.position);

    let (tx, mut rx) = mpsc::channel::<Outgoing>(ctx.settings.outgoing_queue_capacity);
    let (chunk_tx, chunk_rx) = mpsc::channel::<ChunkPos>(ctx.settings.chunk_queue_capacity);

    let mut player = PlayerHandle::new(
        id,
        username.clone(),
        addr,
        tx,
        chunk_tx,
        ctx.settings.slow_client_drop_threshold,
        start,
        AntiCheat::new(
            ctx.settings.anticheat.clone(),
            ctx.settings.view_distance_chunks,
            start,
        ),
    );
    player.uuid = Some(restored.uuid);
    {
        // Seeded before anyone can see the handle: the tick loop reads
        // this state, and a player who flickers through spawn with an
        // empty pack before their own arrives is a visible glitch and a
        // window in which their things are briefly not theirs.
        let mut state = player.state.lock().unwrap_or_else(|e| e.into_inner());
        state.yaw = restored.yaw;
        state.pitch = restored.pitch;
        state.inventory = restored.inventory;
        state.selected_slot = restored.selected_slot;
        state.vitals.set_health(restored.health);
        state.inventory_dirty = true;
    }
    let handle = Arc::new(player);
    // One session per identity, decided under the registry's own lock.
    // Two copies of the game logged in under one name are two copies of
    // one rucksack, and tipping both into a chest doubles it -- see
    // `Registry::insert_unique`, which also says why this refuses the
    // newcomer rather than kicking the session already in.
    if !ctx.registry.insert_unique(Arc::clone(&handle)) {
        if ctx.options.logging {
            println!("[net] refusing {username} from {addr}: already logged in");
        }
        let _ = write_message(
            &mut write_half,
            &ServerMessage::Rejected(DisconnectReason::Other(
                "that name is already logged in".to_string(),
            )),
        )
        .await;
        return Ok(());
    }

    handle.send(ServerMessage::Welcome {
        your_id: id,
        protocol_version: PROTOCOL_VERSION,
        server_name: ctx.settings.server_name.clone(),
        tick_rate_hz: ctx.settings.tick_rate_hz,
        view_distance_chunks: ctx.settings.view_distance_chunks,
        world_seed: ctx.world.seed(),
        // Where *this* player starts, which is where they left off.
        spawn: start,
        time_of_day: ctx.clock.time_of_day(),
        day_length_seconds: ctx.settings.day_length_seconds,
    });

    // The starting health, so the bar is populated before the player has
    // had a chance to hurt themselves. Everything after this is sent
    // only when the value changes.
    handle.send(ServerMessage::Health {
        current: restored.health,
        max: crate::logic::survival::MAX_HEALTH,
    });
    // ...and the (empty) inventory, so the bar is drawn from real state
    // rather than from the client's guess at what it starts with.
    crate::send_inventory(&handle);

    if ctx.options.logging {
        println!(
            "[net] {username} ({}) connected from {addr} ({} online, {})",
            restored.uuid,
            ctx.registry.len(),
            if restored.returning { "returning" } else { "first visit" },
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
            crate::logic::plugins::Value::Int(id as i64),
            crate::logic::plugins::Value::Text(username.clone()),
        ],
        None,
    );

    // ---- writer ----
    // Two shapes in the queue, one framing between them: a per-recipient
    // message is serialised here, a broadcast arrives as bytes that were
    // serialised once for everyone. See `Outgoing`.
    let writer = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            let written = match out {
                Outgoing::Message(msg) => write_message(&mut write_half, &msg).await,
                Outgoing::Raw(frame) => write_frame(&mut write_half, &frame).await,
            };
            if written.is_err() {
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

    // Their pack and their place of exit, the moment the connection is
    // over -- not after the teardown below. Waiting costs seconds (the
    // writer's queue only closes once the registry drops its handle),
    // and someone who reconnects inside that window would be restored
    // from a profile that had not been written yet.
    crate::store_profile(&ctx, &handle);

    // ...and out of the registry immediately after, for the same reason
    // and one more. Until this happens the player is still online as far
    // as everything else on the server is concerned: they answer to
    // `/list`, they count against `max_players`, and they hold their own
    // name against a reconnect. See this function's own note.
    if ctx.registry.remove(id).is_some() {
        crate::fire_plugin_hook(
            &ctx,
            "on_leave",
            vec![
                crate::logic::plugins::Value::Int(id as i64),
                crate::logic::plugins::Value::Text(username.clone()),
            ],
            None,
        );
        if ctx.options.logging {
            let (sent, dropped) = handle.stats();
            println!(
                "[net] {username} (#{id}, {addr}) disconnected after {:.0}s -- sent {sent}, dropped {dropped}",
                handle.joined_at.elapsed().as_secs_f32()
            );
        }
        ctx.registry.broadcast(ServerMessage::PlayerLeft { id });
    }

    pump.abort();
    drop(handle); // release our clone so the writer's queue can close
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;

    read_result
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
                    crate::logic::plugins::Value::Int(handle.id as i64),
                    crate::logic::plugins::Value::Int(global_x as i64),
                    crate::logic::plugins::Value::Int(global_y as i64),
                    crate::logic::plugins::Value::Int(global_z as i64),
                ];
                if block_id != BLOCK_AIR {
                    hook_args.push(crate::logic::plugins::Value::Int(block_id as i64));
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
                    if let Some(occupant) = ctx.registry.player_occupying_block(
                        global_x,
                        global_y,
                        global_z,
                        block_id,
                    ) {
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

                // Rock, ore and standing timber cannot be taken apart
                // with bare hands, and the server is where that is true.
                //
                // The client already refuses to start swinging at one,
                // so an honest player never sends this; a modified one
                // would otherwise quarry a hillside by asking politely.
                //
                // **What is in the selected slot is part of the
                // question** now that there are tools. The server reads
                // it from its own copy of the inventory rather than
                // taking the client's word for what it is holding: a
                // claimed iron pick would otherwise be the cheapest one
                // in the game.
                if block_id == BLOCK_AIR {
                    let target = ctx
                        .world
                        .cached_block(global_x, global_y, global_z)
                        .unwrap_or(BLOCK_AIR);
                    let held = {
                        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let slot = state.selected_slot;
                        state.inventory.block_in(slot)
                    };
                    if target != BLOCK_AIR
                        && !primitive_shared::types::is_breakable_with(target, held)
                    {
                        handle.send(ServerMessage::Error(
                            "you need a better tool for that".to_string(),
                        ));
                        continue;
                    }
                }

                // Nothing that needs the ground may be hung in the air.
                //
                // The generator has always refused to plant grass on
                // rock, and mining the ground out from under a plant has
                // taken the plant with it since collapse existed -- but
                // *placing* one had no such rule, so a player could
                // build a tuft of grass into the sky by hand. Three
                // places asking the same question and one of them not
                // asking it at all.
                //
                // Checked before the item is spent, so a refusal costs
                // nothing.
                if primitive_shared::types::needs_support(block_id) {
                    let under = ctx
                        .world
                        .cached_block(global_x, global_y - 1, global_z)
                        .unwrap_or(BLOCK_AIR);
                    if !primitive_shared::types::can_grow_on(block_id, under) {
                        handle.send(ServerMessage::Error(
                            "that needs solid ground under it".to_string(),
                        ));
                        continue;
                    }
                }

                // The inventory decides what an edit costs and what it
                // yields. This is the whole point of the inventory being
                // server-side: a placement spends a real block, and a
                // break produces a real one, neither on the client's word.
                let was = ctx.world.cached_block(global_x, global_y, global_z);
                // What a loose material is allowed to become here, and
                // whether it costs anything. See `layer_placement`.
                let layering = if block_id == BLOCK_AIR {
                    Placement::Thicken // costs nothing; breaking never does
                } else {
                    match primitive_shared::types::layer_placement(
                        was.unwrap_or(BLOCK_AIR),
                        block_id,
                    ) {
                        Some(verdict) => verdict,
                        None => {
                            handle.send(ServerMessage::Error(
                                "that does not go there".to_string(),
                            ));
                            if let Some(actual) = was {
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
                };
                if block_id == BLOCK_AIR {
                    // Breaking. Nothing is credited here -- the drop
                    // goes into the world and is picked up by walking
                    // over it.
                } else if matches!(layering, Placement::Thicken) {
                    // Thickening material the cell was already paid for
                    // (see `types::layer_placement`), so it costs
                    // nothing -- but you still have to be *holding* the
                    // stuff. Free is not the same as out of nothing: a
                    // modified client that skipped this could fill in
                    // every drift in the world with an empty pack.
                    let carrying = {
                        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let slot = state.selected_slot;
                        state.inventory.block_in(slot)
                            == Some(primitive_shared::types::block_kind(block_id))
                    };
                    if !carrying {
                        handle.send(ServerMessage::Error(
                            "you are not carrying that".to_string(),
                        ));
                        continue;
                    }
                } else {
                    // What comes out of the pack is the *kind*: a log
                    // is a log however the player chose to lay it, and
                    // an inventory that told three kinds of log apart
                    // would spend three slots on one material.
                    let carried = primitive_shared::types::block_kind(block_id);
                    let spent = {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let slot = state.selected_slot;
                        if state.inventory.block_in(slot) == Some(carried)
                            && state.inventory.take_from(slot, 1) == 1
                        {
                            state.inventory_dirty = true;
                            true
                        } else {
                            false
                        }
                    };
                    if !spent {
                        handle.send(ServerMessage::Error(
                            "you are not carrying that".to_string(),
                        ));
                        continue;
                    }
                    crate::send_inventory(&handle);
                }

                if !ctx.world.set_block(global_x, global_y, global_z, block_id) {
                    handle.send(ServerMessage::Error("block edit out of bounds".to_string()));
                    // The block was already taken out of the pack, so it
                    // has to go back in. Refusing after spending is how
                    // players quietly lose things. Nothing was spent on
                    // a layer added to material already in the cell, so
                    // there is nothing to give back for one.
                    if block_id != BLOCK_AIR && matches!(layering, Placement::Fresh) {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        state
                            .inventory
                            .add(primitive_shared::types::block_kind(block_id), 1);
                        state.inventory_dirty = true;
                        drop(state);
                        crate::send_inventory(&handle);
                    }
                    continue;
                }
                ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);

                if block_id == BLOCK_AIR {
                    if let Some(broken) = was {
                        // A chest goes with what is inside it. Emptied
                        // *after* the block is gone, so the stacks land
                        // in a cell that is now air rather than inside
                        // the block they came out of.
                        if primitive_shared::types::is_container(broken) {
                            crate::spill_chest(&ctx, (global_x, global_y, global_z));
                        }
                        crate::spawn_block_drop(
                            &ctx,
                            broken,
                            (global_x, global_y, global_z),
                        );
                    }
                }

                // Sand above or at this cell may now be unsupported.
                {
                    let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                    sim.on_block_changed(global_x, global_y, global_z);
                    crate::notify_mechanics(&ctx, global_x, global_y, global_z);
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

                // Whatever was growing on this cell may have just lost
                // the ground under it -- dig out the dirt and the tuft
                // of grass on top comes with it, rather than hanging in
                // the air. Same chunk by construction (straight up), so
                // it goes to the same subscribers.
                for fallen in crate::collapse_unsupported(&ctx, global_x, global_y, global_z) {
                    for subscriber in ctx.registry.subscribers(chunk_pos) {
                        subscriber.send(ServerMessage::BlockUpdate(fallen));
                    }
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

                // Only positions the anti-cheat accepted feed the fall
                // tracker. Letting a rejected one through would mean a
                // client could claim a 60-block drop it never made and
                // then be "killed" by it -- or, more usefully to a
                // cheat, claim to be on the ground forever and never
                // fall at all.
                if verdict.is_allowed() {
                    // Enough water to land in, not merely a cell that
                    // has some in it. Water comes in eighths now, and a
                    // film left behind by a receding puddle is not
                    // something that breaks a fall -- which, before this
                    // asked how deep it was, was a free landing pad
                    // anywhere water had ever been.
                    let landed_in_liquid = ctx
                        .world
                        .cached_block(x.floor() as i32, y.floor() as i32, z.floor() as i32)
                        .is_some_and(|block| primitive_shared::fluid::covers(block, 0.5));
                    let outcome = {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        // Weight comes straight out of the inventory the
                        // server is holding, so there is nothing here for
                        // a client to assert or for the two sides to
                        // disagree about.
                        let carried = state.inventory.total_weight();
                        state.vitals.set_carried_weight(carried);
                        state.vitals.on_transform(y, on_ground, landed_in_liquid)
                    };
                    crate::report_vitals(&ctx, &handle, outcome);
                }

                match verdict {
                    Verdict::Allow => {}
                    Verdict::Reject { reason, correction } => {
                        ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
                        if let Some(pos) = correction {
                            let mut state =
                                handle.state.lock().unwrap_or_else(|e| e.into_inner());
                            state.position = pos;
                            state.anticheat.reset_to(pos);
                            // A rubber-band can move a player several
                            // blocks downwards. Without this, being
                            // corrected would arrive as fall damage.
                            state.vitals.clear_fall();
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
                        crate::logic::plugins::Value::Int(handle.id as i64),
                        crate::logic::plugins::Value::Text(text.clone()),
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
                    // Looked up per command rather than remembered from
                    // the handshake, so a player who is made an operator
                    // mid-session has the rights on their very next line
                    // instead of on their next login -- and one who is
                    // demoted loses them just as promptly, which is the
                    // half that has to be immediate.
                    let permission = match handle.uuid {
                        Some(uuid) => {
                            let profiles =
                                ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
                            if profiles.is_operator(uuid) {
                                crate::logic::commands::Permission::Operator
                            } else {
                                crate::logic::commands::Permission::Player
                            }
                        }
                        // No profile, no authority. Unreachable in
                        // practice -- the UUID is set before the handle
                        // is published -- but the fallback that costs
                        // nothing is the one that grants nothing.
                        None => crate::logic::commands::Permission::Player,
                    };
                    for reply in crate::run_command(&ctx, &text, permission, Some(handle.id)) {
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

            ClientMessage::SelectSlot { slot } => {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.selected_slot =
                    (slot as usize).min(primitive_shared::inventory::HOTBAR_SLOTS - 1);
            }

            // The four rearrange-my-pack messages all end the same way:
            // do it, mark the inventory dirty whatever the answer was,
            // and push the result back. Dirty even when nothing moved,
            // so a client whose screen disagrees with the server is put
            // straight by the next click rather than staying wrong.
            ClientMessage::MoveSlots { from, to } => {
                {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.inventory.move_or_merge(from as usize, to as usize);
                    state.inventory_dirty = true;
                }
                crate::send_inventory(&handle);
            }

            ClientMessage::SplitSlot { from, to } => {
                {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.inventory.split_into(from as usize, to as usize);
                    state.inventory_dirty = true;
                }
                crate::send_inventory(&handle);
            }

            ClientMessage::QuickMoveSlot { slot } => {
                {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.inventory.quick_move(slot as usize);
                    state.inventory_dirty = true;
                }
                crate::send_inventory(&handle);
            }

            ClientMessage::SortInventory => {
                {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.inventory.sort_storage();
                    state.inventory_dirty = true;
                }
                crate::send_inventory(&handle);
            }

            ClientMessage::DropSlot { slot, whole_stack } => {
                crate::drop_from_slot(&ctx, &handle, slot as usize, whole_stack);
            }

            ClientMessage::Craft { index, times } => {
                let made = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    match primitive_shared::crafting::recipe(index as usize) {
                        // Repeated until the ingredients or the room run
                        // out. Bounded by `times`, which the client caps
                        // as well -- a loop whose length a client picks
                        // is a loop a client can make expensive.
                        Some(recipe) => {
                            let mut made = 0u32;
                            for _ in 0..times.min(MAX_CRAFTS_PER_REQUEST) {
                                if !primitive_shared::crafting::craft(&mut state.inventory, recipe) {
                                    break;
                                }
                                made += 1;
                            }
                            state.inventory_dirty |= made > 0;
                            made > 0
                        }
                        None => false,
                    }
                };
                if made {
                    crate::send_inventory(&handle);
                } else {
                    handle.send(ServerMessage::Error("cannot make that".to_string()));
                }
            }

            ClientMessage::OpenChest {
                global_x,
                global_y,
                global_z,
            } => {
                crate::open_chest(&ctx, &handle, (global_x, global_y, global_z));
            }

            ClientMessage::CloseChest => {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.open_chest = None;
            }

            ClientMessage::ChestMove { from, to, half } => {
                crate::chest_move(&ctx, &handle, from, to, half);
            }

            ClientMessage::ChestQuickMove { side, slot } => {
                crate::chest_quick_move(&ctx, &handle, side, slot);
            }

            ClientMessage::ChestBulkMove { to_chest } => {
                crate::chest_bulk_move(&ctx, &handle, to_chest);
            }

            ClientMessage::Attack { target } => {
                crate::melee_attack(&ctx, &handle, target);
            }

            ClientMessage::Respawn => {
                // Ignored unless they are actually dead, so a client
                // cannot use this as a free teleport home.
                let dead = {
                    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.vitals.is_dead()
                };
                if dead {
                    crate::respawn_player(&ctx, &handle);
                }
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

            // A reference count, not 32 KB of block array. The cache
            // already owns this chunk and nobody is going to change it.
            if !handle.send(ServerMessage::ChunkData(Arc::clone(&chunk))) {
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
