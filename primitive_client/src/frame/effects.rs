//! **Everything small that moves, once a frame.**
//!
//! Rain and grit and snow, the chips off a broken block, sparks over a
//! fire, white water on a rapid, the float of a line, blood where an
//! animal was struck, the butterflies and frogs, and the wind made
//! visible.
//!
//! **After the body and before the camera is used to draw them**, so
//! what is on screen is where they are this instant rather than where
//! they were last one. That ordering is the whole reason this is a phase
//! of its own rather than something the renderer does.
//!
//! The blood and the collapses are drained here rather than where the
//! snapshot that caused them is applied: several snapshots can land in
//! one frame, and one burst per blow is a property of the *frame*, not
//! of the socket.

use std::time::Instant;

use crate::audio::{self, Audio};
use crate::engine::camera::Camera;
use crate::engine::sky::Sky;
use crate::engine::{self, breeze, critters, particles};
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::inventory::Inventory;
use crate::logic::physics::Player;
use crate::logic::{self, hand};
use crate::net::network;
use crate::settings::ClientSettings;
use crate::ui;
use crate::ui::debug::DebugStats;
use crate::ui::input;
use crate::ui::keybinds;
use crate::{bleeding_for_a_photograph, falling_on, BLEED_EVERY, RAIN_WIND_SPEED};
use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::ClientMessage;

