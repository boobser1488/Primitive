//! The host side of every mod API call.
//!
//! One `extern "C"` function per entry in every table in
//! [`primitive_modapi`]. They are all the same shape:
//!
//! 1. Turn the opaque handle back into the server's context, refusing if
//!    it is null.
//! 2. Take whatever lock the answer needs, **for the length of the
//!    answer and no longer**.
//! 3. Write through the out-pointer, having checked it is not null.
//! 4. Return a [`Status`].
//!
//! ## The three rules
//!
//! **Never panic across the boundary.** A panic unwinding into a mod's
//! stack frame is undefined behaviour, and the release profile aborts on
//! panic anyway -- so a bad argument is a `Status`, never an `expect`.
//! Every indexing operation here is a `get`, every division is guarded,
//! and every float that came from a mod is checked with `is_finite`.
//!
//! **Never trust a pointer.** Out-pointers are checked for null before
//! they are written through. There is nothing to be done about a mod
//! that hands over a pointer to freed memory, but null is the mistake
//! people actually make.
//!
//! **Never hold a lock across a call back into a mod.** Nothing here
//! calls into a mod at all, which is what makes that easy: the only path
//! that does is [`super::ModHost::dispatch`], and it takes no locks.
//!
//! ## Why the world is read and not generated
//!
//! [`WorldApi::get_block`] answers `NotFound` for a chunk nobody has
//! loaded rather than generating it. That is the same rule the
//! anti-cheat's ground check follows and it is there for the same
//! reason: a call that could make the server generate terrain by asking
//! about it is a denial of service with a pleasant interface.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use primitive_modapi::*;

use super::mods::HostContext;
use crate::Context;

/// The context behind a handle, or `None` for a null one.
///
/// # Safety
/// The handle must be the one the host put in `HostApi::handle`, which
/// points at a `HostContext` the host keeps alive for as long as any mod
/// is loaded.
unsafe fn ctx<'a>(handle: HostHandle) -> Option<&'a Arc<Context>> {
    if handle.is_null() {
        return None;
    }
    Some(&(*(handle as *const HostContext)).0)
}

/// Writes a string into a mod's buffer and says how long it wanted to
/// be.
///
/// The convention every text-returning call here uses: pass a `cap` of
/// zero to ask for the length, then call again with a buffer. Answers
/// `Ok` when the whole thing fitted and `BadArgument` when it did not,
/// with `written` set to the length either way -- so a mod that gets
/// `BadArgument` knows exactly how much to allocate.
unsafe fn write_str(text: &str, out: *mut u8, cap: usize, written: *mut usize) -> Status {
    if !written.is_null() {
        *written = text.len();
    }
    if cap == 0 || out.is_null() {
        return if text.len() <= cap { Status::Ok } else { Status::BadArgument };
    }
    if text.len() > cap {
        return Status::BadArgument;
    }
    std::ptr::copy_nonoverlapping(text.as_ptr(), out, text.len());
    Status::Ok
}

/// A position from a mod, refused if it is not a number.
///
/// `f32::NAN as i32` is zero, so an unchecked NaN is not a crash -- it
/// is a silent write at the origin, which is worse.
fn finite(v: Vec3) -> Option<(f32, f32, f32)> {
    (v.x.is_finite() && v.y.is_finite() && v.z.is_finite()).then_some((v.x, v.y, v.z))
}

// ---------------------------------------------------------------- core

pub fn core_table() -> CoreApi {
    CoreApi {
        log,
        tick,
        time_of_day,
        set_time_of_day,
        day_length_seconds,
        tick_rate_hz,
        setting,
        run_command,
    }
}

unsafe extern "C" fn log(handle: HostHandle, level: LogLevel, message: Str) {
    let Some(_context) = ctx(handle) else { return };
    // **Not gated on `options.logging`, and it used to be.**
    //
    // That gate is about the server's own chatter -- the banner, the
    // stats line -- which a game client is quiet about because its
    // stdout belongs to the player. A mod's line is not that. Nothing
    // reaches here unless a mod is loaded, and a mod is loaded only
    // because somebody put a folder in `mods/` on purpose; swallowing
    // what it says leaves them with a mod that does nothing and no way
    // to find out why. That was invisible while singleplayer loaded no
    // mods at all, and became the first thing anybody hit when it
    // started.
    let text = message.as_str();
    match level {
        LogLevel::Error | LogLevel::Warn => eprintln!("[mod] {text}"),
        _ => println!("[mod] {text}"),
    }
}

unsafe extern "C" fn tick(handle: HostHandle) -> u64 {
    ctx(handle).map(|c| c.clock.tick()).unwrap_or(0)
}

unsafe extern "C" fn time_of_day(handle: HostHandle) -> f32 {
    ctx(handle).map(|c| c.clock.time_of_day()).unwrap_or(0.0)
}

unsafe extern "C" fn set_time_of_day(handle: HostHandle, t: f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !t.is_finite() {
        return Status::BadArgument;
    }
    context.clock.set_time_of_day(t);
    crate::time_changed(context, t);
    let now = context.clock.tick();
    context
        .registry
        .broadcast(primitive_shared::protocol::ServerMessage::TimeSync {
            tick: now,
            time_of_day: t,
            world_days: context.clock.world_days(),
        });
    Status::Ok
}

unsafe extern "C" fn day_length_seconds(handle: HostHandle) -> f32 {
    ctx(handle)
        .map(|c| c.clock.day_length_seconds())
        .unwrap_or(0.0)
}

unsafe extern "C" fn tick_rate_hz(handle: HostHandle) -> f32 {
    ctx(handle).map(|c| c.settings.tick_rate_hz).unwrap_or(0.0)
}

unsafe extern "C" fn setting(
    handle: HostHandle,
    key: Str,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let wanted = key.as_str();
    let host = context.mods.lock().unwrap_or_else(|e| e.into_inner());
    match host.setting(wanted) {
        Some(value) => write_str(&value, out, cap, written),
        None => {
            if !written.is_null() {
                *written = 0;
            }
            Status::NotFound
        }
    }
}

unsafe extern "C" fn run_command(handle: HostHandle, line: Str) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let text = line.as_str();
    if text.trim().is_empty() {
        return Status::BadArgument;
    }
    // At operator permission, as the console runs it. A mod is native
    // code in the server's own process; pretending it has less authority
    // than the person who installed it would be theatre.
    crate::run_command(
        context,
        text,
        crate::commands::Permission::Operator,
        None,
    );
    Status::Ok
}

// --------------------------------------------------------------- world

pub fn world_table() -> WorldApi {
    WorldApi {
        get_block,
        set_block,
        fill,
        world_seed,
        spawn_point,
        is_chunk_loaded,
        request_chunk,
        loaded_chunk_count,
        weather,
        set_weather,
        temperature_at,
        break_block,
        surface_at,
        disturb,
    }
}

unsafe extern "C" fn get_block(handle: HostHandle, at: BlockPos, out: *mut BlockId) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    // Cached only. See the module note.
    match context.world.cached_block(at.x, at.y, at.z) {
        Some(block) => {
            *out = block;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn set_block(handle: HostHandle, at: BlockPos, block: BlockId) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !primitive_shared::types::is_known_block(block) {
        return Status::BadArgument;
    }
    if !place_one(context, at, block) {
        return Status::BadArgument;
    }
    Status::Ok
}

/// One block, through every path a player's edit takes.
///
/// Shared by `set_block` and `fill` so that a mod's edit is announced,
/// noticed by the falling sand and the water, and seen by the fires --
/// which is the difference between a mod that changes the world and one
/// that changes an array.
fn place_one(context: &Arc<Context>, at: BlockPos, block: BlockId) -> bool {
    if !context.world.set_block(at.x, at.y, at.z, block) {
        return false;
    }
    context.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    {
        let mut sim = context.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.on_block_changed(at.x, at.y, at.z);
    }
    crate::notify_mechanics(context, at.x, at.y, at.z);
    // Through the host's own announce path rather than a subscriber loop
    // of its own, so that a mod's edit reaches other mods as
    // `Event::BlockChanged` on exactly the terms a player's edit does.
    crate::broadcast_block(context, (at.x, at.y, at.z), block);
    true
}

/// The most cells one `fill` may write.
///
/// A mod asking for a million-block region would hold whatever thread it
/// is on for as long as it took, and on the tick loop that is every
/// player's snapshot. The cap turns "the server froze" into "the mod got
/// a smaller box than it asked for and was told so" -- the same
/// reasoning, and roughly the same number, as the scripted plugins'
/// `MAX_FILL_BLOCKS`.
const MAX_FILL: u32 = 4096;

unsafe extern "C" fn fill(
    handle: HostHandle,
    from: BlockPos,
    to: BlockPos,
    block: BlockId,
    written: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !primitive_shared::types::is_known_block(block) {
        return Status::BadArgument;
    }
    let (x0, x1) = (from.x.min(to.x), from.x.max(to.x));
    let (y0, y1) = (from.y.min(to.y), from.y.max(to.y));
    let (z0, z1) = (from.z.min(to.z), from.z.max(to.z));
    let mut count = 0u32;
    let mut truncated = false;
    'outer: for y in y0..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                if count >= MAX_FILL {
                    truncated = true;
                    break 'outer;
                }
                if place_one(context, BlockPos { x, y, z }, block) {
                    count += 1;
                }
            }
        }
    }
    if !written.is_null() {
        *written = count;
    }
    // Said rather than done silently: a mod that asked for a bigger box
    // than it got has to be able to find out, or the bug is a hole in
    // somebody's build.
    if truncated {
        Status::Refused
    } else {
        Status::Ok
    }
}

unsafe extern "C" fn world_seed(handle: HostHandle) -> u32 {
    ctx(handle).map(|c| c.world.seed()).unwrap_or(0)
}

unsafe extern "C" fn spawn_point(handle: HostHandle) -> Vec3 {
    match ctx(handle) {
        Some(context) => {
            let (x, y, z) = context.world.spawn_point();
            Vec3 { x, y, z }
        }
        None => Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    }
}

unsafe extern "C" fn is_chunk_loaded(handle: HostHandle, pos: ChunkPos) -> bool {
    let Some(context) = ctx(handle) else {
        return false;
    };
    context
        .world
        .cached(primitive_shared::types::ChunkPos::new(pos.x, pos.z))
        .is_some()
}

unsafe extern "C" fn request_chunk(handle: HostHandle, pos: ChunkPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    context
        .chunks
        .request(primitive_shared::types::ChunkPos::new(pos.x, pos.z), 0);
    Status::Ok
}

unsafe extern "C" fn loaded_chunk_count(handle: HostHandle) -> u32 {
    ctx(handle)
        .map(|c| c.world.stats().cached_chunks as u32)
        .unwrap_or(0)
}

unsafe extern "C" fn weather(handle: HostHandle) -> Weather {
    let Some(context) = ctx(handle) else {
        return Weather::Clear;
    };
    let sky = context.sky.lock().unwrap_or_else(|e| e.into_inner());
    match sky.weather() {
        primitive_shared::weather::Weather::Clear => Weather::Clear,
        primitive_shared::weather::Weather::Rain => Weather::Rain,
        primitive_shared::weather::Weather::Storm => Weather::Storm,
    }
}

unsafe extern "C" fn set_weather(handle: HostHandle, wanted: Weather) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let wanted = match wanted {
        Weather::Clear => primitive_shared::weather::Weather::Clear,
        Weather::Rain => primitive_shared::weather::Weather::Rain,
        Weather::Storm => primitive_shared::weather::Weather::Storm,
    };
    let changed = {
        let mut sky = context.sky.lock().unwrap_or_else(|e| e.into_inner());
        sky.set(wanted)
    };
    if changed {
        context
            .fires
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_weather(wanted);
        crate::weather_changed(context, wanted);
        context
            .registry
            .broadcast(primitive_shared::protocol::ServerMessage::WeatherSync { weather: wanted });
    }
    Status::Ok
}

