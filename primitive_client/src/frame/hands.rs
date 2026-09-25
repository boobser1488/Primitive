//! **The hands: mining, blows, the cut, the mouthful, and the arm.**
//!
//! Breaking takes time, so it advances here rather than on the click, and
//! everything that follows from a swing follows from *that* rather than
//! deciding any of it: a blow lands on somebody, or a block is coming
//! apart, and the arm is what that looks like.
//!
//! Where it sits: after the body, because what is under the crosshair
//! depends on where physics put the player; before the sound, because the
//! ear reads what this decided -- which is why the answer comes back as
//! [`Worked`] instead of being worked out a second time from the keys.

use std::time::Instant;

use crate::audio::{self, Audio, Soundscape};
use crate::engine::camera::Camera;
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::inventory::Inventory;
use crate::logic::physics::Player;
use crate::logic::{self, hand, mining as mining_mod, stamina};
use crate::net::network;
use crate::net::remote_players::RemotePlayers;
use crate::ui::debug::DebugStats;
use crate::ui::{death, input};
use crate::{
    aimed_block, aimed_block_to_mine, aimed_face_to_mine, animal_under_crosshair,
    photographed_swing, player_under_crosshair, request_break, request_dig, send_blow, Aimed, Cut,
    CutStep, DigSignal, Meal, MealStep,
};
use primitive_shared::protocol::Action as protocol_action;
use primitive_shared::protocol::ClientMessage;
use primitive_shared::types::BlockId;

/// **What the hands did this frame, for the ear to read.**
///
/// Four answers the soundscape needs and must not work out a second time:
/// a swing that misses is a different sound from a swing that lands, and
/// a second list of conditions is a second list to keep in step. See the
/// bob's own history in `frame::body` for what that costs.
pub struct Worked {
    /// Whether a swing was possible at all -- in a world, not paused, not
    /// dead, not on the ground, the cursor grabbed, no hand on the sheets.
    pub can_mine: bool,
    /// What the client believes is actually coming apart under the
    /// crosshair. `None` is a swing at thin air or at bedrock.
    pub aim: Option<((i32, i32, i32), BlockId)>,
    /// Whether somebody is being swung at, which keeps mining out of the
    /// way: the wall behind them must not come apart between blows.
    pub struck: bool,
    /// What the blow is struck *with*: its rhythm and half its noise.
    pub held: Option<BlockId>,
}

