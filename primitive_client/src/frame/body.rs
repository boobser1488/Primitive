//! **The body, predicted.**
//!
//! Everything between "what the player asked for" and "where the player
//! now is": the oars, the reins, the load and the armour folded into one
//! speed, the collider stepped in fixed slices, and the stamina billed
//! for whatever actually happened.
//!
//! **Prediction only.** The server decides where bodies are; this is the
//! client keeping up so that a step feels like a step rather than
//! arriving a fifth of a second late. A disagreement is a correction the
//! client applies -- see `respawn_gate` and the position-correction arm
//! of `drain_network`.
//!
//! Where it sits in the frame: after the socket and after streaming, so
//! the ground under the player is the ground the server has already sent;
//! before the particles and the camera, so what is drawn is where the
//! body is this instant rather than where it was last one.

use std::time::Instant;

use glam::Vec3;

use crate::engine::camera::Camera;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::inventory::Inventory;
use crate::logic::physics::Player;
use crate::logic::{self, stamina};
use crate::net::network;
use crate::net::remote_players::RemotePlayers;
use crate::settings::ClientSettings;
use crate::ui::debug::DebugStats;
use crate::ui::{self, chat, chest_screen, death, input, inventory_screen, keybinds, station_screen};
use crate::{tiredness_speed, wish_direction, PHYSICS_STEP};
use primitive_shared::protocol::ClientMessage;