unsafe extern "C" fn temperature_at(handle: HostHandle, at: Vec3, out: *mut f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(position) = finite(at) else {
        return Status::BadArgument;
    };
    let sky = {
        let sky = context.sky.lock().unwrap_or_else(|e| e.into_inner());
        sky.weather()
    };
    let ambient = {
        let fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
        crate::climate::Ambient::of(
            &context.world,
            &fires,
            position,
            context.clock.world_days(),
            sky,
        )
    };
    *out = ambient.temperature_c;
    Status::Ok
}

// ---------------------------------------------------------- generation

pub fn generation_table() -> GenerationApi {
    GenerationApi {
        height_at,
        biome_at,
        biome_name,
        climate_at,
        register_decorator,
    }
}

unsafe extern "C" fn height_at(handle: HostHandle, x: i32, z: i32) -> i32 {
    ctx(handle)
        .map(|c| c.world.height_at(x, z))
        .unwrap_or(0)
}

unsafe extern "C" fn biome_at(handle: HostHandle, x: i32, z: i32) -> u32 {
    ctx(handle)
        .map(|c| c.world.biome_at(x, z) as u32)
        .unwrap_or(0)
}

unsafe extern "C" fn biome_name(
    handle: HostHandle,
    biome: u32,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    if ctx(handle).is_none() {
        return Status::BadArgument;
    }
    let Some(name) = primitive_shared::worldgen::Biome::ALL
        .get(biome as usize)
        .map(|b| b.name())
    else {
        if !written.is_null() {
            *written = 0;
        }
        return Status::NotFound;
    };
    write_str(name, out, cap, written)
}

unsafe extern "C" fn climate_at(
    handle: HostHandle,
    at: BlockPos,
    warmth: *mut f32,
    humidity: *mut f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if warmth.is_null() || humidity.is_null() {
        return Status::BadArgument;
    }
    let (w, h) = context.world.climate_at(at.x, at.y, at.z);
    *warmth = w;
    *humidity = h;
    Status::Ok
}

unsafe extern "C" fn register_decorator(handle: HostHandle, decorator: ChunkDecorator) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let mut host = context.mods.lock().unwrap_or_else(|e| e.into_inner());
    host.add_decorator(decorator);
    Status::Ok
}

// -------------------------------------------------------------- blocks

pub fn blocks_table() -> BlocksApi {
    BlocksApi {
        count: block_count,
        id_at,
        name: block_name,
        by_name,
        properties,
        tooling,
        break_seconds,
        same_kind,
    }
}

unsafe extern "C" fn block_count(_handle: HostHandle) -> u32 {
    primitive_shared::types::ALL_BLOCK_IDS.len() as u32
}

