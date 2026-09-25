//! **The interface, rebuilt only when it changed.**
//!
//! One vertex list for the whole overlay -- the sleeper's dark, the red
//! of a body on the ground, the hotbar, the gauges, the chat, the pack,
//! the chest, the station, the death screen, the F3 panel and the pause
//! menu -- appended in the order they stack, because they share a
//! pipeline and a buffer.
//!
//! **Not on a clock.** It used to be laid out and uploaded a hundred and
//! twenty times a second whether anything on it had changed or not, and
//! almost every frame nothing had; a full-screen menu is tens of
//! thousands of vertices. Now the frame reduces the build's inputs to a
//! [`crate::UiKey`] and rebuilds only when that differs from last
//! frame's. What moves on time alone -- a fading chat line, the death
//! screen settling in, a running bar at an anvil -- says so instead, and
//! falls back to the [`crate::DYNAMIC_REBUILD_HZ`] clock for exactly as
//! long as it is moving.
//!
//! The interface is laid out in its own space: y in [-1, 1], x in
//! [-aspect, aspect]. Each screen is grown about its own anchor by what
//! fits *it* (`widgets::Layout`), and **anything that hit-tests must be
//! the exact inverse of what draws it** -- see `place_cursor`, and the
//! tests that say so.

use std::time::Instant;

use crate::engine::camera::Camera;
use crate::engine::renderer::GraphicsState;
use crate::engine::sky::Sky;
use crate::engine::texture;
use crate::logic::inventory::Inventory;
use crate::logic::physics::Player;
use crate::logic::{self, entities, stamina, worlds};
use crate::platform;
use crate::settings::ClientSettings;
use crate::ui::debug::{DebugStats, FrameInfo};
use crate::ui::menu::Menu;
use crate::ui::{
    self, chat, chest_screen, death, hotbar, hud, input, inventory_screen, station_screen, widgets,
};
use crate::{menu_context, player_mark, text_fingerprint, wounds_fingerprint, UiKey};
use crate::inventory_fingerprint;
use primitive_shared::lighting::LightMap;

