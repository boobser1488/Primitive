//! **What the player did, turned into what the game does about it.**
//!
//! The three arms that were the bulk of `run`'s event match -- a finger,
//! a key, a mouse button -- plus the half of touch that is not an event
//! at all and has to be *polled* once a frame.
//!
//! They are functions rather than inline arms for two reasons. The
//! first is that six thousand lines in one body is a body nobody can
//! hold in their head. The second is the one that bites: a scenario
//! plays a right click, and while the dispatch lived inside a match arm
//! the harness had to keep its own copy of it -- see
//! [`super::interact`], which is where the shared half of that arm now
//! lives.
//!
//! **What could not move, and why.** Anything that ends the event loop
//! or starts a session: `quit!`, `abandon_pending!` and `handle_action!`
//! close over the loop's own `exit` and over the tokio runtime. So the
//! key and the mouse hand an `Action` *back* and `run` applies it, which
//! is one place deciding what a menu action means rather than two.

use std::time::Instant;

use crate::audio::{self, Audio};
use crate::engine::camera::Camera;
use crate::engine::renderer::GraphicsState;
use crate::logic::physics::Player;
use crate::net::network;
use crate::platform::{self, MouseButton};
use crate::settings;
use crate::ui;
use crate::ui::debug::DebugStats;
use crate::ui::menu::Menu;
use crate::ui::menu::{Action, Screen};
use crate::logic::chunk_manager::ChunkManager;
use crate::net::remote_players::RemotePlayers;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::ChunkPos;
use std::collections::{HashMap, VecDeque};
use crate::ui::{chat, chest_screen, death, hotbar, input, inventory_screen, keybinds};
use crate::ui::{station_screen, widgets};
use crate::logic::inventory::Inventory;
use crate::platform::Key as KeyCode;
use crate::settings::ClientSettings;
use crate::{
    ask_if_operator, close_chat, close_chest, close_station, eat_from, grab_cursor, hover_swap,
    hotbar_gesture_event, keys_may_type_into_the_field, menu_key, place_cursor, player_mark,
    release_cursor, send_journal_command, send_station_intent, something_to_throw, submit_chat,
    works_while_dead, world_owns_the_glass,
};
use crate::{
    aimed_block, chest_intent_message, player_under_crosshair, reconcile_the_editor, use_gesture,
    UseGesture, INTERACT_RANGE,
};
use crate::logic::physics;
use primitive_shared::protocol::ClientMessage;