unsafe extern "C" fn id_at(_handle: HostHandle, index: u32, out: *mut BlockId) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    match primitive_shared::types::ALL_BLOCK_IDS.get(index as usize) {
        Some(&(id, _)) => {
            *out = id;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn block_name(
    _handle: HostHandle,
    block: BlockId,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    if !primitive_shared::types::is_known_block(block) {
        if !written.is_null() {
            *written = 0;
        }
        return Status::NotFound;
    }
    write_str(primitive_shared::types::block_name(block), out, cap, written)
}

unsafe extern "C" fn by_name(_handle: HostHandle, name: Str, out: *mut BlockId) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    let wanted = name.as_str();
    match primitive_shared::types::ALL_BLOCK_IDS
        .iter()
        .find(|&&(_, n)| n == wanted)
    {
        Some(&(id, _)) => {
            *out = id;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn properties(
    _handle: HostHandle,
    block: BlockId,
    out: *mut BlockProperties,
) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    if !primitive_shared::types::is_known_block(block) {
        return Status::NotFound;
    }
    let def = primitive_shared::blocks::definition(block);
    *out = BlockProperties {
        id: def.id,
        hardness: def.hardness.unwrap_or(-1.0),
        opacity: def.opacity,
        emission: def.emission,
        solid: primitive_shared::types::is_collidable(block),
        placeable: def.placeable,
        falls: def.falls,
        container: def.container,
        weight_kg: def.weight,
        // Through `types::stack_limit` rather than off the row, because
        // that function is where the rule actually lives and one block
        // (the jug) has a limit the table does not state -- see the
        // note there. A mod told a jug stacks to four would build a
        // recipe that produces four of them into one slot, and three
        // sets of contents would stop existing.
        stack_limit: primitive_shared::types::stack_limit(block),
        drops: def.drop.unwrap_or(0),
    };
    Status::Ok
}

// --------------------------------------------------------------- items

pub fn items_table() -> ItemsApi {
    ItemsApi {
        spawn: item_spawn,
        remove: item_remove,
        count: item_count,
        near: items_near,
        spawn_worn: item_spawn_worn,
        clear_near,
        lifetime_seconds: item_lifetime_seconds,
        capacity: item_capacity,
    }
}

unsafe extern "C" fn item_spawn(
    handle: HostHandle,
    at: Vec3,
    velocity: Vec3,
    stack: ItemStack,
    out: *mut EntityId,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(position) = finite(at) else {
        return Status::BadArgument;
    };
    let motion = finite(velocity).unwrap_or((0.0, 0.0, 0.0));
    if stack.count == 0 || !primitive_shared::types::is_known_block(stack.block) {
        return Status::BadArgument;
    }
    let spawned = {
        let mut items = context.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn(
            stack.block,
            stack.count,
            primitive_shared::geometry::wide(position),
            motion,
            None,
            std::time::Instant::now(),
        )
    };
    if !out.is_null() {
        // The item store hands back whether it took the stack rather
        // than an id, so there is no id to report. Zero, which the
        // contract says is never a valid one.
        *out = 0;
    }
    if spawned {
        Status::Ok
    } else {
        // The world is at its item cap, which is a refusal rather than a
        // failure -- and one a mod dropping loot in a loop needs to be
        // told about.
        Status::Refused
    }
}

unsafe extern "C" fn item_remove(_handle: HostHandle, _id: EntityId) -> Status {
    // The item store is keyed by position and merge rather than by a
    // stable id, so there is nothing here to name. Reported honestly
    // rather than as a silent success: a mod that thinks it cleaned up
    // and did not is worse than one that knows it cannot.
    Status::Unavailable
}

unsafe extern "C" fn item_count(handle: HostHandle) -> u32 {
    let Some(context) = ctx(handle) else { return 0 };
    let items = context.items.lock().unwrap_or_else(|e| e.into_inner());
    items.len() as u32
}

unsafe extern "C" fn items_near(
    handle: HostHandle,
    at: Vec3,
    radius: f32,
    out: *mut ItemStack,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(position) = finite(at) else {
        return Status::BadArgument;
    };
    if !radius.is_finite() || radius < 0.0 {
        return Status::BadArgument;
    }
    let found: Vec<ItemStack> = {
        let items = context.items.lock().unwrap_or_else(|e| e.into_inner());
        items
            .states()
            .into_iter()
            .filter_map(|state| {
                let primitive_shared::protocol::EntityKind::Item { block, count } = state.kind
                else {
                    return None;
                };
                let (dx, dy, dz) = (
                    (state.x - f64::from(position.0)) as f32,
                    (state.y - f64::from(position.1)) as f32,
                    (state.z - f64::from(position.2)) as f32,
                );
                (dx * dx + dy * dy + dz * dz <= radius * radius).then_some(ItemStack {
                    block,
                    count,
                    damage: 0,
                })
            })
            .collect()
    };
    if !written.is_null() {
        *written = found.len();
    }
    if out.is_null() || cap == 0 {
        return Status::Ok;
    }
    for (i, stack) in found.iter().take(cap).enumerate() {
        *out.add(i) = *stack;
    }
    Status::Ok
}

// ------------------------------------------------------------ entities

pub fn entities_table() -> EntitiesApi {
    EntitiesApi {
        count: entity_count,
        position: entity_position,
        health: entity_health,
        damage: entity_damage,
        species: entity_species,
        species_name,
        spawn: entity_spawn,
        remove: entity_remove,
        near: entities_near,
        species_count,
        species_info,
        species_drop,
        heal: entity_heal,
        all: entities_all,
        kill: entity_kill,
    }
}

unsafe extern "C" fn entity_count(handle: HostHandle) -> u32 {
    let Some(context) = ctx(handle) else { return 0 };
    let animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
    animals.len() as u32
}

unsafe extern "C" fn entity_position(
    handle: HostHandle,
    id: EntityId,
    out: *mut Vec3,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
    match animals.position(id) {
        Some((x, y, z)) => {
            *out = Vec3 { x, y, z };
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn entity_health(handle: HostHandle, id: EntityId, out: *mut f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
    match animals.health(id) {
        Some(health) => {
            *out = health;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn entity_damage(handle: HostHandle, id: EntityId, amount: f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !amount.is_finite() || amount < 0.0 {
        return Status::BadArgument;
    }
    // What is left of it, where it fell, is laid by the tick loop when the
    // body has finished going down (`animals::FALL_SECONDS`) -- the same
    // answer a player's blow gets, from the same place. Laying it here as
    // well was two carcasses.
    let _ = context.animals.lock().unwrap_or_else(|e| e.into_inner()).hurt(id, amount);
    Status::Ok
}

unsafe extern "C" fn entity_species(handle: HostHandle, id: EntityId, out: *mut u32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
    match animals.species_index(id) {
        Some(index) => {
            *out = index;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn species_name(
    _handle: HostHandle,
    species: u32,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    match primitive_shared::animals::Species::ALL.get(species as usize) {
        Some(s) => write_str(s.name(), out, cap, written),
        None => {
            if !written.is_null() {
                *written = 0;
            }
            Status::NotFound
        }
    }
}

unsafe extern "C" fn entity_spawn(
    handle: HostHandle,
    species: u32,
    at: Vec3,
    out: *mut EntityId,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(position) = finite(at) else {
        return Status::BadArgument;
    };
    let Some(&kind) = primitive_shared::animals::Species::ALL.get(species as usize) else {
        return Status::NotFound;
    };
    let id = {
        let mut animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.spawn_at(kind, position)
    };
    if !out.is_null() {
        *out = id.unwrap_or(0);
    }
    if id.is_some() {
        Status::Ok
    } else {
        Status::Refused
    }
}

unsafe extern "C" fn entity_remove(handle: HostHandle, id: EntityId) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let mut animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
    if animals.forget(id) {
        Status::Ok
    } else {
        Status::NotFound
    }
}

unsafe extern "C" fn entities_near(
    handle: HostHandle,
    at: Vec3,
    radius: f32,
    out: *mut EntityId,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(position) = finite(at) else {
        return Status::BadArgument;
    };
    if !radius.is_finite() || radius < 0.0 {
        return Status::BadArgument;
    }
    let found: Vec<EntityId> = {
        let animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.within(position, radius)
    };
    if !written.is_null() {
        *written = found.len();
    }
    if out.is_null() || cap == 0 {
        return Status::Ok;
    }
    for (i, id) in found.iter().take(cap).enumerate() {
        *out.add(i) = *id;
    }
    Status::Ok
}

// ------------------------------------------------------------- players

pub fn players_table() -> PlayersApi {
    PlayersApi {
        count: player_count,
        all: players_all,
        name: player_name,
        position: player_position,
        teleport: player_teleport,
        vitals: player_vitals,
        damage: player_damage,
        heal: player_heal,
        feed: player_feed,
        water: player_water,
        kick: player_kick,
        is_operator: player_is_operator,
        set_flying: player_set_flying,
        is_flying: player_is_flying,
        look: player_look,
        on_ground: player_on_ground,
        selected_slot: player_selected_slot,
        set_selected_slot: player_set_selected_slot,
        respawn: player_respawn,
        set_health: player_set_health,
        set_warmth: player_set_warmth,
        is_submerged: player_is_submerged,
    }
}

unsafe extern "C" fn player_count(handle: HostHandle) -> u32 {
    ctx(handle).map(|c| c.registry.len() as u32).unwrap_or(0)
}

unsafe extern "C" fn players_all(
    handle: HostHandle,
    out: *mut PlayerId,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let ids: Vec<PlayerId> = context.registry.handles().iter().map(|h| h.id).collect();
    if !written.is_null() {
        *written = ids.len();
    }
    if out.is_null() || cap == 0 {
        return Status::Ok;
    }
    for (i, id) in ids.iter().take(cap).enumerate() {
        *out.add(i) = *id;
    }
    Status::Ok
}

unsafe extern "C" fn player_name(
    handle: HostHandle,
    player: PlayerId,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        if !written.is_null() {
            *written = 0;
        }
        return Status::NotFound;
    };
    write_str(&h.username, out, cap, written)
}

unsafe extern "C" fn player_position(
    handle: HostHandle,
    player: PlayerId,
    out: *mut Vec3,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = Vec3 {
        x: state.position.0 as f32,
        y: state.position.1 as f32,
        z: state.position.2 as f32,
    };
    Status::Ok
}

unsafe extern "C" fn player_teleport(handle: HostHandle, player: PlayerId, to: Vec3) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some((x, y, z)) = finite(to) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    crate::teleport(&h, x, y, z, "moved by a mod");
    Status::Ok
}

unsafe extern "C" fn player_set_flying(
    handle: HostHandle,
    player: PlayerId,
    enabled: bool,
    speed: f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    // A speed that is not a number is a mod bug rather than a request,
    // and the host has a sane answer for it -- see `set_flight`, which
    // falls back to the default rather than refusing.
    crate::set_flight(&h, enabled, speed);
    Status::Ok
}

unsafe extern "C" fn player_is_flying(handle: HostHandle, player: PlayerId) -> bool {
    let Some(context) = ctx(handle) else {
        return false;
    };
    let Some(h) = context.registry.get(player) else {
        return false;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    state.flying
}

unsafe extern "C" fn player_vitals(
    handle: HostHandle,
    player: PlayerId,
    out: *mut PlayerVitals,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = PlayerVitals {
        health: state.vitals.health(),
        max_health: crate::survival::MAX_HEALTH,
        nourishment: state.vitals.nourishment_fraction(),
        hydration: state.vitals.hydration_fraction(),
        breath: state.vitals.breath_fraction(),
        body_temperature_c: state.vitals.temperature(),
        ambient_c: state.ambient.temperature_c,
        wetness: state.vitals.wetness(),
        carried_kg: state.vitals.carried_weight(),
        dead: state.vitals.is_dead(),
    };
    Status::Ok
}

unsafe extern "C" fn player_damage(
    handle: HostHandle,
    player: PlayerId,
    amount: f32,
    cause: Str,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !amount.is_finite() || amount < 0.0 {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let why = cause.as_str();
    let why = if why.is_empty() { "was struck down" } else { why };
    // Through the armour path, exactly as a punch is. There is no way
    // here to bypass a cuirass, which is the point: a mod that wanted to
    // would have to say so by asking for more damage.
    //
    // Blunt, because the ABI has no word for what the mod's blow *is*, and
    // a bruise is the wound that asks least of a player who could not see
    // it coming: it fades by itself. Widening the call to name a kind is a
    // change to the mod interface, and belongs with the next one.
    let outcome =
        crate::strike_player(context, &h, amount, why, primitive_shared::injury::Blow::Blunt);
    crate::report_vitals(context, &h, outcome);
    Status::Ok
}

unsafe extern "C" fn player_heal(handle: HostHandle, player: PlayerId, amount: f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !amount.is_finite() || amount < 0.0 {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    // Through the one path that also tells the mods -- the same one the
    // body's own regeneration goes through, so `Event::PlayerHealed`
    // means the same thing whichever of the two moved the number.
    let outcome = crate::heal_player(context, &h, amount);
    crate::report_vitals(context, &h, outcome);
    Status::Ok
}

unsafe extern "C" fn player_feed(
    handle: HostHandle,
    player: PlayerId,
    nourishment: f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !nourishment.is_finite() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = state.vitals.nourishment();
        state.vitals.set_nourishment(now + nourishment);
    }
    crate::send_nourishment(&h);
    Status::Ok
}

unsafe extern "C" fn player_water(handle: HostHandle, player: PlayerId, hydration: f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !hydration.is_finite() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = state.vitals.hydration();
        state.vitals.set_hydration(now + hydration);
    }
    crate::send_body(&h);
    Status::Ok
}

unsafe extern "C" fn player_kick(handle: HostHandle, player: PlayerId, reason: Str) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    h.request_kick(primitive_shared::protocol::DisconnectReason::Other(
        reason.as_str().to_string(),
    ));
    context.metrics.kicks.fetch_add(1, Ordering::Relaxed);
    Status::Ok
}

unsafe extern "C" fn player_is_operator(handle: HostHandle, player: PlayerId) -> bool {
    let Some(context) = ctx(handle) else {
        return false;
    };
    let Some(h) = context.registry.get(player) else {
        return false;
    };
    // Тот же ответ, что даёт разбор команды из чата, и это обязано быть
    // одним ответом: мод, спросивший «оператор ли он», и сервер,
    // решающий, пускать ли его к `/time`, разошлись бы в своём мире —
    // команда прошла бы, а мод отказал. См. `RunOptions::local_operator`.
    if context.options.local_operator {
        return true;
    }
    let Some(uuid) = h.uuid else { return false };
    let profiles = context.profiles.lock().unwrap_or_else(|e| e.into_inner());
    profiles.is_operator(uuid)
}

// ----------------------------------------------------------- inventory

pub fn inventory_table() -> InventoryApi {
    InventoryApi {
        slot_count,
        get_slot,
        set_slot,
        give,
        take,
        count_of,
        get_equipment,
        set_equipment,
        held,
        spill,
        drop_slot,
        used_slots,
    }
}

unsafe extern "C" fn slot_count(_handle: HostHandle) -> u32 {
    primitive_shared::inventory::SLOTS as u32
}

unsafe extern "C" fn get_slot(
    handle: HostHandle,
    player: PlayerId,
    slot: u32,
    out: *mut ItemStack,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    let Some(cell) = state.inventory.slots().get(slot as usize) else {
        return Status::BadArgument;
    };
    *out = match cell {
        Some(stack) => ItemStack {
            block: stack.block,
            count: stack.count,
            damage: stack.damage,
        },
        None => ItemStack::EMPTY,
    };
    Status::Ok
}

unsafe extern "C" fn set_slot(
    handle: HostHandle,
    player: PlayerId,
    slot: u32,
    stack: ItemStack,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    if slot as usize >= primitive_shared::inventory::SLOTS {
        return Status::BadArgument;
    }
    if stack.count > 0 && !primitive_shared::types::is_known_block(stack.block) {
        return Status::BadArgument;
    }
    let displaced = {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        let taken = state.inventory.take_slot(slot as usize);
        if stack.count > 0 {
            state.inventory.put_in_slot(
                slot as usize,
                primitive_shared::inventory::Stack::worn(stack.block, stack.count, stack.damage),
            );
        }
        state.inventory_dirty = true;
        taken
    };
    // **What was there is dropped rather than deleted.** The host will
    // not silently destroy a player's things on a mod's behalf: a mod
    // that meant to take them can say so with `take`.
    if let Some(old) = displaced {
        let position = {
            let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
            state.position
        };
        let mut items = context.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn(
            old.block,
            old.count,
            position,
            (0.0, 0.0, 0.0),
            None,
            std::time::Instant::now(),
        );
    }
    crate::send_inventory(&h);
    crate::refresh_carried_weight(&h);
    Status::Ok
}

unsafe extern "C" fn give(
    handle: HostHandle,
    player: PlayerId,
    stack: ItemStack,
    left_over: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    if stack.count == 0 || !primitive_shared::types::is_known_block(stack.block) {
        return Status::BadArgument;
    }
    let left = {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        let left = state.inventory.add(stack.block, stack.count);
        state.inventory_dirty = true;
        left
    };
    if !left_over.is_null() {
        *left_over = left;
    }
    crate::send_inventory(&h);
    crate::refresh_carried_weight(&h);
    Status::Ok
}

unsafe extern "C" fn take(
    handle: HostHandle,
    player: PlayerId,
    block: BlockId,
    count: u32,
    taken: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let got = {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        let have = state.inventory.count(block).min(count);
        if have > 0 && state.inventory.take_exact(block, have) {
            state.inventory_dirty = true;
            have
        } else {
            0
        }
    };
    if !taken.is_null() {
        *taken = got;
    }
    crate::send_inventory(&h);
    crate::refresh_carried_weight(&h);
    Status::Ok
}

unsafe extern "C" fn count_of(
    handle: HostHandle,
    player: PlayerId,
    block: BlockId,
    out: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = state.inventory.count(block);
    Status::Ok
}

fn body_slot(slot: BodySlot) -> primitive_shared::equipment::Slot {
    match slot {
        BodySlot::Head => primitive_shared::equipment::Slot::Head,
        BodySlot::Chest => primitive_shared::equipment::Slot::Chest,
        BodySlot::Legs => primitive_shared::equipment::Slot::Legs,
        BodySlot::Feet => primitive_shared::equipment::Slot::Feet,
    }
}

unsafe extern "C" fn get_equipment(
    handle: HostHandle,
    player: PlayerId,
    slot: BodySlot,
    out: *mut ItemStack,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = match state.equipment.in_slot(body_slot(slot)) {
        Some(stack) => ItemStack {
            block: stack.block,
            count: 1,
            damage: stack.damage,
        },
        None => ItemStack::EMPTY,
    };
    Status::Ok
}

unsafe extern "C" fn set_equipment(
    handle: HostHandle,
    player: PlayerId,
    slot: BodySlot,
    stack: ItemStack,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let wanted = body_slot(slot);
    // **The host decides what fits where, not the mod.** A garment goes
    // in its own slot and nowhere else -- that is a fact about the
    // garment (`equipment::slot_of`) -- so a mod asking to put boots on
    // a head is refused rather than obeyed.
    if stack.count > 0
        && primitive_shared::equipment::slot_of(stack.block) != Some(wanted)
    {
        return Status::BadArgument;
    }
    let displaced = {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        let old = state.equipment.take(wanted);
        if stack.count > 0 {
            state
                .equipment
                .wear(primitive_shared::inventory::Stack::worn(
                    stack.block,
                    1,
                    stack.damage,
                ));
        }
        state.equipment_dirty = true;
        old
    };
    if let Some(old) = displaced {
        let position = {
            let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
            state.position
        };
        let mut items = context.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn(
            old.block,
            1,
            position,
            (0.0, 0.0, 0.0),
            None,
            std::time::Instant::now(),
        );
    }
    crate::send_equipment(&h);
    crate::refresh_carried_weight(&h);
    Status::Ok
}

// ------------------------------------------------------------- network

pub fn network_table() -> NetworkApi {
    NetworkApi {
        broadcast,
        tell,
        send_to,
        broadcast_data,
    }
}

unsafe extern "C" fn broadcast(handle: HostHandle, text: Str) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    context
        .registry
        .broadcast(primitive_shared::protocol::ServerMessage::Chat {
            from: None,
            username: "server".to_string(),
            text: primitive_shared::protocol::sanitize_chat(text.as_str()),
        });
    Status::Ok
}

unsafe extern "C" fn tell(handle: HostHandle, player: PlayerId, text: Str) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    h.send(primitive_shared::protocol::ServerMessage::Chat {
        from: None,
        username: "server".to_string(),
        text: primitive_shared::protocol::sanitize_chat(text.as_str()),
    });
    Status::Ok
}

unsafe extern "C" fn send_to(
    handle: HostHandle,
    _player: PlayerId,
    _channel: Str,
    _data: *const u8,
    _len: usize,
) -> Status {
    // **Honestly unavailable.** The protocol has no mod-channel message
    // yet, and inventing one that the stock client silently drops would
    // be a call that reports success and does nothing -- which is how a
    // mod author spends an afternoon debugging their own code. When a
    // channel message is added this becomes real without the signature
    // moving, which is what the table's shape is for.
    let _ = ctx(handle);
    Status::Unavailable
}

unsafe extern "C" fn broadcast_data(
    handle: HostHandle,
    _channel: Str,
    _data: *const u8,
    _len: usize,
) -> Status {
    let _ = ctx(handle);
    Status::Unavailable
}

// -------------------------------------------------------------- events

pub fn events_table() -> EventsApi {
    EventsApi {
        subscribe,
        unsubscribe,
        register_command,
    }
}

unsafe extern "C" fn subscribe(handle: HostHandle, event: Event) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let mut host = context.mods.lock().unwrap_or_else(|e| e.into_inner());
    if host.subscribe_current(event) {
        Status::Ok
    } else {
        // Called from outside a hook and outside `on_load`, so there is
        // no "current mod" to subscribe. A mod that stashed the handle
        // and called this from a thread of its own would land here.
        Status::Refused
    }
}

unsafe extern "C" fn unsubscribe(handle: HostHandle, event: Event) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let mut host = context.mods.lock().unwrap_or_else(|e| e.into_inner());
    if host.unsubscribe_current(event) {
        Status::Ok
    } else {
        Status::Refused
    }
}

unsafe extern "C" fn register_command(handle: HostHandle, name: Str, help: Str) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let name = name.as_str().trim().trim_start_matches('/').to_string();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Status::BadArgument;
    }
    let mut host = context.mods.lock().unwrap_or_else(|e| e.into_inner());
    host.register_command(name, help.as_str().to_string());
    Status::Ok
}

// ------------------------------------------------------------- physics

pub fn physics_table() -> PhysicsApi {
    PhysicsApi {
        gravity,
        walk_speed,
        sprint_speed,
        jump_speed,
        terminal_velocity,
        is_solid,
        liquid_depth,
        raycast,
        carry_capacity_kg,
        load_speed_scale,
        load_fall_multiplier,
        block_drag,
        block_grip,
        safe_fall_blocks,
    }
}

// **The movement numbers a *server* knows.**
//
// The integrator lives on the client -- a player's motion is simulated
// where the input is, or every step would cost a round trip -- so the
// server does not have a gravity constant to report. What it has is the
// envelope it *judges* movement against, and that is what a mod actually
// wants: the fastest anything is allowed to go, the furthest it may
// climb, the point past which a claimed position is a cheat.
//
// Reporting the client's own constants here would be reporting numbers
// this process does not use and cannot enforce, which is worse than
// reporting nothing.

unsafe extern "C" fn gravity(handle: HostHandle) -> f32 {
    // Negative, downward, and derived from the fall the anti-cheat will
    // tolerate rather than from an integrator this process does not run.
    let Some(context) = ctx(handle) else { return 0.0 };
    -context.settings.anticheat.max_vertical_speed
}

unsafe extern "C" fn walk_speed(handle: HostHandle) -> f32 {
    let Some(context) = ctx(handle) else { return 0.0 };
    context.settings.anticheat.max_horizontal_speed
}

unsafe extern "C" fn sprint_speed(handle: HostHandle) -> f32 {
    let Some(context) = ctx(handle) else { return 0.0 };
    context.settings.anticheat.max_horizontal_speed
}

unsafe extern "C" fn jump_speed(handle: HostHandle) -> f32 {
    let Some(context) = ctx(handle) else { return 0.0 };
    context.settings.anticheat.max_airborne_ascent
}

unsafe extern "C" fn terminal_velocity(handle: HostHandle) -> f32 {
    let Some(context) = ctx(handle) else { return 0.0 };
    -context.settings.anticheat.max_vertical_speed
}

unsafe extern "C" fn is_solid(_handle: HostHandle, block: BlockId) -> bool {
    primitive_shared::types::is_collidable(block)
}

unsafe extern "C" fn liquid_depth(_handle: HostHandle, block: BlockId) -> f32 {
    if !primitive_shared::types::is_liquid(block) {
        return 0.0;
    }
    primitive_shared::fluid::surface_height(block)
}

unsafe extern "C" fn raycast(
    handle: HostHandle,
    from: Vec3,
    direction: Vec3,
    max_distance: f32,
    hit: *mut BlockPos,
    normal: *mut BlockPos,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let (Some(origin), Some(dir)) = (finite(from), finite(direction)) else {
        return Status::BadArgument;
    };
    if !max_distance.is_finite() || max_distance <= 0.0 {
        return Status::BadArgument;
    }
    let length = (dir.0 * dir.0 + dir.1 * dir.1 + dir.2 * dir.2).sqrt();
    if length <= 1e-6 {
        return Status::BadArgument;
    }
    let step = 0.05f32;
    let dir = (dir.0 / length, dir.1 / length, dir.2 / length);
    let mut travelled = 0.0f32;
    let mut previous = (
        origin.0.floor() as i32,
        origin.1.floor() as i32,
        origin.2.floor() as i32,
    );
    while travelled <= max_distance {
        let point = (
            origin.0 + dir.0 * travelled,
            origin.1 + dir.1 * travelled,
            origin.2 + dir.2 * travelled,
        );
        let cell = (
            point.0.floor() as i32,
            point.1.floor() as i32,
            point.2.floor() as i32,
        );
        if cell != previous {
            if let Some(block) = context.world.cached_block(cell.0, cell.1, cell.2) {
                if primitive_shared::types::is_collidable(block) {
                    if !hit.is_null() {
                        *hit = BlockPos {
                            x: cell.0,
                            y: cell.1,
                            z: cell.2,
                        };
                    }
                    if !normal.is_null() {
                        *normal = BlockPos {
                            x: previous.0 - cell.0,
                            y: previous.1 - cell.1,
                            z: previous.2 - cell.2,
                        };
                    }
                    return Status::Ok;
                }
            }
            previous = cell;
        }
        travelled += step;
    }
    Status::NotFound
}

// ---------------------------------------------------------------- save

pub fn save_table() -> SaveApi {
    SaveApi {
        store,
        load,
        save_world,
    }
}

unsafe extern "C" fn store(handle: HostHandle, data: *const u8, len: usize) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if data.is_null() && len > 0 {
        return Status::BadArgument;
    }
    let bytes = if len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(data, len).to_vec()
    };
    let mut host = context.mods.lock().unwrap_or_else(|e| e.into_inner());
    if host.store_blob(bytes) {
        Status::Ok
    } else {
        Status::Refused
    }
}

unsafe extern "C" fn load(
    handle: HostHandle,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let host = context.mods.lock().unwrap_or_else(|e| e.into_inner());
    let Some(bytes) = host.load_blob() else {
        if !written.is_null() {
            *written = 0;
        }
        return Status::NotFound;
    };
    if !written.is_null() {
        *written = bytes.len();
    }
    if out.is_null() || cap == 0 {
        return Status::Ok;
    }
    if bytes.len() > cap {
        return Status::BadArgument;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
    Status::Ok
}

unsafe extern "C" fn save_world(handle: HostHandle) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if context.world_dir.is_none() {
        return Status::Unavailable;
    }
    crate::run_command(
        context,
        "/save",
        crate::commands::Permission::Operator,
        None,
    );
    Status::Ok
}

// ------------------------------------------------------------ crafting

pub fn crafting_table() -> CraftingApi {
    CraftingApi {
        recipe_count,
        recipe,
        recipe_name,
        recipe_input,
        recipe_return,
        recipes_making,
        recipes_using,
        feasibility,
        heat_at,
        craft,
        station_name,
    }
}

/// The mod API's [`Station`] from the game's own.
///
/// A match rather than a cast, so that adding a station to the game is a
/// compile error here rather than a mod reading `Bloomery` where the
/// game meant something new.
fn station_of(station: primitive_shared::crafting::Station) -> Station {
    use primitive_shared::crafting::Station as S;
    match station {
        S::Hands => Station::Hands,
        S::Heat => Station::Heat,
        S::Forge => Station::Forge,
        S::Bloomery => Station::Bloomery,
        S::Bench => Station::Bench,
        S::Mason => Station::Mason,
        S::Wheel => Station::Wheel,
        S::Leather => Station::Leather,
    }
}

unsafe extern "C" fn recipe_count(_handle: HostHandle) -> u32 {
    primitive_shared::crafting::RECIPES.len() as u32
}

unsafe extern "C" fn recipe(_handle: HostHandle, index: u32, out: *mut RecipeInfo) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(r) = primitive_shared::crafting::recipe(index as usize) else {
        return Status::NotFound;
    };
    *out = RecipeInfo {
        output: r.output.0,
        output_count: r.output.1,
        station: station_of(r.station),
        input_count: r.inputs.len() as u32,
        return_count: r.returns.len() as u32,
    };
    Status::Ok
}