/// Lays the whole overlay out, if anything on it changed, and says
/// whether it did -- which is what tells the renderer to re-upload.
#[allow(clippy::too_many_arguments)]
pub fn build(
    now: Instant,
    settings: &ClientSettings,
    worlds: &worlds::Worlds,
    graphics: &GraphicsState,
    rebuild_due: bool,
    loading: Option<f32>,
    paused: bool,
    hud_hidden: bool,
    touch_controls: bool,
    debug_panel_allowed: bool,
    health: f32,
    max_health: f32,
    recent_health: f32,
    breath: f32,
    nourishment: f32,
    heat: primitive_shared::crafting::Heat,
    weather: primitive_shared::weather::Weather,
    trimming: Option<primitive_shared::protocol::EntityId>,
    face_layers: &texture::FaceLayers,
    info: Option<&FrameInfo>,
    notice: &Option<(String, Instant)>,
    player: &Player,
    camera: &Camera,
    light: &LightMap,
    sky: &Sky,
    inventory: &Inventory,
    equipment: &primitive_shared::inventory::Equipment,
    body: hud::BodyGauges,
    input: &input::InputState,
    entities: &entities::Entities,
    riding: &logic::riding::Riding,
    stamina: &stamina::Stamina,
    sleep: &logic::posture::Sleep,
    rod_hold: &logic::fishing::Hold,
    fishing_float: Option<logic::fishing::Float>,
    touch: &platform::touch::Touch,
    journal: &ui::journal::Journal,
    chat: &chat::Chat,
    death: &death::DeathScreen,
    chest_screen: &chest_screen::ChestScreen,
    station_screen: &station_screen::StationScreen,
    inventory_screen: &inventory_screen::InventoryScreen,
    menu: &mut Menu,
    debug_stats: &DebugStats,
    hud_attention: &mut hud::Attention,
    ui_key: &mut Option<UiKey>,
    ui_vertices: &mut Vec<hotbar::HotbarVertex>,
) -> bool {
    // --- UI ---
    //
    // One vertex list for the whole overlay: hotbar,
    // then the F3 panel, then the pause screen on top.
    // They share a pipeline and a buffer, so the order
    // they are appended in is the order they stack.
    // The hotbar is hidden behind the loading screen --
    // there is nothing to place yet, and it would sit on
    // top of the dim.
    //
    // Rebuilt when its inputs changed, not on a clock:
    // see `UiKey`. The clock survives only as the pace
    // for the elements that animate on time alone.

    // Whatever the server last refused, until it has
    // been on screen long enough to read.
    let notice_drawn = notice.as_ref().and_then(|(text, at)| {
        let age = now.duration_since(*at).as_secs_f32();
        let left = hud::NOTICE_SECONDS - age;
        (left > 0.0)
            .then(|| (text.as_str(), (left / hud::NOTICE_FADE_SECONDS).min(1.0)))
    });
    let debug_panel_shown =
        info.is_some() && debug_stats.console_enabled && debug_panel_allowed;
    let key = UiKey {
        in_game: true,
        aspect: graphics.aspect().to_bits(),
        loading: loading.is_some(),
        hotbar_slot: input.hotbar_slot,
        inventory: inventory_fingerprint(inventory),
        health: health.to_bits(),
        max_health: max_health.to_bits(),
        recent_health: recent_health.to_bits(),
        // The horse's wind while riding, as the strip draws it.
        stamina: entities.horseback.as_ref().map_or(stamina.fraction(), |h| h.wind_fraction()).to_bits(),
        exhausted: entities.horseback.as_ref().map_or(stamina.is_exhausted(), |h| h.body.wind <= 0.0),
        breath: breath.to_bits(),
        nourishment: nourishment.to_bits(),
        heat,
        notice: notice_drawn
            .map(|(text, fade)| (text_fingerprint(text), fade >= 1.0)),
        chat: chat.ui_key(now),
        inventory_screen: inventory_screen.ui_key(),
        wounds: wounds_fingerprint(&body.injuries),
        chest_screen: chest_screen.ui_key(),
        station_screen: station_screen.ui_key(),
        death: death.ui_key(),
        sleep: sleep.ui_key(),
        journal: journal.ui_key(
            player_mark(player.position.as_vec3(), camera.yaw),
            graphics.aspect(),
        ),
        debug_panel: debug_panel_shown,
        hud_hidden,
        menu: paused.then(|| {
            menu.ui_key(&menu_context(settings, worlds, graphics, None))
        }),
        language: settings.language,
    };
    // The parts that change with no event behind them,
    // for which time is the only trigger there is.
    let ui_animating = chat.is_fading(now)
        || matches!(notice_drawn, Some((_, fade)) if fade < 1.0)
        || death.is_animating()
        || debug_panel_shown
        // A meter that is killing the player flashes.
        || hud::alarming(nourishment, breath, body)
        // ...and so does the red of a body on the ground,
        // whose clock is counting down on it.
        || body.downed.is_some();
    // **A running bar every frame, not at the animation
    // rate.** The marker is what a blow is timed against,
    // and one drawn a thirtieth of a second stale is a
    // blow aimed at where it was.
    let ui_rebuilt = ui_key.as_ref() != Some(&key)
        || (ui_animating && rebuild_due)
        || station_screen.is_running();
    if ui_rebuilt {
    *ui_key = Some(key);
    ui_vertices.clear();
    // How much bigger than it was drawn, and where each
    // piece grows from. See `widgets::scale_about`: the
    // origin is the decision, not the factor.
    //
    // The factor is now **per screen**, not one number
    // for the whole interface: the hotbar is pinned to
    // the bottom edge with a screen of room above it and
    // takes the size asked for, while a centred screen
    // takes whatever its own extent leaves. One cap for
    // all of them is what made INTERFACE SIZE do
    // nothing -- see `widgets::Layout`.
    let ui_aspect = graphics.aspect();
    let ui_scale = graphics.ui_scale();
    let layout = widgets::Layout::for_screen(ui_aspect, ui_scale);
    if loading.is_none() {
        // The dark a sleeper's screen goes, first, so the
        // hotbar, the gauges and the thumb controls are all
        // drawn over it -- see `ui::sleep` for why a phone
        // needs them there.
        ui::sleep::build_into(
            graphics.textures.font,
            sleep,
            settings.language,
            ui_aspect,
            ui_scale,
            ui_vertices,
        );
        // ...and the red at the edges of a body on the
        // ground, under the gauges for the sleep's reason:
        // the health bar and the pack's belt are what the
        // player is reaching for. See `ui::downed`.
        ui::downed::build_into(
            graphics.textures.font,
            body.downed,
            settings.language,
            ui_aspect,
            ui_scale,
            now,
            ui_vertices,
        );
        let hud_from = ui_vertices.len();
        let bar_from = ui_vertices.len();
        hotbar::build_into(
            &graphics.textures,
            inventory,
            input.hotbar_slot,
            ui_vertices,
        );
        // Stack counts and the health bar sit on top of
        // the bar, so they are appended after it.
        hud::build_into(
            graphics.textures.font,
            health,
            max_health,
            recent_health,
            // **On a horse the strip is the horse's wind**:
            // the rider is not spending their own, and the
            // number that decides whether the next stretch
            // can be a gallop is the horse's.
            entities.horseback.as_ref().map_or(stamina.fraction(), |h| h.wind_fraction()),
            entities.horseback.as_ref().map_or(stamina.is_exhausted(), |h| h.body.wind <= 0.0),
            breath,
            nourishment,
            body,
            inventory,
            notice_drawn,
            hud_attention,
            now,
            ui_vertices,
        );
        // **The first minute**: one line over the belt,
        // for a player who has held nothing and is
        // carrying nothing, and gone for good the moment
        // they pick anything up. Driven by the same
        // knowledge the recipe book and the path page are
        // (`Journal::first_minute`), so it cannot disagree
        // with either, and it costs nothing at all for
        // anybody who has ever held a flake.
        if journal.first_minute(inventory) {
            let mut painter = widgets::Painter::onto(
                graphics.textures.font,
                std::mem::take(ui_vertices),
            );
            hud::first_minute_line(
                &mut painter,
                settings.language.text(ui::lang::Msg::StepStone),
            );
            *ui_vertices = painter.into_vertices();
        }
        // The bar and its gauges are one thing pinned to
        // the bottom of the screen, so they grow as one
        // and upward -- the HUD is laid out against
        // `hotbar::BOTTOM` and would come apart from it
        // otherwise.
        widgets::scale_about(
            &mut ui_vertices[bar_from..],
            widgets::anchor::BOTTOM(ui_aspect),
            ui_scale,
        );

        // **The line**: the rod being wound back, or the
        // strain on a fish. Pinned to the crosshair with the
        // sail's dial rather than stacked with the gauges of
        // the body, and for the sail's reason: it is being
        // read exactly when nothing is wrong, so it must not
        // fade with them. See `hud::line_gauge`.
        if rod_hold.charge().is_some()
            || fishing_float.is_some_and(|float| float.phase == logic::fishing::Phase::Fighting)
        {
            let from = ui_vertices.len();
            let mut painter = widgets::Painter::onto(
                graphics.textures.font,
                std::mem::take(ui_vertices),
            );
            hud::line_gauge(
                &mut painter,
                rod_hold.charge(),
                fishing_float
                    .filter(|float| float.phase == logic::fishing::Phase::Fighting)
                    .map(|float| float.strain),
            );
            *ui_vertices = painter.into_vertices();
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::CENTRE(ui_aspect),
                ui_scale,
            );
        }

        // Aboard a raft with its sail up, the dial
        // that says what the trim is doing against the wind.
        //
        // Not part of the stack above: it is pinned to the
        // top of the screen, it is not a gauge of the body,
        // and it must not fade out when the player is well --
        // a sail is being read exactly when nothing is wrong.
        if let Some((raft, body)) = riding.aboard_raft().filter(|(_, body)| body.sail) {
            let from = ui_vertices.len();
            let mut painter = widgets::Painter::onto(
                graphics.textures.font,
                std::mem::take(ui_vertices),
            );
            let wind = primitive_shared::raft::wind(sky.world_days(), weather);
            hud::sail_gauge(
                &mut painter,
                wind.toward - body.yaw,
                body.sail_angle,
                wind.strength,
                trimming == Some(raft),
            );
            *ui_vertices = painter.into_vertices();
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::TOP(ui_aspect),
                ui_scale,
            );
        }

        // **Which way is north**: a needle while a water
        // compass is in the hand, and a line off the sky
        // while the player is looking at it. Pinned to the
        // top with the sail's dial and never faded, for the
        // sail's reason. See `logic::bearing` for why the
        // sky's reading is a look and not an instrument.
        {
            let from = ui_vertices.len();
            let mut painter = widgets::Painter::onto(
                graphics.textures.font,
                std::mem::take(ui_vertices),
            );
            let held_compass = inventory
                .block_in(input.hotbar_slot)
                .is_some_and(|held| primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_WATER_COMPASS);
            if held_compass {
                let sailing = riding.aboard_raft().is_some_and(|(_, body)| body.sail);
                hud::compass_dial(
                    &mut painter,
                    logic::bearing::needle(camera.yaw),
                    if sailing { hud::COMPASS_BESIDE_SAIL } else { 0.0 },
                );
            }
            let eye = camera.position.floor();
            let reading = logic::bearing::read_sky(&logic::bearing::SkyView {
                look: camera.forward(),
                to_sun: -sky.sun_direction(),
                to_moon: -sky.moon_direction(),
                moon_lit: primitive_shared::moon::illumination(sky.world_days()),
                overcast: sky.overcast(),
                open_sky: light.sky(eye.x as i32, eye.y as i32, eye.z as i32)
                    >= primitive_shared::types::MAX_LIGHT,
            });
            if let Some(reading) = reading {
                use logic::bearing::{Guide, Side};
                use ui::lang::Msg;
                let by = settings.language.text(match reading.guide {
                    Guide::Sun => Msg::SkyBySun,
                    Guide::Moon => Msg::SkyByMoon,
                    Guide::Stars => Msg::SkyByStars,
                });
                let north = settings.language.text(match reading.north {
                    Side::Ahead => Msg::NorthAhead,
                    Side::Right => Msg::NorthRight,
                    Side::Behind => Msg::NorthBehind,
                    Side::Left => Msg::NorthLeft,
                });
                hud::sky_hint(&mut painter, &format!("{by}: {north}"));
            }
            *ui_vertices = painter.into_vertices();
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::TOP(ui_aspect),
                ui_scale,
            );
        }

        // The thumb controls, over the HUD and under
        // everything that can be opened: a player with
        // their pack open is not steering. Never drawn
        // on a desktop -- see `is_touch_primary`. (The
        // compass to a bag that stood here is gone; the way
        // back is the map's -- see `ui::journal`.)
        if touch_controls
            && !paused
            && !inventory_screen.open
            && !chest_screen.is_open()
            && !station_screen.is_open()
            && !journal.is_open()
            && !death.is_open()
        {
            let mut painter = widgets::Painter::onto(
                graphics.textures.font,
                std::mem::take(ui_vertices),
            );
            hud::touch_controls(&mut painter, touch.layout(), |control| {
                touch.is_held(control)
            }, settings.language);
            *ui_vertices = painter.into_vertices();
        }

        // Hidden with Tab: everything since the sleeper's
        // dark goes -- hotbar, gauges, notices, thumbs -- and
        // chat, the screens and the menus drawn after this
        // still come. A phone keeps its thumb controls,
        // which are how it would ever press the key again.
        if hud_hidden && !touch_controls {
            ui_vertices.truncate(hud_from);
        }

        // Chat sits over the HUD and under the
        // inventory: it is readable while playing, and
        // it is not what a player opening their pack is
        // looking at.
        if chat.has_anything_to_draw(now) {
            let from = ui_vertices.len();
            chat.build_into(
                graphics.textures.font,
                graphics.aspect(),
                // The box carries its own send and
                // leave buttons where there is no
                // keyboard to press Enter and Escape
                // on. See `chat::input_row`.
                touch_controls,
                settings.language,
                now,
                ui_vertices,
            );
            // Pinned to the bottom-left: the box is
            // typed there and the log fills upward
            // from it.
            let grown = layout.fit_from_corner(chat::EXTENT);
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::BOTTOM_LEFT(ui_aspect),
                grown,
            );
            // ...and then up, clear of the on-screen
            // keyboard, which the corner it is pinned to
            // knows nothing about. See
            // `chat::keyboard_lift`: the hit-test below
            // subtracts the same number, and it is the
            // same call so the two cannot drift.
            widgets::lift(
                &mut ui_vertices[from..],
                chat::keyboard_lift(touch_controls, chat.is_typing(), grown),
            );
        }

        // The inventory sits over the HUD, and the pause
        // screen (appended below) over both. Guarded
        // rather than left to `build_into`'s early
        // return, because assembling the argument clones
        // the texture table -- an allocation a frame for
        // a screen that is almost always shut.
        if inventory_screen.open {
            // The face table is the one built at startup,
            // not a fresh copy: `face_layers()` clones
            // the whole lookup, and doing that once a
            // frame for a screen that is open for
            // seconds at a time is an allocation nobody
            // asked for.
            let from = ui_vertices.len();
            inventory_screen.build_into(
                graphics.textures.font,
                face_layers,
                inventory,
                equipment,
                &body.injuries,
                // The health page's readings, gathered
                // here because this is the one place
                // that has all four: health and
                // nourishment come in their own
                // messages, stamina is predicted on the
                // client, and the rest ride `body`.
                &ui::inventory_screen::Vitals {
                    health: if max_health > 0.0 {
                        (health / max_health).clamp(0.0, 1.0)
                    } else {
                        0.0
                    },
                    nourishment,
                    stamina: stamina.fraction(),
                    body,
                },
                // What the path tab is drawn from:
                // what this player has held, and what
                // their keys are bound to. See
                // `ui::ladder_screen::Learning`.
                ui::ladder_screen::Learning {
                    discovered: journal.discovered(),
                    keys: &settings.keybinds,
                },
                settings.language,
                ui_vertices,
            );
            // A screen the player opens grows outward
            // from the middle, which is also the point
            // `cursor_to_ui` inverts -- so a
            // click still lands on the slot under it.
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::CENTRE(ui_aspect),
                inventory_screen::grow_by(layout),
            );
        }

        // The chest sits where the inventory does,
        // and the two are never open at once -- opening
        // either closes the other.
        if chest_screen.is_open() {
            let from = ui_vertices.len();
            chest_screen.build_into(
                graphics.textures.font,
                face_layers,
                inventory,
                settings.language,
                ui_vertices,
            );
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::CENTRE(ui_aspect),
                chest_screen.grow_by(layout),
            );
        }

        if station_screen.is_open() {
            let from = ui_vertices.len();
            station_screen.build_into(
                graphics.textures.font,
                face_layers,
                settings.language,
                ui_vertices,
            );
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::CENTRE(ui_aspect),
                station_screen.grow_by(layout),
            );
        }

        // The death screen goes over all of it. The
        // pause menu is drawn after this whole block and
        // therefore still sits on top, which is right:
        // it is the only screen that can leave the
        // world, and a dead player is exactly who wants
        // to.
        // The journal is laid out against the window, like
        // the menu, rather than grown about the middle:
        // it fills the glass and has nowhere to grow to.
        if journal.is_open() && !death.is_open() {
            journal.build_into(
                graphics.textures.font,
                face_layers,
                inventory,
                player_mark(player.position.as_vec3(), camera.yaw),
                ui_aspect,
                settings.language,
                ui_vertices,
            );
        }

        if death.is_open() {
            let from = ui_vertices.len();
            death.build_into(graphics.textures.font, settings.language, ui_vertices);
            widgets::scale_about(
                &mut ui_vertices[from..],
                widgets::anchor::CENTRE(ui_aspect),
                death::grow_by(layout),
            );
        }
    }
    if let Some(info) = info
        .as_ref()
        .filter(|_| debug_stats.console_enabled && debug_panel_allowed)
    {
        // **Deliberately not scaled.** It is a readout,
        // not a screen: the player asking for a bigger
        // interface is asking about the things they
        // press, and thirty lines of diagnostics grown
        // half again covered two thirds of a phone. It
        // fits itself to the window instead -- see
        // `debug::panel_into`.
        ui::debug::panel_into(
            &debug_stats.overlay_lines(info),
            ui_aspect,
            graphics.textures.font,
            ui_vertices,
        );
    }
    if paused {
        // Laid out rather than scaled -- see the menu
        // built for the title screen above.
        menu.build_into(
            &menu_context(settings, worlds, graphics, None),
            ui_vertices,
        );
    }
    } // ui_rebuilt

    ui_rebuilt
}