/// **A finger, turned into whatever a hand would have done.**
///
/// The buttons that stand for a *click* or a *keystroke* become exactly
/// that event and fall through the frame's match with the mouse and the
/// keyboard, so there is one path for placing a block and not two. The
/// ones that stand for a *held* state -- the dig button, the movement
/// stick, the drag that turns the camera -- are not events at all; they
/// are read off `touch` once a frame by [`poll_touch`], where the
/// keyboard's own held keys are read.
///
/// `None` means the finger was spoken for and there is nothing left of
/// it to deliver.
#[allow(clippy::too_many_arguments)]
pub fn touch_to_event(
    id: platform::TouchId,
    phase: platform::TouchPhase,
    x: f32,
    y: f32,
    net: Option<&network::NetworkHandle>,
    paused: bool,
    graphics: &GraphicsState,
    window: &dyn platform::Window,
    audio: &Audio,
    keybinds: &keybinds::Keybinds,
    touch_controls: bool,
    touch_layout: settings::TouchLayout,
    player: &Player,
    camera: &Camera,
    body: &ui::hud::BodyGauges,
    input: &mut input::InputState,
    menu: &mut Menu,
    chat: &mut chat::Chat,
    journal: &mut ui::journal::Journal,
    death: &mut death::DeathScreen,
    chest_screen: &mut chest_screen::ChestScreen,
    station_screen: &mut station_screen::StationScreen,
    inventory_screen: &mut inventory_screen::InventoryScreen,
    pointer: &mut platform::touch::Pointer,
    touch: &mut platform::touch::Touch,
    bar_gestures: &mut hotbar::Gestures,
    give_up_button: &mut ui::downed::GiveUpButton,
    thumb_quick: &mut bool,
    debug_stats: &mut DebugStats,
) -> Option<platform::Event> {
    // **A finger is two different things, and which one
    // depends on what is on screen.**
    //
    // In the world it is a gamepad: the left of the glass
    // is a stick, the right turns the camera, and the
    // buttons dig and place. On a *menu* it is a mouse --
    // and it has to be, because a menu has no stick and no
    // camera. This used to be missing, and the effect was
    // that the whole interface did nothing on a phone:
    // every tap went into the movement stick and no button
    // was ever pressed, because there is no world behind
    // the main menu for a stick to move anybody in.
    //
    // The same condition the rest of the frame uses to
    // decide whether the world has the input.
    let world_has_input = world_owns_the_glass(
        net.is_some(),
        paused,
        inventory_screen.open,
        chest_screen.is_open() || station_screen.is_open(),
        death.is_open(),
        chat.is_typing(),
    ) && !journal.is_open();

    let event = if !world_has_input {
        // **The journal takes the finger whole.** The map is
        // dragged in two directions and `Pointer` hands out
        // vertical scrolls only, so while the journal is open
        // it reads the raw touch itself -- see `ui::journal`.
        if journal.is_open() && net.is_some() && !paused && !death.is_open() {
            let at = widgets::cursor_to_ui(
                (x as f64, y as f64),
                (graphics.size.width, graphics.size.height),
                1.0,
            );
            let mark = player_mark(player.position.as_vec3(), camera.yaw);
            if journal.touch(id, phase, at, graphics.aspect(), mark)
                == ui::journal::Outcome::Closed
            {
                audio.play(audio::Sfx::Click);
                grab_cursor(window, input);
            }
            send_journal_command(journal, net, debug_stats);
            return None;
        }
        // A pointer that is wherever the finger is. Placed
        // first and directly rather than as a queued
        // `CursorMoved`, because a tap is a move *and* a
        // click on the same screen at the same instant, and
        // a click at the last frame's cursor position is a
        // click on whatever the player was pointing at
        // before.
        let size = graphics.size;
        // Undivided, because the interface no longer has one
        // scale: each screen grows by what fits *it*. The
        // division that used to be here happens per screen
        // in `place_cursor`, against the same number that
        // screen's geometry was multiplied by.
        let at = widgets::cursor_to_ui(
            (x as f64, y as f64),
            (size.width, size.height),
            1.0,
        );
        // **The arrangement editor takes the finger whole**, before
        // `Pointer` can turn it into a click -- the same exception the
        // journal's map makes two blocks up, and for the same reason.
        // A control is dragged, and a drag is the one thing `Pointer`
        // is built to swallow: it decides on the lift, it decides
        // nothing when the finger travelled, and it never sends a
        // release at all. See `Menu::arranging_touch` for what each of
        // those three did to this screen.
        //
        // Only while it is carrying something. A finger that grabbed no
        // control goes on to be an ordinary tap, which is what presses
        // RESET and DONE.
        //
        // The same condition `place_cursor` gives the menu the pointer
        // under, and not merely "the editor's screen is the last one it
        // was on": a chest, the death screen and the journal all leave
        // the world without input, and a finger meant for one of them
        // must not reach a screen that is not in front of it.
        if (net.is_none() || paused)
            && menu.is_arranging()
            && menu.arranging_touch(id, phase, at, graphics.ui_scale())
        {
            return None;
        }
        // **What the finger turned out to mean.** A press
        // used to be sent through as a mouse button the
        // instant it landed, which decided the question
        // before the answer existed: a finger that goes on
        // to travel was scrolling a list, and pressing what
        // it started on as well is worse than not scrolling
        // at all. `Pointer` waits.
        let gesture = pointer.handle(
            graphics.size,
            id,
            phase,
            x,
            y,
            Instant::now(),
        );
        // Set from this gesture and from nothing else, so a
        // plain tap that follows a modified one clears it.
        *thumb_quick = matches!(
            gesture,
            platform::touch::Gesture::Tapped(platform::touch::Chord::Quick),
        );
        // The pointer goes where the finger is, and leaves
        // when it does -- so a row does not stay lit under a
        // thumb that is no longer on the glass. A tap counts
        // as being over it: see `Gesture::carries_a_point`,
        // which is a method rather than a list here because
        // getting that list wrong made every tap in the game
        // do nothing.
        place_cursor(
            gesture.carries_a_point().then_some(at),
            net.is_none() || paused,
            widgets::Layout::for_screen(graphics.aspect(), graphics.ui_scale()),
            menu,
            death,
            chest_screen,
            station_screen,
            inventory_screen,
        );
        // **The chat box before anything else, because
        // there is no Enter key on a phone.** A player
        // could open the box with the button on the glass,
        // type into it with the on-screen keyboard, and
        // then neither send nor leave: sending hangs on
        // `Enter` and leaving on `Escape`, and an input
        // method may send neither -- its action key is
        // often "Done" and often produces nothing a game
        // can read. The box carries its own two answers on
        // a touch device; see `chat::Tap`.
        //
        // Taken back through the growth the widget was
        // drawn with, exactly as the hotbar's own hit-test
        // is, because a finger lands in the grown picture
        // and the rectangles are authored in the small one.
        if chat.is_typing() && matches!(gesture, platform::touch::Gesture::Tapped(_)) {
            let grown = widgets::Layout::for_screen(
                graphics.aspect(),
                graphics.ui_scale(),
            )
            .fit_from_corner(chat::EXTENT);
            // The lift comes off first, because it went on
            // last: the widget was grown and then pushed up
            // the glass, so a finger has to come down the
            // glass and then be shrunk. Doing it the other
            // way round misses by the lift times the scale,
            // which on a phone is most of a button.
            let lifted = (
                at.0,
                at.1 - chat::keyboard_lift(touch_controls, true, grown),
            );
            let authored = widgets::unscale_about(
                lifted,
                widgets::anchor::BOTTOM_LEFT(graphics.aspect()),
                grown,
            );
            match chat.tapped(graphics.aspect(), touch_controls, authored) {
                Some(chat::Tap::Send) => {
                    submit_chat(chat, net, debug_stats);
                    close_chat(chat, window, input, paused);
                }
                Some(chat::Tap::Leave) => {
                    close_chat(chat, window, input, paused);
                }
                // ...and a tap on the line itself puts the
                // caret in it, which is the first thing
                // anybody tries on text they can see.
                // ...unless the platform's own editor holds
                // the line, in which case it holds the
                // caret too and ours would be drawn where
                // the next character is *not* going. The
                // same guard the form fields carry -- see
                // `Menu::place_caret_under_cursor`.
                Some(chat::Tap::Caret(at)) if !window.ime_owns_text() => {
                    chat.place_caret(at)
                }
                // ...and nothing at all where it does: the
                // editor holds the caret with the text, and
                // ours would be a bar drawn where the next
                // character is not going.
                Some(chat::Tap::Caret(_)) => {}
                // A tap anywhere else while the box is up is
                // a misclick, which is what it has always
                // been -- see the `MouseButton` arm.
                None => {}
            }
            return None;
        }
        match gesture {
            // A tap, at the point the finger came *up*.
            // Only the press is sent: every screen in this
            // game acts on the press and returns on the
            // release -- see the `MouseButton` arm -- so a
            // release would be a second event that does
            // nothing.
            // A rested press is the right button -- but
            // **only where a right button means anything**.
            // Everywhere else in this game a click is a
            // click, and a player who pressed PLAY slowly
            // would otherwise have pressed nothing at all:
            // the menus act on the left button and drop the
            // right one in silence. The screens that do use
            // it are the two that hold stacks of things,
            // which is where the gesture was invented for.
            platform::touch::Gesture::Tapped(chord) => {
                let containers_are_open =
                    inventory_screen.open || chest_screen.is_open() || station_screen.is_open();
                let secondary = matches!(
                    chord,
                    platform::touch::Chord::Secondary,
                ) && containers_are_open;
                platform::Event::MouseButton {
                    button: if secondary {
                        MouseButton::Right
                    } else {
                        MouseButton::Left
                    },
                    pressed: true,
                }
            }
            // ...and a drag, in the units the wheel already
            // speaks, so every list that could be scrolled
            // with a wheel can now be scrolled with a thumb
            // and none of them had to learn anything.
            platform::touch::Gesture::Scrolled(lines) => {
                platform::Event::MouseWheel { lines }
            }
            _ => return None,
        }
    } else {
        // **Giving up, on glass**, before the bar and the thumb
        // controls: the button is drawn over the look area, and
        // what is drawn on top is what is pressed. Taken back
        // through the same growth the overlay's words were
        // drawn with -- see `ui::downed::give_up_rect`.
        if touch_controls {
            let point = widgets::cursor_to_ui(
                (x as f64, y as f64),
                (graphics.size.width, graphics.size.height),
                1.0,
            );
            let authored = widgets::unscale_about(
                point,
                widgets::anchor::CENTRE(graphics.aspect()),
                graphics.ui_scale(),
            );
            match give_up_button.handle(
                id,
                phase,
                authored,
                body.downed.is_some(),
                graphics.ui_scale(),
            ) {
                ui::downed::GiveUpTap::Ignored => {}
                ui::downed::GiveUpTap::Held => return None,
                // The same message the respawn key sends while
                // down, which the server takes as giving up.
                ui::downed::GiveUpTap::GiveUp => {
                    if let Some(net) = net {
                        audio.play(audio::Sfx::Click);
                        net.send(ClientMessage::Respawn);
                    }
                    return None;
                }
            }
        }
        let mut return_event = None;
        // **The bar first, and only where it is drawn.**
        //
        // A phone has no number row and no wheel, so the
        // hotbar was the one part of the HUD a thumb could
        // see and not use: the only way to change what you
        // were holding was to open the pack and drag. It is
        // checked before the thumb controls because it sits
        // over them in the drawing -- and what is drawn on
        // top has to be what is pressed, or the interface
        // is lying about which thing the finger is on.
        //
        // **Four gestures, not a tap.** The bar answers for
        // the number row, the wheel, `E` and `Q` -- seven
        // controls a phone has nowhere to put, all of which
        // point at the ten squares already on screen. What
        // tells them apart is what the finger does; see
        // `hotbar::Gestures`.
        {
            let point = widgets::cursor_to_ui(
                (x as f64, y as f64),
                (graphics.size.width, graphics.size.height),
                1.0,
            );
            // Back through the growth the HUD was drawn
            // with. See `widgets::unscale_about`: the bar
            // is authored at one size and grown about the
            // bottom of the screen, so a finger lands in
            // the grown picture and has to be asked about
            // in the authored one.
            let authored = widgets::unscale_about(
                point,
                widgets::anchor::BOTTOM(graphics.aspect()),
                graphics.ui_scale(),
            );
            // The same count the bar is drawn with, from
            // the same constant, so the hit-test cannot be
            // asking about a bar of a different width.
            let slot = hotbar::slot_at(
                authored.0,
                authored.1,
                crate::logic::inventory::HOTBAR_SLOTS,
            );
            // Whether this finger is the bar's, asked
            // *before* the gesture is fed in: a lift is the
            // event that ends the bar's ownership, and
            // asking afterwards would let the lift fall
            // through to the controls underneath.
            let claimed = slot.is_some() || bar_gestures.owns(id);
            let touched = bar_gestures.handle(
                phase,
                id,
                slot,
                authored,
                Instant::now(),
            );
            if let Some(event) =
                hotbar_gesture_event(touched, input, keybinds)
            {
                return_event = Some(event);
            } else if claimed {
                // The bar has the finger and made nothing of
                // it yet; nothing else may also have it.
                return None;
            }
        }
        if let Some(event) = return_event {
            event
        } else {

        touch.resize(graphics.size, touch_layout, graphics.ui_scale());
        // **A button on the glass is a key.** What it sends
        // is carried in the arrangement, not decided here,
        // and both edges are sent -- down when the thumb
        // lands, up when it lifts -- so what reaches the
        // game is indistinguishable from a keyboard. That
        // is the whole point: every binding the game has
        // works on a phone without being taught to, and so
        // does every one it grows later.
        //
        // This used to be a `match` on four named controls
        // turning each into the action it "meant", which is
        // why a touch player could not eat, drop, or reach
        // anything nobody had thought to name.
        let hit = touch.handle(id, phase, x, y, Instant::now());
        // A short tap in the look area is the right mouse
        // button, and it goes down the *same* path a real
        // one does -- placing a block, opening a chest,
        // striking a fire are one piece of code with one
        // set of rules, and a second copy for phones is a
        // second copy to keep in step. See
        // `touch::Touch::is_mining` for why the hands
        // moved off the glass.
        if touch.take_place() {
            platform::Event::MouseButton {
                button: MouseButton::Right,
                pressed: true,
            }
        } else {
        let (slot, pressed) = match hit {
            platform::touch::Hit::Pressed(slot) => (slot, true),
            platform::touch::Hit::Released(slot) => (slot, false),
            platform::touch::Hit::Nothing => return None,
        };
        match touch_layout.buttons[slot].emits {
            // The wheel is about the glass rather than about
            // the game: it has already opened or closed
            // itself inside `touch`, and there is nothing
            // downstream to tell. See `settings::Emits::More`.
            settings::Emits::More => return None,
            settings::Emits::Mine => platform::Event::MouseButton {
                button: MouseButton::Left,
                pressed,
            },
            settings::Emits::Place => platform::Event::MouseButton {
                button: MouseButton::Right,
                pressed,
            },
            settings::Emits::Key(key) => platform::Event::Keyboard {
                key: Some(key),
                // No text: a button on the glass is a key
                // being *pressed*, not a character being
                // typed. Sending text as well would put a
                // letter in whatever field had focus every
                // time the player jumped.
                text: platform::Text::default(),
                pressed,
                repeat: false,
            },
        }
        }
        }
    };
    Some(event)
}

