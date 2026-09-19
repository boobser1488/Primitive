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
use crate::net::players::{frame, AdmissionError, Outgoing, PlayerHandle};
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
        state.vitals.set_nourishment(restored.nourishment);
        state.equipment = restored.body.equipment.clone();
        state.vitals.set_hydration(restored.body.hydration);
        state.vitals.set_warmth(restored.body.body_c, restored.body.wetness);
        // **Tiredness and wounds, which were saved and never put back.**
        // The profile kept both and this block restored neither, so a
        // player who logged out exhausted with a broken leg came back
        // rested and whole -- the exploit the notes on both fields say
        // saving them exists to close.
        state.vitals.set_fatigue(restored.body.fatigue);
        state.vitals.set_injuries(restored.body.injuries);
        // ...and the illness, for the same reason: "если игрок выходит,
        // отравление проходит" was the third meter a reconnect cleared.
        state.vitals.set_sick_for(restored.body.sick_for);
        state.injuries_reported = restored.body.injuries;
        state.inventory_dirty = true;
        state.equipment_dirty = true;
        state.discovered = restored.discovered.clone();
        state.bags = restored.bags.clone();
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
        preset: ctx.world.preset(),
        zone: ctx.world.zone(),
        scale: ctx.world.scale(),
        // Where *this* player starts, which is where they left off.
        spawn: start,
        time_of_day: ctx.clock.time_of_day(),
        world_days: ctx.clock.world_days(),
        day_length_seconds: ctx.settings.day_length_seconds,
    });

    // The starting health, so the bar is populated before the player has
    // had a chance to hurt themselves. Everything after this is sent
    // only when the value changes.
    handle.send(ServerMessage::Health {
        current: restored.health,
        max: crate::logic::survival::MAX_HEALTH,
    });
    // ...the hunger bar, for the same reason and with the same
    // contract: sent once here, and after this only when it changes.
    handle.send(ServerMessage::Nourishment {
        fraction: (restored.nourishment / primitive_shared::food::MAX_NOURISHMENT)
            .clamp(0.0, 1.0),
    });
    // ...what the sky is doing, which is world state exactly the way the
    // time of day is. A player who joins into a storm has to *arrive* in
    // one rather than see clear skies until the next roll.
    handle.send(ServerMessage::WeatherSync {
        weather: ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather(),
    });
    // ...the two gauges the world drives, on the same contract as the
    // hunger bar: sent once here so a player who logs back in freezing
    // sees that they are freezing, and after this only when they move.
    handle.send(ServerMessage::Body {
        temperature_c: restored.body.body_c,
        comfort: primitive_shared::body::Comfort::of(restored.body.body_c),
        hydration: (restored.body.hydration / primitive_shared::body::MAX_HYDRATION)
            .clamp(0.0, 1.0),
        // ...and how tired they were when they left, which is saved
        // with the rest of the body. See `Vitals::fatigue`.
        fatigue: restored.body.fatigue,
        // Nobody is anybody in particular about comfort on arrival: it is
        // not saved (see `Vitals::grime`), and the first surveys settle it.
        recovery: 1.0,
        // ...and neither are these three, for the same reason: grime and
        // wetness are not in the save, and a diet is counted from meals
        // this session. Sent as "clean, dry and eating nothing" so the
        // health page has numbers to draw from the first frame instead of
        // a page of dashes until the first survey lands.
        wetness: 0.0,
        grime: 0.0,
        diet_groups: 0,
    });
    // ...and what is wrong with them, which is saved for the same reason:
    // a cut still bleeding or a leg half knitted that a reconnect cleared
    // would be an injury nobody carries. Sent whole even when there is
    // nothing, so the mannequin is drawn from the server's answer and not
    // from whatever the last world this client was in left behind.
    handle.send(ServerMessage::Injuries {
        injuries: restored.body.injuries,
    });
    // ...and the (empty) inventory, so the bar is drawn from real state
    // rather than from the client's guess at what it starts with.
    crate::send_inventory(&handle);
    // ...and what they have on, for the same reason and with the same
    // contract.
    crate::send_equipment(&handle);
    // ...and what they have held, which is the recipe book, and where
    // they can find their way back to. Both after the inventory, so the
    // book a returning player opens first is the one their pack has
    // already added to.
    crate::send_discovered(&handle);
    crate::send_landmarks(&ctx, &handle);

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
    // ...and the newcomer is told who is already here. See
    // `ServerMessage::PlayerPresent` for the silence this ends.
    for other in ctx.registry.handles() {
        if other.id != id {
            handle.send(ServerMessage::PlayerPresent {
                id: other.id,
                username: other.username.clone(),
            });
        }
    }
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
    //
    // **Off the horse first**, so what is written is feet on the ground
    // beside it (`horses::dismount`) and not the saddle: a rider who left in
    // the saddle came back standing in the middle of their own horse, a
    // metre and a half up.
    crate::horses::dismount(&ctx, &handle, None);
    crate::store_profile(&ctx, &handle);
    // ...and off the oars of any raft, which would otherwise keep a rower
    // nobody can reach: nobody else could take them until the raft broke.
    crate::rafts::forget(&ctx, id);
    crate::horses::forget(&ctx, id);

    // ...and out of the registry immediately after, for the same reason
    // and one more. Until this happens the player is still online as far
    // as everything else on the server is concerned: they answer to
    // `/list`, they count against `max_players`, and they hold their own
    // name against a reconnect. See this function's own note.
    // What they were standing at, read before the registry forgets them:
    // `tell_chest_lid` counts who is at the chest *now*, so it has to be
    // asked after the leaver is out of the count and cannot be asked what
    // they were at afterwards.
    let was_at_chest = handle.state.lock().unwrap_or_else(|e| e.into_inner()).open_chest;
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
        // ...and the lid of whatever they were rummaging in comes down,
        // unless somebody else is still at it. A chest left standing open by
        // a player whose connection dropped is open until the chunk is
        // loaded again, and a world unloaded that way never is.
        if let Some(at) = was_at_chest {
            crate::tell_chest_lid(&ctx, at);
        }
    }

    pump.abort();
    drop(handle); // release our clone so the writer's queue can close
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;

    read_result
}