unsafe extern "C" fn recipe_name(
    _handle: HostHandle,
    index: u32,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    match primitive_shared::crafting::recipe(index as usize) {
        Some(r) => write_str(r.name, out, cap, written),
        None => {
            if !written.is_null() {
                *written = 0;
            }
            Status::NotFound
        }
    }
}

/// One entry of a recipe's inputs or returns, as a stack.
///
/// Shared by both because they are the same shape and the only
/// difference is which list is walked -- two copies of a bounds-checked
/// index is two places to get it wrong.
unsafe fn recipe_list_entry(
    index: u32,
    which: u32,
    out: *mut ItemStack,
    pick: impl Fn(&primitive_shared::crafting::Recipe) -> &'static [(BlockId, u32)],
) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(r) = primitive_shared::crafting::recipe(index as usize) else {
        return Status::NotFound;
    };
    match pick(r).get(which as usize) {
        Some(&(block, count)) => {
            *out = ItemStack {
                block,
                count,
                damage: 0,
            };
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn recipe_input(
    _handle: HostHandle,
    index: u32,
    which: u32,
    out: *mut ItemStack,
) -> Status {
    recipe_list_entry(index, which, out, |r| r.inputs)
}

unsafe extern "C" fn recipe_return(
    _handle: HostHandle,
    index: u32,
    which: u32,
    out: *mut ItemStack,
) -> Status {
    recipe_list_entry(index, which, out, |r| r.returns)
}

/// The indices of every recipe matching a predicate, into a mod's
/// buffer.
///
/// Same convention as every other list-returning call here: `written` is
/// how many there were, `cap` is how many fitted, and a `cap` of zero
/// asks for the count.
unsafe fn recipe_indices(
    matches: impl Fn(&primitive_shared::crafting::Recipe) -> bool,
    out: *mut u32,
    cap: usize,
    written: *mut usize,
) -> Status {
    let found: Vec<u32> = primitive_shared::crafting::RECIPES
        .iter()
        .enumerate()
        .filter(|(_, r)| matches(r))
        .map(|(i, _)| i as u32)
        .collect();
    if !written.is_null() {
        *written = found.len();
    }
    if out.is_null() || cap == 0 {
        return Status::Ok;
    }
    for (i, index) in found.iter().take(cap).enumerate() {
        *out.add(i) = *index;
    }
    Status::Ok
}

unsafe extern "C" fn recipes_making(
    _handle: HostHandle,
    block: BlockId,
    out: *mut u32,
    cap: usize,
    written: *mut usize,
) -> Status {
    // By kind rather than by exact id: a mod asking "how does a player
    // get a log" means the material, not the one orientation a recipe
    // happens to name.
    let kind = primitive_shared::types::block_kind(block);
    recipe_indices(
        |r| primitive_shared::types::block_kind(r.output.0) == kind,
        out,
        cap,
        written,
    )
}

unsafe extern "C" fn recipes_using(
    _handle: HostHandle,
    block: BlockId,
    out: *mut u32,
    cap: usize,
    written: *mut usize,
) -> Status {
    let kind = primitive_shared::types::block_kind(block);
    recipe_indices(
        |r| {
            r.inputs
                .iter()
                .any(|&(id, _)| primitive_shared::types::block_kind(id) == kind)
        },
        out,
        cap,
        written,
    )
}

/// The mod API's [`Feasibility`] from the game's own.
fn feasibility_of(f: primitive_shared::crafting::Feasibility) -> Feasibility {
    use primitive_shared::crafting::Feasibility as F;
    match f {
        F::Ready => Feasibility::Ready,
        F::MissingIngredients => Feasibility::MissingIngredients,
        F::NoRoom => Feasibility::NoRoom,
        F::NeedsFire => Feasibility::NeedsFire,
        F::NeedsForge => Feasibility::NeedsForge,
        F::NeedsBloomery => Feasibility::NeedsBloomery,
        F::NeedsWorkshop(_) => Feasibility::NeedsWorkshop,
    }
}

unsafe extern "C" fn feasibility(
    handle: HostHandle,
    player: PlayerId,
    index: u32,
    out: *mut Feasibility,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let Some(r) = primitive_shared::crafting::recipe(index as usize) else {
        return Status::NotFound;
    };
    // The fire is asked about *before* the player's own lock is taken.
    // `heat_within_reach` locks the fire map and the player's state in
    // turn; taking the state lock first here would be the one ordering
    // the tick loop does not use, which is the deadlock that only
    // happens on a busy server.
    let heat = crate::heat_within_reach(context, &h);
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = feasibility_of(primitive_shared::crafting::feasibility(
        &state.inventory,
        r,
        heat,
    ));
    Status::Ok
}

unsafe extern "C" fn heat_at(handle: HostHandle, player: PlayerId, out: *mut CraftHeat) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let heat = crate::heat_within_reach(context, &h);
    *out = CraftHeat {
        fire: heat.fire,
        kiln: heat.kiln,
        bloomery: heat.bloomery,
    };
    Status::Ok
}

unsafe extern "C" fn craft(
    handle: HostHandle,
    player: PlayerId,
    index: u32,
    times: u32,
    made: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    if primitive_shared::crafting::recipe(index as usize).is_none() {
        return Status::NotFound;
    }
    // Through the player's own path, which is what makes a mod's craft
    // and a player's craft the same event: the fire is checked against
    // the server's copy of where they stand, `ItemCrafted` is fired and
    // may be cancelled, and the new pack goes out.
    // What was *made*: a knapping that shattered its flint ran, but a
    // mod counting its output must not count a nodule that is now gravel.
    let count = crate::craft_for(context, &h, index as usize, times).made;
    if !made.is_null() {
        *made = count;
    }
    if count > 0 {
        Status::Ok
    } else {
        Status::Refused
    }
}

unsafe extern "C" fn station_name(
    _handle: HostHandle,
    station: Station,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> Status {
    let name = match station {
        Station::Hands => "hands",
        Station::Heat => "fire",
        Station::Forge => "kiln",
        Station::Bloomery => "bloomery",
        Station::Bench => "workbench",
        Station::Mason => "mason block",
        Station::Wheel => "potter's wheel",
        Station::Leather => "leather bench",
    };
    write_str(name, out, cap, written)
}

// ------------------------------------------------------------ lighting

pub fn lighting_table() -> LightingApi {
    LightingApi {
        sky_light,
        block_light,
        open_to_sky,
        max_light,
    }
}

/// Light for the chunk a cell is in, computed here and now.
///
/// **There is no light map on a server.** Light is computed where it is
/// drawn, and that is the client; a second copy maintained here would be
/// a second copy nothing reads and one more thing to keep in step with
/// the mesher. So a mod that asks pays for the answer, and the answer
/// costs one chunk's flood fill.
///
/// It is computed *in isolation*, with the four neighbouring chunks
/// treated as walls -- which is what `lighting::compute_isolated` means,
/// and is stated on `LightingApi` rather than hidden: sunlight down a
/// column is exact, and a torch on the far side of a chunk boundary does
/// not reach across.
///
/// Deliberately not cached. A cache would have to be invalidated by
/// every block change in the world -- including the ones the water and
/// the fire make, twenty times a second -- and a stale light level is a
/// worse answer than a slow one.
fn chunk_light(context: &Arc<Context>, at: BlockPos) -> Option<Vec<u8>> {
    use primitive_shared::types::CHUNK_SIZE_Y;
    if at.y < 0 || at.y >= CHUNK_SIZE_Y as i32 {
        return None;
    }
    let (pos, _, _) = primitive_shared::types::ChunkPos::from_global(at.x, at.z);
    let chunk = context.world.cached(pos)?;
    // Decoded from the packed cache first: the light pass is written for
    // a flat array, and decoding is a small fraction of the flood fill a
    // mod asking this is already paying for.
    Some(primitive_shared::lighting::compute_isolated(&chunk.to_blocks()))
}

/// The index of a cell inside its own chunk.
fn cell_index(at: BlockPos) -> usize {
    let (_, lx, lz) = primitive_shared::types::ChunkPos::from_global(at.x, at.z);
    primitive_shared::types::Chunk::index(lx, at.y as usize, lz)
}

unsafe extern "C" fn sky_light(handle: HostHandle, at: BlockPos, out: *mut u8) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(light) = chunk_light(context, at) else {
        return Status::NotFound;
    };
    // Low nibble is sky, high is block light -- see `lighting::LightMap`,
    // which packs both into one byte so a cell's two levels share a
    // cache line.
    *out = light.get(cell_index(at)).copied().unwrap_or(0) & 0x0F;
    Status::Ok
}

unsafe extern "C" fn block_light(handle: HostHandle, at: BlockPos, out: *mut u8) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(light) = chunk_light(context, at) else {
        return Status::NotFound;
    };
    *out = (light.get(cell_index(at)).copied().unwrap_or(0) >> 4) & 0x0F;
    Status::Ok
}