/// **A key, and what it is allowed to mean.**
///
/// The order the tests are asked in is the whole of this function, and
/// it is not arbitrary: a rebind swallows the next key whatever it is,
/// a character typed into a field must not also be read as a shortcut,
/// the chat box owns the keyboard while it is open, then the journal,
/// then the screens, and only what is left reaches the world. Each of
/// those is written at its own site with the failure it prevents.
///
/// Returns the menu action the key asked for, if any. It cannot carry
/// the action out itself: leaving a world stops a server and quitting
/// ends the event loop, and both of those are `run`'s to do.
#[allow(clippy::too_many_arguments)]
pub fn on_key(
    key: Option<KeyCode>,
    text: platform::Text,
    pressed: bool,
    repeat: bool,
    net: Option<&network::NetworkHandle>,
    window: &dyn platform::Window,
    audio: &Audio,
    inventory: &Inventory,
    body: &ui::hud::BodyGauges,
    forced_look: Option<&crate::engine::capture::Look>,
    settings: &mut ClientSettings,
    settings_dirty: &mut bool,
    paused: &mut bool,
    held_shift: &mut bool,
    held_ctrl: &mut bool,
    hud_hidden: &mut bool,
    fog_enabled: &mut bool,
    input: &mut input::InputState,
    menu: &mut Menu,
    chat: &mut chat::Chat,
    journal: &mut ui::journal::Journal,
    death: &mut death::DeathScreen,
    chest_screen: &mut chest_screen::ChestScreen,
    station_screen: &mut station_screen::StationScreen,
    inventory_screen: &mut inventory_screen::InventoryScreen,
    meal: &mut Option<crate::Meal>,
    hand: &mut crate::logic::hand::Hand,
    debug_stats: &mut DebugStats,
) -> Option<Action> {
    // **Shift and control are tracked here, not read off
    // `input`.** `InputState` is deliberately emptied
    // whenever a screen takes the keyboard
    // (`release_all`), so in the one place the two
    // modifiers mean something -- editing text in a
    // form -- it knows nothing about them. Two bools in
    // the frame loop, updated before anything branches
    // on the screen, so a key pressed in a menu and
    // released in the world cannot leave one stuck.
    if let Some(code) = key {
        match code {
            KeyCode::ShiftLeft | KeyCode::ShiftRight => *held_shift = pressed,
            KeyCode::ControlLeft | KeyCode::ControlRight => *held_ctrl = pressed,
            _ => {}
        }
    }
    let is_pressed = pressed;
    let in_menu = net.is_none() || *paused;

    if in_menu {
        if !is_pressed {
            return None;
        }
        // Rebinding swallows the next key whatever it
        // is, so it has to come before every other
        // reading of the keyboard -- otherwise binding
        // an action to Escape or to a menu shortcut
        // would navigate instead of binding.
        if let Some(action) = menu.awaiting_key() {
            if let Some(code) = key {
                if code == KeyCode::Escape {
                    // Escape cancels rather than binds:
                    // it is the way out of every screen,
                    // and an action bound to it would
                    // have no way back.
                    menu.finish_rebind(false);
                } else if keybinds::is_bindable(code) {
                    settings.keybinds.bind(action, code);
                    *settings_dirty = true;
                    menu.finish_rebind(true);
                } else {
                    menu.finish_rebind(false);
                }
            }
            return None;
        }
        // **Whether keys may edit this field at all.**
        //
        // On a phone they may not, and the reason is
        // double entry rather than tidiness. While the
        // input method owns the field (see
        // `platform::Window::ime_owns_text`) every edit
        // has already been made in *its* copy and
        // arrives through the mirror in `AboutToWait`.
        // A soft keyboard that also sends a Backspace
        // as a key event -- and several do -- would
        // then delete one character here and one there,
        // and the player would watch two letters
        // vanish for one tap.
        //
        // Only the editing keys are held back. Escape,
        // Enter and Tab are navigation and still get
        // through: they are how a form is left,
        // submitted and moved around, and the input
        // method has no opinion about any of that.
        let ime_holds_the_field =
            menu.accepts_text() && window.ime_owns_text();
        // Text first: a character typed into a field must
        // not also be read as a shortcut.
        if keys_may_type_into_the_field(
            menu.accepts_text(),
            window.ime_owns_text(),
        ) {
            let mut typed = false;
            for c in text.chars() {
                if crate::engine::texture::has_glyph(c) {
                    menu.type_char(c);
                    typed = true;
                }
            }
            if typed {
                return None;
            }
        }
        if let Some(code) = key {
            // Every key that edits or moves within the
            // text, not just the two that delete: while
            // an input method owns the field it owns
            // the caret too, and a Home sent through
            // here would move the game's copy out from
            // under the one the player can see in their
            // keyboard's own strip.
            if ime_holds_the_field
                && matches!(
                    code,
                    KeyCode::Backspace
                        | KeyCode::Delete
                        | KeyCode::Home
                        | KeyCode::End
                        | KeyCode::ArrowLeft
                        | KeyCode::ArrowRight
                )
            {
                return None;
            }
            if let Some(key) =
                menu_key(code, text.first(), ime_holds_the_field, *held_shift, *held_ctrl)
            {
                if let Some(action) = menu.key(key) {
                    return Some(action);
                }
            }
        }
        return None;
    }

    // The chat box owns the keyboard while it is open:
    // every letter is text, not a shortcut, or walking
    // keys would move the player as they type.
    if chat.is_typing() {
        if !is_pressed {
            return None;
        }
        // The same division as the menu forms above:
        // while the input method owns the line, the
        // letters and the deletions arrive through the
        // mirror and a key that also made them would
        // make them twice.
        let ime_holds_the_line = window.ime_owns_text();
        if !ime_holds_the_line {
            for c in text.chars() {
                chat.type_char(c);
            }
        }
        if let Some(code) = key {
            // The whole editing set, not just
            // Backspace: while an input method owns the
            // line it owns the caret in it too, and a
            // Home sent through here would move the
            // game's copy out from under the one the
            // player can see in their keyboard's strip.
            // The same guard the menu forms carry --
            // see `menu_key`.
            if ime_holds_the_line
                && matches!(
                    code,
                    KeyCode::Backspace
                        | KeyCode::Delete
                        | KeyCode::Home
                        | KeyCode::End
                        | KeyCode::ArrowLeft
                        | KeyCode::ArrowRight
                )
            {
                return None;
            }
            match code {
                KeyCode::Enter | KeyCode::NumpadEnter if !repeat => {
                    submit_chat(chat, net, debug_stats);
                    close_chat(chat, window, input, *paused);
                }
                KeyCode::Escape => {
                    close_chat(chat, window, input, *paused);
                }
                // The caret keys, on the same terms the
                // forms have them -- see `ui::field`.
                KeyCode::Backspace => {
                    chat.edit(chat::Edit::Backspace { word: *held_ctrl })
                }
                KeyCode::Delete => {
                    chat.edit(chat::Edit::Delete { word: *held_ctrl })
                }
                KeyCode::ArrowLeft => chat.edit(chat::Edit::Move {
                    motion: if *held_ctrl {
                        ui::field::Motion::WordLeft
                    } else {
                        ui::field::Motion::Left
                    },
                    extend: *held_shift,
                }),
                KeyCode::ArrowRight => chat.edit(chat::Edit::Move {
                    motion: if *held_ctrl {
                        ui::field::Motion::WordRight
                    } else {
                        ui::field::Motion::Right
                    },
                    extend: *held_shift,
                }),
                KeyCode::Home => chat.edit(chat::Edit::Move {
                    motion: ui::field::Motion::Home,
                    extend: *held_shift,
                }),
                KeyCode::End => chat.edit(chat::Edit::Move {
                    motion: ui::field::Motion::End,
                    extend: *held_shift,
                }),
                KeyCode::KeyA if *held_ctrl => chat.edit(chat::Edit::SelectAll),
                // Up and down are the sent lines, not
                // the log: the log is scrolled with the
                // wheel, and a player holding a
                // keyboard is reaching for the arrows
                // to get a command back. See
                // `Chat::recall`.
                KeyCode::ArrowUp => chat.recall(-1),
                KeyCode::ArrowDown => chat.recall(1),
                _ => {}
            }
        }
        return None;
    }

    // The journal, while it is open, has the keyboard the
    // way the chat box does: Escape shuts it, Tab turns the
    // page, and on the recipe page letters are a search
    // rather than shortcuts -- or typing "bronze" would
    // throw away whatever is in the hand.
    if journal.is_open() && net.is_some() && !*paused && !death.is_open() {
        if !is_pressed {
            return None;
        }
        let binds = &settings.keybinds;
        let action = key.and_then(|code| {
            keybinds::Action::ALL.into_iter().find(|a| binds.key(*a) == Some(code))
        });
        match key {
            Some(KeyCode::Escape) => {
                journal.close();
                grab_cursor(window, input);
            }
            Some(KeyCode::Tab) => journal.switch_tab(),
            Some(KeyCode::Backspace) if journal.takes_text() => journal.backspace(),
            _ => {
                let mut typed = false;
                if journal.takes_text() {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        journal.type_char(c);
                        typed = true;
                    }
                }
                let tab = match action {
                    Some(keybinds::Action::Map) => Some(ui::journal::Tab::Map),
                    Some(keybinds::Action::Recipes) => Some(ui::journal::Tab::Recipes),
                    Some(keybinds::Action::Give) => Some(ui::journal::Tab::Give),
                    _ => None,
                };
                if let (false, Some(tab)) = (typed, tab) {
                    ask_if_operator(net);
                    if !journal.toggle(tab) {
                        grab_cursor(window, input);
                    }
                }
            }
        }
        return None;
    }

    if let Some(code) = key {
        if is_pressed {
            // Escape is deliberately not rebindable: it
            // is the way out of every screen, including
            // the one where keys are rebound, and a
            // player who bound it away would have no way
            // back.
            let binds = &settings.keybinds;
            let action = keybinds::Action::ALL
                .into_iter()
                .find(|a| binds.key(*a) == Some(code));
            match (code, action) {
                // Enter opens the chat box. Not
                // rebindable, for the same reason
                // Escape is not: it is the way out of
                // what it opens.
                //
                // Not while the inventory has the
                // screen: two things claiming the cursor
                // and the keyboard at once ends with the
                // inventory unusable behind a grabbed
                // pointer. And not on a key repeat --
                // holding Enter would open and close the
                // box tens of times a second.
                (KeyCode::Enter | KeyCode::NumpadEnter, _)
                    if net.is_some()
                        && !*paused
                        && !inventory_screen.open
                        && !chest_screen.is_open()
            && !station_screen.is_open()
                        && !death.is_open()
                        && !repeat =>
                {
                    chat.open(Instant::now());
                    // A phone has no keyboard until it
                    // is asked for one. Nothing on a
                    // desktop, where it is already
                    // there.
                    window.set_ime_visible(true);
                    release_cursor(window, input);
                    input.release_all();
                }
                // The chest closes on the same two
                // keys the inventory does, and tells the
                // server -- which stops sending updates
                // for it and stops accepting gestures
                // against it.
                (KeyCode::Escape, _) | (_, Some(keybinds::Action::Inventory))
                    if chest_screen.is_open() =>
                {
                    close_chest(
                        chest_screen,
                        net,
                        debug_stats,
                    );
                }
                // The station screen, on the same two keys and
                // for the same reason: the server is holding a
                // seat open for this player and has to be told
                // the player has left it.
                (KeyCode::Escape, _) | (_, Some(keybinds::Action::Inventory))
                    if station_screen.is_open() =>
                {
                    close_station(
                        station_screen,
                        net,
                        debug_stats,
                    );
                }
                // **The blow, on the space bar.** No keybind of
                // its own: a run is four presses over five
                // seconds on a screen that says what to press,
                // and a binding nobody would ever change is a
                // row in the keybinds screen that only makes it
                // longer. Jump is what space does in the world,
                // and the world does not have the keyboard while
                // this screen is up. Key repeat is refused --
                // a held space is one blow, not forty.
                (KeyCode::Space, _) if station_screen.is_open() && !repeat => {
                    // Asked before the press, because the
                    // press is what ends the run -- see
                    // `StationScreen::striking`.
                    let blow = station_screen.striking();
                    let intent = station_screen.press();
                    if let Some(game) = blow {
                        audio.play_flat(audio::bank::station_blow(game), 0.7, 1.0);
                    }
                    if let (Some(intent), Some(net)) = (intent, net) {
                        send_station_intent(
                            intent,
                            station_screen,
                            net,
                            debug_stats,
                            audio,
                        );
                    }
                }
                (KeyCode::Escape, _) if inventory_screen.open => {
                    // Esc backs out of the inventory
                    // before it reaches for the pause
                    // menu: one screen at a time.
                    inventory_screen.close();
                    grab_cursor(window, input);
                }
                // **A forced look is a photograph, and
                // a photograph cannot be paused.**
                //
                // A capture run is unattended and the
                // window it opens is not the one the
                // desktop has focus on; a single stray
                // Escape -- from a focus change, from
                // the terminal it was launched out of --
                // put the pause menu over every frame of
                // a seventy-frame sweep and the run
                // produced seventy photographs of a
                // menu. The failure is silent: the files
                // are written, the count is right, and
                // the picture is of the wrong thing.
                (KeyCode::Escape, _) if forced_look.is_some() => {}
                (KeyCode::Escape, _) => {
                    *paused = true;
                    menu.open(Screen::Paused);
                    release_cursor(window, input);
                    input.release_all();
                }
                (_, Some(keybinds::Action::ToggleStats)) => {
                    debug_stats.toggle_console()
                }
                (_, Some(keybinds::Action::ToggleHud)) => *hud_hidden = !*hud_hidden,
                (_, Some(keybinds::Action::Respawn))
                    if death.is_open() =>
                {
                    if let Some(net) = net {
                        net.send(ClientMessage::Respawn);
                    }
                }
                // ...and on the ground the same key lets go:
                // the server takes it as giving up (see its
                // `Respawn` arm), and the death screen that
                // follows has its own respawn on it.
                (_, Some(keybinds::Action::Respawn))
                    if body.downed.is_some() =>
                {
                    if let Some(net) = net {
                        net.send(ClientMessage::Respawn);
                    }
                }
                // The death screen's buttons, from the
                // keyboard. Every other screen in the
                // game can be driven without the mouse,
                // and the one that arrives uninvited is
                // the worst one to make an exception of.
                (KeyCode::ArrowUp, _) if death.is_open() => {
                    death.move_focus(-1)
                }
                (KeyCode::ArrowDown, _) if death.is_open() => {
                    death.move_focus(1)
                }
                (KeyCode::Enter | KeyCode::NumpadEnter, _)
                    if death.is_open() =>
                {
                    match death.focused() {
                        Some(death::Choice::Respawn) => {
                            if let Some(net) = net {
                                net.send(ClientMessage::Respawn);
                            }
                        }
                        Some(death::Choice::LeaveWorld) => {
                            return Some(Action::LeaveWorld);
                        }
                        None => {}
                    }
                }
                // **Dead hands hold nothing.** See
                // `works_while_dead` for what fell through
                // the death screen before this arm.
                (_, Some(bound))
                    if death.is_open() && !works_while_dead(bound) => {}
                (
                    _,
                    Some(
                        keybinds::Action::Map
                        | keybinds::Action::Recipes
                        | keybinds::Action::Give,
                    ),
                ) if !chest_screen.is_open() && !station_screen.is_open() => {
                    let tab = match action {
                        Some(keybinds::Action::Map) => ui::journal::Tab::Map,
                        Some(keybinds::Action::Give) => ui::journal::Tab::Give,
                        _ => ui::journal::Tab::Recipes,
                    };
                    // One screen at a time, for the reason
                    // the pack gives below.
                    inventory_screen.close();
                    chat.close();
                    ask_if_operator(net);
                    // Only a journal that actually opened
                    // takes the mouse. The give key of a
                    // player who is not an operator is
                    // refused by `toggle`, and releasing
                    // the cursor anyway left the camera
                    // dead with no screen to show for it
                    // -- the player saw the mouse come
                    // loose "for a window" that never came.
                    if journal.toggle(tab) {
                        release_cursor(window, input);
                        input.release_all();
                    }
                }
                (_, Some(keybinds::Action::Inventory)) => {
                    if inventory_screen.open {
                        inventory_screen.close();
                        grab_cursor(window, input);
                    } else {
                        // One screen at a time: the chat
                        // box and the inventory both
                        // want the cursor and the keys.
                        chat.close();
                        release_cursor(window, input);
                        input.release_all();
                        // Seeded with the middle of the
                        // screen: with no starting
                        // position the first click does
                        // nothing, which reads as the
                        // inventory ignoring the mouse.
                        inventory_screen.open_at(Some((0.0, 0.0)));
                    }
                }
                (_, Some(keybinds::Action::Eat)) => {
                    // The hovered slot while a screen is
                    // open, the selected one otherwise
                    // -- exactly the rule the drop key
                    // follows, because it is the same
                    // question about the same slot.
                    let slot = if inventory_screen.open {
                        inventory_screen.hovered_slot()
                    } else {
                        Some(input.hotbar_slot)
                    };
                    eat_from(
                        slot,
                        inventory,
                        meal,
                        hand,
                    );
                }
                (_, Some(keybinds::Action::Drop)) => {
                    // The hovered slot while a screen
                    // is open, the selected one
                    // otherwise. Sprint modifier for the
                    // whole stack.
                    let slot = if inventory_screen.open {
                        inventory_screen.hovered_slot()
                    } else if chest_screen.is_open() {
                        // Only out of the pack: there is
                        // no message for throwing
                        // something out of a chest, and
                        // a chest is somewhere you put
                        // things rather than a bin.
                        chest_screen.hovered().and_then(|(side, slot)| {
                            (side == primitive_shared::protocol::Side::Pack)
                                .then_some(slot)
                        })
                    } else {
                        Some(input.hotbar_slot)
                    };
                    let slot = something_to_throw(slot, inventory);
                    if let (Some(slot), Some(net)) = (slot, net) {
                        net.send(ClientMessage::DropSlot {
                            slot: slot as u8,
                            whole_stack: input.action_down(
                                binds,
                                keybinds::Action::Sprint,
                            ),
                        });
                        audio.play(audio::Sfx::Drop);
                        // ...and what it was. `Sfx::Drop`
                        // is the throw -- a hand opening,
                        // the same every time -- and
                        // what the player is actually
                        // listening for is the thing
                        // hitting the ground. An ingot
                        // and a handful of berries left
                        // the hand identically before
                        // this, which made throwing
                        // something away feel like
                        // pressing a key rather than
                        // like putting it down.
                        if let Some(thrown) = inventory.block_in(slot) {
                            audio.play_flat(
                                audio::Sfx::Material(
                                    audio::bank::Impact::Place,
                                    audio::bank::Material::of(thrown),
                                ),
                                0.5,
                                1.0,
                            );
                        }
                        debug_stats.network_messages_out_this_second += 1;
                    }
                }
                // A number key over a slot sends what is
                // in it to that place on the bar. The
                // gesture everyone brings with them from
                // other games, and the fastest way to
                // lay a bar out: point, press, done.
                (key, _)
                    if inventory_screen.open
                        && input::hotbar_slot_for(key).is_some() =>
                {
                    let from = inventory_screen.hovered_slot();
                    let to = input::hotbar_slot_for(key);
                    if let (Some(from), Some(to), Some(net)) =
                        (from, to, net)
                    {
                        if from != to {
                            net.send(ClientMessage::MoveSlots {
                                from: from as u8,
                                to: to as u8,
                            });
                            debug_stats.network_messages_out_this_second += 1;
                        }
                    }
                }
                (_, Some(keybinds::Action::ToggleFullscreen)) => {
                    // Borderless: a window the size of
                    // the screen with no frame. Toggled
                    // here and *remembered*, because a
                    // player who plays fullscreen plays
                    // fullscreen tomorrow as well.
                    settings.fullscreen = !settings.fullscreen;
                    window.set_fullscreen(settings.fullscreen);
                    *settings_dirty = true;
                }
                (_, Some(keybinds::Action::ToggleFog)) => {
                    // Change the setting, not a separate
                    // flag. They used to be two truths:
                    // pressing F turned fog off, and the
                    // next tweak of any setting at all
                    // silently turned it back on.
                    settings.fog_enabled = !settings.fog_enabled;
                    *fog_enabled = settings.fog_enabled;
                    *settings_dirty = true;
                }
                // **A number over a slot swaps it with that
                // square of the bar**, in the pack and at
                // an open container alike: "сделай
                // сочетания клавиш для работы в
                // хранилищах". Picking a stack up and
                // carrying it to the bar was two clicks and
                // an aim for the thing a player does most.
                // The bar's selection is left alone while a
                // screen is up, as the wheel's is.
                (_, _) if hover_swap(code, inventory_screen, chest_screen).is_some() => {
                    if let (Some(message), Some(net)) =
                        (hover_swap(code, inventory_screen, chest_screen), net)
                    {
                        audio.play(audio::Sfx::Click);
                        net.send(message);
                        debug_stats.network_messages_out_this_second += 1;
                    }
                }
                _ => input.set_key(code, true),
            }
        } else {
            input.set_key(code, false);
        }
    }

    None
}

