//! **The screens the player did not ask for.**
//!
//! Dying, a chest the server opened, a station's seat, the morning after
//! a night's sleep, a box asking what to call the cairn just built.
//!
//! All of these arrive as *messages* rather than as events, which is why
//! the cursor changes hands here and nowhere else: there is no keypress
//! to hang it on, and a hand-off written at each message site would be
//! several hand-offs to keep in step. Done once, on the frame the answer
//! changes -- which is what `was_dead`, `chest_was_open` and
//! `station_was_open` are for.

use crate::audio::{self, Audio};
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::mining::Mining;
use crate::logic::{self, posture};
use crate::net::network;
use crate::platform;
use crate::settings::ClientSettings;
use crate::ui;
use crate::ui::debug::DebugStats;
use crate::ui::{chat, chest_screen, death, input, inventory_screen, station_screen};
use crate::{grab_cursor, release_cursor, send_station_intent};
use std::time::Instant;

/// The hand-offs this frame's messages caused, in the order the screens
/// sit in.
#[allow(clippy::too_many_arguments)]
pub fn hand_off(
    dt: f32,
    paused: bool,
    settings: &ClientSettings,
    net: &network::NetworkHandle,
    window: &dyn platform::Window,
    audio: &Audio,
    chunks: &ChunkManager,
    resting: posture::Resting,
    sleep: &mut posture::Sleep,
    mining: &mut Mining,
    input: &mut input::InputState,
    chat: &mut chat::Chat,
    journal: &mut ui::journal::Journal,
    inventory_screen: &mut inventory_screen::InventoryScreen,
    chest_screen: &mut chest_screen::ChestScreen,
    station_screen: &mut station_screen::StationScreen,
    death: &mut death::DeathScreen,
    notice: &mut Option<(String, Instant)>,
    was_dead: &mut bool,
    chest_was_open: &mut bool,
    station_was_open: &mut bool,
    chest_lid: &mut bool,
    debug_stats: &mut DebugStats,
) {
    // **A cairn the server has just agreed to asks for its
    // name**, in the chat box (see `Chat::open_naming` for
    // why that box). Not over a screen the player has open
    // or a line they are typing: the heap is already a mark
    // on this player's map, and this only gives it a word.
    //
    // **...and only for a player carrying a map**, because
    // for anybody else there is no mark to name: the server
    // writes one down for the placer only if they had the
    // hide on them (`trail::Trail::mark`). A box asking
    // "name this cairn for your map" from somebody with no
    // map is a prompt whose answer goes nowhere.
    if let Some(cell) = mining.take_piled_cairn() {
        if journal.carries_map()
            && !paused
            && !chat.is_typing()
            && !inventory_screen.open
            && !chest_screen.is_open()
            && !station_screen.is_open()
            && !journal.is_open()
            && !death.is_open()
        {
            let current = journal.explored.mark_name(cell).unwrap_or("").to_string();
            chat.open_naming(cell, &current, Instant::now());
            window.set_ime_visible(true);
            release_cursor(window, input);
            input.release_all();
        }
    }

    // Dying and coming back are the two moments the
    // cursor changes hands without the player pressing
    // anything, and both of them arrive as a message
    // rather than as an event -- so the hand-off is done
    // here, once, on the frame the answer changes.
    // The dark a sleeper's screen goes, and the morning it
    // lifts on. The morning is said once the dark has gone
    // rather than when the server wakes the player, or the
    // line would spend its three seconds printed on black --
    // and only to a player still lying there: one a blow
    // woke is on their feet by now and has no morning to be
    // told about.
    if sleep.tick(dt) && matches!(resting, logic::posture::Resting::Lying { .. }) {
        *notice = Some((
            settings.language.text(ui::lang::Msg::SleepMorning).to_string(),
            Instant::now(),
        ));
    }

    death.tick(dt);
    if death.is_open() != *was_dead {
        *was_dead = death.is_open();
        if *was_dead {
            // One screen at a time, and this one is not
            // optional.
            chat.close();
            inventory_screen.close();
            chest_screen.close();
            station_screen.close();
            release_cursor(window, input);
            input.release_all();
        } else if !paused && !chat.is_typing() {
            grab_cursor(window, input);
        }
    }

    // A chest opens when the server answers, not when
    // the player clicks, so the cursor changes hands
    // here for the same reason dying does.
    if chest_screen.is_open() != *chest_was_open {
        *chest_was_open = chest_screen.is_open();
        // The lid, on the frame the answer changes --
        // which is when the server says so, not when the
        // player clicked.
        if *chest_was_open {
            *chest_lid = chest_screen.at().and_then(|(x, y, z)| chunks.block_at(x, y, z)).is_some_and(
                |block| primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_CHEST,
            );
        }
        if *chest_lid {
            audio.play(if *chest_was_open { audio::Sfx::ChestOpen } else { audio::Sfx::ChestClose });
        }
        if *chest_was_open {
            // One screen at a time.
            inventory_screen.close();
            chat.close();
            release_cursor(window, input);
            input.release_all();
        } else if !paused && !death.is_open() && !chat.is_typing() {
            grab_cursor(window, input);
        }
    }

    // ...and the station screen, which opens on the server's
    // answer exactly as the chest does.
    if station_screen.is_open() != *station_was_open {
        *station_was_open = station_screen.is_open();
        if *station_was_open {
            inventory_screen.close();
            chat.close();
            release_cursor(window, input);
            input.release_all();
        } else if !paused && !death.is_open() && !chat.is_typing() {
            grab_cursor(window, input);
        }
    }
    // A run whose last window has gone by is handed in without
    // the player doing anything: the blows they did not strike
    // are misses, and a screen that waited for ever for a
    // fourth press would be a screen holding their bar.
    if let Some(intent) = station_screen.poll() {
        send_station_intent(intent, station_screen, net, debug_stats, audio);
    }
}