unsafe extern "C" fn open_to_sky(handle: HostHandle, at: BlockPos) -> bool {
    use primitive_shared::types::CHUNK_SIZE_Y;
    let Some(context) = ctx(handle) else {
        return false;
    };
    if at.y >= CHUNK_SIZE_Y as i32 {
        return true; // above the world there is nothing but sky
    }
    if at.y < 0 {
        return false;
    }
    let (pos, lx, lz) = primitive_shared::types::ChunkPos::from_global(at.x, at.z);
    let Some(chunk) = context.world.cached(pos) else {
        return false; // an unloaded chunk reads as dark, never as daylight
    };
    // One column, straight up. Exact -- unlike the two calls above this
    // needs no neighbour, and therefore has no seam.
    for y in (at.y as usize + 1)..CHUNK_SIZE_Y {
        if primitive_shared::types::is_opaque(chunk.get(lx, y, lz)) {
            return false;
        }
    }
    true
}

unsafe extern "C" fn max_light(_handle: HostHandle) -> u8 {
    primitive_shared::types::MAX_LIGHT
}

// ---------------------------------------------------------------- food

pub fn food_table() -> FoodApi {
    FoodApi {
        is_food,
        nutrition,
        harm,
        max_nourishment,
        worth_eating,
        eat,
    }
}

unsafe extern "C" fn is_food(_handle: HostHandle, block: BlockId) -> bool {
    primitive_shared::food::is_food(block)
}