/// One frame of the body, and whether the player is *actually* running.
///
/// The answer is returned rather than read off the keys again because it
/// is used twice -- stamina is billed for it and the view bobs to it --
/// and two lists of conditions disagreed in both directions. See the
/// comment where it is worked out.
#[allow(clippy::too_many_arguments)]
pub fn step(
    dt: f32,
    now: Instant,
    world_ready: bool,
    paused: bool,
    settings: &ClientSettings,
    chunks: &ChunkManager,
    worldgen: &primitive_shared::worldgen::WorldGen,
    sky: &Sky,
    weather: primitive_shared::weather::Weather,
    inventory: &Inventory,
    equipment: &primitive_shared::inventory::Equipment,
    body: &ui::hud::BodyGauges,
    sleep: &logic::posture::Sleep,
    death: &death::DeathScreen,
    inventory_screen: &inventory_screen::InventoryScreen,
    chest_screen: &chest_screen::ChestScreen,
    station_screen: &station_screen::StationScreen,
    journal: &ui::journal::Journal,
    chat: &chat::Chat,
    input: &input::InputState,
    other_positions: &[glam::DVec3],
    net: &network::NetworkHandle,
    player: &mut Player,
    camera: &mut Camera,
    entities: &mut logic::entities::Entities,
    riding: &mut logic::riding::Riding,
    remote_players: &mut RemotePlayers,
    stamina: &mut stamina::Stamina,
    rising: &mut logic::posture::Rising,
    resting: &mut logic::posture::Resting,
    debug_stats: &mut DebugStats,
) -> bool {
    // Whether the player is *actually* running, decided
    // once and used twice: stamina is billed for it and
    // the view bobs to it.
    //
    // **One value rather than two lists of conditions.**
    // The bob used to build its own -- the Sprint key,
    // not paused, not dead, grounded, not swimming --
    // and the two lists disagreed in both directions. A
    // phone has no Sprint key at all (the stick sprints
    // when it is pushed to the rim), so the view never
    // bobbed on Android; and a player holding Shift with
    // no stamina left bobbed while walking, which is the
    // one thing `walking_without_sprinting_does_not_bob`
    // exists to forbid. Read from the value that is
    // charged for and the bob cannot mean anything other
    // than "you are paying to run".
    let mut really_running = false;
    if world_ready {
        // Physics still runs while paused -- gravity does
        // not stop for a menu on an authoritative server
        // -- but the player stops steering.
        // A dead player steers no more than a paused one
        // does. Gravity still applies to both -- the
        // server is authoritative about where bodies
        // are, and a corpse hovering where it died would
        // rubber-band the moment it respawned.
        // ...and a sleeping player steers least of
        // all. The server has stopped reading their
        // transforms entirely (see
        // `ServerMessage::Asleep`), so a client that
        // kept walking would be walking a body nobody
        // else can see move -- and would then be
        // snapped back the moment they woke.
        let frozen = paused
            || death.is_open()
            || sleep.is_asleep()
            || inventory_screen.open
            || chest_screen.is_open()
            || station_screen.is_open()
            || journal.is_open()
            || chat.is_typing();
        let wish_dir = if frozen {
            Vec3::ZERO
        } else {
            wish_direction(input, camera, &settings.keybinds)
        };
        // --- a raft: the oars, and the deck underfoot ---
        //
        // **At the oars, the keys that walk row instead**, and so
        // does the stick: forward and back is the stroke, left
        // and right the turn, measured against where the camera
        // looks the way walking is. A breathless rower pulls at
        // `raft::TIRED_STROKE` rather than not at all. See
        // `riding::oars_from_axes`.
        let rowing = entities.steering().map(|steering| steering.id);
        let oars = match rowing {
            Some(_) if !frozen => logic::riding::oars_from_axes(
                wish_dir.dot(camera.forward_horizontal()) * input.stick_speed(),
                wish_dir.dot(camera.right_horizontal()) * input.stick_speed(),
                !stamina.can_sprint(),
            ),
            _ => primitive_shared::raft::Oars::REST,
        };
        let wish_dir = if rowing.is_some() { Vec3::ZERO } else { wish_dir };
        // The yard as this client's own hand is holding it,
        // before the prediction that pushes against it: the
        // sail's angle is what the wind is measured against
        // (`raft::Body::sail_normal`), so the drag has to be
        // in the raft *this frame steps*, not in the one the
        // next snapshot brings.
        entities.set_trim(riding.held_trim(now));
        // ...and the river under the rowed raft, which the
        // server's step reads off its own generator: see
        // `WorldGen::river_current`.
        entities.set_river_current(entities.steering().map_or((0.0, 0.0), |steering| {
            worldgen.river_current(steering.body.x as f32, steering.body.y, steering.body.z as f32)
        }));
        entities.predict(
            oars,
            primitive_shared::raft::wind(sky.world_days(), weather),
            sky.world_days(),
            chunks,
            dt,
        );
        if let Some(message) = rowing.and_then(|raft| riding.row_message(raft, oars, now)) {
            net.send(message);
            debug_stats.network_messages_out_this_second += 1;
        }
        // ...and the angle itself, which the server is the
        // authority on: it decides whether this player is
        // standing where a hand reaches the sheets.
        if let Some(message) = riding.trim_message(now) {
            net.send(message);
            debug_stats.network_messages_out_this_second += 1;
        }
        // Carried before the collider runs -- see
        // `Riding::carry_player` for the shudder the other order
        // makes -- and the decks handed to it as they are now.
        let decks = entities.rafts();
        riding.carry_player(&decks, &mut player.position, &mut camera.yaw);
        player.decks = decks.iter().map(|pose| pose.now).collect();
        // --- a horse: the reins, and the body on its saddle ---
        //
        // **On a horse the keys that walk ride**, the oars'
        // rule: forward and back, left and right off the same
        // wish the walk is made of, measured against the
        // camera so a rider looking over their shoulder still
        // rides where the keys say (`Horseback::reins_from_keys`).
        // The body is put on the predicted saddle and not
        // stepped at all -- see the physics loop below.
        let on_horse = entities.horseback.is_some();
        if let Some(mut horseback) = entities.horseback.take() {
            let (forward, turn) = if frozen {
                (0.0, 0.0)
            } else {
                (
                    wish_dir.dot(camera.forward_horizontal()) * input.stick_speed(),
                    wish_dir.dot(camera.right_horizontal()) * input.stick_speed(),
                )
            };
            let reins = logic::horseback::Horseback::reins_from_keys(
                forward,
                turn,
                !frozen && input.action_down(&settings.keybinds, keybinds::Action::Sprint),
                !frozen && input.action_down(&settings.keybinds, keybinds::Action::Rein),
                !frozen && input.action_pressed(&settings.keybinds, keybinds::Action::Jump),
            );
            horseback.predict(reins, &|x, y, z| chunks.block_at(x, y, z), dt);
            if let Some(message) = horseback.rein_message(now) {
                net.send(message);
                debug_stats.network_messages_out_this_second += 1;
            }
            // **Getting down**: the rein key with the horse
            // standing and nothing asked of it. At a trot the
            // same key is a walk, which is how a rider comes
            // to a stop and then off.
            if !frozen
                && forward.abs() < 0.05
                && horseback.may_get_down()
                && input.action_pressed(&settings.keybinds, keybinds::Action::Rein)
            {
                net.send(ClientMessage::Dismount);
                debug_stats.network_messages_out_this_second += 1;
            }
            player.position = horseback.rider_feet();
            player.velocity = Vec3::ZERO;
            player.grounded = horseback.body.on_ground;
            entities.set_ridden(Some((horseback.horse, horseback.feet(), horseback.body.yaw)));
            entities.horseback = Some(horseback);
        }
        // **Getting up.** The screen said "asleep -- press
        // any key to get up" for as long as sleep existed,
        // and nothing behind it listened: the only way out of
        // a bed was a right click on that bed. A step or a
        // jump asks now, lying or sitting, and the dark says
        // so in those words (`ui::sleep`). A sitter is
        // already on their feet as far as the client is
        // concerned -- sitting is not a lock, and the step
        // they asked for is the one that gets them off the
        // stool -- while a sleeper waits for the server to
        // say where they stand (`ServerMessage::Posture`).
        //
        // **Not a key that was already down when the body
        // came to rest**, which is `Rising`'s whole reason: a
        // player who walked up to a bed holding forward was
        // stood back up by that same key on the first frame
        // they lay in it.
        let controls_free = !paused
            && !death.is_open()
            && !inventory_screen.open
            && !chest_screen.is_open()
            && !station_screen.is_open()
            && !journal.is_open()
            && !chat.is_typing();
        // A rower's movement keys are the oars, so only the
        // jump gets a rower up off the stern -- and a rider's
        // are the reins and the jump is the horse's, so
        // nothing here gets a rider down (the rein key does,
        // above).
        let asked = controls_free
            && !on_horse
            && ((rowing.is_none()
                && wish_direction(input, camera, &settings.keybinds) != Vec3::ZERO)
                || input.action_pressed(&settings.keybinds, keybinds::Action::Jump));
        if rising.ask(*resting, asked) {
            net.send(ClientMessage::StandUp);
            debug_stats.network_messages_out_this_second += 1;
            if resting.is_sitting() {
                *resting = logic::posture::Resting::Standing;
            }
        }
        // Weight slows you down and stamina decides
        // whether the sprint is available at all. Both
        // are folded in here so physics only ever sees
        // one speed and one flag.
        //
        // **Armour costs twice, and the two costs are
        // different things.** Its *weight* goes through
        // the same load rules a heavy pack does, which is
        // why it is added to the carried total rather
        // than handled apart; its *bulk* is that plate
        // is stiff, which has nothing to do with how much
        // it weighs and is a second multiplier. A player
        // in full iron is slow because they are carrying
        // thirty kilos and slower still because they
        // cannot bend -- see `equipment::Worn::mobility`.
        let carried = inventory.total_weight() + equipment.weight();
        player.speed_scale = primitive_shared::load::speed_scale(carried)
            * equipment.worn().mobility()
            // ...and how tired they are, which is the
            // fourth thing that decides a pace. Read
            // off the same number the server keeps (it
            // arrives with the other gauges), so the
            // two sides agree without a second rule:
            // the client is *applying* the server's
            // fatigue, not inventing one.
            * tiredness_speed(body.fatigue)
            // ...and a broken leg, set or not: a splint
            // is what lets it knit, not a leg to walk on.
            // See `Injuries::speed_factor`.
            //
            // **On the ground it is the crawl instead**, not
            // as well: a body on its belly is not favouring
            // a leg, and the crawl in `downed` is already the
            // whole of how fast it goes.
            * body.downed.map_or(body.injuries.speed_factor(), |down| down.crawl())
            // How hard a thumb is pushing, and 1.0 on
            // anything with a keyboard. It belongs in
            // the same multiplier that weight and
            // armour use, so physics still sees one
            // speed however the player asked for it.
            // Only ever *reduces* the scale, which is
            // why it needs nothing from the server: no
            // anti-cheat check has ever complained
            // about someone moving too slowly.
            * input.stick_speed();
        // ...and snowshoes, which are not a speed but a
        // surface: see `types::surface_drag_shod`.
        player.snowshoes = equipment.snowshoes();
        // ...and how much of the water's lift is left,
        // off the same weight. **Not multiplied by the
        // armour or the thumb**: those are about how
        // fast a body moves, and this is about whether
        // it floats -- a player in iron floats exactly
        // as well as the same weight of stone would.
        // See `load::buoyancy`, which the server bills
        // the breath by.
        player.buoyancy = primitive_shared::load::buoyancy(carried);
        // ...and whether it was the *client* that just took
        // the movement keys away, in which case the body
        // treads water rather than settling to the depth its
        // load asks for.
        //
        // Read off `frozen` -- the one value that already
        // means "the player is not steering because a screen
        // is up" -- rather than from a fresh list of screens
        // beside it. A second list is a second list to keep
        // in step, and the one screen left off it would be
        // the one a drowning player had open.
        //
        // See `Player::treading`: a player with a heavy pack
        // in deep water could only lighten it by opening the
        // pack, and opening the pack was what sank them.
        player.treading = frozen;
        let wants_sprint = !frozen
            && (input.action_down(&settings.keybinds, keybinds::Action::Sprint)
                // A thumb pushed to the rim, for a
                // screen with no Shift on it.
                || input.stick_sprinting());
        // ...and legs that will take a run. A broken one
        // will not, and the jump stays -- see
        // `Injuries::may_sprint` for why the one goes and
        // not the other.
        let sprinting = wants_sprint
            && stamina.can_sprint()
            && body.injuries.may_sprint()
            && body.downed.is_none();
        // **Up on jump, down on sprint** -- the two keys
        // a hand is already on, and neither of them does
        // anything else while flying: there is no ground
        // to jump off and no stamina to spend running.
        //
        // Read off the *keys* rather than off
        // `sprinting` above, which is gated on having
        // stamina left. An exhausted player who could
        // not descend would be stuck in the air with no
        // way to understand why.
        player.climb = if player.flying && !frozen {
            let up = input
                .action_down(&settings.keybinds, keybinds::Action::Jump);
            let down = input
                .action_down(&settings.keybinds, keybinds::Action::Sprint);
            (up as i32 - down as i32) as f32
        } else {
            0.0
        };
        // Physics in fixed slices, not one step of
        // however long the frame was.
        //
        // A step resolves collisions by moving and then
        // pushing back out, so its size bounds how far
        // the player may travel inside one: at 100 ms
        // and terminal velocity that is several blocks,
        // which goes *through* a floor. The server then
        // rejects the position and rubber-bands them
        // back, and what the player feels is the physics
        // lurching every time the frame rate hiccups.
        //
        // Bounded, so a stall cannot turn into a spiral
        // of catch-up steps that causes the next one.
        // A jump is a push, and an exhausted player has
        // nothing to push with. Refused here rather than
        // inside physics, which has no idea what stamina
        // is -- and refused rather than weakened, since
        // half a jump is a way to end up stuck in a hole.
        //
        // **Up a tree a jump is a climb**: dearer, slower
        // between pulls, and heavier under a load (see
        // `Stamina::climb_cost`). And nobody jumps at all
        // under more than they can carry (`load::can_jump`).
        let climbing = player.footing_is_a_tree(chunks);
        // ...and nobody jumps off the ground they are lying
        // on. The held key still swims a downed body up in
        // water: that is a stroke, not a jump, and a body
        // that could not surface would drown in the first
        // pond it crawled into rather than choose to.
        let may_jump = primitive_shared::load::can_jump(carried)
            && body.downed.is_none()
            && if climbing { stamina.can_climb(carried) } else { stamina.can_jump() };
        // The river the player is in, once a frame: a
        // current changes over metres, not over the slices
        // of one frame. Asked only in water, because it is
        // a few columns of the generator. See
        // `physics::Player::current`.
        player.current = if player.in_water {
            worldgen.river_current(player.position.x as f32, player.position.y as f32, player.position.z as f32)
        } else {
            (0.0, 0.0)
        };
        let mut left = dt;
        let mut first = true;
        // **A sleeper's body is pinned, not simulated.** The
        // server lays it across the bed with an upright
        // collider, and under a roof two blocks up that
        // collider is inside the ceiling; the push-out that
        // follows walked sleepers out of their beds and
        // through the wall beside them. The server moves
        // nobody who is asleep, so nothing is lost by not
        // stepping.
        // ...and a rider's is carried by the horse: it was put
        // on the saddle above, and a collider stepped under it
        // would drop it through the horse's back.
        while left > 0.0 && !on_horse && !matches!(resting, logic::posture::Resting::Lying { .. }) {
            let step = left.min(PHYSICS_STEP);
            player.update(
                chunks,
                &other_positions,
                wish_dir,
                // Where the camera points, not where the
                // player faces. Only water reads it, and
                // it is what lets a swimmer dive: see
                // `physics::stroke_direction`.
                camera.forward(),
                // Only the first slice may jump: the
                // press edge is one event, and firing it
                // in every slice is a jump that scales
                // with how bad the frame was.
                first
                    && !frozen
                    && may_jump
                    && input.action_pressed(
                        &settings.keybinds,
                        keybinds::Action::Jump,
                    ),
                !frozen
                    && input.action_down(
                        &settings.keybinds,
                        keybinds::Action::Jump,
                    ),
                sprinting,
                step,
            );
            // Billed per push that actually happened,
            // not per press: physics refuses a jump in
            // mid-air, and swimming up is not one at all.
            if player.jumped {
                if climbing {
                    stamina.spend_climb(carried);
                } else {
                    stamina.spend_jump();
                }
            }
            left -= step;
            first = false;
        }

        // Billed for the sprint the player actually got,
        // not the one they asked for: physics refuses it
        // in water and standing still, and charging for
        // a sprint that did not happen is the sort of
        // thing players notice and cannot explain.
        // Which deck the feet ended on, and everyone else put on
        // the decks as they are drawn this frame (see
        // `RemotePlayers::ride`).
        riding.settle(&decks, player.position);
        remote_players.ride(&decks, dt);
        // ...and everyone astride a horse put on its saddle as
        // it is drawn this frame (`RemotePlayers::mount`), the
        // decks' reason again.
        remote_players.mount(&entities.ridden_horses(Instant::now()));
        really_running = (sprinting
            && player.grounded
            && !player.swimming
            && player.horizontal_speed() > 0.5)
            // **Rowing costs breath as running does**, which is
            // what makes the sail's free pace worth its four
            // leathers. Not a bob, though: `footed` below is read
            // off the body's own speed, which a rower has none of.
            || oars.pulling();
        stamina.update(
            dt,
            primitive_shared::load::load_fraction(carried),
            really_running,
        );
    }

    really_running
}