/// **Where a wheel notch goes, which depends on what is on screen.**
///
/// A list if there is one, the chat log if the box is up, the map or the
/// recipe book in the journal, the recipe column in the pack -- and only
/// what is left reaches the hotbar. The bar is last on purpose: with a
/// screen open its selection is not on screen, so scrolling moved it
/// invisibly and the player found out later, having placed the wrong
/// block.
///
/// A thumb drag arrives here too, in lines, so every list that could be
/// scrolled with a wheel can be scrolled with a thumb and none of them
/// had to learn anything. See `touch_to_event`.
#[allow(clippy::too_many_arguments)]
pub fn on_wheel(
    lines: f32,
    net: Option<&network::NetworkHandle>,
    paused: bool,
    graphics: &GraphicsState,
    player: &Player,
    camera: &Camera,
    input: &mut input::InputState,
    menu: &mut Menu,
    chat: &mut chat::Chat,
    journal: &mut ui::journal::Journal,
    inventory_screen: &mut inventory_screen::InventoryScreen,
) {
    // Wheel away from the player moves *up* the bar, so
    // "forward" through the slots is the wheel coming
    // back. Normalised to lines by the backend; the
    // hotbar only ever wants the sign.
    let forward = lines < 0.0;
    // On a menu the wheel belongs to whatever list is on
    // screen. It used to belong to nothing at all: the
    // handler returned before looking, so the only way
    // down a list longer than its panel was the arrow
    // keys, and the world list gave no sign there was
    // anything below the last row it had drawn.
    if net.is_none() || paused {
        menu.scroll(if forward { 1 } else { -1 });
        return;
    }
    // With the box open the wheel belongs to the log,
    // which is longer than the twelve rows on screen.
    // A thumb drag arrives here as well (see the
    // `Gesture::Scrolled` arm above), so the phone got
    // a scrollable chat without a control of its own.
    if chat.is_typing() {
        chat.scroll_by(if forward { -1 } else { 1 });
        return;
    }
    // The journal: zoom on the map, rows in the book.
    if journal.is_open() {
        journal.wheel(lines, graphics.aspect(), player_mark(player.position.as_vec3(), camera.yaw));
        return;
    }
    // With the inventory open the wheel belongs to the
    // recipe list, which is longer than the window on
    // it. It must *not* reach the hotbar there: the
    // bar's selection is not on screen, so scrolling
    // moved it invisibly and the player found out later,
    // having placed the wrong block.
    if inventory_screen.open {
        inventory_screen.scroll_recipes(if forward { 1 } else { -1 });
        return;
    }
    input.cycle_hotbar(forward);
}