/// One frame of the weather, the particles, the small life and the rod.
#[allow(clippy::too_many_arguments)]
pub fn step(
    dt: f32,
    settings: &ClientSettings,
    worldgen: &primitive_shared::worldgen::WorldGen,
    sky: &Sky,
    weather: primitive_shared::weather::Weather,
    chunks: &ChunkManager,
    light: &LightMap,
    player: &Player,
    camera: &Camera,
    input: &input::InputState,
    inventory: &Inventory,
    audio: &Audio,
    net: &network::NetworkHandle,
    particles: &mut particles::Particles,
    critters: &mut critters::Critters,
    breeze: &mut breeze::Breeze,
    entities: &mut logic::entities::Entities,
    hand: &mut hand::Hand,
    rod_hold: &mut logic::fishing::Hold,
    fishing_float: &mut Option<logic::fishing::Float>,
    notice: &mut Option<(String, Instant)>,
    bleed_in: &mut f32,
    debug_stats: &mut DebugStats,
) {
    // **Particles, once a frame.**
    //
    // After physics and before the camera is used to
    // draw them, so what is on screen is where they are
    // this instant rather than where they were last one.
    // The weather emits into the same pool: rain is
    // particles now, spawned above the player at a rate
    // and dying on whatever they land on -- see
    // `engine::particles`.
    // Snow or rain is the season's call as much as
    // the climate's: see `season::falls_as_snow`.
    // ...and the latitude's: a tropical winter is no
    // winter (`season::seasonal_swing`), and rain in
    // the tropics must not turn to snow when the server
    // says the air is warm.
    // **Snow is not the only other answer to rain.**
    // Hot dry country gets the same front as grit
    // (`weather::Precipitation`), which is the
    // player's «сделай погоду в биомах нормальной»:
    // one sky over the world, and what reaches the
    // ground decided by the column it reaches.
    // Worked out from the two numbers the client
    // already has for the leaf tint, so it costs
    // nothing and cannot disagree with the ground.
    let (falling, wetness) =
        falling_on(worldgen, sky, weather, player.position);
    // **The rain falls in the world's own wind**, the
    // one that moves the rafts
    // (`raft::wind`) -- not a drift of its own.
    //
    // It *was* a drift of its own: a slow circle on the
    // frame clock, chosen so the rain was not a set of
    // vertical lines. Two weathers in one sky is what
    // that was. A player watching their sail braced hard
    // over to a wind out of the north, in rain falling
    // straight down the other way, is being told by the
    // game that one of the two is a decoration -- and the
    // sail is the one that costs four leathers, so the
    // rain is the one that has to give.
    //
    // Nothing is sent for this: both sides work the wind
    // out from the clock and the weather, which is the
    // whole reason `raft::wind` is a function and not a
    // message.
    let wind = primitive_shared::raft::wind(sky.world_days(), weather);
    let (wx, wz) = wind.vector();
    let drift = glam::Vec3::new(wx * RAIN_WIND_SPEED, 0.0, wz * RAIN_WIND_SPEED);
    particles.weather(
        // ...and it comes down harder in a squall and
        // eases off in a lull, so a calm reads as a calm
        // through the window as well as on the water.
        // Never to nothing, because a rain that stopped
        // would be the sky saying "clear" while the fires
        // were still going out.
        //
        // **And only as much of it as the clouds have
        // brought** (`Sky::rain_arrived`): the deck
        // closes first and the rain follows it.
        //
        // **...and harder in wet country than in dry**
        // (`weather::local_intensity`): one spell of
        // rain is a downpour in a marsh and a thin
        // shower on the steppe, which is the monsoon
        // in one multiply and the rest of what the
        // player meant by weather that suits the
        // country it falls on.
        primitive_shared::weather::local_intensity(weather, wetness)
            * (0.65 + 0.5 * wind.strength)
            * sky.rain_arrived(),
        falling,
        player.position.as_vec3(),
        drift,
        dt,
    );
    // ...and sparks over anything burning nearby.
    particles.fires(chunks, player.position.as_vec3(), dt);
    // ...and white water on any rapid in view: the
    // river's current is the generator's, and it is
    // what tells a player from the bank where not to
    // swim. See `WorldGen::river_current`.
    particles.rapids(chunks, player.position.as_vec3(), |x, y, z| worldgen.river_current(x, y, z), dt);
    // ...and the float of a line in the water, until the
    // line would be out of it or a fish comes up on it.
    // The fish in the pack is the bite: a count higher
    // than when the line went in. See `logic::fishing`.
    if let Some(float) = fishing_float.as_mut() {
        let landed = float.age == 0.0;
        float.age += dt;
        float.since += dt;
        let block_at = |x, y, z| chunks.block_at(x, y, z);
        let eye = (camera.position.x, camera.position.y, camera.position.z);
        let at = float.position(block_at);
        let at = glam::Vec3::new(at.0, at.1, at.2);
        // **The float's own plop, where it came down**, on
        // the first frame it is drawn -- which is the frame
        // it appears on the water, so the eye and the ear
        // agree. Up in pitch and down in level against the
        // hand in a pool the same recording once was: this
        // is a cork, a dozen blocks off. See `Sfx::FloatPlop`.
        if landed {
            audio.play_at(audio::Sfx::FloatPlop, at.as_dvec3(), 0.8, 1.35);
        }
        // The splash of the fish coming out is drawn
        // where the float goes (`ServerMessage::Line`),
        // which is the frame the pack has grown in.
        if float.holds(primitive_shared::geometry::narrow(eye), input.hotbar_slot, inventory.block_in(input.hotbar_slot), block_at) {
            particles.float(at, dt);
            // The ring a fish makes taking the bait, and
            // the water's own blip under it -- quieter
            // than a splash, because the whole of the
            // strike is noticing something small.
            // Placed at the float, not in the player's
            // head: it was the hand-in-water sound at full
            // level, which said "you" about something a
            // dozen blocks off. See `Sfx::FloatBite`.
            if float.phase == logic::fishing::Phase::Dipping && float.since < dt * 1.5 {
                particles.float_ring(at);
                audio.play_at(audio::Sfx::FloatBite, at.as_dvec3(), 1.0, 1.0);
            }
            // ...and the water breaking over a fish that
            // is on and running.
            if float.phase == logic::fishing::Phase::Fighting && float.strain > 0.6 {
                particles.float_ring(at);
            }
        } else {
            *fishing_float = None;
        }
    }
    // **The rod itself**: winding up, throwing, striking
    // and reeling, all off one button. See
    // `logic::fishing::Hold::step` for why this is state
    // and not events.
    {
        let holding_rod = inventory
            .block_in(input.hotbar_slot)
            .map(primitive_shared::types::block_kind)
            == Some(primitive_shared::types::BLOCK_FISHING_ROD);
        let fighting = fishing_float
            .is_some_and(|float| float.phase == logic::fishing::Phase::Fighting);
        let giving_way = input.action_down(&settings.keybinds, keybinds::Action::Sprint);
        // A screen up means the button's state has
        // stopped arriving: the wind-up is dropped
        // rather than let go of. See `Hold::cancel`.
        if !input.mouse_grabbed {
            rod_hold.cancel();
        }
        for order in rod_hold.step(
            dt,
            input.using && input.mouse_grabbed,
            holding_rod,
            fishing_float.is_some(),
            fighting,
            giving_way,
        ) {
            use logic::fishing::Order;
            let message = match order {
                Order::Cast(power) => {
                    // Judged here first, in the player's
                    // own language, against this side's
                    // own chunks: a throw onto the bank
                    // is never sent.
                    let eye = (
                        camera.position.x as f32,
                        camera.position.y as f32,
                        camera.position.z as f32,
                    );
                    let look = camera.forward();
                    match logic::fishing::cast_along(
                        eye,
                        (look.x, look.y, look.z),
                        power,
                        |x, y, z| chunks.block_at(x, y, z),
                    ) {
                        Ok(at) => {
                            *fishing_float = Some(logic::fishing::Float {
                                at,
                                slot: input.hotbar_slot,
                                fish_before: inventory
                                    .count(primitive_shared::types::BLOCK_RAW_FISH),
                                age: 0.0,
                                phase: logic::fishing::Phase::Settling,
                                strain: 0.0,
                                liveliness: 0.0,
                                since: 0.0,
                            });
                            audio.play(audio::Sfx::Swing);
                            // ...and the rod whips forward
                            // from wherever it was drawn
                            // back to. See `hand::Rod`.
                            hand.cast_rod();
                            Some(ClientMessage::CastLine { power })
                        }
                        Err(said) => {
                            *notice = Some((
                                settings.language.text(said.msg()).to_string(),
                                Instant::now(),
                            ));
                            None
                        }
                    }
                }
                Order::Strike => {
                    // **Said here, not by the server.**
                    // This side knows whether the float
                    // was under; the server refuses the
                    // strike either way
                    // (`Fishing::strike`), and what it
                    // sends back is the float going.
                    if fishing_float
                        .is_some_and(|float| float.phase != logic::fishing::Phase::Dipping)
                    {
                        *notice = Some((
                            settings
                                .language
                                .text(ui::lang::Msg::FishingStruckAtNothing)
                                .to_string(),
                            Instant::now(),
                        ));
                    }
                    Some(ClientMessage::Strike)
                }
                Order::Reel(pulling) => Some(ClientMessage::Reel { pulling }),
                Order::In => {
                    *fishing_float = None;
                    Some(ClientMessage::ReelIn)
                }
            };
            if let Some(message) = message {
                net.send(message);
                debug_stats.network_messages_out_this_second += 1;
            }
        }
    }
    // ...and blood wherever the server says an
    // animal was struck since the last frame. Here
    // rather than in `drain_network` because a
    // snapshot is applied there and this is a
    // *drawing* of it: several snapshots can arrive
    // in one frame, and one burst per blow is a
    // property of the frame rather than of the
    // socket. See `Entities::take_blows`.
    for at in entities.take_blows() {
        particles.blood(at);
    }
    // ...and the player's own, on a timer, when
    // somebody is photographing it. See
    // `bleeding_for_a_photograph`.
    if bleeding_for_a_photograph() {
        *bleed_in -= dt;
        if *bleed_in <= 0.0 {
            *bleed_in = BLEED_EVERY;
            particles.blood(
                player.eye_position().as_vec3() - glam::Vec3::Y * 0.35,
            );
        }
    }
    particles.update(chunks, dt);
    // ...and the butterflies, fireflies and frogs. The
    // wolves the frogs fall quiet for are the ones the
    // last snapshot brought -- behind a hill or in the
    // dark, but not unknown to the client.
    let dangers: Vec<glam::Vec3> = entities
        .heard()
        .into_iter()
        .filter(|animal| animal.species.is_hostile())
        .map(|animal| animal.at.as_vec3())
        .collect();
    let country = |gx: i32, gy: i32, gz: i32| {
        (
            worldgen.biome_at(gx, gz),
            worldgen.climate_at(gx, gy, gz).0,
            primitive_shared::season::seasonal_swing(worldgen.latitude_degrees(gz)),
        )
    };
    critters.update(
        &engine::critters::Surroundings {
            chunks,
            player: player.position.as_vec3(),
            world_time: sky.world_days(),
            wet: weather.is_wet(),
            dangers: &dangers,
            country: &country,
            light,
            lantern: inventory
                .block_in(input.hotbar_slot)
                .is_some_and(primitive_shared::types::is_lit_torch),
        },
        dt,
    );
    breeze.update(
        &engine::breeze::Air {
            chunks,
            light,
            player: player.position.as_vec3(),
            wind: primitive_shared::raft::wind(sky.world_days(), weather),
            world_time: sky.world_days(),
            wet: weather.is_wet(),
        },
        dt,
    );
}