unsafe extern "C" fn nutrition(_handle: HostHandle, block: BlockId, out: *mut f32) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    match primitive_shared::food::nutrition(block) {
        Some(value) => {
            *out = value;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn harm(_handle: HostHandle, block: BlockId, out: *mut FoodHarm) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    match primitive_shared::food::harm(block) {
        Some(h) => {
            *out = FoodHarm {
                health: h.health,
                nourishment: h.nourishment,
            };
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn max_nourishment(_handle: HostHandle) -> f32 {
    primitive_shared::food::MAX_NOURISHMENT
}

unsafe extern "C" fn worth_eating(handle: HostHandle, player: PlayerId, block: BlockId) -> bool {
    let Some(context) = ctx(handle) else {
        return false;
    };
    let Some(h) = context.registry.get(player) else {
        return false;
    };
    let have = {
        let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        state.vitals.nourishment()
    };
    primitive_shared::food::worth_eating(have, block)
}

unsafe extern "C" fn eat(handle: HostHandle, player: PlayerId, slot: u32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    if slot as usize >= primitive_shared::inventory::SLOTS {
        return Status::BadArgument;
    }
    // The player's own gesture, entire: a full jug is drunk rather than
    // eaten, the toadstool takes health, the weight changes, and a death
    // from the last mushroom is announced by the one path that knows
    // how.
    if crate::eat_from_slot(context, &h, slot as usize) {
        Status::Ok
    } else {
        Status::Refused
    }
}

// -------------------------------------------------------------- combat

pub fn combat_table() -> CombatApi {
    CombatApi {
        melee_reach,
        reach_tolerance,
        melee_damage,
        melee_cooldown_seconds,
        within_reach,
        damage_through_armour,
        weapon_damage,
        minimum_damage_fraction,
    }
}

unsafe extern "C" fn melee_reach(_handle: HostHandle) -> f32 {
    primitive_shared::combat::MELEE_REACH
}

unsafe extern "C" fn reach_tolerance(_handle: HostHandle) -> f32 {
    primitive_shared::combat::REACH_TOLERANCE
}

unsafe extern "C" fn melee_damage(_handle: HostHandle) -> f32 {
    primitive_shared::combat::MELEE_DAMAGE
}

unsafe extern "C" fn melee_cooldown_seconds(_handle: HostHandle) -> f32 {
    primitive_shared::combat::MELEE_COOLDOWN_SECS
}

unsafe extern "C" fn within_reach(_handle: HostHandle, from: Vec3, to: Vec3) -> bool {
    let (Some(a), Some(b)) = (finite(from), finite(to)) else {
        return false;
    };
    // Bare-handed, because the ABI has no place to put a weapon and
    // changing it would break every mod compiled against it. A mod
    // asking "are these two close enough to touch" gets the arm's
    // answer; the weapon tables are the game's own business.
    primitive_shared::combat::within_reach(primitive_shared::geometry::wide(a), primitive_shared::geometry::wide(b), None)
}

unsafe extern "C" fn damage_through_armour(
    handle: HostHandle,
    player: PlayerId,
    damage: f32,
    out: *mut f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    if !damage.is_finite() || damage < 0.0 {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = state.equipment.worn().through(damage);
    Status::Ok
}

unsafe extern "C" fn weapon_damage(_handle: HostHandle, block: BlockId, out: *mut f32) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    // Zero is "bare hands" rather than a block id, which is the same
    // convention `BlocksApi::break_seconds` uses for a tool.
    let held = (block != 0).then_some(block);
    if held.is_some_and(|id| !primitive_shared::types::is_known_block(id)) {
        return Status::NotFound;
    }
    *out = crate::hunting_damage(held);
    Status::Ok
}

unsafe extern "C" fn minimum_damage_fraction(_handle: HostHandle) -> f32 {
    primitive_shared::equipment::MIN_DAMAGE_THROUGH
}

// --------------------------------------------------------------- fluid

pub fn fluid_table() -> FluidApi {
    FluidApi {
        is_liquid,
        is_source,
        place_source,
        remove: remove_fluid,
        column_depth,
        surface_height,
        pending: fluid_pending,
    }
}

unsafe extern "C" fn is_liquid(_handle: HostHandle, block: BlockId) -> bool {
    primitive_shared::types::is_liquid(block)
}

unsafe extern "C" fn is_source(_handle: HostHandle, block: BlockId) -> bool {
    primitive_shared::fluid::is_source(block)
}

unsafe extern "C" fn place_source(handle: HostHandle, at: BlockPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let source = primitive_shared::fluid::with_depth(primitive_shared::fluid::SOURCE_DEPTH);
    if place_one(context, at, source) {
        Status::Ok
    } else {
        Status::BadArgument
    }
}

unsafe extern "C" fn remove_fluid(handle: HostHandle, at: BlockPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    match context.world.cached_block(at.x, at.y, at.z) {
        Some(block) if primitive_shared::types::is_liquid(block) => {}
        _ => return Status::NotFound,
    }
    // Through the same path a player's edit takes, so the neighbours
    // flow back in rather than the cell staying as a hole in a lake --
    // which is what a bare write to the chunk would leave.
    if place_one(context, at, primitive_shared::types::BLOCK_AIR) {
        Status::Ok
    } else {
        Status::BadArgument
    }
}

unsafe extern "C" fn column_depth(handle: HostHandle, at: BlockPos, out: *mut f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(top) = context.world.cached_block(at.x, at.y, at.z) else {
        return Status::NotFound;
    };
    if !primitive_shared::types::is_liquid(top) {
        *out = 0.0;
        return Status::Ok;
    }
    // The top cell contributes however full it is; every whole cell
    // under it contributes one. Bounded by the world's floor, so a
    // column of water reaching bedrock terminates.
    let mut depth = primitive_shared::fluid::surface_height(top);
    let mut y = at.y - 1;
    while y >= 0 {
        match context.world.cached_block(at.x, y, at.z) {
            Some(block) if primitive_shared::types::is_liquid(block) => depth += 1.0,
            _ => break,
        }
        y -= 1;
    }
    *out = depth;
    Status::Ok
}

unsafe extern "C" fn surface_height(_handle: HostHandle, block: BlockId, out: *mut f32) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    if !primitive_shared::types::is_liquid(block) {
        *out = 0.0;
        return Status::NotFound;
    }
    *out = primitive_shared::fluid::surface_height(block);
    Status::Ok
}

unsafe extern "C" fn fluid_pending(handle: HostHandle) -> u32 {
    let Some(context) = ctx(handle) else { return 0 };
    let mechanics = context.mechanics.lock().unwrap_or_else(|e| e.into_inner());
    // Every registered mechanic's queue, summed. Water is the one that
    // ships; a mod asking "has the flood settled" wants the total rather
    // than a breakdown it would have to know the names in.
    mechanics.pending().iter().map(|&(_, n)| n).sum::<usize>() as u32
}

// ---------------------------------------------------------- containers

pub fn containers_table() -> ContainersApi {
    ContainersApi {
        slot_count: container_slot_count,
        container_kind,
        get_slot: container_get_slot,
        set_slot: container_set_slot,
        give: container_give,
        take: container_take,
        spill: container_spill,
        all: containers_all,
        close: container_close,
    }
}

/// The cell a container call names, refused if there is not one there.
///
/// **A container is a block, not a record**: the store is keyed by
/// position and a position with no container block on it is a mod
/// talking about somewhere else. Answering `NotFound` here rather than
/// creating an entry is what stops a mod from filling the save file
/// with chests nobody can open.
fn container_at(
    context: &Arc<Context>,
    at: BlockPos,
) -> Option<(crate::containers::ChestPos, u32)> {
    let block = context.world.cached_block(at.x, at.y, at.z)?;
    if primitive_shared::rack::is_rack(block) {
        return Some(((at.x, at.y, at.z), primitive_shared::rack::USED_SLOTS as u32));
    }
    if primitive_shared::hearth::Kind::of(block).is_some() {
        return Some((
            (at.x, at.y, at.z),
            primitive_shared::hearth::USED_SLOTS as u32,
        ));
    }
    // A set-down jug is one slot, not forty: a mod told otherwise would
    // write into slot nine of a jug and the grain would be spilled loose
    // the moment the jug was picked up, because nothing folds slot nine
    // back into it.
    if primitive_shared::types::opens_as_vessel(block) {
        return Some((
            (at.x, at.y, at.z),
            (primitive_shared::inventory::VESSEL_SLOT + 1) as u32,
        ));
    }
    if primitive_shared::types::is_container(block) {
        // A mod looking into a ruin chest nobody has opened sees what a
        // player would, rather than an empty box the next player to open
        // it finds full.
        crate::unseal_ruin_chest(context, (at.x, at.y, at.z));
        return Some((
            (at.x, at.y, at.z),
            primitive_shared::inventory::SLOTS as u32,
        ));
    }
    None
}

unsafe extern "C" fn container_slot_count(
    handle: HostHandle,
    at: BlockPos,
    out: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some((_, slots)) = container_at(context, at) else {
        return Status::NotFound;
    };
    *out = slots;
    Status::Ok
}

unsafe extern "C" fn container_kind(handle: HostHandle, at: BlockPos, out: *mut u32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(block) = context.world.cached_block(at.x, at.y, at.z) else {
        return Status::NotFound;
    };
    *out = if primitive_shared::rack::is_rack(block) {
        2
    } else if primitive_shared::hearth::Kind::of(block).is_some() {
        1
    } else if primitive_shared::types::opens_as_vessel(block) {
        3
    } else if primitive_shared::types::is_container(block) {
        0
    } else {
        return Status::NotFound;
    };
    Status::Ok
}

unsafe extern "C" fn container_get_slot(
    handle: HostHandle,
    at: BlockPos,
    slot: u32,
    out: *mut ItemStack,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some((pos, slots)) = container_at(context, at) else {
        return Status::NotFound;
    };
    if slot >= slots {
        return Status::BadArgument;
    }
    let contents = {
        let chests = context.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents(pos)
    };
    *out = match contents.slots().get(slot as usize).copied().flatten() {
        Some(stack) => ItemStack {
            block: stack.block,
            count: stack.count,
            damage: stack.damage,
        },
        None => ItemStack::EMPTY,
    };
    Status::Ok
}

unsafe extern "C" fn container_set_slot(
    handle: HostHandle,
    at: BlockPos,
    slot: u32,
    stack: ItemStack,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some((pos, slots)) = container_at(context, at) else {
        return Status::NotFound;
    };
    if slot >= slots {
        return Status::BadArgument;
    }
    if stack.count > 0 && !primitive_shared::types::is_known_block(stack.block) {
        return Status::BadArgument;
    }
    // **What fits where is the container's business, not the mod's**,
    // exactly as it is for a garment. Nothing goes into a hearth's
    // output slots -- they are where it puts things, and a mod that can
    // fill them can jam the furnace in a way the player cannot undo.
    if stack.count > 0 && !crate::container_accepts(context, pos, slot as usize, stack.block) {
        return Status::Refused;
    }
    let displaced = {
        let mut chests = context.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(pos, |contents| {
            let taken = contents.take_slot(slot as usize);
            if stack.count > 0 {
                contents.put_in_slot(
                    slot as usize,
                    primitive_shared::inventory::Stack::worn(
                        stack.block,
                        stack.count,
                        stack.damage,
                    ),
                );
            }
            taken
        })
    };
    // Displaced rather than deleted, on the same rule
    // `InventoryApi::set_slot` follows: the host does not destroy a
    // player's things on a mod's behalf, and what is in a chest is a
    // player's things.
    if let Some(old) = displaced {
        let mut items = context.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn_worn(
            old.block,
            old.count,
            old.damage,
            (f64::from(at.x) + 0.5, f64::from(at.y) + 0.5, f64::from(at.z) + 0.5),
            (0.0, 0.0, 0.0),
            None,
            std::time::Instant::now(),
        );
    }
    crate::broadcast_chest_state(context, pos);
    crate::refresh_rack_block(context, pos);
    Status::Ok
}

unsafe extern "C" fn container_give(
    handle: HostHandle,
    at: BlockPos,
    stack: ItemStack,
    left_over: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some((pos, _)) = container_at(context, at) else {
        return Status::NotFound;
    };
    if stack.count == 0 || !primitive_shared::types::is_known_block(stack.block) {
        return Status::BadArgument;
    }
    let left = {
        let mut chests = context.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(pos, |contents| {
            contents.add_worn(stack.block, stack.count, stack.damage)
        })
    };
    if !left_over.is_null() {
        *left_over = left;
    }
    crate::broadcast_chest_state(context, pos);
    crate::refresh_rack_block(context, pos);
    Status::Ok
}

unsafe extern "C" fn container_take(
    handle: HostHandle,
    at: BlockPos,
    block: BlockId,
    count: u32,
    taken: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some((pos, _)) = container_at(context, at) else {
        return Status::NotFound;
    };
    let got = {
        let mut chests = context.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(pos, |contents| {
            let have = contents.count(block).min(count);
            if have > 0 && contents.take_exact(block, have) {
                have
            } else {
                0
            }
        })
    };
    if !taken.is_null() {
        *taken = got;
    }
    crate::broadcast_chest_state(context, pos);
    crate::refresh_rack_block(context, pos);
    Status::Ok
}

unsafe extern "C" fn container_spill(handle: HostHandle, at: BlockPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some((pos, _)) = container_at(context, at) else {
        return Status::NotFound;
    };
    // The same path breaking the block takes: the screen closes for
    // everybody first, so nobody is looking at a chest whose contents
    // are already on the floor.
    crate::spill_chest(context, pos);
    crate::refresh_rack_block(context, pos);
    Status::Ok
}

unsafe extern "C" fn containers_all(
    handle: HostHandle,
    out: *mut BlockPos,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let found: Vec<BlockPos> = {
        let chests = context.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests
            .positions()
            .into_iter()
            .map(|(x, y, z)| BlockPos { x, y, z })
            .collect()
    };
    if !written.is_null() {
        *written = found.len();
    }
    if out.is_null() || cap == 0 {
        return Status::Ok;
    }
    for (i, pos) in found.iter().take(cap).enumerate() {
        *out.add(i) = *pos;
    }
    Status::Ok
}

unsafe extern "C" fn container_close(handle: HostHandle, at: BlockPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    crate::close_chest_for_everyone(context, (at.x, at.y, at.z));
    Status::Ok
}

// ------------------------------------------------------------ stations

pub fn stations_table() -> StationsApi {
    StationsApi {
        light_fire,
        feed_fire,
        extinguish,
        fire_fuel_left,
        fuel_seconds,
        fire_within,
        burning_count,
        smelting_progress,
        drying_progress,
        set_drying_progress,
        cures_into,
        drying_rate,
    }
}

unsafe extern "C" fn light_fire(handle: HostHandle, at: BlockPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let lit = {
        let mut fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
        fires.light((at.x, at.y, at.z))
    };
    if !lit {
        // Not a hearth, or already burning. Refused rather than reported
        // as success: a mod that thinks it lit a fire and did not will
        // wait forever for the smelt.
        return Status::Refused;
    }
    // The block itself has to change -- a hearth that is alight is a
    // different id, and that is what the client draws and what
    // `heat_within_reach` reads back.
    crate::notify_mechanics(context, at.x, at.y, at.z);
    Status::Ok
}

unsafe extern "C" fn feed_fire(handle: HostHandle, at: BlockPos, seconds: f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return Status::BadArgument;
    }
    let mut fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
    if fires.feed((at.x, at.y, at.z), seconds) {
        Status::Ok
    } else {
        Status::NotFound
    }
}

unsafe extern "C" fn extinguish(handle: HostHandle, at: BlockPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let was_burning = {
        let mut fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
        let was = fires.fuel_left((at.x, at.y, at.z)).is_some();
        fires.extinguish((at.x, at.y, at.z));
        was
    };
    if !was_burning {
        return Status::NotFound;
    }
    // Announced with the lock let go, on the one rule this whole file
    // is built on: nothing calls into a mod while anything is locked.
    crate::fire_died(context, (at.x, at.y, at.z));
    crate::notify_mechanics(context, at.x, at.y, at.z);
    Status::Ok
}

unsafe extern "C" fn fire_fuel_left(handle: HostHandle, at: BlockPos, out: *mut f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
    match fires.fuel_left((at.x, at.y, at.z)) {
        Some(seconds) => {
            *out = seconds;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn fuel_seconds(_handle: HostHandle, block: BlockId, out: *mut f32) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    match crate::fire::fuel_value(block) {
        Some(seconds) => {
            *out = seconds;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn fire_within(handle: HostHandle, at: Vec3, range: f32) -> bool {
    let Some(context) = ctx(handle) else {
        return false;
    };
    let Some(point) = finite(at) else { return false };
    if !range.is_finite() || range < 0.0 {
        return false;
    }
    let fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
    fires.any_within(point, range)
}

unsafe extern "C" fn burning_count(handle: HostHandle) -> u32 {
    let Some(context) = ctx(handle) else { return 0 };
    let fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
    fires.len() as u32
}

unsafe extern "C" fn smelting_progress(
    handle: HostHandle,
    at: BlockPos,
    out: *mut f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(block) = context.world.cached_block(at.x, at.y, at.z) else {
        return Status::NotFound;
    };
    let Some(kind) = primitive_shared::hearth::Kind::of(block) else {
        return Status::NotFound;
    };
    // **One lock at a time, and in the tick loop's order.** The tick
    // loop takes the fires, then the chests, then the smelting; this
    // runs on whatever thread a mod is on, and taking any two of them
    // the other way round is the deadlock that only happens on a busy
    // server. Nothing here needs two at once.
    let contents = {
        let chests = context.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents((at.x, at.y, at.z))
    };
    let progress = {
        let smelting = context.smelting.lock().unwrap_or_else(|e| e.into_inner());
        smelting.progress((at.x, at.y, at.z), &contents, kind)
    };
    *out = progress.fraction();
    Status::Ok
}

unsafe extern "C" fn drying_progress(handle: HostHandle, at: BlockPos, out: *mut f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(block) = context.world.cached_block(at.x, at.y, at.z) else {
        return Status::NotFound;
    };
    if !primitive_shared::rack::is_rack(block) {
        return Status::NotFound;
    }
    let racks = context.drying.lock().unwrap_or_else(|e| e.into_inner());
    *out = racks.progress_at((at.x, at.y, at.z));
    Status::Ok
}

unsafe extern "C" fn set_drying_progress(
    handle: HostHandle,
    at: BlockPos,
    progress: f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !progress.is_finite() {
        return Status::BadArgument;
    }
    let Some(block) = context.world.cached_block(at.x, at.y, at.z) else {
        return Status::NotFound;
    };
    if !primitive_shared::rack::is_rack(block) {
        return Status::NotFound;
    }
    {
        let mut racks = context.drying.lock().unwrap_or_else(|e| e.into_inner());
        racks.set_progress((at.x, at.y, at.z), progress.clamp(0.0, 1.0));
    }
    // The bar moves for whoever has it open. Not the block: a rack only
    // changes its picture when the frame empties, which the next step
    // does.
    crate::broadcast_chest_state(context, (at.x, at.y, at.z));
    Status::Ok
}

unsafe extern "C" fn cures_into(_handle: HostHandle, block: BlockId, out: *mut BlockId) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    match primitive_shared::rack::cures_into(block) {
        Some(into) => {
            *out = into;
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn drying_rate(handle: HostHandle, at: Vec3, out: *mut f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(point) = finite(at) else {
        return Status::BadArgument;
    };
    let weather = {
        let sky = context.sky.lock().unwrap_or_else(|e| e.into_inner());
        sky.weather()
    };
    let ambient = {
        let fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
        crate::climate::Ambient::of(
            &context.world,
            &fires,
            point,
            context.clock.world_days(),
            weather,
        )
    };
    *out = crate::drying::Drying::rate(&ambient, weather);
    Status::Ok
}

// ---------------------------------------------------------- simulation

pub fn simulation_table() -> SimulationApi {
    SimulationApi {
        falling_pending,
        falling_entities,
        growth_pending,
        watch_growth,
        regrow_seconds,
        crop_stage_seconds,
        is_standing_trunk,
        fell_tree,
    }
}

unsafe extern "C" fn falling_pending(handle: HostHandle) -> u32 {
    let Some(context) = ctx(handle) else { return 0 };
    let sim = context.falling.lock().unwrap_or_else(|e| e.into_inner());
    sim.pending() as u32
}

unsafe extern "C" fn falling_entities(handle: HostHandle) -> u32 {
    let Some(context) = ctx(handle) else { return 0 };
    let sim = context.falling.lock().unwrap_or_else(|e| e.into_inner());
    sim.entity_count() as u32
}

unsafe extern "C" fn growth_pending(handle: HostHandle) -> u32 {
    let Some(context) = ctx(handle) else { return 0 };
    let growth = context.growth.lock().unwrap_or_else(|e| e.into_inner());
    growth.pending() as u32
}

unsafe extern "C" fn watch_growth(handle: HostHandle, at: Vec3) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(point) = finite(at) else {
        return Status::BadArgument;
    };
    // Added to the sample the tick loop already walks out from the
    // players, rather than replacing it: a mod asking the growth pass to
    // look at a farm must not stop it looking where people are standing.
    // The tick loop rebuilds the list every tick from the players, so
    // this is a nudge for this tick and a mod that means a permanent
    // farm calls it on every one.
    let mut growth = context.growth.lock().unwrap_or_else(|e| e.into_inner());
    growth.watch(vec![point]);
    Status::Ok
}

unsafe extern "C" fn regrow_seconds(_handle: HostHandle) -> f32 {
    crate::growth::REGROW_SECONDS
}

unsafe extern "C" fn crop_stage_seconds(_handle: HostHandle) -> f32 {
    crate::growth::CROP_STAGE_SECONDS
}

unsafe extern "C" fn is_standing_trunk(_handle: HostHandle, block: BlockId) -> bool {
    crate::felling::is_standing_trunk(block)
}

unsafe extern "C" fn fell_tree(handle: HostHandle, at: BlockPos, felled: *mut u32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    // No feller, which is what decides which way it goes: with nobody
    // swinging, the tree falls the way the wood itself says. See
    // `logic::felling`.
    let count = crate::fell_tree(context, (at.x, at.y, at.z), None);
    if !felled.is_null() {
        *felled = count;
    }
    if count > 0 {
        Status::Ok
    } else {
        Status::Refused
    }
}

// ------------------------------------------- what 2.0 appended to the
// ------------------------------------------- tables that already existed
//
// Grouped here rather than filed under their own tables above, so that
// the diff for a version is readable as a version. The table
// constructors themselves name them in place.

// ---- WorldApi ----

unsafe extern "C" fn break_block(handle: HostHandle, at: BlockPos, drop: bool) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(was) = context.world.cached_block(at.x, at.y, at.z) else {
        return Status::NotFound;
    };
    if was == primitive_shared::types::BLOCK_AIR {
        return Status::NotFound;
    }
    // What is left behind, not always air: a berry bush that has been
    // picked leaves the bush. The same rule the player's own break
    // follows -- see `blocks::BlockDef::leaves_behind`.
    let residue = primitive_shared::types::block_residue(was);
    // A fire taken apart stops burning, and a container goes with what
    // is inside it. Before the cell changes, so the maps are straight
    // even if the drop path bails.
    if primitive_shared::types::is_hearth(was) {
        {
            let mut fires = context.fires.lock().unwrap_or_else(|e| e.into_inner());
            fires.extinguish((at.x, at.y, at.z));
        }
        crate::fire_died(context, (at.x, at.y, at.z));
    }
    // A jug is folded back into its own drop instead -- see the same
    // exception in the player's break path, and `pick_up_vessel`. With
    // `drop` false the mod asked for no drop, and the contents are
    // spilled rather than deleted: a jug that is not dropped still had
    // grain in it.
    if primitive_shared::types::is_container(was)
        && (!primitive_shared::types::opens_as_vessel(was) || !drop)
    {
        crate::spill_chest(context, (at.x, at.y, at.z));
    }
    if crate::drying::is_rack(was) {
        crate::forget_rack(context, (at.x, at.y, at.z));
    }
    // ...and a pit kiln or a log pile gives back what went into it, which
    // is not in its table row. The player's break always did
    // (`net::connection`); a mod's lost the pots, the fibre and the logs.
    if primitive_shared::pit::is_pit_kiln(was) || primitive_shared::pit::is_log_pile(was) {
        crate::spill_pit(context, (at.x, at.y, at.z), was);
    }
    if !place_one(context, at, residue) {
        return Status::BadArgument;
    }
    if drop {
        crate::spawn_block_drop(context, was, (at.x, at.y, at.z));
    }
    // A mod breaking half a bed breaks the bed, as a player's hand does.
    crate::break_bed_partner(context, (at.x, at.y, at.z), was);
    // ...and whatever was standing on it comes down, which is the other
    // half of what makes this a break rather than a write.
    let fallen = crate::collapse_unsupported(context, at.x, at.y, at.z);
    crate::broadcast_changes(context, fallen);
    // ...and a palm's crown that the broken cell held, as a player's break
    // brings it down. Broadcast by the function itself.
    crate::drop_unheld_palm_crown(context, (at.x, at.y, at.z));
    Status::Ok
}

unsafe extern "C" fn surface_at(handle: HostHandle, x: i32, z: i32, out: *mut i32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let (pos, lx, lz) = primitive_shared::types::ChunkPos::from_global(x, z);
    let Some(chunk) = context.world.cached(pos) else {
        return Status::NotFound;
    };
    for y in (0..primitive_shared::types::CHUNK_SIZE_Y).rev() {
        if chunk.get(lx, y, lz) != primitive_shared::types::BLOCK_AIR {
            *out = y as i32;
            return Status::Ok;
        }
    }
    // A column of nothing but air, which worldgen does not produce and
    // a mod that dug one can. Reported as the floor rather than as an
    // error: "nothing here" is an answer.
    *out = -1;
    Status::Ok
}

unsafe extern "C" fn disturb(handle: HostHandle, at: BlockPos) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    {
        let mut sim = context.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.on_block_changed(at.x, at.y, at.z);
    }
    crate::notify_mechanics(context, at.x, at.y, at.z);
    Status::Ok
}

// ---- BlocksApi ----

unsafe extern "C" fn tooling(
    _handle: HostHandle,
    block: BlockId,
    out: *mut BlockTooling,
) -> Status {
    use primitive_shared::blocks as b;
    if out.is_null() {
        return Status::BadArgument;
    }
    if !primitive_shared::types::is_known_block(block) {
        return Status::NotFound;
    }
    let def = b::definition(block);
    let tier = |t: b::Tier| match t {
        b::Tier::Hand => Tier::Hand,
        b::Tier::Stone => Tier::Stone,
        b::Tier::Flint => Tier::Flint,
        b::Tier::Copper => Tier::Copper,
        b::Tier::Bronze => Tier::Bronze,
        b::Tier::Iron => Tier::Iron,
    };
    *out = BlockTooling {
        needs: tier(def.needs),
        work: match def.work {
            b::Work::Any => Work::Any,
            b::Work::Stone => Work::Stone,
            b::Work::Wood => Work::Wood,
            b::Work::Plant => Work::Plant,
            b::Work::Ground => Work::Ground,
        },
        tool: def.tool.map(tier).unwrap_or(Tier::Hand),
        is_tool: def.tool.is_some(),
        durability: def.durability.unwrap_or(0),
        // Negative for "there is no such state", on the same convention
        // `BlockProperties::hardness` uses for "nothing takes this
        // apart": a sentinel a mod can test with one comparison beats an
        // extra bool nobody reads.
        felled: def.felled.unwrap_or(-1.0),
        leaves_behind: def.leaves_behind.unwrap_or(0),
        drag: def.drag,
        grip: def.grip,
        thickness: def.thickness,
    };
    Status::Ok
}

unsafe extern "C" fn break_seconds(
    _handle: HostHandle,
    block: BlockId,
    tool: BlockId,
    out: *mut f32,
) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    if !primitive_shared::types::is_known_block(block) {
        return Status::NotFound;
    }
    let held = (tool != 0).then_some(tool);
    if held.is_some_and(|id| !primitive_shared::types::is_known_block(id)) {
        return Status::NotFound;
    }
    match primitive_shared::types::break_seconds_with(block, held) {
        Some(seconds) => {
            *out = seconds;
            Status::Ok
        }
        // Not `NotFound`: the block exists and so does the tool. What is
        // refused is the pairing, and that is the whole of the tool
        // ladder -- a flint pick does not get into iron ore, ever.
        None => Status::Refused,
    }
}

unsafe extern "C" fn same_kind(_handle: HostHandle, a: BlockId, b: BlockId) -> bool {
    primitive_shared::types::block_kind(a) == primitive_shared::types::block_kind(b)
}

// ---- ItemsApi ----

unsafe extern "C" fn item_spawn_worn(
    handle: HostHandle,
    at: Vec3,
    velocity: Vec3,
    stack: ItemStack,
    out: *mut EntityId,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(position) = finite(at) else {
        return Status::BadArgument;
    };
    let motion = finite(velocity).unwrap_or((0.0, 0.0, 0.0));
    if stack.count == 0 || !primitive_shared::types::is_known_block(stack.block) {
        return Status::BadArgument;
    }
    let spawned = {
        let mut items = context.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn_worn(
            stack.block,
            stack.count,
            stack.damage,
            primitive_shared::geometry::wide(position),
            motion,
            None,
            std::time::Instant::now(),
        )
    };
    if !out.is_null() {
        // The item store is keyed by position and merge rather than by a
        // stable id, so there is no id to report. See `ItemsApi::remove`.
        *out = 0;
    }
    if spawned {
        Status::Ok
    } else {
        Status::Refused
    }
}

unsafe extern "C" fn clear_near(
    handle: HostHandle,
    at: Vec3,
    radius: f32,
    removed: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(position) = finite(at) else {
        return Status::BadArgument;
    };
    if !radius.is_finite() || radius < 0.0 {
        return Status::BadArgument;
    }
    let gone = {
        let mut items = context.items.lock().unwrap_or_else(|e| e.into_inner());
        items.clear_near(primitive_shared::geometry::wide(position), radius)
    };
    if !removed.is_null() {
        *removed = gone;
    }
    Status::Ok
}

unsafe extern "C" fn item_lifetime_seconds(_handle: HostHandle) -> f32 {
    crate::items::LIFETIME.as_secs_f32()
}

unsafe extern "C" fn item_capacity(_handle: HostHandle) -> u32 {
    crate::items::MAX_ITEMS as u32
}

// ---- EntitiesApi ----

unsafe extern "C" fn species_count(_handle: HostHandle) -> u32 {
    primitive_shared::animals::Species::ALL.len() as u32
}

unsafe extern "C" fn species_info(
    _handle: HostHandle,
    species: u32,
    out: *mut SpeciesInfo,
) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(&kind) = primitive_shared::animals::Species::ALL.get(species as usize) else {
        return Status::NotFound;
    };
    *out = SpeciesInfo {
        max_health: kind.health(),
        damage: kind.damage(),
        walk_speed: kind.walk_speed(),
        run_speed: kind.run_speed(),
        hostile: kind.is_hostile(),
        awareness: kind.awareness(),
        provoke_range: kind.provoke_range(),
        height: kind.height(),
        width: kind.width(),
        length: kind.length(),
        // Still the old heap, and on purpose: it is what a mod is told
        // about a *species*, and a species does not have a carcass yet
        // when the question is asked. What it leaves on the ground when
        // it dies is now a block (`Species::carcass`) that is taken
        // apart by hand -- see `Species::butchering` -- and this list
        // is what that block falls back to over water. A mod that wants
        // the cuts reads the block table.
        drop_count: kind.drops().len() as u32,
    };
    Status::Ok
}

unsafe extern "C" fn species_drop(
    _handle: HostHandle,
    species: u32,
    index: u32,
    out: *mut ItemStack,
) -> Status {
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(&kind) = primitive_shared::animals::Species::ALL.get(species as usize) else {
        return Status::NotFound;
    };
    match kind.drops().get(index as usize) {
        Some(&(block, count)) => {
            *out = ItemStack {
                block,
                count,
                damage: 0,
            };
            Status::Ok
        }
        None => Status::NotFound,
    }
}

unsafe extern "C" fn entity_heal(handle: HostHandle, id: EntityId, amount: f32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !amount.is_finite() || amount < 0.0 {
        return Status::BadArgument;
    }
    let mut animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
    if animals.heal(id, amount) {
        Status::Ok
    } else {
        Status::NotFound
    }
}

unsafe extern "C" fn entities_all(
    handle: HostHandle,
    out: *mut EntityId,
    cap: usize,
    written: *mut usize,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let found: Vec<EntityId> = {
        let animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.ids()
    };
    if !written.is_null() {
        *written = found.len();
    }
    if out.is_null() || cap == 0 {
        return Status::Ok;
    }
    for (i, id) in found.iter().take(cap).enumerate() {
        *out.add(i) = *id;
    }
    Status::Ok
}

unsafe extern "C" fn entity_kill(handle: HostHandle, id: EntityId) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    // Enough damage that nothing survives it, through the same path a
    // blow takes -- so the carcass lands where it fell and whatever else
    // watches a death hears about it. `f32::MAX` rather than the
    // species' health, because asking for that is a second lookup and a
    // race with a boar being hit at the same moment.
    let mut animals = context.animals.lock().unwrap_or_else(|e| e.into_inner());
    if animals.health(id).is_none() {
        return Status::NotFound;
    }
    // The body falls, and the tick loop lays it: see `entity_damage`.
    let _ = animals.hurt(id, f32::MAX);
    Status::Ok
}

// ---- PlayersApi ----

unsafe extern "C" fn player_look(handle: HostHandle, player: PlayerId, out: *mut Vec3) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let (yaw, pitch) = {
        let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.yaw, state.pitch)
    };
    // The same basis the client camera's `forward` uses, and the same
    // one a thrown stack leaves along -- see `drop_from_slot`. Two
    // conventions for "which way is somebody facing" is one too many.
    *out = Vec3 {
        x: yaw.cos() * pitch.cos(),
        y: pitch.sin(),
        z: yaw.sin() * pitch.cos(),
    };
    Status::Ok
}

unsafe extern "C" fn player_on_ground(handle: HostHandle, player: PlayerId) -> bool {
    let Some(context) = ctx(handle) else {
        return false;
    };
    let Some(h) = context.registry.get(player) else {
        return false;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    state.on_ground
}

unsafe extern "C" fn player_selected_slot(
    handle: HostHandle,
    player: PlayerId,
    out: *mut u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = state.selected_slot as u32;
    Status::Ok
}

unsafe extern "C" fn player_set_selected_slot(
    handle: HostHandle,
    player: PlayerId,
    slot: u32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    if slot as usize >= primitive_shared::inventory::HOTBAR_SLOTS {
        return Status::BadArgument;
    }
    crate::select_slot(context, &h, slot as usize);
    Status::Ok
}

unsafe extern "C" fn player_respawn(handle: HostHandle, player: PlayerId) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let dead = {
        let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        state.vitals.is_dead()
    };
    if !dead {
        // A respawn is not a teleport. Refused rather than obeyed,
        // because obeying it would silently give a living player full
        // health and put them at spawn, which is a completely different
        // thing from what the call is named after.
        return Status::Refused;
    }
    crate::respawn_player(context, &h);
    Status::Ok
}

unsafe extern "C" fn player_set_health(
    handle: HostHandle,
    player: PlayerId,
    health: f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !health.is_finite() || health < 0.0 {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        // Refused for the dead, on `heal_player`'s argument: writing a
        // number into a corpse through `set_health` clears `dead` and
        // raises it where it fell, with the death screen up and the
        // respawn refused. `player_respawn` is the call for that.
        if state.vitals.is_dead() {
            return Status::Refused;
        }
        state
            .vitals
            .set_health(health.min(crate::survival::MAX_HEALTH));
    }
    // `Changed` and never `Died`, whatever the number was: dying is a
    // whole event -- a cause, a screen, a pack on the ground -- and
    // inferring it from a zero somebody wrote is how a player ends up
    // dead with their inventory still on them. See the doc comment.
    crate::report_vitals(context, &h, crate::survival::Outcome::Changed);
    Status::Ok
}

unsafe extern "C" fn player_set_warmth(
    handle: HostHandle,
    player: PlayerId,
    body_temperature_c: f32,
    wetness: f32,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if !body_temperature_c.is_finite() || !wetness.is_finite() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .vitals
            .set_warmth(body_temperature_c, wetness.clamp(0.0, 1.0));
    }
    crate::send_body(&h);
    Status::Ok
}