/// **A mouse button, and who has the screen.**
///
/// The order the screens are offered the click in is the order they sit
/// in: the death screen over the journal over the station over the chest
/// over the pack, and only what none of them wanted reaches the world.
/// Each test is written at its own site with the thing it prevents --
/// a click that took the mouse for the camera is what left the map
/// undraggable, and a chest that could not be shut when the connection
/// dropped trapped the player at the worst possible moment.
///
/// Returns the menu action the click asked for, if any; see `on_key`
/// for why it cannot carry one out itself.
#[allow(clippy::too_many_arguments)]
pub fn on_mouse_button(
    button: MouseButton,
    pressed: bool,
    net: Option<&network::NetworkHandle>,
    paused: bool,
    traced: bool,
    thumb_quick: bool,
    held_ctrl: bool,
    touch_controls: bool,
    last_cursor: Option<(f32, f32)>,
    window: &dyn platform::Window,
    audio: &Audio,
    graphics: &GraphicsState,
    settings: &ClientSettings,
    player: &Player,
    camera: &Camera,
    body: &ui::hud::BodyGauges,
    inventory: &Inventory,
    remote_players: &RemotePlayers,
    entities: &crate::logic::entities::Entities,
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    arrivals: &mut crate::Arrivals,
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut crate::MeshQueueSet,
    chunk_versions: &mut HashMap<ChunkPos, u64>,
    input: &mut input::InputState,
    menu: &mut Menu,
    ime: &mut crate::ui::ime::Mirror,
    chat: &mut chat::Chat,
    journal: &mut ui::journal::Journal,
    death: &mut death::DeathScreen,
    chest_screen: &mut chest_screen::ChestScreen,
    station_screen: &mut station_screen::StationScreen,
    inventory_screen: &mut inventory_screen::InventoryScreen,
    mining: &mut crate::logic::mining::Mining,
    hand: &mut crate::logic::hand::Hand,
    cut: &mut Option<crate::Cut>,
    meal: &mut Option<crate::Meal>,
    notice: &mut Option<(String, Instant)>,
    debug_stats: &mut DebugStats,
) -> Option<Action> {
    // Tracked whatever the game is doing, so releasing
    // the button over a menu doesn't leave the world
    // thinking it is still held.
    if button == MouseButton::Left {
        input.breaking = pressed
            && input.mouse_grabbed
            && !paused
            && !inventory_screen.open
            && !chest_screen.is_open()
            && !station_screen.is_open()
            && !journal.is_open()
            && !chat.is_typing();
    }
    // The same, for the button the rod is wound back with
    // (`logic::fishing::Hold`). Tracked wherever the game
    // is, for `breaking`'s reason: a button released over a
    // menu must not leave the rod winding for ever.
    if button == MouseButton::Right {
        input.using = pressed
            && input.mouse_grabbed
            && !paused
            && !inventory_screen.open
            && !chest_screen.is_open()
            && !station_screen.is_open()
            && !journal.is_open()
            && !chat.is_typing();
    }
    // Clicking while typing is a misclick, not a swing:
    // the cursor is loose because the chat box has it.
    // **Except in the box itself**, where it is a player
    // pointing at the letter they want to fix -- the
    // same gesture the form fields answer. Taken back
    // through the growth the widget was drawn with, on
    // exactly the terms the finger is: see the touch
    // path above, which is where that arithmetic is
    // explained.
    if chat.is_typing() {
        if button == MouseButton::Left && pressed {
            if let Some(at) = last_cursor {
                let grown = widgets::Layout::for_screen(
                    graphics.aspect(),
                    graphics.ui_scale(),
                )
                .fit_from_corner(chat::EXTENT);
                let lifted = (
                    at.0,
                    at.1 - chat::keyboard_lift(
                        touch_controls,
                        true,
                        grown,
                    ),
                );
                let authored = widgets::unscale_about(
                    lifted,
                    widgets::anchor::BOTTOM_LEFT(graphics.aspect()),
                    grown,
                );
                if let Some(chat::Tap::Caret(caret)) =
                    chat.tapped(graphics.aspect(), touch_controls, authored)
                {
                    if !window.ime_owns_text() {
                        chat.place_caret(caret);
                    }
                }
            }
        }
        return None;
    }
    // **A release matters on exactly one screen.** The
    // arrangement editor is the only place in the menus
    // where a press and a release are different events:
    // everywhere else a click is decided when the button
    // goes down. Letting go is how a control is put
    // down, so it cannot be dropped here with the rest.
    if !pressed {
        if button == MouseButton::Left {
            menu.release_control();
        }
        return None;
    }
    // The death screen takes the click before anything
    // else does. A dead player has nothing else to
    // click on, and the world behind is not theirs to
    // touch until they are back in it.
    if death.is_open() && net.is_some() && !paused {
        if button == MouseButton::Left {
            match death.click() {
                Some(death::Choice::Respawn) => {
                    if let Some(net) = net {
                        net.send(ClientMessage::Respawn);
                    }
                }
                // Straight through the pause menu's own
                // path, so leaving from here saves and
                // tears down exactly as leaving from
                // there does.
                Some(death::Choice::LeaveWorld) => {
                    return Some(Action::LeaveWorld);
                }
                None => {}
            }
        }
        return None;
    }
    // The journal has the cursor while it is open, so it
    // has the click: a button, a row, or the start of a
    // drag across the map.
    if journal.is_open() && net.is_some() && !paused {
        if button == MouseButton::Left {
            if pressed {
                let mark = player_mark(player.position.as_vec3(), camera.yaw);
                if journal.press(graphics.aspect(), mark) == ui::journal::Outcome::Closed {
                    audio.play(audio::Sfx::Click);
                    grab_cursor(window, input);
                }
                send_journal_command(journal, net, debug_stats);
            } else {
                journal.release();
            }
        }
        return None;
    }
    // The station screen, before the chest's: the two are
    // never open at once, and this one wants the click
    // wherever it lands (a blow is not aimed).
    if station_screen.is_open() && !paused && button == MouseButton::Left {
        // The same blow the strike key makes, and the same
        // reason it is asked first: while a run is up, the
        // whole panel is the hammer.
        let blow = station_screen.striking();
        let clicked = station_screen.click();
        if let Some(game) = blow {
            audio.play_flat(audio::bank::station_blow(game), 0.7, 1.0);
        }
        if let Some(intent) = clicked {
            if intent == station_screen::Intent::Close {
                audio.play(audio::Sfx::Back);
                close_station(station_screen, net, debug_stats);
                return None;
            }
            if let Some(net) = net {
                send_station_intent(
                    intent,
                    station_screen,
                    net,
                    debug_stats,
                    audio,
                );
            }
        }
        return None;
    }
    // The chest screen, on the same footing as the
    // inventory: it has the cursor, so it has the click.
    if chest_screen.is_open() && net.is_some() && !paused {
        let click = match button {
            MouseButton::Left => Some(inventory_screen::Button::Left),
            MouseButton::Right => Some(inventory_screen::Button::Right),
            _ => None,
        };
        if let Some(click) = click {
            let quick = thumb_quick
                || input.action_down(
                    &settings.keybinds,
                    keybinds::Action::Sprint,
                );
            let intent = chest_screen.click(inventory, click, quick, held_ctrl);
            // Closing goes through `close_chest`, which
            // also tells the server to stop sending
            // updates for the container. Handled before
            // the arm below so that it still works when
            // the connection has gone: a screen that
            // traps the player when the server drops is
            // the worst time to trap them.
            if matches!(intent, Some(chest_screen::Intent::Close)) {
                audio.play(audio::Sfx::Back);
                close_chest(chest_screen, net, debug_stats);
                return None;
            }
            if let (Some(intent), Some(net)) = (intent, net) {
                // **A jug in the hand is not a container
                // the server has open**, so none of the
                // container messages below mean anything
                // for it: every gesture on it becomes one
                // of the pack's own jug messages, against
                // the jug's slot. See
                // `chest_screen::held_vessel_message`.
                if let Some(jug) = chest_screen.held_vessel() {
                    if let Some(message) =
                        chest_screen::held_vessel_message(intent, jug)
                    {
                        audio.play(audio::Sfx::Click);
                        net.send(message);
                        debug_stats.network_messages_out_this_second += 1;
                    }
                    return None;
                }
                audio.play(audio::Sfx::Click);
                // `Close` was dealt with above, before the
                // connection was unwrapped, and is the one
                // intent with no message.
                if let Some(message) = chest_intent_message(intent) {
                    net.send(message);
                }
                debug_stats.network_messages_out_this_second += 1;
            }
        }
        return None;
    }
    // The inventory takes the click before the world
    // does; it is the reason the cursor is loose.
    if inventory_screen.open && net.is_some() && !paused {
        let click = match button {
            MouseButton::Left => Some(inventory_screen::Button::Left),
            MouseButton::Right => Some(inventory_screen::Button::Right),
            _ => None,
        };
        if let Some(click) = click {
            // The screen decides *what* to ask for; the
            // server decides whether it happens. Nothing
            // moves locally, so there is no prediction to
            // be undone by the next snapshot.
            let quick = thumb_quick
                || input.action_down(
                    &settings.keybinds,
                    keybinds::Action::Sprint,
                );
            let intent = inventory_screen.click(inventory, click, quick);
            // Closing is the screen's own business and
            // needs no server: handled here, before the
            // arm below that needs a connection, because
            // a way out that stops working when the
            // connection drops is the wrong way out.
            if matches!(intent, Some(inventory_screen::Intent::Close)) {
                audio.play(audio::Sfx::Click);
                inventory_screen.close();
                grab_cursor(window, input);
                return None;
            }
            if let (Some(intent), Some(net)) = (intent, net) {
                use inventory_screen::Intent;
                // Three sounds for a dozen gestures, and
                // the split is by what the gesture *is*
                // rather than by which message it sends:
                // putting armour on is a different event
                // from moving a stack, and making
                // something is a different event again.
                audio.play(match &intent {
                    // A dressing going on is a thing put on
                    // the body, which is what the equip
                    // sound already is.
                    Intent::Equip(_) | Intent::Unequip(_) | Intent::Treat { .. } => {
                        audio::Sfx::Equip
                    }
                    // **A workshop is heard as itself.**
                    // The saw at the bench, the chisel at
                    // the mason's block, the wheel, the
                    // knife at the currier's -- so that
                    // carrying a bench into the woods is
                    // audible and not just a row in the
                    // menu turning white. Anything made
                    // in the hands or at a fire keeps the
                    // generic craft sound; see
                    // `bank::workshop_of`.
                    Intent::Craft { index, .. } => {
                        primitive_shared::crafting::RECIPES
                            .get(*index)
                            .and_then(|recipe| audio::bank::workshop_of(recipe.station))
                            .unwrap_or(audio::Sfx::Craft)
                    }
                    _ => audio::Sfx::Click,
                });
                net.send(match intent {
                    Intent::Move { from, to } => ClientMessage::MoveSlots {
                        from: from as u8,
                        to: to as u8,
                    },
                    Intent::Split { from, to } => ClientMessage::SplitSlot {
                        from: from as u8,
                        to: to as u8,
                    },
                    Intent::QuickMove(slot) => {
                        ClientMessage::QuickMoveSlot { slot: slot as u8 }
                    }
                    Intent::Sort => ClientMessage::SortInventory,
                    Intent::Equip(slot) => {
                        ClientMessage::Equip { slot: slot as u8 }
                    }
                    Intent::Unequip(slot) => {
                        ClientMessage::Unequip { slot: slot as u8 }
                    }
                    Intent::PourIntoJug { from, jug } => {
                        ClientMessage::PourIntoJug {
                            from: from as u8,
                            jug: jug as u8,
                        }
                    }
                    Intent::EmptyJug(slot) => {
                        ClientMessage::EmptyJug { slot: slot as u8 }
                    }
                    Intent::Treat { slot, part } => ClientMessage::TreatInjury {
                        slot: slot as u8,
                        part: part.index() as u8,
                    },
                    Intent::Craft { index, times } => ClientMessage::Craft {
                        index: index as u16,
                        times,
                    },
                    // Dealt with above, before the
                    // connection was unwrapped: closing
                    // a screen is not something to tell
                    // a server about.
                    Intent::Close => unreachable!(
                        "closing the pack is handled before the server is asked"
                    ),
                });
                debug_stats.network_messages_out_this_second += 1;
            }
        }
        return None;
    }
    if net.is_none() || paused {
        if button == MouseButton::Left {
            // A press that lands on none of the
            // screen's own buttons, on the arrangement
            // screen, is a press on a control -- so it
            // is offered to the controls only after the
            // buttons have had their say.
            let arranging = menu.is_arranging();
            // **The editor is read before the click is
            // acted on, and that ordering is the whole
            // of it.** The platform's copy of the field
            // is *polled* once a frame, not delivered as
            // an event, so a character committed after
            // the last poll and before this press is
            // still sitting in the editor when the press
            // arrives -- and the press may be the one
            // that moves the focus to the next box.
            // Reconciled afterwards, that character is
            // either read into the box the player has
            // just tapped or thrown away; reconciled
            // here, it lands in the field it was typed
            // into. A phone is where this happens: the
            // last letter of a world's name, and then a
            // finger on the seed.
            reconcile_the_editor(ime, window, menu, chat, traced);
            let clicked = menu.click();
            if clicked.is_none()
                && arranging
                && menu.grab_at_cursor(settings.ui_scale)
            {
                return None;
            }
            if let Some(action) = clicked {
                // Going back sounds different from going
                // in: one clip is a rising interval and
                // the other a falling one, and that is
                // the cheapest way there is to tell a
                // player which direction they just
                // moved.
                audio.play(match action {
                    Action::Back | Action::Cancel => audio::Sfx::Back,
                    _ => audio::Sfx::Click,
                });
                return Some(action);
            }
        }
        return None;
    }
    // Never while a screen that wants the pointer is up:
    // a click there taking the mouse for the camera is
    // what left the map undraggable. See `Journal::press`.
    if !input.mouse_grabbed
        && !window.is_touch_primary()
        && !journal.is_open()
        && !inventory_screen.open
        && !chest_screen.is_open()
            && !station_screen.is_open()
    {
        grab_cursor(window, input);
        // The click that grabs the cursor is not also a
        // swing at whatever happens to be under the
        // crosshair.
        input.breaking = false;
    // **Never on a phone**, where there is no pointer to
    // capture and this branch would only eat the first
    // tap of every session -- and, since the hands moved
    // into the look area, that tap is a block the player
    // asked to place. `set_cursor_grabbed` answers
    // "granted" on Android without doing anything, so
    // the flag is normally true and this was normally
    // skipped; normally is not a guarantee, and the cost
    // of the exception is one silently lost action.
    } else if button == MouseButton::Right {
        // **On the ground the right click is a hand to your
        // own body**, and a hand to the world only for the
        // river and the hearth beside you. A dressing in the
        // hand goes on (`downed::part_to_dress` picks where);
        // food and a jug go through the ordinary eat and
        // drink below; everything else -- building, opening
        // a door, climbing into a bed -- is not something a
        // body with a clock on it does, and the server
        // refuses it anyway (`barred_while_downed`).
        //
        // **Standing, a click on somebody lying on the ground
        // is a hand to them** (`ClientMessage::HelpUp`), with
        // whatever is in it; the server decides whether it is
        // what they need.
        if body.downed.is_some() {
            let held = inventory.block_in(input.hotbar_slot);
            if let Some((net, treatment)) =
                net.zip(held.and_then(primitive_shared::injury::Treatment::of))
            {
                let part = primitive_shared::downed::part_to_dress(&body.injuries, treatment);
                net.send(ClientMessage::TreatInjury {
                    slot: input.hotbar_slot as u8,
                    part: part.index() as u8,
                });
                debug_stats.network_messages_out_this_second += 1;
                return None;
            }
            let aimed = aimed_block(chunks, camera);
            if !matches!(
                use_gesture(aimed.map(|(_, block)| block), held),
                UseGesture::Eat | UseGesture::Water | UseGesture::Hearth
            ) {
                return None;
            }
        } else if let Some(net) = net {
            let held = inventory.block_in(input.hotbar_slot);
            if let Some(target) = player_under_crosshair(remote_players, chunks, camera, None)
                .filter(|&id| remote_players.is_down(id))
            {
                if held.is_some() {
                    net.send(ClientMessage::HelpUp { target });
                    debug_stats.network_messages_out_this_second += 1;
                }
                return None;
            }
        }
        // A block you can *open* takes the right click
        // before a block you could place does. Otherwise
        // the only way to use a chest with something in
        // hand would be to empty your hand first, and
        // the block would go on the front of it.
        // ...with one exception, and it is the fire.
        // A hearth is a container now, so a right click
        // opens it -- but *striking flint on it* is
        // still a gesture, and it is the one gesture
        // that is not about what is inside. Holding the
        // striker means lighting; holding anything else
        // means opening.
        //
        // Which of the four this gesture is, decided in
        // one place so the order can be checked by a
        // test rather than read down the page. See
        // `UseGesture`.
        // **A raft takes the click before anything behind it**:
        // the timber is in front of the lake, and a rower
        // reaching for the sail is not reaching for a drink.
        // What it does -- the oars, or the sail -- is the
        // server's to decide (`ClientMessage::UseRaft`). A tap
        // on the glass is this same right click, so a phone
        // takes the oars and raises the sail the same way.
        if let (Some((raft, distance)), Some(net)) = (
            entities.aimed_raft(camera.position, camera.forward(), INTERACT_RANGE),
            net,
        ) {
            if physics::raycast_block(chunks, camera.position, camera.forward(), distance)
                .is_none()
            {
                net.send(ClientMessage::UseRaft { raft });
                debug_stats.network_messages_out_this_second += 1;
                return None;
            }
        }
        let held = inventory.block_in(input.hotbar_slot);
        // **A horse takes a click with nothing to tend it in
        // hand**: a leg up, or -- with the rein key held --
        // a hand into its saddlebags. Whether it will have
        // you is the server's (`ClientMessage::Mount`), and a
        // wild horse says so; feed, a saddle or the bags in
        // hand go on to the tending below instead.
        if let Some(net) = net {
            let tending = held.is_some_and(primitive_shared::husbandry::is_tending_tool);
            let aimed = entities.aimed_at(camera.position, camera.forward(), INTERACT_RANGE);
            if let Some((horse, distance)) = aimed.filter(|_| !tending && entities.horseback.is_none()) {
                if entities.species_of(horse) == Some(primitive_shared::animals::Species::Horse)
                    && physics::raycast_block(chunks, camera.position, camera.forward(), distance).is_none()
                {
                    let bags = input.action_down(&settings.keybinds, keybinds::Action::Rein);
                    net.send(if bags {
                        ClientMessage::OpenBags { horse }
                    } else {
                        ClientMessage::Mount { horse }
                    });
                    debug_stats.network_messages_out_this_second += 1;
                    return None;
                }
            }
        }
        // **An animal takes the click next, with something to
        // tend it with in hand** -- feed, a knife, a bowl
        // (`husbandry::is_tending_tool`) -- and nothing else
        // does: a player walking planks past a sheep is
        // building, not asking it anything. What the click
        // does is the server's (`ClientMessage::TendAnimal`).
        if let (Some(held), Some(net)) = (held, net) {
            if primitive_shared::husbandry::is_tending_tool(held) {
                if let Some((animal, distance)) =
                    entities.aimed_at(camera.position, camera.forward(), INTERACT_RANGE)
                {
                    let is_animal = primitive_shared::protocol::entity_source(animal)
                        == Some(primitive_shared::protocol::EntitySource::Animal);
                    if is_animal
                        && physics::raycast_block(chunks, camera.position, camera.forward(), distance)
                            .is_none()
                    {
                        net.send(ClientMessage::TendAnimal { animal });
                        debug_stats.network_messages_out_this_second += 1;
                        return None;
                    }
                }
            }
        }
        // **A map in the hand opens the map**, whatever is
        // in front of it: it is a sheet of hide, there is
        // nothing else to do with one, and a player who has
        // just made their first map will hold it and click.
        // The map key does the same thing and is the one a
        // player ends up using; this is the gesture they
        // *try*, and a thing that does nothing when you use
        // it reads as a thing that is broken.
        //
        // **The plain click only.** With the modifier held
        // a map is laid on the ground like every other
        // thing a hand carries (`can_be_set_down`), and a
        // map that could not be put on a shelf would be the
        // one carried item that cannot.
        if held.is_some_and(|held| {
            primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_MAP
        }) && !(thumb_quick || input.action_down(&settings.keybinds, keybinds::Action::Sprint))
        {
            if journal.toggle(ui::journal::Tab::Map) {
                release_cursor(window, input);
                input.release_all();
            }
            return None;
        }
        // Everything that is about the *block* in front of the
        // player -- the door, the chest, the fire, the carcass,
        // the water, the set-down, the placement -- is one
        // function, and it is the one the scenarios play. See
        // `frame::interact`.
        let others: Vec<glam::DVec3> = remote_players.iter_positions().collect();
        super::interact::right_click_on_the_world(
            thumb_quick,
            held,
            settings,
            net,
            audio,
            player,
            camera,
            body,
            entities,
            &others,
            input,
            inventory,
            chunks,
            light,
            arrivals,
            urgent,
            dirty_set,
            chunk_versions,
            chest_screen,
            station_screen,
            mining,
            hand,
            cut,
            meal,
            notice,
            debug_stats,
        );
    }

    None
}
