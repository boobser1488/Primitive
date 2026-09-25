//! **What a right click in the world asks of the server.**
//!
//! The order the gestures are tried in is the whole of this file, and it
//! is written at each site with the thing it prevents. A door swings
//! before it is sent (a door that waited a round trip sticks, and the
//! player walking through it walks into it); a chest takes the click
//! before a placement (or the only way to use one with something in hand
//! would be to empty your hand first); a fire takes it before a chest's
//! rule, because striking flint on a hearth is not about what is inside
//! it; a modifier means "not that" and lays the thing down.
//!
//! **This is the piece the scenarios used to keep a copy of.** The
//! harness plays right clicks -- a chest opened, a fire struck, a raft
//! planked, a jug filled -- and while the dispatch lived inside a match
//! arm it had to guess the order. Two answers to "what does a right click
//! send" is a scenario passing on the one the game does not use. Now the
//! game and the harness call this.
//!
//! What is *not* here is the part that needs the rest of the client: a
//! body on the ground reaching for a dressing, a hand held out to
//! somebody lying down, a raft, a horse, an animal, a map. Those are in
//! `events::on_mouse_button`, above the call to this, because each of
//! them is about an entity or a screen rather than about a block.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use crate::audio::{self, Audio};
use crate::engine::camera::Camera;
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::inventory::Inventory;
use crate::logic::mining::Mining;
use crate::logic::physics::Player;
use crate::logic::{self, hand, physics};
use crate::net::network;
use crate::settings::ClientSettings;
use crate::ui;
use crate::ui::debug::DebugStats;
use crate::ui::{chest_screen, input, keybinds, station_screen};
use crate::{
    aimed_block, apply_change, blazeable, door_swing, eat_from, ground_fire_claim, set_down_cell,
    swallow_expected, try_place_block, use_gesture, Arrivals, Cut, Meal, MeshQueueSet, UseGesture,
    INTERACT_RANGE,
};
use primitive_shared::protocol::ClientMessage;
use primitive_shared::types::{BlockId, ChunkPos};