unsafe extern "C" fn player_is_submerged(handle: HostHandle, player: PlayerId) -> bool {
    let Some(context) = ctx(handle) else {
        return false;
    };
    let Some(h) = context.registry.get(player) else {
        return false;
    };
    let position = {
        let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        state.position
    };
    crate::head_under_water(context, primitive_shared::geometry::narrow(position))
}

// ---- InventoryApi ----

unsafe extern "C" fn held(handle: HostHandle, player: PlayerId, out: *mut ItemStack) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    let slot = state.selected_slot;
    *out = match state.inventory.slots().get(slot).copied().flatten() {
        Some(stack) => ItemStack {
            block: stack.block,
            count: stack.count,
            damage: stack.damage,
        },
        None => ItemStack::EMPTY,
    };
    Status::Ok
}

unsafe extern "C" fn spill(handle: HostHandle, player: PlayerId, dropped: *mut u32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let (contents, position, count) = {
        let mut state = h.state.lock().unwrap_or_else(|e| e.into_inner());
        let contents = state.inventory.clone();
        let count = contents.slots().iter().filter(|s| s.is_some()).count() as u32;
        state.inventory = primitive_shared::inventory::Inventory::new();
        state.inventory_dirty = true;
        (contents, state.position, count)
    };
    crate::spill_inventory(context, &contents, primitive_shared::geometry::narrow(position));
    crate::send_inventory(&h);
    crate::refresh_carried_weight(&h);
    if !dropped.is_null() {
        *dropped = count;
    }
    Status::Ok
}

