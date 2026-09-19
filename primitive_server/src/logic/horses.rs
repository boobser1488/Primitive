//! Riding: the server's half of `primitive_shared::horse`.
//!
//! The rules -- what a gallop is worth, what a slope costs, where a river is
//! too deep -- are shared, because the rider's client predicts with them. The
//! animal is the animals' (`Animals::mount`, `carry`). What lives here is the
//! part that touches a player: getting on and off, the reins off the wire,
//! putting the rider on the saddle every tick, the bags as a container, and
//! what a dead horse leaves on the ground.
//!
//! ## The rower's pattern, said again
//!
//! Everything here is the raft's (`rafts`), for the raft's reasons. The rider
//! is put on the server's horse every tick before the snapshots are built
//! (`tick`), so the rider and the horse in one snapshot are the same instant
//! on everybody's screen; the reins are the only thing the client says about
//! where the horse goes; a stale rein stops pulling. What differs is the
//! anti-cheat: a rower sends nothing but a deck and a look, and a rider's
//! client goes on sending its own transform -- the saddle of the horse it
//! predicts -- which the anti-cheat reads with a mounted allowance
//! (`AntiCheat::set_mounted`). The transform never moves the body (the horse
//! does that); what it buys is that a client claiming to ride while its
//! positions climb into the sky is caught by the same flight rules as one on
//! foot, and put off the horse.

use std::time::{Duration, Instant};

use primitive_shared::horse::{self, Fettle, Gait, Mount, Reins};
use primitive_shared::protocol::{EntityId, Posture, ServerMessage};

use primitive_shared::notice::Notice;

use crate::logic::animals::{Mounting, Unbuckled};

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// How often a rider is told their horse's wind: see `ServerMessage::Mounted`.
const TELL_WIND_EVERY: Duration = Duration::from_millis(500);

/// How far to the side of its middle a rider steps down, in blocks: the
/// horse's half-width and a player's, and a hand between.
const STEP_DOWN: f32 = horse::HALF_WIDTH + primitive_shared::geometry::PLAYER_HALF_WIDTH + 0.15;

/// `ClientMessage::Mount`.
pub(crate) fn mount(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
) {
    let feet = {
        let state = lock(&handle.state);
        if state.vitals.is_dead()
            || state.sleeping_in.is_some()
            || state.sitting_on.is_some()
            || state.rowing.is_some()
            || state.riding.is_some()
        {
            return;
        }
        primitive_shared::geometry::narrow(state.position)
    };
    let mounting = lock(&ctx.animals).mount(id, handle.id, feet);
    match mounting {
        Mounting::Refused(why) => {
            handle.send(ServerMessage::Notice { what: why });
        }
        Mounting::Thrown { at } => {
            // **On the ground beside it, and hurt a little**: the fall is a
            // player's own fall, through the door every blow comes through,
            // so armour and the death screen both know about it.
            let (sin, cos) = (lock(&handle.state).yaw + std::f32::consts::PI).sin_cos();
            let down = (at.0 + f64::from(cos * STEP_DOWN * 1.5), at.1, at.2 + f64::from(sin * STEP_DOWN * 1.5));
            {
                let mut state = lock(&handle.state);
                state.position = down;
                state.anticheat.reset_to(down);
                state.vitals.clear_fall();
            }
            handle.send(ServerMessage::PositionCorrection {
                x: down.0,
                y: down.1,
                z: down.2,
                reason: "thrown".to_string(),
            });
            let outcome = crate::strike_player(
                ctx,
                handle,
                primitive_shared::husbandry::THROW_DAMAGE,
                "was thrown by a horse",
                primitive_shared::injury::Blow::Blunt,
            );
            crate::report_vitals(ctx, handle, outcome);
            tell(handle, Notice::HorseThrowsYou);
        }
        Mounting::Riding { body, fettle, broke } => {
            seat(handle, id, body, fettle);
            if broke {
                tell(handle, Notice::HorseIsYours);
            }
        }
    }
}

/// Puts a rider on horse `id` at `body`, and tells them.
fn seat(handle: &std::sync::Arc<crate::players::PlayerHandle>, id: EntityId, body: Mount, fettle: Fettle) {
    let saddle = body.saddle();
    {
        let mut state = lock(&handle.state);
        state.riding = Some(id);
        state.aboard = None;
        state.position = (saddle[0], saddle[1], saddle[2]);
        state.on_ground = true;
        state.mount_told = Some(Instant::now());
        let at = state.position;
        state.anticheat.reset_to(at);
        state.anticheat.set_mounted(true);
        state.vitals.clear_fall();
    }
    // The posture first, which puts the body on the saddle; then the horse,
    // which the client starts predicting from there.
    handle.send(ServerMessage::Posture { posture: Posture::Mounted, at: Some((saddle[0], saddle[1], saddle[2])), yaw: body.yaw });
    handle.send(ServerMessage::Mounted { horse: Some(id), at: (body.x, body.y, body.z), yaw: body.yaw, wind: body.wind, fettle });
}