/// What authority this connection's commands carry, by
/// `commands::permission_for` -- the rule the chat path and the give
/// menu's question both go through.
fn permission_of(ctx: &Arc<Context>, handle: &Arc<PlayerHandle>) -> crate::logic::commands::Permission {
    let profile_says = handle.uuid.map(|uuid| {
        let profiles = ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
        profiles.is_operator(uuid)
    });
    crate::logic::commands::permission_for(ctx.options.local_operator, profile_says)
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
                    // Not `y - 1`: a bracket fungus is held by the trunk
                    // beside it. See `types::support_at`.
                    let (dx, dy, dz) = primitive_shared::types::support_at(block_id);
                    let holding = ctx
                        .world
                        .cached_block(global_x + dx, global_y + dy, global_z + dz)
                        .unwrap_or(BLOCK_AIR);
                    if !primitive_shared::types::can_grow_on(block_id, holding) {
                        handle.send(ServerMessage::Error(
                            "that needs solid ground under it".to_string(),
                        ));
                        continue;
                    }
                }

                // **A bed takes two cells**, and the second has to pass
                // every test the first just did: somewhere to go, ground
                // under it, and nobody standing in it. Asked before the
                // item is spent for the same reason as the rest, and
                // refused with the reason -- the cell that is in the way is
                // the one the player is not looking at. See `types::BED_HEAD`.
                // **A lean-to takes fifteen** (`lean_to::cells_from_mouth`):
                // the cell clicked is its mouth, and the hut runs back from it
                // three long, three wide and two high over the front two rows.
                // Every cell free and nobody in it, and a whole floor under
                // every cell of the ground row -- a hut half over a ditch is a
                // roof with a hole in it. Asked before the bed's question,
                // which would ask the same of one cell of the fifteen, and
                // before the item is spent.
                if block_id != BLOCK_AIR && primitive_shared::lean_to::is_lean_to(block_id) {
                    let mouth_floor = ctx.world.cached_block(global_x, global_y - 1, global_z).unwrap_or(BLOCK_AIR);
                    let mut blocked = !primitive_shared::types::has_full_top(mouth_floor);
                    for (cell, shape) in primitive_shared::lean_to::partners((global_x, global_y, global_z), block_id) {
                        let free = ctx
                            .world
                            .cached_block(cell.0, cell.1, cell.2)
                            .is_some_and(|b| primitive_shared::types::layer_placement(b, shape).is_some());
                        let occupied = ctx.registry.player_occupying_block(cell.0, cell.1, cell.2, shape).is_some();
                        let footed = cell.1 != global_y || {
                            let under = ctx.world.cached_block(cell.0, cell.1 - 1, cell.2).unwrap_or(BLOCK_AIR);
                            primitive_shared::types::has_full_top(under)
                        };
                        if !free || occupied || !footed {
                            blocked = true;
                            break;
                        }
                    }
                    if blocked {
                        handle.send(ServerMessage::Error(
                            "a lean-to needs three by three cells of clear, level ground in front of you and room over them".to_string(),
                        ));
                        continue;
                    }
                }

                if block_id != BLOCK_AIR && !primitive_shared::lean_to::is_lean_to(block_id) {
                    if let Some((head_at, head)) = primitive_shared::types::bed_partner(
                        (global_x, global_y, global_z),
                        block_id,
                    ) {
                        let there = ctx.world.cached_block(head_at.0, head_at.1, head_at.2);
                        let under = ctx
                            .world
                            .cached_block(head_at.0, head_at.1 - 1, head_at.2)
                            .unwrap_or(BLOCK_AIR);
                        let free = there.is_some_and(|b| {
                            primitive_shared::types::layer_placement(b, head).is_some()
                        });
                        let occupied = ctx
                            .registry
                            .player_occupying_block(head_at.0, head_at.1, head_at.2, head)
                            .is_some();
                        if !free || occupied || !primitive_shared::types::can_grow_on(head, under) {
                            handle.send(ServerMessage::Error(
                                "a bed needs a second free cell on solid ground behind it"
                                    .to_string(),
                            ));
                            continue;
                        }
                    }
                }

                // **A drying rack takes four**: two cells along its ridge and
                // two high (`types::rack_cells`). Asked on the same terms as
                // the bed's second cell, and before the rack is spent -- a
                // frame that ate the item and then found a wall in the way
                // would be a rack nobody got back.
                if block_id != BLOCK_AIR && primitive_shared::rack::is_rack(block_id) {
                    let mut blocked = false;
                    for (cell, shape) in primitive_shared::types::rack_partners(
                        (global_x, global_y, global_z),
                        block_id,
                    ) {
                        let free = ctx
                            .world
                            .cached_block(cell.0, cell.1, cell.2)
                            .is_some_and(|b| primitive_shared::types::layer_placement(b, shape).is_some());
                        let occupied = ctx
                            .registry
                            .player_occupying_block(cell.0, cell.1, cell.2, shape)
                            .is_some();
                        // Ground under the far pair, as under the near one.
                        let footed = primitive_shared::types::rack_is_top(shape) || {
                            let under = ctx
                                .world
                                .cached_block(cell.0, cell.1 - 1, cell.2)
                                .unwrap_or(BLOCK_AIR);
                            primitive_shared::types::can_grow_on(shape, under)
                        };
                        if !free || occupied || !footed {
                            blocked = true;
                            break;
                        }
                    }
                    if blocked {
                        handle.send(ServerMessage::Error(
                            "a drying rack needs two cells of clear ground and two of air over them".to_string(),
                        ));
                        continue;
                    }
                }

                // **A standing torch takes two cells too**, the pole and the
                // flame a cell over it: the second has to be free and nobody's
                // head in it. What is under the pole was asked above
                // (`needs_support`, and `can_grow_on`'s flat-ground rule).
                // **...and a door**, its top half a cell over the one put
                // down, on the same terms (`types::door_partner`). The floor
                // under the lower half was asked above.
                if let Some((top_at, top)) =
                    primitive_shared::wildfire::standing_torch_partner((global_x, global_y, global_z), block_id)
                        .filter(|_| primitive_shared::types::block_kind(block_id) == primitive_shared::types::BLOCK_STANDING_TORCH)
                        .or_else(|| {
                            primitive_shared::types::door_partner((global_x, global_y, global_z), block_id)
                                .filter(|_| primitive_shared::types::block_kind(block_id) == primitive_shared::types::BLOCK_DOOR)
                        })
                {
                    let free = ctx
                        .world
                        .cached_block(top_at.0, top_at.1, top_at.2)
                        .is_some_and(|b| primitive_shared::types::layer_placement(b, top).is_some());
                    let occupied = ctx
                        .registry
                        .player_occupying_block(top_at.0, top_at.1, top_at.2, top)
                        .is_some();
                    if !free || occupied {
                        let reason = if primitive_shared::types::is_door(block_id) {
                            "a door needs a free cell over it for its top half"
                        } else {
                            "a standing torch needs a free cell over it for its flame"
                        };
                        handle.send(ServerMessage::Error(reason.to_string()));
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
                // The wear field of whatever a placement spends. Zero for
                // every placeable block but one: a jug keeps what has been
                // poured into it there (`inventory::jug_contents`), and a
                // placement that forgot it set down an empty jug and
                // deleted the grain.
                let mut spent_damage = 0;
                // ...and the thing itself, to give back whole if the write
                // fails: a green log refunded as a seasoned one, or a wet one
                // as dry, would be the refusal doing the drying.
                let mut spent_block = primitive_shared::types::block_kind(block_id);
                // What the break was made with, for the nettle's sting after
                // the drop (`crate::sting_from_nettle`). Read before the wear,
                // so a knife that broke on the stroke still cut the stalk.
                let mut cut_with = None;
                if block_id == BLOCK_AIR {
                    // Breaking. Nothing is credited here -- the drop
                    // goes into the world and is picked up by walking
                    // over it.
                    //
                    // **What it costs is the tool.** One swing off
                    // whatever is in the selected slot, and if that was
                    // the last swing the tool is gone and the player is
                    // told so -- a tool that vanished silently would read
                    // as an inventory bug. Spent here, on the server,
                    // against the server's copy of what is held: wear a
                    // client could decline to report is wear that never
                    // happens.
                    let (wear, was_holding, tool_slot) = {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let slot = state.selected_slot;
                        // Read before the wear, because a tool that
                        // broke is not in the slot any more and
                        // `Event::ToolBroke` has to be able to say what
                        // it was.
                        let was_holding = state.inventory.block_in(slot).unwrap_or(0);
                        let wear = state.inventory.wear_tool(slot);
                        state.inventory_dirty |=
                            !matches!(wear, primitive_shared::inventory::Wear::None);
                        (wear, was_holding, slot)
                    };
                    cut_with = (was_holding != 0).then_some(was_holding);
                    match wear {
                        primitive_shared::inventory::Wear::None => {}
                        primitive_shared::inventory::Wear::Worn => {
                            crate::send_inventory(&handle);
                        }
                        primitive_shared::inventory::Wear::Broke => {
                            crate::send_inventory(&handle);
                            handle.send(ServerMessage::Error(
                                "your tool broke".to_string(),
                            ));
                            crate::tool_broke(&ctx, handle.id, was_holding, tool_slot);
                        }
                    }
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
                    // What comes out of the pack is the thing the
                    // placement was made from (`types::spends`): a log
                    // however the player chose to lay it and however green
                    // it was, wet or dry as it is in the hand.
                    let spent = {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let slot = state.selected_slot;
                        let (held, damage) = state
                            .inventory
                            .slots()
                            .get(slot)
                            .copied()
                            .flatten()
                            .map_or((0, 0), |stack| (stack.block, stack.damage));
                        if primitive_shared::types::spends(held, block_id) && state.inventory.take_from(slot, 1) == 1
                        {
                            state.inventory_dirty = true;
                            spent_damage = damage;
                            spent_block = held;
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

                // What actually goes into the cell.
                //
                // The same thing the client asked for, except for the
                // one block that is *harvested* rather than removed: a
                // berry bush that has been picked leaves the bush behind
                // (see `blocks::BlockDef::leaves_behind`). Worked out
                // here rather than inside `set_block`, because every
                // other caller of that -- the falling sand, the water,
                // the fires going out -- means exactly what it says.
                let written = if block_id == BLOCK_AIR {
                    primitive_shared::types::block_residue(was.unwrap_or(BLOCK_AIR))
                } else {
                    block_id
                };
                // **A ruin chest nobody has opened is filled before the
                // cell changes**, or it would spill nothing: once the
                // break is written the cell counts as edited, which is
                // what says a chest's seal is already broken. See
                // `unseal_ruin_chest`.
                if was.is_some_and(primitive_shared::types::is_container) {
                    crate::unseal_ruin_chest(&ctx, (global_x, global_y, global_z));
                }
                if !ctx.world.set_block(global_x, global_y, global_z, written) {
                    handle.send(ServerMessage::Error("block edit out of bounds".to_string()));
                    // The block was already taken out of the pack, so it
                    // has to go back in. Refusing after spending is how
                    // players quietly lose things. Nothing was spent on
                    // a layer added to material already in the cell, so
                    // there is nothing to give back for one.
                    if block_id != BLOCK_AIR && matches!(layering, Placement::Fresh) {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        // `add_worn`: what goes back is the jug of grain
                        // that came out, not a new empty one.
                        state.inventory.add_worn(spent_block, 1, spent_damage);
                        state.inventory_dirty = true;
                        drop(state);
                        crate::send_inventory(&handle);
                    }
                    continue;
                }
                ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
                // A jug set down takes what was poured into it along, into
                // the container store at its cell. After the write, so a
                // refused placement never leaves grain in a cell with no
                // jug in it.
                if block_id != BLOCK_AIR && primitive_shared::types::opens_as_vessel(written) {
                    crate::set_down_vessel(&ctx, (global_x, global_y, global_z), spent_damage);
                }
                // A stall put down is the placer's. After the write, for the
                // jug's reason: a refused placement owns nothing.
                if block_id != BLOCK_AIR && crate::is_stall(written) {
                    crate::stall_placed(&ctx, &handle, (global_x, global_y, global_z));
                }
                // **A wet thing put down starts drying where it stands**, on
                // the wet walls' clock (`logic::walls`; `wet`, "Put down wet").
                if block_id != BLOCK_AIR && primitive_shared::wet::is_wet(written) {
                    ctx.walls.lock().unwrap_or_else(|e| e.into_inner()).lay((global_x, global_y, global_z));
                }
                // Noted for hunger: an accepted edit is what the server
                // counts as this player working. See `last_edit`.
                {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.last_edit = Some(std::time::Instant::now());
                    // **Digging dirties.** Loose ground broken by this
                    // player is on their hands; mud and dung thrice over.
                    // Here, off the server's own `was`, rather than a flag
                    // the client sends. See `comfort::step_grime`.
                    if block_id == BLOCK_AIR {
                        if let Some(broken) = was {
                            use primitive_shared::types::{block_kind, BLOCK_DUNG, BLOCK_MUD};
                            let kind = block_kind(broken);
                            if kind == BLOCK_MUD || kind == BLOCK_DUNG {
                                state.vitals.soil(primitive_shared::comfort::GRIME_PER_FILTHY_BLOCK);
                            } else if primitive_shared::blocks::definition(broken).matter
                                == primitive_shared::blocks::Matter::Loose
                            {
                                state.vitals.soil(primitive_shared::comfort::GRIME_PER_LOOSE_BLOCK);
                            }
                        }
                    }
                    // A block set down is a short swing everybody near sees.
                    // A break is not one more gesture: the swinging that led
                    // up to it is `digging`, already on the snapshot.
                    if block_id != BLOCK_AIR {
                        state.gesture.made(primitive_shared::protocol::Action::Place);
                    }
                }

                if block_id == BLOCK_AIR {
                    if let Some(broken) = was {
                        // Dung cleared off a furrow is dug into it. See
                        // `manure_the_furrow_under`.
                        if primitive_shared::types::block_kind(broken) == primitive_shared::types::BLOCK_DUNG {
                            crate::manure_the_furrow_under(&ctx, (global_x, global_y, global_z));
                        }
                        // A fire taken apart stops burning, whatever it
                        // had left. Before the drop, so the map is
                        // straight even if the drop path bails.
                        if primitive_shared::types::is_hearth(broken) {
                            ctx.fires
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .extinguish((global_x, global_y, global_z));
                        }
                        // A chest goes with what is inside it. Emptied
                        // *after* the block is gone, so the stacks land
                        // in a cell that is now air rather than inside
                        // the block they came out of.
                        // **Except a jug**, whose contents go back *into*
                        // the jug that drops rather than onto the floor
                        // beside it -- `spawn_block_drop` below folds them
                        // (see `pick_up_vessel`), and spilling first would
                        // leave it nothing to fold.
                        // ...but a jug of water set down is knocked over
                        // first, while its store still holds it: see
                        // `tip_out_water`.
                        if primitive_shared::types::is_set_down(broken) {
                            crate::tip_out_water(&ctx, broken, (global_x, global_y, global_z));
                        }
                        // A stall taken down by its owner goes into their
                        // pack first, so the spill below finds nothing; one
                        // broken by anybody else spills. See `stall_broken`.
                        if crate::is_stall(broken) {
                            crate::stall_broken(&ctx, &handle, (global_x, global_y, global_z));
                        }
                        if primitive_shared::types::is_container(broken)
                            && !primitive_shared::types::opens_as_vessel(broken)
                        {
                            crate::spill_chest(&ctx, (global_x, global_y, global_z));
                        }
                        // ...and a rack goes with whatever was stretched
                        // on it -- which the line above already did, a
                        // rack being a container. What is left is how
                        // far along it was. A half-dry skin spills as a
                        // skin, so breaking a rack costs the time and
                        // never the hide.
                        if crate::drying::is_rack(broken) {
                            crate::forget_rack(&ctx, (global_x, global_y, global_z));
                        }
                        // ...and a pit kiln or a log pile gives back what
                        // went into it, which is not in its table row (see
                        // `logic::pits::Pits::broken`).
                        if primitive_shared::pit::is_pit_kiln(broken) || primitive_shared::pit::is_log_pile(broken) {
                            crate::spill_pit(&ctx, (global_x, global_y, global_z), broken);
                        }
                        // A nettle cut with a knife gives its bast instead of
                        // its fibre (`types::strips_bast`).
                        if !crate::strip_nettle(&ctx, broken, cut_with, (global_x, global_y, global_z)) {
                            crate::spawn_block_drop(
                                &ctx,
                                broken,
                                (global_x, global_y, global_z),
                            );
                        }
                        // ...and the other half of a bed goes with it,
                        // giving nothing: the half broken gave the bed.
                        // (And the other half of a tall plant, the same way.)
                        crate::break_bed_partner(&ctx, (global_x, global_y, global_z), broken);
                        // ...and a nettle pulled up bare-handed stings.
                        crate::sting_from_nettle(&handle, broken, cut_with);
                        // ...and a hand in a wild hive is stung, less if it
                        // held a torch or the tree was smoked (`bees`).
                        crate::sting_from_bees(&ctx, &handle, broken, (global_x, global_y, global_z), cut_with);
                    }
                } else if primitive_shared::lean_to::is_lean_to(written) {
                    // The hut's other fourteen cells, checked free above.
                    for (cell, shape) in primitive_shared::lean_to::partners((global_x, global_y, global_z), written) {
                        if ctx.world.set_block(cell.0, cell.1, cell.2, shape) {
                            crate::broadcast_block(&ctx, cell, shape);
                            let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                            sim.on_block_changed(cell.0, cell.1, cell.2);
                            drop(sim);
                            crate::notify_mechanics(&ctx, cell.0, cell.1, cell.2);
                        }
                    }
                } else if primitive_shared::rack::is_rack(written) {
                    // The rack's other three cells, checked free above.
                    for (cell, shape) in
                        primitive_shared::types::rack_partners((global_x, global_y, global_z), written)
                    {
                        if ctx.world.set_block(cell.0, cell.1, cell.2, shape) {
                            crate::broadcast_block(&ctx, cell, shape);
                            let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                            sim.on_block_changed(cell.0, cell.1, cell.2);
                            drop(sim);
                            crate::notify_mechanics(&ctx, cell.0, cell.1, cell.2);
                        }
                    }
                } else if let Some((head_at, head)) =
                    primitive_shared::types::bed_partner((global_x, global_y, global_z), written).or_else(|| {
                        // ...or the flame over a standing torch's pole, on the
                        // bed's terms: checked free above.
                        primitive_shared::wildfire::standing_torch_partner((global_x, global_y, global_z), written)
                            .filter(|_| primitive_shared::types::block_kind(written) == primitive_shared::types::BLOCK_STANDING_TORCH)
                    })
                    .or_else(|| {
                        // ...or a door's top half over its lower one.
                        primitive_shared::types::door_partner((global_x, global_y, global_z), written)
                            .filter(|_| primitive_shared::types::block_kind(written) == primitive_shared::types::BLOCK_DOOR)
                    })
                {
                    // The head half, behind the foot the player put down.
                    // Checked free above, before the bed was spent.
                    if ctx.world.set_block(head_at.0, head_at.1, head_at.2, head) {
                        crate::broadcast_block(&ctx, head_at, head);
                        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                        sim.on_block_changed(head_at.0, head_at.1, head_at.2);
                        drop(sim);
                        crate::notify_mechanics(&ctx, head_at.0, head_at.1, head_at.2);
                    }
                }

                // **A tree comes down with its own base.** Before the
                // sand, because felling *is* a set of block changes and
                // everything that watches cells should hear about them
                // in the same order it hears about any other edit -- see
                // `crate::fell_tree`.
                crate::fell_tree(&ctx, (global_x, global_y, global_z), Some(&handle));
                // ...and a palm's crown with its heart, or with the top of
                // the trunk the heart sat on. See `drop_unheld_palm_crown`.
                crate::drop_unheld_palm_crown(&ctx, (global_x, global_y, global_z));

                // Sand above or at this cell may now be unsupported.
                {
                    let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                    sim.on_block_changed(global_x, global_y, global_z);
                    crate::notify_mechanics(&ctx, global_x, global_y, global_z);
                }

                // Only players who actually have this chunk loaded, via
                // the reverse index -- not a scan of every player -- and
                // the mods, which `broadcast_block` is the choke point
                // for. This was the subscriber loop written out inline,
                // which is how `Event::BlockChanged` came to be declared
                // and never fired: there were four places a cell changed
                // and not one of them said so.
                crate::broadcast_block(&ctx, (global_x, global_y, global_z), written);
                let (chunk_pos, _, _) = ChunkPos::from_global(global_x, global_z);

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
                // **A sleeper is not moved by their own keys.** The
                // client has been told it is asleep and stops
                // predicting (`ServerMessage::Asleep`), but a message
                // already in flight -- or a client that ignores the
                // message -- must not walk the body out of the bed. So
                // the transform is dropped here rather than corrected
                // twenty times a second, which is what the first
                // version did and what made a sleeping player vibrate.
                //
                // Deliberately silent: this is not a violation and the
                // player is not cheating, they are asleep.
                //
                // ...and nor is a rower, for the same reason: the server keeps
                // their body on the raft's seat (`rafts::tick`), and a
                // transform from a client that has not yet heard it has the
                // oars would pull the rower off the stern and back, a tick at
                // a time.
                let pinned = {
                    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.sleeping_in.is_some() || state.rowing.is_some()
                };
                if pinned {
                    continue;
                }
                // **A rider's transform is judged and moves nothing.** The
                // body is the saddle of the server's horse (`horses::tick`);
                // what the client says is where the saddle of *its* horse is,
                // and the anti-cheat reads that path with a mounted allowance
                // (`AntiCheat::set_mounted`). A gallop is inside it; a rider
                // whose saddle climbs into the sky is not, and is put off the
                // horse and back where the server has them.
                let riding = handle.state.lock().unwrap_or_else(|e| e.into_inner()).riding.is_some();
                if riding {
                    let verdict = {
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let verdict = state.anticheat.check_transform(x, y, z, on_ground, sequence, &ctx.world);
                        if verdict.is_allowed() {
                            if yaw.is_finite() {
                                state.yaw = yaw;
                            }
                            if pitch.is_finite() {
                                state.pitch = pitch;
                            }
                        }
                        verdict
                    };
                    match verdict {
                        Verdict::Allow => {}
                        Verdict::Reject { reason, .. } => {
                            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
                            crate::horses::dismount(&ctx, &handle, None);
                            let at = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
                            handle.send(ServerMessage::PositionCorrection { x: at.0, y: at.1, z: at.2, reason });
                        }
                        Verdict::Kick(reason) => {
                            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
                            handle.request_kick(DisconnectReason::AntiCheat(reason));
                            return Ok(());
                        }
                    }
                    continue;
                }
                let verdict = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let verdict = state
                        .anticheat
                        .check_transform(x, y, z, on_ground, sequence, &ctx.world);
                    if verdict.is_allowed() {
                        // A world position is a client saying its feet are
                        // not on a raft's deck any more.
                        state.aboard = None;
                        state.position = (x, y, z);
                        // **Only a number is a direction.** The anti-cheat
                        // judges x, y and z and nothing else, so a modified
                        // client's NaN or infinite yaw was stored as it came
                        // -- and from here it is the aim of everything the
                        // player throws (`drop_from_slot`) and the head of
                        // their body in every other player's snapshot. A
                        // look that is not a number keeps the last one.
                        if yaw.is_finite() {
                            state.yaw = yaw;
                        }
                        if pitch.is_finite() {
                            state.pitch = pitch;
                        }
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
                    // Water half a block up the cell, which is the line
                    // the mesher draws and the collider swims in rather
                    // than a second opinion about it.
                    //
                    // **It no longer sorts a puddle from a pool, and it
                    // is not supposed to.** This was written when a
                    // cell's depth was visible, to keep the film left by
                    // a receding puddle from being a free landing pad
                    // anywhere water had ever been. `surface_height` is
                    // the same for every depth now, on purpose -- a sea
                    // with a staircase in it is worse than a shallow
                    // cell that catches you -- so an eighth-deep cell
                    // answers yes here, exactly as it is drawn. Asking
                    // the depth instead would put the server's answer
                    // somewhere the player cannot see it, which is the
                    // one thing `fluid` exists to prevent.
                    //
                    // Kept as `covers` rather than simplified to
                    // `is_liquid` because the question is the honest one
                    // and the day a drop past half a block ever means
                    // something, this is already asking it.
                    let landed_in_liquid = ctx
                        .world
                        .cached_block(x.floor() as i32, y.floor() as i32, z.floor() as i32)
                        .is_some_and(|block| primitive_shared::fluid::covers(block, 0.5));
                    // ...and whether the feet came down on the point of a
                    // stalagmite, out of the server's own world for the same
                    // reason: a drop onto one cuts (`dripstone`). Read before
                    // the player's lock, like the water.
                    let on_spike = primitive_shared::dripstone::spike_under((x, y, z), |bx, by, bz| {
                        ctx.world.cached_block(bx, by, bz).unwrap_or(primitive_shared::types::BLOCK_AIR)
                    });
                    // ...and whether the body is among the points of a stake,
                    // on the same terms (`spikes`).
                    let half = f64::from(primitive_shared::geometry::PLAYER_HALF_WIDTH);
                    let among_stakes = primitive_shared::spikes::touches(
                        [x - half, y, z - half],
                        [x + half, y + f64::from(primitive_shared::geometry::PLAYER_HEIGHT), z + half],
                        |bx, by, bz| ctx.world.cached_block(bx, by, bz).unwrap_or(primitive_shared::types::BLOCK_AIR),
                    );
                    let staked = {
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        if state.flying {
                            crate::logic::survival::Outcome::Unchanged
                        } else {
                            state.vitals.on_stakes(among_stakes, x, z, std::time::Instant::now())
                        }
                    };
                    // Heard by everybody near, the victim included, before the
                    // health is told: `Unchanged` is the only answer that
                    // means no point went in. At the knee, where a stake is.
                    if staked != crate::logic::survival::Outcome::Unchanged {
                        crate::broadcast_staked(&ctx, (x, y + 0.5, z));
                    }
                    crate::report_vitals(&ctx, &handle, staked);
                    let outcome = {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        // Weight comes straight out of the inventory the
                        // server is holding, so there is nothing here for
                        // a client to assert or for the two sides to
                        // disagree about.
                        let carried = state.inventory.total_weight();
                        state.vitals.set_carried_weight(carried);
                        state.vitals.set_on_spike(on_spike);
                        if state.flying {
                            // A flyer is not falling, however far below
                            // them the ground is. Cleared rather than
                            // merely not accumulated, so the drop that
                            // starts when flight is withdrawn is
                            // measured from where it was withdrawn --
                            // see `set_flight`.
                            state.vitals.clear_fall();
                            crate::logic::survival::Outcome::Unchanged
                        } else {
                            state.vitals.on_transform(y as f32, on_ground, landed_in_liquid)
                        }
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

            // **"Am I an operator?"** The give menu asks before it draws
            // itself and again every time the journal is opened: `/op`
            // takes effect on the next command, so an answer settled at
            // the handshake would go on being wrong for the rest of the
            // session. Cheap enough to ask that often -- one lock of the
            // profiles, and only when a player opens a screen.
            ClientMessage::AmIAnOperator => {
                let yes = matches!(
                    permission_of(&ctx, &handle),
                    crate::logic::commands::Permission::Operator
                );
                handle.send(ServerMessage::Operator { yes });
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
                    let permission = permission_of(&ctx, &handle);
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

            ClientMessage::RequestExtensions => {
                // On the chat bucket rather than a bucket of its own.
                // The two are the same kind of ask -- a thing a player
                // does by hand, a few times a session -- and building
                // this list takes both extension locks, which is the
                // last pair a flood should be able to reach.
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
                handle.send(ServerMessage::Extensions(crate::extension_list(&ctx)));
            }

            ClientMessage::Pong { .. } => {
                // `touch()` above already did the work; the nonce is only
                // useful once we start measuring per-client RTT.
            }

            ClientMessage::SelectSlot { slot } => {
                // Through `select_slot` rather than straight into the
                // field, so that a mod hears about the player's own
                // change and about its own on the same terms. See
                // `crate::select_slot`.
                crate::select_slot(&ctx, &handle, slot as usize);
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

            ClientMessage::SortChest => {
                crate::chest_sort(&ctx, &handle);
            }

            ClientMessage::DropSlot { slot, whole_stack } => {
                crate::drop_from_slot(&ctx, &handle, slot as usize, whole_stack);
            }

            ClientMessage::Craft { index, times } => {
                // **Through `craft_for`, which is also what a mod
                // calls.** The whole of the rule lives there: the fire
                // is checked against the *server's* copy of where the
                // player is standing -- a client that decided for itself
                // would be a client that smelts bronze in a meadow --
                // and the mods are asked before anything is spent.
                //
                // Bounded here rather than there, because the bound is
                // about *this* caller: a loop whose length a client
                // picks is a loop a client can make expensive, and a mod
                // is native code in this process that is not
                // second-guessed.
                let crafts = crate::craft_for(
                    &ctx,
                    &handle,
                    index as usize,
                    times.min(MAX_CRAFTS_PER_REQUEST) as u32,
                );
                // Nothing *ran*, rather than nothing was made: a click
                // that shattered its flint ran, and `craft_for` has
                // already said so -- "cannot make that" on top of it
                // would be two messages for one blow, and the wrong one.
                if crafts.ran() == 0 {
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
                handle.state.lock().unwrap_or_else(|e| e.into_inner()).open_bags = None;
                // The mod API's `ContainerClosed` promises "closed it, or was
                // made to", and it used to fire only when the block went: a
                // mod counting who stands at a chest saw every player open it
                // and none of them ever walk away.
                let closed = handle.state.lock().unwrap_or_else(|e| e.into_inner()).open_chest.take();
                if let Some(at) = closed {
                    crate::container_closed(&ctx, handle.id, at);
                    // ...and the lid comes down for everyone who can see it,
                    // unless somebody else is still at the chest.
                    crate::tell_chest_lid(&ctx, at);
                }
            }

            ClientMessage::OpenStation {
                global_x,
                global_y,
                global_z,
            } => {
                crate::open_station(&ctx, &handle, (global_x, global_y, global_z));
            }

            ClientMessage::StationBegin { job } => {
                crate::station_begin(&ctx, &handle, job);
            }

            ClientMessage::StationRun { presses } => {
                crate::station_run(&ctx, &handle, presses);
            }

            ClientMessage::CloseStation => {
                crate::close_station(&handle);
            }

            ClientMessage::ChestMove { from, to, half } => {
                crate::chest_move(&ctx, &handle, from, to, half);
            }

            ClientMessage::ChestQuickMove { side, slot } => {
                crate::chest_quick_move(&ctx, &handle, side, slot);
            }

            ClientMessage::ChestMoveKind { side, slot } => {
                crate::chest_move_kind(&ctx, &handle, side, slot);
            }

            ClientMessage::ChestBulkMove { to_chest } => {
                crate::chest_bulk_move(&ctx, &handle, to_chest);
            }

            ClientMessage::Attack { target } => {
                crate::melee_attack(&ctx, &handle, target);
            }

            ClientMessage::AttackEntity { target } => {
                crate::attack_animal(&ctx, &handle, target);
            }

            // Only the figure other people see; nothing in the world moves
            // on it. See `ClientMessage::Digging`.
            ClientMessage::Digging { digging } => {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.digging_until = digging
                    .then(|| std::time::Instant::now() + super::players::DIGGING_LAPSES_AFTER);
            }

            ClientMessage::Eat { slot } => {
                crate::eat_from_slot(&ctx, &handle, slot as usize);
            }

            ClientMessage::TreatInjury { slot, part } => {
                crate::treat_from_slot(&handle, slot as usize, part as usize);
            }

            ClientMessage::UseBlock {
                global_x,
                global_y,
                global_z,
            } => {
                crate::use_block(&ctx, &handle, (global_x, global_y, global_z));
            }

            // **Fishing.** The throw, the strike, the hand on the reel and
            // the line taken out of the water. Every one of them is judged
            // against the cast the server is holding, never against what the
            // client says is happening -- see `crate::cast_line` and
            // `crate::strike_line`.
            ClientMessage::CastLine { power } => {
                crate::cast_line(&ctx, &handle, power);
            }

            ClientMessage::Strike => {
                crate::strike_line(&ctx, &handle);
            }

            ClientMessage::Reel { pulling } => {
                crate::reel_line(&ctx, &handle, pulling);
            }

            ClientMessage::ReelIn => {
                crate::reel_in_line(&ctx, &handle);
            }

            ClientMessage::StandUp => {
                crate::stand_up(&ctx, &handle, None);
            }

            ClientMessage::UseRaft { raft } => {
                crate::rafts::use_raft(&ctx, &handle, raft);
            }

            ClientMessage::Mount { horse } => {
                crate::horses::mount(&ctx, &handle, horse);
            }

            ClientMessage::Dismount => {
                crate::horses::dismount(&ctx, &handle, None);
            }

            ClientMessage::Rein { horse, forward, turn, gait, jump } => {
                crate::horses::rein(&ctx, &handle, horse, forward, turn, gait, jump);
            }

            ClientMessage::OpenBags { horse } => {
                crate::horses::open_bags(&ctx, &handle, horse);
            }

            ClientMessage::Row { raft, stroke, turn } => {
                crate::rafts::row(&ctx, &handle, raft, stroke, turn);
            }

            ClientMessage::Trim { raft, angle } => {
                crate::rafts::trim(&ctx, &handle, raft, angle);
            }

            // The sequence is not checked here: a deck transform is judged
            // against the deck (`rafts::deck`), not against the speed budget
            // the sequence exists to protect from replays.
            ClientMessage::Deck {
                raft,
                x,
                y,
                z,
                yaw,
                pitch,
                on_ground,
                sequence: _,
            } => {
                crate::rafts::deck(&ctx, &handle, raft, [x as f32, y as f32, z as f32], yaw, pitch, on_ground);
            }

            ClientMessage::PileLog {
                global_x,
                global_y,
                global_z,
            } => {
                crate::pile_log(&ctx, &handle, (global_x, global_y, global_z));
            }

            ClientMessage::SetDown {
                global_x,
                global_y,
                global_z,
            } => {
                crate::set_down_item(&ctx, &handle, (global_x, global_y, global_z));
            }

            ClientMessage::Equip { slot } => {
                crate::equip_from_slot(&ctx, &handle, slot as usize);
            }

            ClientMessage::Unequip { slot } => {
                crate::unequip_slot(&ctx, &handle, slot as usize);
            }

            // Neither of these takes the context: pouring dry goods
            // about inside one pack changes nothing outside it, so
            // there is no world to reach and no hook to fire.
            ClientMessage::PourIntoJug { from, jug } => {
                crate::pour_into_jug(&handle, from as usize, jug as usize);
            }

            ClientMessage::EmptyJug { slot } => {
                crate::empty_jug(&handle, slot as usize);
            }

            ClientMessage::TakeFromJug { jug, to, half } => {
                crate::take_from_jug(&handle, jug as usize, to as usize, half);
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

            ClientMessage::Dig {
                global_x,
                global_y,
                global_z,
                face,
            } => {
                dig_one_slice(&ctx, &handle, (global_x, global_y, global_z), face);
            }

            ClientMessage::TendAnimal { animal } => {
                crate::tend_animal(&ctx, &handle, animal);
            }

            ClientMessage::StallOffer { row, offer } => {
                crate::stall_offer(&ctx, &handle, row, offer);
            }

            ClientMessage::StallBuy { row, offer } => {
                crate::stall_buy(&ctx, &handle, row, offer);
            }

            ClientMessage::Build { global_x, global_y, global_z, along_x } => {
                build_in_place(&ctx, &handle, (global_x, global_y, global_z), along_x);
            }

            ClientMessage::Disconnect => return Ok(()),
        }
    }
}

/// One swing of a dig: takes a slice off the block at `at` from the face
/// the digger is working, and writes what is left back into the world.
///
/// **Everything that happens when a block finally goes is deliberately not
/// here.** The drop, the tool's wear, the grime on the hands, the collapse
/// of what stood on it, the fire it put out, the chest it spilled -- all of
/// that is the `SetBlock` path, and the client sends that message for the
/// last slice (`dig::next_bite` answers `None` for it). A second copy of
/// four hundred lines that has to agree with the first is exactly the kind
/// of drift `primitive_shared` exists to prevent, and it would have been a
/// copy that forgot one thing: the first version of this forgot the
/// falling sand.
///
/// What *is* here is the short list a slice actually needs: the anti-cheat,
/// the tool rule, the write, and telling everyone who can see the cell.
fn dig_one_slice(
    ctx: &Arc<Context>,
    handle: &Arc<PlayerHandle>,
    at: (i32, i32, i32),
    face: (i8, i8, i8),
) {
    use primitive_shared::dig;
    let (global_x, global_y, global_z) = at;
    let Some(side) = dig::Side::from_normal((
        i32::from(face.0),
        i32::from(face.1),
        i32::from(face.2),
    )) else {
        // Not one axial step, so not a face. A client that sends this has
        // not aimed at anything; say nothing and write nothing.
        return;
    };
    let target = ctx
        .world
        .cached_block(global_x, global_y, global_z)
        .unwrap_or(BLOCK_AIR);
    let Some(next) = dig::next_bite(target, side) else {
        // Either the cell holds something that comes away whole, or this
        // is the swing that finishes it -- and the client sends `SetBlock`
        // for that. Tell it what is really there so a client that got the
        // count wrong is put straight rather than left grinding at a block
        // the server thinks is gone.
        handle.send(ServerMessage::BlockUpdate(BlockChange {
            global_x,
            global_y,
            global_z,
            block_id: target,
        }));
        return;
    };
    // The same reach, rate and range test every other edit passes, on the
    // id the cell is actually going to hold: a dig is an edit, and a
    // modified client that quarried from across the valley would otherwise
    // have found the one edit nobody checked.
    let verdict = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .anticheat
            .check_block_edit(global_x, global_y, global_z, next)
    };
    match verdict {
        Verdict::Allow => {}
        Verdict::Reject { reason, .. } => {
            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
            handle.send(ServerMessage::Error(format!("edit refused: {reason}")));
            handle.send(ServerMessage::BlockUpdate(BlockChange {
                global_x,
                global_y,
                global_z,
                block_id: target,
            }));
            return;
        }
        Verdict::Kick(reason) => {
            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
            handle.request_kick(DisconnectReason::AntiCheat(reason));
            return;
        }
    }
    // **The tool rule, against the server's own copy of what is held.**
    // The same question the break path asks, and it has to be asked on
    // *every* slice: without it a bare hand could take three quarters off
    // a granite block and only be refused the last of it.
    let held = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        state.inventory.block_in(slot)
    };
    if !primitive_shared::types::is_breakable_with(target, held) {
        handle.send(ServerMessage::Error(
            "you need a better tool for that".to_string(),
        ));
        return;
    }
    // **The plugin's break hook fires on the first slice and not on the
    // rest.** A protection plugin's answer to "may this player break this
    // cell" cannot change half way through a block, so asking it once --
    // while the cell is still whole, before anything has been taken off
    // it -- is the answer for the whole dig, and a refused wall is never
    // even started on. Asked on every slice it would be four calls and
    // four counted breaks for one block; asked on none, a protected wall
    // could be quarried down to its last quarter before the break path
    // refused it.
    if dig::bite(target).is_none()
        && !crate::fire_plugin_hook(
            ctx,
            "on_block_break",
            vec![
                crate::logic::plugins::Value::Int(handle.id as i64),
                crate::logic::plugins::Value::Int(global_x as i64),
                crate::logic::plugins::Value::Int(global_y as i64),
                crate::logic::plugins::Value::Int(global_z as i64),
            ],
            Some(vec![(global_x, global_y, global_z)]),
        )
    {
        handle.send(ServerMessage::Error(
            "a plugin refused that change".to_string(),
        ));
        handle.send(ServerMessage::BlockUpdate(BlockChange {
            global_x,
            global_y,
            global_z,
            block_id: target,
        }));
        return;
    }
    if !ctx.world.set_block(global_x, global_y, global_z, next) {
        return; // out of bounds; nothing was spent, so nothing to give back
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.last_edit = Some(std::time::Instant::now());
    }
    crate::broadcast_block(ctx, (global_x, global_y, global_z), next);
    // **A handful for the quarter**, out of the face that was worked
    // (`build::slice_handful`), where the digger is standing: out of the cell
    // it would pop out of rock still standing in it. Asked of the cell as it
    // was, so the sod peeled off turf is nothing and the first quarter of a
    // soil is a handful of it.
    if let Some(handful) = primitive_shared::build::slice_handful(target) {
        let out = (
            global_x as f32 + 0.5 + f32::from(face.0) * 0.7,
            global_y as f32 + 0.5 + f32::from(face.1) * 0.7,
            global_z as f32 + 0.5 + f32::from(face.2) * 0.7,
        );
        ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
            handful,
            1,
            primitive_shared::geometry::wide(out),
            (0.0, 0.0, 0.0),
            None,
            std::time::Instant::now(),
        );
    }
    // **The cell has changed shape, so everything that reads its shape is
    // told.** A bite is not air, so nothing falls into it -- but the water
    // beside it may now wash the rest of it away (`logic::water`), and a
    // prop or a growing thing next door reads the cell through the same
    // notification every other edit sends.
    {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.on_block_changed(global_x, global_y, global_z);
    }
    crate::notify_mechanics(ctx, global_x, global_y, global_z);
}

/// One stage of a wall, or one handful of a heap, laid at `at` with what is
/// in the selected slot (`build::lay`).
///
/// **Its own path, not `SetBlock`'s**, for `dig_one_slice`'s reason turned
/// round: a placement spends one of the kind it writes, and a course of brick
/// spends a brick and a trowel of mortar and writes a wall, so the one rule
/// the placement path is built on is the one thing this is not. What it
/// shares with every edit it asks the same way: the reach and the rate, the
/// plugin's placement hook, and nobody standing where the courses go.
fn build_in_place(ctx: &Arc<Context>, handle: &Arc<PlayerHandle>, at: (i32, i32, i32), along_x: bool) {
    use primitive_shared::build;
    let (x, y, z) = at;
    let Some(target) = ctx.world.cached_block(x, y, z) else {
        return; // not loaded; nothing to build on
    };
    let under = ctx.world.cached_block(x, y - 1, z).unwrap_or(BLOCK_AIR);
    let put_straight = |reason: &str| {
        handle.send(ServerMessage::Error(reason.to_string()));
        handle.send(ServerMessage::BlockUpdate(BlockChange { global_x: x, global_y: y, global_z: z, block_id: target }));
    };
    let (slot, held, have, mortar) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        let held = state.inventory.block_in(slot).unwrap_or(BLOCK_AIR);
        // **The pack's, not the square's.** A course of field stone takes
        // two and a wattle three rods, and a player with the last one in the
        // hand and a stack beside it was told they were not carrying enough.
        // Counted by kind, as a recipe counts: the one in the hand is what
        // says which kind.
        let have = if held == BLOCK_AIR { 0 } else { state.inventory.count(held) };
        let mortar = state.inventory.count(primitive_shared::types::BLOCK_MORTAR) > 0;
        (slot, held, have, mortar)
    };
    let laid = match build::lay(target, held, mortar, under, along_x) {
        Ok(laid) => laid,
        Err(reason) => return put_straight(reason),
    };
    if have < laid.spends {
        return put_straight("you are not carrying enough of that");
    }
    let verdict = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.anticheat.check_build(x, y, z)
    };
    match verdict {
        Verdict::Allow => {}
        Verdict::Reject { reason, .. } => {
            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
            return put_straight(&format!("edit refused: {reason}"));
        }
        Verdict::Kick(reason) => {
            ctx.metrics.anticheat_flags.fetch_add(1, Ordering::Relaxed);
            handle.request_kick(DisconnectReason::AntiCheat(reason));
            return;
        }
    }
    if !crate::fire_plugin_hook(
        ctx,
        "on_block_place",
        vec![
            crate::logic::plugins::Value::Int(handle.id as i64),
            crate::logic::plugins::Value::Int(x as i64),
            crate::logic::plugins::Value::Int(y as i64),
            crate::logic::plugins::Value::Int(z as i64),
            crate::logic::plugins::Value::Int(laid.result as i64),
        ],
        Some(vec![at]),
    ) {
        return put_straight("a plugin refused that change");
    }
    // A course laid round somebody's ankles is a course laid inside them.
    if ctx.registry.player_occupying_block(x, y, z, laid.result).is_some() {
        return put_straight("can't build inside somebody");
    }
    // Spent before the write and given back if the write fails: the order the
    // placement path keeps, for its reason -- refusing after spending is how
    // players quietly lose things.
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        // The hand first, then the rest of the pack for what it was short.
        // Counted before either is touched, under the one lock, so the
        // second take cannot come up short after the first has happened.
        let enough = state.inventory.block_in(slot) == Some(held) && state.inventory.count(held) >= laid.spends;
        let spent = enough && {
            let in_hand = state.inventory.count_in(slot).min(laid.spends);
            state.inventory.take_from(slot, in_hand) == in_hand
                && (in_hand == laid.spends || state.inventory.take_exact(held, laid.spends - in_hand))
        };
        let mortared = spent && (!laid.mortar || state.inventory.take_exact(primitive_shared::types::BLOCK_MORTAR, 1));
        if spent && !mortared {
            state.inventory.add(held, laid.spends);
        }
        if !mortared {
            std::mem::drop(state);
            return put_straight("you are not carrying enough of that");
        }
        state.inventory_dirty = true;
    }
    if !ctx.world.set_block(x, y, z, laid.result) {
        {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            state.inventory.add(held, laid.spends);
            if laid.mortar {
                state.inventory.add(primitive_shared::types::BLOCK_MORTAR, 1);
            }
        }
        crate::send_inventory(handle);
        return;
    }
    crate::send_inventory(handle);
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.last_edit = Some(std::time::Instant::now());
    }
    // **Wet mud starts drying the moment it is on the wall** (`logic::walls`).
    if build::is_wet(laid.result) {
        ctx.walls.lock().unwrap_or_else(|e| e.into_inner()).lay(at);
    }
    crate::broadcast_block(ctx, at, laid.result);
    {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.on_block_changed(x, y, z);
    }
    crate::notify_mechanics(ctx, x, y, z);
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
    // Requests taken off the socket that are not in the world yet.
    //
    // The pump used to have no such thing: it took a position and
    // *waited* for terrain, one chunk at a time. Now it hands the whole
    // batch to the generator pool and keeps the positions here until
    // they turn up, which is what lets one player's chunks be made on
    // every spare core at once. See `logic::chunkgen`.
    //
    // Bounded by the same setting that bounds the socket queue behind
    // it, so a client that asks for a continent gets a continent's worth
    // of refusals rather than a growing list.
    let mut waiting: Vec<ChunkPos> = Vec::new();

    loop {
        ticker.tick().await;

        // Everything the client has asked for since the last tick, moved
        // out of the channel in one go. Draining rather than taking a
        // budget's worth: a request is a few bytes on a list, and
        // leaving them in the channel would only mean the generator
        // starts on them later.
        let room = ctx
            .settings
            .chunk_queue_capacity
            .saturating_sub(waiting.len());
        for _ in 0..room {
            match chunk_rx.try_recv() {
                Ok(pos) => waiting.push(pos),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }

        if !waiting.is_empty() {
            // Nearest first, from where the player actually is. The
            // generator pool orders its whole queue by this, so a join
            // fills in the ground underfoot before the horizon -- and
            // two players standing together share the work rather than
            // doing it twice each.
            let (px, _, pz) = {
                let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.position
            };
            let at = ChunkPos::from_world(px, pz);
            ctx.chunks.request_many(waiting.iter().map(|&pos| {
                // Widened *before* the subtraction, not after. `pos` is
                // the client's number, and `(pos.x - at.x) as i64`
                // overflowed the i32 first and only then widened the
                // wreckage -- a panic on this task in a debug build, and
                // a nonsense sort key in release.
                let (dx, dz) = (
                    pos.x as i64 - at.x as i64,
                    pos.z as i64 - at.z as i64,
                );
                (pos, dx * dx + dz * dz)
            }));
        }

        let mut sent = 0usize;
        let budget = ctx.settings.chunk_send_budget_per_tick;
        // Whatever is ready, in the order it was asked for. A chunk that
        // is not ready costs one cache probe and stays on the list.
        waiting.retain(|&pos| {
            if sent >= budget {
                return true;
            }
            let Some(chunk) = ctx.chunks.take(pos) else {
                return true; // still being made; ask again next tick
            };

            // **Framed here, and the flat copy let go at once.** The cache
            // keeps chunks packed and the protocol carries a flat `Chunk`
            // (run-length encoded on the wire, about 13 KB), so sending
            // one means unpacking it. Queueing the unpacked chunk as a
            // message -- which is what this used to queue, back when it
            // was a reference count into the cache -- would park 131 KB
            // per message in a queue `outgoing_queue_capacity` deep: 67 MB
            // for one client that has fallen behind. The bytes are what
            // the writer task would have produced from it anyway, so the
            // serialising moves here rather than happening twice.
            //
            // Framed before anything is recorded: a chunk that cannot be
            // serialised is a bug, and it must not leave the player
            // subscribed to a chunk they were never sent. The client's
            // retry timer asks for it again.
            let Some(bytes) = frame(&ServerMessage::ChunkData(Arc::new(chunk.unpack()))) else {
                return false;
            };
            {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.loaded_chunks.insert(pos);
            }
            ctx.registry.subscribe(handle.id, pos);

            if !handle.send_raw(bytes) {
                // Queue full: the client is behind. Drop it and let the
                // client's own retry timer ask again.
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.loaded_chunks.remove(&pos);
                drop(state);
                ctx.registry.unsubscribe(handle.id, pos);
                // Stop for this tick: one full queue means the rest of
                // the batch would be dropped too.
                sent = budget;
                return true;
            }
            // What any body in it is wearing, right behind the chunk: the
            // client draws a body it has heard nothing about bare, and a
            // body is in a chunk exactly when that chunk is sent. A
            // palette check per section, so a chunk with no body in it
            // costs a few dozen lookups (`PackedChunk::cells_where`).
            let (base_x, base_z) = (
                pos.x * primitive_shared::types::CHUNK_SIZE_X as i32,
                pos.z * primitive_shared::types::CHUNK_SIZE_Z as i32,
            );
            chunk.cells_where(
                |id| primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_CORPSE,
                |x, y, z| {
                    if let Some(message) =
                        crate::body_worn_message(&ctx, (base_x + x as i32, y as i32, base_z + z as i32))
                    {
                        handle.send(message);
                    }
                },
            );
            // ...and what is in every pit kiln in it, on the same terms: a
            // client that has not heard draws plain pots. See
            // `ServerMessage::PitPottery`.
            chunk.cells_where(primitive_shared::pit::is_pit_kiln, |x, y, z| {
                if let Some(message) =
                    crate::pit_pottery_message(&ctx, (base_x + x as i32, y as i32, base_z + z as i32))
                {
                    handle.send(message);
                }
            });
            // ...and what lies in every cell a hand set something down in:
            // a client that has not heard draws nothing there. See
            // `ServerMessage::SetDownItem`.
            chunk.cells_where(primitive_shared::types::is_set_down, |x, y, z| {
                if let Some(message) =
                    crate::set_down_item_message(&ctx, (base_x + x as i32, y as i32, base_z + z as i32))
                {
                    handle.send(message);
                }
            });
            // ...and every lid in it that is standing open: a client that
            // has not heard draws it shut. See `crate::open_lids_in`.
            for message in crate::open_lids_in(&ctx, pos) {
                handle.send(message);
            }
            ctx.metrics.chunks_sent.fetch_add(1, Ordering::Relaxed);
            sent += 1;
            false
        });

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
            // Requests for terrain the player has walked away from.
            // Dropped rather than served: the client stopped wanting
            // them the moment they left its own radius, and a pump that
            // kept a walking player's whole trail on its list would
            // spend every tick sending chunks nobody is going to draw.
            waiting.retain(|pos| player_chunk.chebyshev_distance(*pos) <= keep);
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