/// One frame of the hands.
#[allow(clippy::too_many_arguments)]
pub fn step(
    dt: f32,
    now: Instant,
    world_ready: bool,
    paused: bool,
    trimming: Option<primitive_shared::protocol::EntityId>,
    death: &death::DeathScreen,
    body: &crate::ui::hud::BodyGauges,
    input: &input::InputState,
    inventory: &Inventory,
    player: &Player,
    remote_players: &mut RemotePlayers,
    chunks: &ChunkManager,
    camera: &Camera,
    entities: &logic::entities::Entities,
    rod_hold: &logic::fishing::Hold,
    net: &network::NetworkHandle,
    audio: &Audio,
    soundscape: &mut Soundscape,
    mining: &mut mining_mod::Mining,
    stamina: &mut stamina::Stamina,
    strikes: &mut hand::Strikes,
    hand: &mut hand::Hand,
    dig_signal: &mut DigSignal,
    cut: &mut Option<Cut>,
    meal: &mut Option<Meal>,
    meal_sent: &mut Option<Instant>,
    debug_stats: &mut DebugStats,
) -> Worked {
    // --- mining, and hitting people ---
    //
    // Breaking takes time, so it advances here rather
    // than on the click. A dead or paused player is not
    // swinging at anything.
    // ...and a hand on the sheets is not a hand on a pick.
    // The same button does both, so the one the player is
    // actually using has to take it: without this, bracing
    // the yard from the stern would also be swinging at the
    // planks under the rower, and five of those break the
    // raft (`raft::HITS_TO_BREAK`).
    // ...and a body on the ground has hands for itself and
    // nothing else: no pick, no blow (the server refuses
    // both anyway -- `barred_while_downed` -- and a crack
    // that grew on a block nobody could break would be a lie).
    let can_mine = world_ready
        && !paused
        && !death.is_open()
        && body.downed.is_none()
        && input.mouse_grabbed
        && trimming.is_none();

    // Someone under the crosshair takes the swing before
    // the world behind them does. Nearer than whatever
    // block is there, or a punch through a wall would
    // land -- the server checks the distance, but it has
    // no idea what is between the two of them.
    // ...and an animal takes it before the world does,
    // for the same reason and one rung down: a person in
    // front of a deer takes the blow, and a deer in
    // front of a wall stops the wall coming apart.
    //
    // Looked for when a blow could start *or land*: a
    // thrust lands when its point is out
    // (`hand::impact_seconds`), which can be after the
    // button was let go, so the button is not the only
    // reason to cast the ray any more.
    let held_now = inventory.block_in(input.hotbar_slot);
    let under_crosshair = if can_mine && (input.breaking || strikes.due(now)) {
        player_under_crosshair(remote_players, chunks, camera, held_now)
            .map(Aimed::Player)
            .or_else(|| {
                animal_under_crosshair(entities, chunks, camera, held_now)
                    .map(Aimed::Animal)
            })
    } else {
        None
    };
    // Someone is being swung at, which keeps mining out of
    // the way: the wall behind them must not come apart in
    // the gaps between blows.
    let struck = input.breaking && under_crosshair.is_some();
    // ...and whether a blow starts or lands this frame.
    // The message leaves on the landing, aimed at whoever
    // is under the crosshair *then*: a deer that stepped
    // off the line while the spear was drawn is a miss,
    // and a miss sends nothing.
    let beat = strikes.frame(now, struck, held_now);
    if beat.lands {
        if let Some(target) = under_crosshair {
            send_blow(target, net, debug_stats);
            soundscape.on_strike(audio, true);
        }
    }

    // Anything a bare hand cannot get through is not
    // aimed at for the purpose of mining: the progress
    // bar never starts and the cracks never appear,
    // rather than filling up and achieving nothing.
    //
    // Nor is anything behind a player being hit: one
    // button, one thing at a time, or a fight in front
    // of a wall quietly digs it out.
    //
    // What "a bare hand" means now depends on what is in
    // the selected slot: the same rock that ignores
    // fingers gives way to a pick. The client predicts
    // this so the bar and the cracks agree with what the
    // server will allow -- both sides read the same
    // `break_seconds_with`, which is the whole reason it
    // lives in `primitive_shared`.
    let held_tool = inventory.block_in(input.hotbar_slot);
    // ...and how well that tool was made, which is a small
    // divisor on the time (`quality::speed_scale`) and the
    // stamina both. Off the stack and not the id: a fine
    // pick and a poor one are the same block.
    let tool_quality = inventory
        .slots()
        .get(input.hotbar_slot)
        .copied()
        .flatten()
        .map_or(primitive_shared::quality::Quality::PLAIN, |s| s.quality());
    //
    // **And the ray this one casts is blind to water**, which
    // the right click's is not: see `aimed_block_to_mine` for
    // the player report that was, and for why one ray cannot
    // answer both gestures.
    let aim = if can_mine && !struck {
        aimed_block_to_mine(chunks, camera).filter(|(_, block)| {
            primitive_shared::types::is_breakable_with(*block, held_tool)
        })
    } else {
        None
    };
    // Digging is work, and it comes out of the same tank
    // running and jumping do. An exhausted player digs
    // slower rather than not at all -- see
    // `stamina::EXHAUSTED_DIG_RATE` -- so the progress
    // the swing makes is scaled here and the bill is
    // paid on the swing that finished.
    // ...and a broken arm digs at half, on the same terms:
    // the swing is slower, the block is not softer. See
    // `injury::BROKEN_ARM_STRENGTH`.
    let dug = mining.update(
        aim,
        can_mine && input.breaking,
        dt * stamina.dig_rate() * body.injuries.strength_factor(),
        held_tool,
        tool_quality,
    );
    // The outline and the cracks go where the target is in
    // the world -- a leaning palm where it leans. See
    // `Mining::fit_outline`.
    mining.fit_outline(|cell, block| {
        primitive_shared::geometry::block_box_for_aim_near(block, cell.0, cell.1, cell.2, false, |dx, dy, dz| {
            chunks
                .block_at(cell.0 + dx, cell.1 + dy, cell.2 + dz)
                .unwrap_or(primitive_shared::types::BLOCK_AIR)
        })
    });
    if let Some(cell) = dug {
        // Billed for the work the swing actually was: a
        // pick makes a block cheaper in seconds, and the
        // tank is measured in seconds, so a better tool
        // is less tiring as well as faster.
        // ...and for the work the *swing* was: a block that
        // comes away in quarters is four swings of a
        // quarter of the bill, which adds up to the bill it
        // always was. See `dig::swing_seconds`.
        if let Some(seconds) = aim.and_then(|(_, block)| {
            primitive_shared::dig::swing_seconds_made(block, held_tool, tool_quality)
        }) {
            stamina.spend_dig(seconds);
        }
        // **A slice or a break, and the block decides
        // which.** Everything that happens when a block
        // finally goes -- the drop, the tool's wear, the
        // collapse -- is the break path, so the last slice
        // is the break message it always was and only the
        // ones before it are `Dig`. See `dig::next_bite`.
        let face = aimed_face_to_mine(chunks, camera);
        let sliced = match (aim, face) {
            (Some((_, block)), Some(face)) => {
                primitive_shared::dig::Side::from_normal((
                    i32::from(face.0),
                    i32::from(face.1),
                    i32::from(face.2),
                ))
                .and_then(|side| primitive_shared::dig::next_bite(block, side))
                .is_some()
            }
            _ => false,
        };
        if sliced {
            request_dig(chunks, cell, face.unwrap_or((0, 1, 0)), net, debug_stats);
        } else {
            request_break(chunks, cell, net, debug_stats);
        }
    }
    // The arm follows from all of that rather than
    // deciding any of it: a blow lands on a player, or a
    // block is coming apart, and the hand is what that
    // looks like. Advanced every frame even though the
    // geometry is only rebuilt at `DYNAMIC_REBUILD_HZ` --
    // the state is a few floats, and letting it skip
    // frames would make the swing's length depend on the
    // frame rate.
    //
    // **One blow on screen per blow struck.** The arm used
    // to be started on every frame someone was under the
    // crosshair, so a fist fight swung twice for each punch
    // the cooldown let through and a spear jabbed three
    // times for each thrust. It starts when `Strikes` says
    // a blow does, and lasts as long as the blow is refused
    // for (`hand::blow_seconds`).
    if beat.starts {
        hand.strike(held_now);
    }
    // Not "the button is down": swinging at thin air or at
    // bedrock is a click, not a rhythm. `aim` is what the
    // client believes is actually coming apart under the
    // crosshair.
    let digging_now = can_mine && input.breaking && aim.is_some();
    // A block set down, a mouthful or a drink the server let
    // through, acted out by the player's own hand. See
    // `Hand::gesture`.
    if let Some(action) = remote_players.take_own_gesture() {
        // A mouthful this hand already lifted for (`Meal`)
        // is not lifted a second time when the server says
        // it went down.
        let echoed = matches!(action, protocol_action::Eat | protocol_action::Drink)
            && meal_sent.is_some_and(|at| at.elapsed().as_secs_f32() < 1.5);
        if !echoed {
            hand.gesture(action);
        }
    }
    // ...and the cut under way, sent when the knife is through.
    if let Some(cutting) = *cut {
        let aimed_now = aimed_block(chunks, camera);
        match cutting.due(aimed_now, input.hotbar_slot, Instant::now()) {
            CutStep::Cutting => {
                // The knife keeps working: a stroke whenever
                // the last one has finished.
                hand.strike(inventory.block_in(input.hotbar_slot));
            }
            CutStep::Abandoned => *cut = None,
            CutStep::Through => {
                *cut = None;
                net.send(ClientMessage::UseBlock {
                    global_x: cutting.cell.0,
                    global_y: cutting.cell.1,
                    global_z: cutting.cell.2,
                });
                debug_stats.network_messages_out_this_second += 1;
            }
        }
    }
    // ...and the mouthful under way, sent when it is chewed.
    if let Some(taking) = *meal {
        match taking.due(inventory, Instant::now()) {
            MealStep::Chewing => {}
            MealStep::Abandoned => *meal = None,
            MealStep::Swallow => {
                *meal = None;
                // The crunch or the swallow, when it goes down.
                audio.play(if taking.drinking { audio::Sfx::Drink } else { audio::Sfx::Eat });
                net.send(ClientMessage::Eat { slot: taking.slot as u8 });
                debug_stats.network_messages_out_this_second += 1;
                *meal_sent = Some(Instant::now());
            }
        }
    }
    hand.update(
        dt,
        digging_now,
        player.velocity.with_y(0.0).length(),
        player.grounded,
        held_now,
    );
    // **A landed blow moves nothing on screen.** The recoil that
    // used to be taken here is gone: a swing is one event but mining
    // is a rhythm of them, and a view that dipped on every blow made
    // a quarry tiring to look at. `Hand::take_landed` is still the
    // one thing that knows when the arm is actually down, and the
    // sound still reads it; see `logic::shake` for the numbers the
    // recoil had and why it is not simply smaller.
    // ...and the same swing, for everybody else's picture
    // of this player. See `DigSignal`.
    // The rod drawn back follows the wind-up. See `hand::Rod`.
    hand.wind_rod(rod_hold.charge(), dt);
    // **A rod being wound up is said the way a swing is.**
    // `Gesture::digging` is "working what is in the hand" to
    // everybody watching, and with a rod in it that work is
    // drawing the rod back (`RemotePlayer::arm`). A second
    // flag would be a byte a player a tick and a message of
    // its own for a thing that is never true at the same
    // time as the first.
    let working = digging_now || rod_hold.charge().is_some();
    if let Some(digging) = dig_signal.frame(Instant::now(), working) {
        net.send(ClientMessage::Digging { digging });
        debug_stats.network_messages_out_this_second += 1;
    }
    // ...and then, if somebody is photographing the
    // blow, held at the phase they asked for. After
    // `update` rather than instead of it, so the bob
    // and everything else still run. See
    // `photographed_swing`.
    if let Some(phase) = photographed_swing() {
        hand.freeze(phase);
    }

    Worked { can_mine, aim, struck, held: held_now }
}