/// News about a horse, in the player's words (`notice`). It was a chat line
/// in English from "server", which a Russian player read as English and every
/// player read as somebody talking.
fn tell(handle: &std::sync::Arc<crate::players::PlayerHandle>, what: Notice) {
    handle.send(ServerMessage::Notice { what });
}

/// `ClientMessage::Dismount`, and every other way off a horse: `why` is said
/// to the player when it is not their own choice.
pub(crate) fn dismount(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    why: Option<&str>,
) {
    let Some(id) = lock(&handle.state).riding.take() else {
        return;
    };
    let body = lock(&ctx.animals).dismount(id, handle.id);
    let down = body.map_or_else(|| lock(&handle.state).position, |body| step_down_from(ctx, &body));
    {
        let mut state = lock(&handle.state);
        state.position = down;
        state.anticheat.set_mounted(false);
        state.anticheat.reset_to(down);
        state.vitals.clear_fall();
        state.mount_told = None;
    }
    handle.send(ServerMessage::Mounted { horse: None, at: down, yaw: 0.0, wind: 0.0, fettle: Fettle::FRESH });
    handle.send(ServerMessage::Posture { posture: Posture::Standing, at: Some(down), yaw: 0.0 });
    if let Some(text) = why {
        handle.send(ServerMessage::Chat { from: None, username: "server".to_string(), text: text.to_string() });
    }
}

