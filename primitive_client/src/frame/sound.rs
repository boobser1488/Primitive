//! **What the world sounds like this frame.**
//!
//! Footsteps, the rhythm of a swing that is still going, weather,
//! anything burning nearby, the gulls and the frogs, and which of the six
//! moods the composer should be in.
//!
//! Here rather than earlier because it reads the results of everything
//! above it -- where physics put the player, and what the hands decided
//! was under the crosshair. That is why it is handed
//! [`super::hands::Worked`] rather than asking the keys again: a swing
//! that misses is a different sound from a swing that lands, and two
//! lists of conditions for the same question is how they come to
//! disagree.

use crate::audio::{self, Audio, Soundscape};
use crate::engine::camera::Camera;
use crate::engine::critters::Critters;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::physics::Player;
use crate::logic::shake;
use crate::logic::entities;
use crate::settings::ClientSettings;
use crate::ui::input;

use super::hands::Worked;

/// One frame of the ear.
#[allow(clippy::too_many_arguments)]
pub fn update(
    dt: f32,
    settings: &ClientSettings,
    world_ready: bool,
    health: f32,
    max_health: f32,
    weather: primitive_shared::weather::Weather,
    worked: &Worked,
    input: &input::InputState,
    player: &Player,
    camera: &Camera,
    chunks: &ChunkManager,
    sky: &Sky,
    critters: &Critters,
    entities: &mut entities::Entities,
    audio: &Audio,
    soundscape: &mut Soundscape,
    shake: &mut shake::Shake,
) {
    // What the world sounds like this frame: footsteps,
    // the rhythm of a swing that is still going,
    // weather, anything burning nearby, and which of the
    // six moods the composer should be in.
    //
    // Here rather than earlier because it reads the
    // results of everything above it -- where physics
    // put the player, and what the mining code decided
    // was under the crosshair.
    audio.set_volumes(settings.master_volume, settings.music_volume);
    soundscape.update(
        audio,
        &audio::soundscape::Frame {
            dt,
            player,
            camera,
            chunks,
            sky,
            weather,
            health_fraction: if max_health > 0.0 {
                health / max_health
            } else {
                1.0
            },
            // Not "is there a connection": a player
            // falling through a world whose floor has
            // not arrived yet is not walking on
            // anything.
            in_world: world_ready,
            digging: (worked.can_mine && input.breaking)
                .then_some(worked.aim)
                .flatten()
                .map(|(cell, _)| cell),
            // Swinging at nothing. The same button, and
            // deliberately a different sound: a swing
            // that misses should be audible as a miss.
            swinging: worked.can_mine
                && input.breaking
                && worked.aim.is_none()
                && !worked.struck,
            // What the blow is struck *with*: its
            // rhythm and half its noise. See
            // `Frame::held`.
            held: worked.held,
        },
    );
    // The floor arriving, jolted into the view. The
    // soundscape owns what counts as a hard landing --
    // one threshold, one event, a thump and a jolt
    // together rather than two effects that each decided
    // for themselves. See `Soundscape::take_landing`.
    if let Some(hardness) = soundscape.take_landing() {
        shake.on_landing(hardness);
    }
    // ...and what lives near the player: gulls, a bird going
    // up, the frogs. A call of its own for the reason
    // `Soundscape::wildlife` gives.
    if world_ready {
        soundscape.wildlife(
            audio,
            dt,
            chunks,
            &entities.heard(),
            &critters.croaking(),
            &critters.buzzing(),
            &critters.flapping(),
            critters.air_here(),
        );
    }
    // ...and whatever let go this frame. Drained here
    // rather than where the snapshot is applied for the
    // reason the blood is: several snapshots can land in
    // one frame, and one collapse is one sound.
    for (at, block) in entities.take_gave_way() {
        soundscape.on_gave_way(audio, at, block);
    }
}