unsafe extern "C" fn drop_slot(
    handle: HostHandle,
    player: PlayerId,
    slot: u32,
    whole_stack: bool,
) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    if slot as usize >= primitive_shared::inventory::SLOTS {
        return Status::BadArgument;
    }
    if crate::drop_from_slot(context, &h, slot as usize, whole_stack) {
        Status::Ok
    } else {
        Status::Refused
    }
}

unsafe extern "C" fn used_slots(handle: HostHandle, player: PlayerId, out: *mut u32) -> Status {
    let Some(context) = ctx(handle) else {
        return Status::BadArgument;
    };
    if out.is_null() {
        return Status::BadArgument;
    }
    let Some(h) = context.registry.get(player) else {
        return Status::NotFound;
    };
    let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
    *out = state.inventory.slots().iter().filter(|s| s.is_some()).count() as u32;
    Status::Ok
}

// ---- PhysicsApi ----

unsafe extern "C" fn carry_capacity_kg(_handle: HostHandle) -> f32 {
    primitive_shared::load::CARRY_CAPACITY_KG
}

unsafe extern "C" fn load_speed_scale(_handle: HostHandle, kilograms: f32) -> f32 {
    primitive_shared::load::speed_scale(primitive_shared::load::sanitize(kilograms))
}

unsafe extern "C" fn load_fall_multiplier(_handle: HostHandle, kilograms: f32) -> f32 {
    primitive_shared::load::fall_multiplier(primitive_shared::load::sanitize(kilograms))
}

unsafe extern "C" fn block_drag(_handle: HostHandle, block: BlockId) -> f32 {
    if !primitive_shared::types::is_known_block(block) {
        return 1.0;
    }
    primitive_shared::blocks::definition(block).drag
}

unsafe extern "C" fn block_grip(_handle: HostHandle, block: BlockId) -> f32 {
    if !primitive_shared::types::is_known_block(block) {
        return 1.0;
    }
    primitive_shared::blocks::definition(block).grip
}

unsafe extern "C" fn safe_fall_blocks(_handle: HostHandle) -> f32 {
    crate::survival::SAFE_FALL_BLOCKS
}