/// Where a rider getting off `body` stands: its left side, then its right,
/// then behind it, then on its back's height where it stands -- the first of
/// those a player fits in.
///
/// **A side and not the middle**, because the middle is the horse, and a
/// player put down inside a horse is a collider pushed out in whichever
/// direction its client happens to pick. Checked against the world with the
/// player's own box, so a rider who got off against a wall is not in it.
fn step_down_from(ctx: &std::sync::Arc<crate::Context>, body: &Mount) -> (f64, f64, f64) {
    use primitive_shared::geometry::{PLAYER_HALF_WIDTH, PLAYER_HEIGHT};
    let block = |x: i32, y: i32, z: i32| ctx.world.cached_block(x, y, z);
    let clear = |x: f64, y: f64, z: f64| {
        let lo = [x - f64::from(PLAYER_HALF_WIDTH), y, z - f64::from(PLAYER_HALF_WIDTH)];
        let hi = [x + f64::from(PLAYER_HALF_WIDTH), y + f64::from(PLAYER_HEIGHT), z + f64::from(PLAYER_HALF_WIDTH)];
        for cx in lo[0].floor() as i32..=hi[0].floor() as i32 {
            for cz in lo[2].floor() as i32..=hi[2].floor() as i32 {
                for cy in lo[1].floor() as i32..=(hi[1] - 1e-6).floor() as i32 {
                    let Some(cell) = block(cx, cy, cz) else { return false };
                    if let Some((bl, bh)) = primitive_shared::geometry::block_box(cell, 0, 0, 0) {
                        let inside = |l: f64, h: f64, a: f32, b: f32, o: i32| l < f64::from(o) + f64::from(b) && h > f64::from(o) + f64::from(a);
                        if inside(lo[0], hi[0], bl[0], bh[0], cx) && inside(lo[1], hi[1], bl[1], bh[1], cy) && inside(lo[2], hi[2], bl[2], bh[2], cz) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    };
    let (sin, cos) = body.yaw.sin_cos();
    let left = (sin, -cos);
    for (dx, dz) in [left, (-left.0, -left.1), (-cos, -sin)] {
        let (x, z) = (body.x + f64::from(dx * STEP_DOWN), body.z + f64::from(dz * STEP_DOWN));
        for rise in [0.0, 1.0] {
            if clear(x, body.y + rise, z) {
                return (x, body.y + rise, z);
            }
        }
    }
    let saddle = body.saddle();
    (saddle[0], saddle[1], saddle[2])
}

/// `ClientMessage::Rein`.
pub(crate) fn rein(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
    forward: f32,
    turn: f32,
    gait: u8,
    jump: bool,
) {
    if lock(&handle.state).riding != Some(id) {
        return;
    }
    let reins = Reins { forward, turn, gait: Gait::from_wire(gait), jump };
    lock(&ctx.animals).rein(id, handle.id, reins);
}

/// A knife on horse `id` (`ClientMessage::TendAnimal`): its saddlebags off
/// into the pack with their load, or its saddle. See `Animals::unbuckle` for
/// why the bags go first and why a load that will not fit keeps them on.
///
/// **The pack is tried on a copy**, and the copy kept only if every stack
/// went in: `Inventory::add` answers what did not fit, and a load that half
/// fitted would be half a load left in bags that are no longer anywhere.
pub(crate) fn unbuckle(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
) {
    use primitive_shared::types::{BLOCK_SADDLE, BLOCK_SADDLEBAGS};
    let eye = {
        let state = lock(&handle.state);
        if state.vitals.is_dead() || state.riding.is_some() {
            return;
        }
        let feet = primitive_shared::geometry::narrow(state.position);
        (feet.0, feet.1 + primitive_shared::geometry::EYE_HEIGHT, feet.2)
    };
    // The pack as it would be with the bags in it: worked out under the
    // animals' lock, which is the order `with_open_bags` takes the two in.
    let mut packed = None;
    let unbuckled = lock(&ctx.animals).unbuckle(id, eye, |bags| {
        let mut pack = lock(&handle.state).inventory.clone();
        let fits = pack.add(BLOCK_SADDLEBAGS, 1) == 0
            && bags.slots().iter().flatten().all(|stack| pack.add_worn(stack.block, stack.count, stack.damage) == 0);
        if fits {
            packed = Some(pack);
        }
        fits
    });
    match unbuckled {
        Unbuckled::Refused(what) => {
            handle.send(ServerMessage::Notice { what });
            return;
        }
        Unbuckled::Bags(_) => {
            let mut state = lock(&handle.state);
            if let Some(pack) = packed {
                state.inventory = pack;
            }
            state.inventory_dirty = true;
        }
        Unbuckled::Saddle => {
            let mut state = lock(&handle.state);
            let left = state.inventory.add(BLOCK_SADDLE, 1);
            state.inventory_dirty = true;
            drop(state);
            // A full pack: the saddle goes down at the player's feet rather
            // than nowhere. It was never refused for want of room -- a
            // saddle is one thing, and the ground takes it.
            if left > 0 {
                let at = lock(&handle.state).position;
                lock(&ctx.items).spawn(BLOCK_SADDLE, left, (at.0, at.1 + 0.5, at.2), (0.0, 0.0, 0.0), None, Instant::now());
            }
        }
    }
    crate::send_inventory(handle);
}

/// `ClientMessage::OpenBags`: the horse's saddlebags, as a container.
pub(crate) fn open_bags(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
) {
    let eye = {
        let state = lock(&handle.state);
        if state.vitals.is_dead() || state.riding.is_some() {
            return;
        }
        primitive_shared::geometry::narrow(state.position)
    };
    let contents = lock(&ctx.animals).bags_within(id, eye, BAGS_REACH).map(|bags| bags.clone());
    let Some(inventory) = contents else {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NoBagsInReach });
        return;
    };
    let cell = bags_cell(id);
    {
        let mut state = lock(&handle.state);
        state.open_chest = None;
        state.open_bags = Some(id);
    }
    handle.send(ServerMessage::ChestState {
        global_x: cell.0,
        global_y: cell.1,
        global_z: cell.2,
        inventory,
        kind: primitive_shared::protocol::ContainerKind::Saddlebags,
        hearth: None,
        rack: None,
    });
}

/// The place a `ChestState` for a horse's bags names.
///
/// **Not where the horse is**, because the client keys its open screen by
/// the place (`ChestScreen::show`): a horse that shifted its weight into the
/// next cell between two gestures would be "a different chest", and the
/// stack in the player's hand would be dropped back. So the place is made
/// from the horse's id, down where no chest can ever be -- a column under
/// the bottom of the world -- and it is the same for as long as the bags are.
fn bags_cell(id: EntityId) -> (i32, i32, i32) {
    // The low bits are the animals' own count (`protocol::entity_id`).
    let ordinal = (id & ((1u64 << 40) - 1)) as i64;
    ((ordinal % 1_000_000) as i32, -1_000, (ordinal / 1_000_000 % 1_000_000) as i32)
}

/// How far from a horse its bags can be reached into, in blocks from the
/// feet: an arm's length past the chest's reach, because a horse is a body
/// that shifts its weight and a player at its flank is at its bags.
pub(crate) const BAGS_REACH: f32 = 3.0;

/// A gesture at the open saddlebags, if this player has some open: `edit` gets
/// the player's pack and the bags, and answers whether anything moved.
///
/// **`None` when no bags are open**, which is what lets every chest gesture ask
/// this first and fall through to the chest when it is a chest. A horse that
/// has walked off or died shuts the screen, the way a broken chest does.
pub(crate) fn with_open_bags(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    edit: impl FnOnce(&mut primitive_shared::inventory::Inventory, &mut primitive_shared::inventory::Inventory) -> bool,
) -> Option<()> {
    let (id, feet) = {
        let state = lock(&handle.state);
        (state.open_bags?, primitive_shared::geometry::narrow(state.position))
    };
    let moved = {
        let mut animals = lock(&ctx.animals);
        let Some(bags) = animals.bags_within(id, feet, BAGS_REACH + 1.0) else {
            drop(animals);
            lock(&handle.state).open_bags = None;
            handle.send(ServerMessage::ChestClosed);
            return Some(());
        };
        let mut state = lock(&handle.state);
        let moved = edit(&mut state.inventory, bags);
        if moved {
            state.inventory_dirty = true;
        }
        moved.then(|| bags.clone())
    };
    if let Some(inventory) = moved {
        crate::send_inventory(handle);
        let cell = bags_cell(id);
        // Everybody at these bags sees the change: two players unloading one
        // horse are two players at one chest.
        for other in ctx.registry.handles() {
            if lock(&other.state).open_bags == Some(id) {
                other.send(ServerMessage::ChestState {
                    global_x: cell.0,
                    global_y: cell.1,
                    global_z: cell.2,
                    inventory: inventory.clone(),
                    kind: primitive_shared::protocol::ContainerKind::Saddlebags,
                    hearth: None,
                    rack: None,
                });
            }
        }
    }
    Some(())
}

/// Puts every rider on their horse's saddle, and gets anybody off whose horse
/// is gone. **Called after the animals have moved and before the snapshots**,
/// for `rafts::tick`'s reason. Answers how far each rider was carried, which
/// the tick loop takes off what it bills as walking.
pub(crate) fn tick(
    ctx: &std::sync::Arc<crate::Context>,
    handles: &[std::sync::Arc<crate::players::PlayerHandle>],
) -> Vec<(primitive_shared::protocol::PlayerId, (f32, f32, f32))> {
    let riders = lock(&ctx.animals).riders();
    let mut carried = Vec::new();
    let now = Instant::now();
    for handle in handles {
        let Some(id) = lock(&handle.state).riding else {
            continue;
        };
        let on = riders.iter().find(|(rider, horse, _)| *rider == handle.id && *horse == id).map(|r| r.2);
        let dead = lock(&handle.state).vitals.is_dead();
        let Some(body) = on.filter(|_| !dead) else {
            // The horse died under them, or was forgotten, or they did.
            dismount(ctx, handle, None);
            if !dead {
                tell(handle, Notice::HorseGone);
            }
            continue;
        };
        let saddle = body.saddle();
        let tell_wind = {
            let mut state = lock(&handle.state);
            let before = state.position;
            state.position = (saddle[0], saddle[1], saddle[2]);
            state.on_ground = body.on_ground;
            state.vitals.clear_fall();
            carried.push((
                handle.id,
                primitive_shared::geometry::narrow((saddle[0] - before.0, saddle[1] - before.1, saddle[2] - before.2)),
            ));
            let due = state.mount_told.is_none_or(|at| now.saturating_duration_since(at) >= TELL_WIND_EVERY);
            if due {
                state.mount_told = Some(now);
            }
            due
        };
        if tell_wind {
            if let Some((_, body, fettle)) = lock(&ctx.animals).ridden_by(handle.id) {
                handle.send(ServerMessage::Mounted { horse: Some(id), at: (body.x, body.y, body.z), yaw: body.yaw, wind: body.wind, fettle });
            }
        }
    }
    carried
}

/// Throws down what dead horses left: saddle, bags and load (`Gear::left_behind`).
pub(crate) fn spill(ctx: &std::sync::Arc<crate::Context>) {
    let spilled = lock(&ctx.animals).take_spilled();
    if spilled.is_empty() {
        return;
    }
    let now = Instant::now();
    let mut items = lock(&ctx.items);
    for (at, left) in spilled {
        for (index, (block, count)) in left.into_iter().enumerate() {
            // Thrown apart, like the raft's timber, so a horse's load reads
            // as a horse's load spilled and not as one heap.
            let (sin, cos) = (index as f32 * 1.7).sin_cos();
            items.spawn(block, count, (at.0, at.1 + 0.8, at.2), (cos * 1.5, 2.5, sin * 1.5), None, now);
        }
    }
}

/// A player left the server: off whatever they were riding.
pub(crate) fn forget(ctx: &std::sync::Arc<crate::Context>, player: primitive_shared::protocol::PlayerId) {
    lock(&ctx.animals).forget_rider(player);
}