/// One right click, once the entities in front of the block have had
/// their say.
#[allow(clippy::too_many_arguments)]
pub fn right_click_on_the_world(
    thumb_quick: bool,
    held: Option<BlockId>,
    settings: &ClientSettings,
    net: Option<&network::NetworkHandle>,
    audio: &Audio,
    player: &Player,
    camera: &Camera,
    body: &ui::hud::BodyGauges,
    entities: &logic::entities::Entities,
    others: &[glam::DVec3],
    input: &input::InputState,
    inventory: &Inventory,
    chunks: &mut ChunkManager,
    light: &mut primitive_shared::lighting::LightMap,
    arrivals: &mut Arrivals,
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    chunk_versions: &mut HashMap<ChunkPos, u64>,
    chest_screen: &mut chest_screen::ChestScreen,
    station_screen: &mut station_screen::StationScreen,
    mining: &mut Mining,
    hand: &mut hand::Hand,
    cut: &mut Option<Cut>,
    meal: &mut Option<Meal>,
    notice: &mut Option<(String, Instant)>,
    debug_stats: &mut DebugStats,
) {
    let aimed = aimed_block(chunks, camera);
    let mut claim = use_gesture(aimed.map(|(_, block)| block), held);
    // **A blaze: the modifier, a knife, a standing
    // tree** (`types::BLOCK_BLAZE`). Before the set-down
    // that the modifier otherwise means, because a knife
    // aimed at the *side* of a trunk has nowhere to be
    // laid down anyway -- the set-down wants a flat top
    // (`set_down_cell`) -- so what this takes over is a
    // gesture that could only ever answer "not there".
    // The plain click stays what it was: a tap for resin
    // or bark, which is the thing a player does twenty
    // times an evening.
    if let (true, Some((cell, block)), Some(held), Some(net)) = (
        thumb_quick || input.action_down(&settings.keybinds, keybinds::Action::Sprint),
        aimed,
        held,
        net,
    ) {
        if primitive_shared::types::is_knife(held) && blazeable(block) {
            net.send(ClientMessage::Blaze {
                global_x: cell.0,
                global_y: cell.1,
                global_z: cell.2,
            });
            debug_stats.network_messages_out_this_second += 1;
            hand.strike(Some(held));
            return;
        }
    }
    // **Set down, with the modifier held**: anything not
    // built with, one at a time, on the top of a block --
    // the jug's gesture (`UseGesture::OpenVessel`) for
    // everything else a hand carries. First, before the
    // food is eaten and the knife scores a trunk: the
    // modifier is the player saying "not that". The cost
    // is the jug's too: the modifier is the sprint key,
    // so a player running with bread who right-clicks
    // lays the loaf down; the eat key still eats.
    //
    // Not at a thing already set down, which the plain
    // click takes back (`use_gesture`): a knife laid on a
    // knife has nothing to lie on.
    // ...and not a torch held to a fire: Shift and the
    // click there light it, as the plain click does. A
    // torch laid on a burning campfire is a torch nobody
    // meant to put down.
    let setting_down = held.is_some_and(primitive_shared::types::can_be_set_down)
        && !aimed.is_some_and(|(_, block)| primitive_shared::types::is_set_down(block))
        && claim != UseGesture::Hearth
        && (thumb_quick
            || input.action_down(&settings.keybinds, keybinds::Action::Sprint));
    if setting_down {
        if let Some(net) = net {
            match set_down_cell(chunks, camera) {
                Some(cell) => {
                    net.send(ClientMessage::SetDown {
                        global_x: cell.0,
                        global_y: cell.1,
                        global_z: cell.2,
                    });
                    debug_stats.network_messages_out_this_second += 1;
                }
                // Said here, in the player's language, and
                // never sent: the server would refuse it
                // in English.
                None => {
                    *notice = Some((
                        settings.language.text(ui::lang::Msg::SetDownWhere).to_string(),
                        Instant::now(),
                    ));
                }
            }
        }
        return;
    }
    // **A rod in hand takes the press out of this path
    // altogether.** The throw is a hold and a release and
    // the strike is a tap, both of them driven off the
    // button's *state* once a frame (`rod_hold` below),
    // because a press here is an event and a wind-up is a
    // duration. Nothing is sent from here with a rod in
    // hand, and nothing must be: this is where a cast
    // used to turn into a `UseBlock`, and leaving it
    // would mean every throw also drank the lake.
    if held.is_some_and(|held| {
        primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_FISHING_ROD
    }) {
        return;
    }
    // **Fishing: what is said instead of a reach into an
    // empty trap**, in the player's language, from the
    // survey the server makes too -- and never sent. See
    // `logic::fishing`.
    if let Some((cell, block)) = aimed {
        if held.is_none() {
            if let Some(said) =
                logic::fishing::trap_notice(|x, y, z| chunks.block_at(x, y, z), cell, block)
            {
                *notice = Some((settings.language.text(said.msg()).to_string(), Instant::now()));
                return;
            }
        }
        // ...and a snare or a salt pan with nothing to
        // give, whatever is in the hand (`set_notice`).
        if let Some(said) = logic::fishing::set_notice(block, held) {
            *notice = Some((settings.language.text(said.msg()).to_string(), Instant::now()));
            return;
        }
    }
    // **Fires in the ground, where the world decides.**
    // `use_gesture` answers a pit kiln or a pile by its
    // block; these two need the cells round the aim --
    // pottery at the floor of an empty pit, and flint
    // at the sticks and log a firepit is laid from --
    // so they are asked here, and only of what would
    // otherwise be a placement. See `ground_fire_claim`.
    if claim == UseGesture::Place && ground_fire_claim(chunks, entities, aimed, held) {
        claim = UseGesture::Pit;
    }
    // ...and a log laid with the modifier held, which is
    // TerraFirmaCraft's log pile: shift and a right
    // click. Into the cell a placement would have used.
    if claim == UseGesture::Place
        && held.is_some_and(primitive_shared::pit::is_log)
        && (thumb_quick
            || input.action_down(&settings.keybinds, keybinds::Action::Sprint))
    {
        if let (Some((_, before)), Some(net)) = (
            physics::raycast_block(chunks, camera.position, camera.forward(), INTERACT_RANGE),
            net,
        ) {
            net.send(ClientMessage::PileLog {
                global_x: before.0,
                global_y: before.1,
                global_z: before.2,
            });
            debug_stats.network_messages_out_this_second += 1;
            return;
        }
    }
    // **A door swings here first, and is told to the
    // server after.** Everything else a right click does
    // waits for the server's answer; a door that waited
    // a round trip would stick on every server further
    // away than the next room, and the player walking
    // through it would walk into it. The server swings
    // the same two cells (`swing_door`) and puts a door
    // it refused back the way it hangs, so what this
    // predicts is only ever corrected, never kept wrong.
    if let (UseGesture::Swing, Some((cell, block)), Some(net)) =
        (claim, aimed, net)
    {
        let swung = door_swing(chunks, cell, block);
        let opening = primitive_shared::types::door_is_open(swung[0].block_id);
        audio.play_at_block(
            if opening { audio::Sfx::ChestOpen } else { audio::Sfx::ChestClose },
            cell,
            0.9,
            1.0,
        );
        for change in swung {
            apply_change(
                chunks,
                light,
                arrivals,
                urgent,
                dirty_set,
                chunk_versions,
                change,
            );
        }
        net.send(ClientMessage::UseBlock {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
        });
        debug_stats.network_messages_out_this_second += 1;
        return;
    }
    // The anvil and the wheel: a question, like a chest's,
    // and the screen opens when the answer comes back. The
    // server decides whether there is a hammer in the hand
    // and how wide the sweet spot is, so a client cannot
    // open a forgiving anvil for itself.
    if let (UseGesture::Station, Some((cell, _)), Some(net)) =
        (claim, aimed, net)
    {
        net.send(ClientMessage::OpenStation {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
        });
        station_screen.asked_to_open();
        debug_stats.network_messages_out_this_second += 1;
        return;
    }
    if let (UseGesture::Open, Some((cell, _)), Some(net)) =
        (claim, aimed, net)
    {
        net.send(ClientMessage::OpenChest {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
        });
        // A new question: whatever it answers is
        // wanted, even about a chest just shut.
        chest_screen.asked_to_open();
        debug_stats.network_messages_out_this_second += 1;
        // The screen opens when the answer arrives.
        // Everything else waits for that, including
        // the cursor -- see the hand-off in the frame
        // loop.
        return;
    }
    // ...and a fire takes it before a placement too,
    // for exactly the same reason a chest does: the
    // only way to light one otherwise would be to
    // empty your hand first, and the flint you were
    // holding would go on the ground beside it.
    //
    // What the gesture *does* is the server's
    // business entirely -- strike a spark, or feed
    // the fire what is in your hand -- so the
    // message carries neither the effect nor the
    // item. See `ClientMessage::UseBlock`.
    //
    // A carcass goes the same way: the message says
    // *which cell*, and the server decides from what
    // it believes is in the hand whether that is a
    // cut, a ruined skin, or a hint about needing a
    // knife.
    //
    // Water joins them for the same reason: what a
    // click at a river does -- a mouthful, a filled
    // jug, or a warning that the sea is salt -- is
    // decided from the vitals and the water's kind,
    // and both live on the server.
    //
    // A pick goes the same way: whether there is room
    // in the pack for the apple is the server's
    // answer, and a client that predicted the leaves
    // bare would show a tree picked into a full pack.
    // **A cut takes the knife's time.** Held here and
    // sent when it is done (`Cut`), not sent on the
    // click; a second click while cutting is nothing.
    if let (UseGesture::Butcher, Some((cell, block))) = (claim, aimed) {
        if cut.is_none() {
            *cut = Some(Cut { cell, block, slot: input.hotbar_slot, started: Instant::now() });
            hand.strike(held);
        }
        return;
    }
    if let (
        UseGesture::Hearth
        | UseGesture::Butcher
        | UseGesture::Water
        | UseGesture::Pick
        | UseGesture::Tend
        | UseGesture::Rest
        | UseGesture::Pit,
        Some((cell, _)),
        Some(net),
    ) = (claim, aimed, net)
    {
        // The swallow, heard when the hand moves. See
        // `swallow_expected` for why the client
        // guesses at a sound it cannot be told.
        if claim == UseGesture::Water
            && swallow_expected(
                aimed.map(|(_, block)| block),
                held,
                body.hydration,
            )
        {
            audio.play(audio::Sfx::Drink);
        }
        net.send(ClientMessage::UseBlock {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
        });
        debug_stats.network_messages_out_this_second += 1;
        return;
    }
    // **Food in the hand is eaten by using it.**
    // The player asked for this in as many words:
    // "сделай возможность есть взяв в руку, а не
    // через HUD". Eating hung on a key and, on a
    // phone, on resting a finger on the hotbar slot
    // -- a gesture on the interface for something
    // the hand does. Both of those stay; this is the
    // one the hand already makes.
    if claim == UseGesture::Eat {
        eat_from(
            Some(input.hotbar_slot),
            inventory,
            meal,
            hand,
        );
        return;
    }
    // **A jug in the hand opens**, unless the
    // modifier is held -- which is how one is set
    // down now. See `UseGesture::OpenVessel`. Nothing
    // goes to the server: the jug and what is in it
    // are already in the pack snapshot.
    if claim == UseGesture::OpenVessel {
        let setting_down = thumb_quick
            || input.action_down(
                &settings.keybinds,
                keybinds::Action::Sprint,
            );
        if !setting_down {
            chest_screen.show_held_vessel(input.hotbar_slot, inventory);
            return;
        }
    }
    // Placing is still instant; only breaking takes
    // time. Held-to-repeat placement would need its
    // own cooldown, and the server rate-limits edits
    // anyway.
    if let Some(net) = net {
        try_place_block(
            chunks,
            camera,
            input,
            player,
            others,
            net,
            inventory,
            mining,
            debug_stats,
        );
    }
}
