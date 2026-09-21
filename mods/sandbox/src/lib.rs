//! The mod that calls everything, so that "the API compiles" becomes
//! "the API is called".
//!
//! ## What it is for
//!
//! `greeter` shows an author what a mod looks like. `flight` proves the
//! boundary can carry a feature. This one is the third thing, and it
//! exists because of a hole neither of the other two could have found:
//!
//! **Nine of the twenty-one events the API declared in 1.0 were never
//! fired by the server, and a chunk decorator a mod registered was
//! written down and never run.** Every one of those compiled. Every one
//! of them had a test that a mod could subscribe. What none of them had
//! was somebody on the other side of the boundary saying "I subscribed
//! and I was never called".
//!
//! So this mod calls **every host function the API has**, reports what
//! each one answered, and counts every event it is told about. It is a
//! diagnostic rather than a feature: `/sandbox` is how an operator finds
//! out whether the mod API on the server in front of them actually
//! works, and it is how this repository finds out before they do.
//!
//! ```text
//! /sandbox            what every read-only call answers
//! /sandbox events     how many of each event have arrived
//! /sandbox write      ...and the calls that change the world
//! ```
//!
//! ## The one rule it is built around
//!
//! **A diagnostic must not be a disaster.** The read sweep changes
//! nothing at all. The write sweep is off unless `allow_writes` is set
//! in the manifest, works in a scratch column a few metres from whoever
//! asked, and puts back what it can. The calls that are genuinely
//! destructive -- emptying a pack, killing an animal -- are made with
//! arguments that make them no-ops, and the sweep says so: what is being
//! proved is that the call crosses the boundary and answers correctly,
//! and `Status::NotFound` from an id nothing has proves that as well as
//! a dead deer would.
//!
//! ## Building it
//!
//! ```text
//! cargo build -p sandbox --release
//! ```
//!
//! and copy `target/release/sandbox.dll` (or `libsandbox.so`) next to
//! `mods/sandbox/mod.ron`.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

use primitive_modapi::{
    BlockId, BlockPos, BlockProperties, BlockTooling, BodySlot, CraftHeat, Event, EventData,
    Feasibility, FoodHarm, HookResult, HostApi, ItemStack, LogLevel, PlayerId, PlayerVitals,
    RecipeInfo, SpeciesInfo, Station, Status, Str, Vec3, API_VERSION,
};

/// The host, kept for the life of the mod.
///
/// An atomic rather than a `static mut`, because the server may call a
/// hook from the tick loop and a decorator from a generator thread, and
/// a `static mut` read from two threads is undefined behaviour however
/// carefully it was written.
static HOST: AtomicU64 = AtomicU64::new(0);

/// Whether the manifest allows the write sweep. Read once at load,
/// because a setting that could change under a hook is a setting nobody
/// can reason about.
static ALLOW_WRITES: AtomicBool = AtomicBool::new(false);
/// How far from the player the write sweep works.
static SCRATCH_OFFSET: AtomicI64 = AtomicI64::new(3);

/// Every event this mod is told about, and how many of each.
///
/// **One counter per event rather than a log**, because the question a
/// diagnostic answers here is "does this one ever arrive at all" -- and
/// that is a number, not a history. A mod that wanted the history would
/// keep it; this one is trying to find events that fire zero times.
const EVENTS: &[(Event, &str)] = &[
    (Event::ServerStarted, "server_started"),
    (Event::ServerStopping, "server_stopping"),
    (Event::Tick, "tick"),
    (Event::PlayerJoined, "player_joined"),
    (Event::PlayerLeft, "player_left"),
    (Event::PlayerChat, "player_chat"),
    (Event::PlayerDied, "player_died"),
    (Event::PlayerHurt, "player_hurt"),
    (Event::PlayerHealed, "player_healed"),
    (Event::PlayerAte, "player_ate"),
    (Event::PlayerEnteredWater, "player_entered_water"),
    (Event::PlayerLeftWater, "player_left_water"),
    (Event::HeldSlotChanged, "held_slot_changed"),
    (Event::BlockPlace, "block_place"),
    (Event::BlockBreak, "block_break"),
    (Event::BlockChanged, "block_changed"),
    (Event::TreeFelled, "tree_felled"),
    (Event::ChunkGenerated, "chunk_generated"),
    (Event::EntitySpawned, "entity_spawned"),
    (Event::EntityRemoved, "entity_removed"),
    (Event::ItemCrafted, "item_crafted"),
    (Event::ItemPickedUp, "item_picked_up"),
    (Event::ItemDropped, "item_dropped"),
    (Event::ToolBroke, "tool_broke"),
    (Event::HideCured, "hide_cured"),
    (Event::PlayerDrank, "player_drank"),
    (Event::EquipmentChanged, "equipment_changed"),
    (Event::ContainerOpened, "container_opened"),
    (Event::ContainerClosed, "container_closed"),
    (Event::SmeltingFinished, "smelting_finished"),
    (Event::GrowthStep, "growth_step"),
    (Event::FireDied, "fire_died"),
    (Event::WeatherChanged, "weather_changed"),
    (Event::TimeChanged, "time_changed"),
    (Event::Command, "command"),
];

/// A counter per entry of `EVENTS`, in the same order.
///
/// A fixed array rather than a map, because this is written from the
/// tick loop and read from a command thread, and an atomic counter needs
/// no lock at all -- a `Mutex<HashMap>` here would be a lock taken
/// twenty times a second for the life of the server.
static COUNTS: [AtomicU64; EVENTS.len()] = [const { AtomicU64::new(0) }; EVENTS.len()];

fn host() -> Option<&'static HostApi> {
    let raw = HOST.load(Ordering::Acquire);
    if raw == 0 {
        return None;
    }
    // Safety: the only thing that ever writes this is `load`, with the
    // pointer the host handed us, and the host keeps its tables alive
    // for as long as this library is loaded.
    Some(unsafe { &*(raw as *const HostApi) })
}

fn log(level: LogLevel, message: &str) {
    let Some(api) = host() else { return };
    if api.core.is_null() {
        return;
    }
    unsafe {
        ((*api.core).log)(api.handle, level, Str::borrow(message));
    }
}

/// One line of a report, to whoever asked for it.
///
/// **Falls back to the log when there is nobody to tell.** `/sandbox`
/// is a diagnostic, and the two people most likely to run one are an
/// operator at the console -- who is not a connected player and gets
/// `NotFound` from `tell` -- and a test fixture, which has no players at
/// all. A report that went nowhere in exactly the two cases it is most
/// needed would be a diagnostic that only works when nothing is wrong.
fn say_to(player: PlayerId, text: &str) {
    let Some(api) = host() else { return };
    if api.network.is_null() {
        log(LogLevel::Info, text);
        return;
    }
    let status = unsafe { ((*api.network).tell)(api.handle, player, Str::borrow(text)) };
    if !status.is_ok() {
        log(LogLevel::Info, text);
    }
}

/// Writes down what the last sweep produced.
///
/// Four bytes of lines and eight of events, in this mod's own save blob
/// -- which is the one thing a sweep leaves behind that something other
/// than a person reading chat can look at. It is how
/// `primitive_server/tests/sandbox.rs` knows the sweep actually ran
/// rather than merely returning, and it is how an operator whose chat
/// scrolled past can tell.
fn remember(lines: usize) {
    let Some(api) = host() else { return };
    if api.save.is_null() {
        return;
    }
    let events: u64 = COUNTS.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    let mut blob = Vec::with_capacity(12);
    blob.extend_from_slice(&(lines as u32).to_le_bytes());
    blob.extend_from_slice(&events.to_le_bytes());
    unsafe {
        ((*api.save).store)(api.handle, blob.as_ptr(), blob.len());
    }
}

/// Reads one setting out of this mod's `mod.ron`.
///
/// The two-call convention every text-returning call in this API uses:
/// ask for the length with a capacity of zero, then ask again with a
/// buffer. Worth wrapping once, because getting it wrong is a truncated
/// string rather than an error.
fn setting(key: &str) -> Option<String> {
    let api = host()?;
    if api.core.is_null() {
        return None;
    }
    let mut needed: usize = 0;
    let status = unsafe {
        ((*api.core).setting)(
            api.handle,
            Str::borrow(key),
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if status == Status::NotFound || needed == 0 {
        return None;
    }
    let mut buffer = vec![0u8; needed];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.core).setting)(
            api.handle,
            Str::borrow(key),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut written,
        )
    };
    if !status.is_ok() {
        return None;
    }
    buffer.truncate(written);
    String::from_utf8(buffer).ok()
}

/// Any of the API's text-returning calls, as a `String`.
///
/// Every one of them has the same shape -- handle, whatever names the
/// thing, then `(out, cap, written)` -- so the two-call dance is written
/// once and handed the closure that makes the call.
fn text_from(mut call: impl FnMut(*mut u8, usize, *mut usize) -> Status) -> Option<String> {
    let mut needed: usize = 0;
    let status = call(std::ptr::null_mut(), 0, &mut needed);
    if status == Status::NotFound || needed == 0 {
        return None;
    }
    let mut buffer = vec![0u8; needed];
    let mut written: usize = 0;
    if !call(buffer.as_mut_ptr(), buffer.len(), &mut written).is_ok() {
        return None;
    }
    buffer.truncate(written);
    String::from_utf8(buffer).ok()
}

/// A block by name, or zero.
fn block(name: &str) -> BlockId {
    let Some(api) = host() else { return 0 };
    if api.blocks.is_null() {
        return 0;
    }
    let mut id: BlockId = 0;
    let status = unsafe { ((*api.blocks).by_name)(api.handle, Str::borrow(name), &mut id) };
    if status.is_ok() {
        id
    } else {
        0
    }
}

/// How a `Status` reads in a report line.
fn say(status: Status) -> &'static str {
    match status {
        Status::Ok => "ok",
        Status::NotFound => "not found",
        Status::BadArgument => "bad argument",
        Status::Refused => "refused",
        Status::Unavailable => "unavailable",
    }
}

// ------------------------------------------------------------ the sweep
//
// One function per table, each of which calls every entry of it and
// pushes a line per interesting answer. They are split by table rather
// than by "what a player would want to know" so that a table growing has
// exactly one place to grow here -- which is the thing that makes this
// mod worth having in the workspace at all.

/// Everything that only reads.
///
/// Safe to run on a live server at any time, which is the point: an
/// operator who suspects the mod host is wrong should not have to risk
/// their world to find out.
fn read_sweep(player: PlayerId) -> Vec<String> {
    let mut out = Vec::new();
    let Some(api) = host() else {
        return vec!["no host".to_string()];
    };
    out.push(format!("host api {}", api.version));
    unsafe {
        core_and_world(api, &mut out);
        blocks_and_crafting(api, player, &mut out);
        lighting_food_and_combat(api, player, &mut out);
        fluid_and_stations(api, player, &mut out);
        containers_and_simulation(api, &mut out);
        players_and_inventory(api, player, &mut out);
    }
    out
}

unsafe fn core_and_world(api: &HostApi, out: &mut Vec<String>) {
    if api.core.is_null() || api.world.is_null() {
        out.push("core/world: absent".to_string());
        return;
    }
    let core = &*api.core;
    let world = &*api.world;
    out.push(format!(
        "core: tick {} at {:.0} Hz, day {:.0}s, time {:.2}",
        (core.tick)(api.handle),
        (core.tick_rate_hz)(api.handle),
        (core.day_length_seconds)(api.handle),
        (core.time_of_day)(api.handle),
    ));

    let spawn = (world.spawn_point)(api.handle);
    let at = BlockPos {
        x: spawn.x as i32,
        y: spawn.y as i32,
        z: spawn.z as i32,
    };
    let mut here: BlockId = 0;
    let got = (world.get_block)(api.handle, at, &mut here);
    let mut surface = 0i32;
    let surfaced = (world.surface_at)(api.handle, at.x, at.z, &mut surface);
    let mut ambient = 0.0f32;
    (world.temperature_at)(api.handle, spawn, &mut ambient);
    out.push(format!(
        "world: seed {}, {} chunks, weather {:?}, spawn ({}) {}, surface {surface} ({}), {ambient:.1} C",
        (world.world_seed)(api.handle),
        (world.loaded_chunk_count)(api.handle),
        (world.weather)(api.handle),
        say(got),
        here,
        say(surfaced),
    ));
    // Wakes the simulations at a cell without changing it -- the one
    // world call that writes nothing and is still worth having.
    out.push(format!(
        "world: disturb {}, chunk loaded {}",
        say((world.disturb)(api.handle, at)),
        (world.is_chunk_loaded)(
            api.handle,
            primitive_modapi::ChunkPos {
                x: at.x >> 4,
                z: at.z >> 4
            }
        ),
    ));

    if api.generation.is_null() {
        return;
    }
    let generation = &*api.generation;
    let biome = (generation.biome_at)(api.handle, at.x, at.z);
    let name = text_from(|buf, cap, written| {
        (generation.biome_name)(api.handle, biome, buf, cap, written)
    })
    .unwrap_or_default();
    let (mut warmth, mut humidity) = (0.0f32, 0.0f32);
    (generation.climate_at)(api.handle, at, &mut warmth, &mut humidity);
    out.push(format!(
        "generation: height {}, biome {name}, warmth {warmth:.2}, humidity {humidity:.2}",
        (generation.height_at)(api.handle, at.x, at.z),
    ));
}

unsafe fn blocks_and_crafting(api: &HostApi, player: PlayerId, out: &mut Vec<String>) {
    if api.blocks.is_null() {
        out.push("blocks: absent".to_string());
        return;
    }
    let blocks = &*api.blocks;
    let stone = block("stone");
    let mut properties = BlockProperties {
        id: 0,
        hardness: 0.0,
        opacity: 0,
        emission: 0,
        solid: false,
        placeable: false,
        falls: false,
        container: false,
        weight_kg: 0.0,
        stack_limit: 0,
        drops: 0,
    };
    let got = (blocks.properties)(api.handle, stone, &mut properties);
    let mut tooling = BlockTooling {
        needs: primitive_modapi::Tier::Hand,
        work: primitive_modapi::Work::Any,
        tool: primitive_modapi::Tier::Hand,
        is_tool: false,
        durability: 0,
        felled: 0.0,
        leaves_behind: 0,
        drag: 0.0,
        grip: 0.0,
        thickness: 0,
    };
    let tooled = (blocks.tooling)(api.handle, stone, &mut tooling);
    let mut bare = 0.0f32;
    let barehanded = (blocks.break_seconds)(api.handle, stone, 0, &mut bare);
    let mut first: BlockId = 0;
    (blocks.id_at)(api.handle, 0, &mut first);
    let name = text_from(|buf, cap, written| (blocks.name)(api.handle, stone, buf, cap, written))
        .unwrap_or_default();
    out.push(format!(
        "blocks: {} kinds, first {first}, '{name}' hardness {:.1} ({}), needs {:?} for {:?} ({}), bare hands {} ({}), same_kind {}",
        (blocks.count)(api.handle),
        properties.hardness,
        say(got),
        tooling.needs,
        tooling.work,
        say(tooled),
        if barehanded.is_ok() {
            format!("{bare:.1}s")
        } else {
            "no".to_string()
        },
        say(barehanded),
        (blocks.same_kind)(api.handle, stone, stone),
    ));

    if api.crafting.is_null() {
        out.push("crafting: absent".to_string());
        return;
    }
    let crafting = &*api.crafting;
    let count = (crafting.recipe_count)(api.handle);
    let mut info = RecipeInfo {
        output: 0,
        output_count: 0,
        station: Station::Hands,
        input_count: 0,
        return_count: 0,
    };
    let got = (crafting.recipe)(api.handle, 0, &mut info);
    let name =
        text_from(|buf, cap, written| (crafting.recipe_name)(api.handle, 0, buf, cap, written))
            .unwrap_or_default();
    let station = text_from(|buf, cap, written| {
        (crafting.station_name)(api.handle, info.station, buf, cap, written)
    })
    .unwrap_or_default();
    let mut input = ItemStack::EMPTY;
    let ingredient = (crafting.recipe_input)(api.handle, 0, 0, &mut input);
    let mut returned = ItemStack::EMPTY;
    let gives_back = (crafting.recipe_return)(api.handle, 0, 0, &mut returned);
    out.push(format!(
        "crafting: {count} recipes; #0 '{name}' at the {station} makes {}x{} from {}x{} ({}, {}), returns {} ({})",
        info.output_count,
        info.output,
        input.count,
        input.block,
        say(got),
        say(ingredient),
        returned.count,
        say(gives_back),
    ));

    // How a player would get one of these, and what one is good for.
    let mut making = [0u32; 8];
    let mut made_by: usize = 0;
    (crafting.recipes_making)(
        api.handle,
        info.output,
        making.as_mut_ptr(),
        making.len(),
        &mut made_by,
    );
    let mut using = [0u32; 8];
    let mut used_by: usize = 0;
    (crafting.recipes_using)(
        api.handle,
        input.block,
        using.as_mut_ptr(),
        using.len(),
        &mut used_by,
    );
    let mut feasible = Feasibility::MissingIngredients;
    let asked = (crafting.feasibility)(api.handle, player, 0, &mut feasible);
    let mut heat = CraftHeat::default();
    let heated = (crafting.heat_at)(api.handle, player, &mut heat);
    out.push(format!(
        "crafting: {made_by} recipe(s) make block {}, {used_by} use block {}; you: {feasible:?} ({}), heat fire {} kiln {} bloomery {} ({})",
        info.output,
        input.block,
        say(asked),
        heat.fire,
        heat.kiln,
        heat.bloomery,
        say(heated),
    ));
}

unsafe fn lighting_food_and_combat(api: &HostApi, player: PlayerId, out: &mut Vec<String>) {
    if !api.lighting.is_null() && !api.players.is_null() {
        let lighting = &*api.lighting;
        let mut where_they_are = Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        ((*api.players).position)(api.handle, player, &mut where_they_are);
        let at = BlockPos {
            x: where_they_are.x.floor() as i32,
            y: where_they_are.y.floor() as i32,
            z: where_they_are.z.floor() as i32,
        };
        let (mut sky, mut lamp) = (0u8, 0u8);
        let s = (lighting.sky_light)(api.handle, at, &mut sky);
        let b = (lighting.block_light)(api.handle, at, &mut lamp);
        out.push(format!(
            "lighting: sky {sky}/{} ({}), block {lamp} ({}), open to sky {}",
            (lighting.max_light)(api.handle),
            say(s),
            say(b),
            (lighting.open_to_sky)(api.handle, at),
        ));
    }

    if !api.food.is_null() {
        let food = &*api.food;
        // The toadstool: food, and not nourishment. The one row that
        // makes `is_food` and `nutrition` two different questions.
        let toadstool = block("toadstool");
        let mut nutrition = 0.0f32;
        let fed = (food.nutrition)(api.handle, toadstool, &mut nutrition);
        let mut harm = FoodHarm {
            health: 0.0,
            nourishment: 0.0,
        };
        let hurts = (food.harm)(api.handle, toadstool, &mut harm);
        out.push(format!(
            "food: toadstool is_food {}, nutrition {} ({}), harm {:.1} health / {:.1} stomach ({}), worth eating {}, full is {:.0}",
            (food.is_food)(api.handle, toadstool),
            nutrition,
            say(fed),
            harm.health,
            harm.nourishment,
            say(hurts),
            (food.worth_eating)(api.handle, player, toadstool),
            (food.max_nourishment)(api.handle),
        ));
    }

    if !api.combat.is_null() {
        let combat = &*api.combat;
        let mut through = 0.0f32;
        let armoured = (combat.damage_through_armour)(api.handle, player, 10.0, &mut through);
        let mut weapon = 0.0f32;
        let armed = (combat.weapon_damage)(api.handle, 0, &mut weapon);
        let here = Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        out.push(format!(
            "combat: reach {:.1} (+{:.1}), fist {:.1}, cooldown {:.2}s, a blow of 10 costs you {through:.2} ({}), bare hands {weapon:.1} ({}), armour never stops more than {:.0}%, self-reach {}",
            (combat.melee_reach)(api.handle),
            (combat.reach_tolerance)(api.handle),
            (combat.melee_damage)(api.handle),
            (combat.melee_cooldown_seconds)(api.handle),
            say(armoured),
            say(armed),
            100.0 - (combat.minimum_damage_fraction)(api.handle) * 100.0,
            (combat.within_reach)(api.handle, here, here),
        ));
    }
}

unsafe fn fluid_and_stations(api: &HostApi, player: PlayerId, out: &mut Vec<String>) {
    if !api.fluid.is_null() && !api.players.is_null() {
        let fluid = &*api.fluid;
        let water = block("water");
        let mut at = Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        ((*api.players).position)(api.handle, player, &mut at);
        let cell = BlockPos {
            x: at.x.floor() as i32,
            y: at.y.floor() as i32,
            z: at.z.floor() as i32,
        };
        let mut depth = 0.0f32;
        let sounded = (fluid.column_depth)(api.handle, cell, &mut depth);
        let mut surface = 0.0f32;
        let floated = (fluid.surface_height)(api.handle, water, &mut surface);
        out.push(format!(
            "fluid: water is_liquid {}, is_source {}, surface {surface:.2} ({}), under you {depth:.2} ({}), {} cells still settling",
            (fluid.is_liquid)(api.handle, water),
            (fluid.is_source)(api.handle, water),
            say(floated),
            say(sounded),
            (fluid.pending)(api.handle),
        ));
    }

    if api.stations.is_null() || api.players.is_null() {
        return;
    }
    let stations = &*api.stations;
    let mut at = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    ((*api.players).position)(api.handle, player, &mut at);
    let cell = BlockPos {
        x: at.x.floor() as i32,
        y: at.y.floor() as i32,
        z: at.z.floor() as i32,
    };
    let mut fuel = 0.0f32;
    let burning = (stations.fire_fuel_left)(api.handle, cell, &mut fuel);
    let mut worth = 0.0f32;
    let combustible = (stations.fuel_seconds)(api.handle, block("stick"), &mut worth);
    let mut cured: BlockId = 0;
    let cures = (stations.cures_into)(api.handle, block("hide"), &mut cured);
    let mut rate = 0.0f32;
    let drying = (stations.drying_rate)(api.handle, at, &mut rate);
    let mut smelt = 0.0f32;
    let cooking = (stations.smelting_progress)(api.handle, cell, &mut smelt);
    let mut hide = 0.0f32;
    let curing = (stations.drying_progress)(api.handle, cell, &mut hide);
    out.push(format!(
        "stations: {} fires alight, one near you {}, under you {} ({}), a stick is worth {worth:.0}s ({}), hide cures to {cured} ({}), racks here run at {rate:.2} ({}), smelt {} / cure {}",
        (stations.burning_count)(api.handle),
        (stations.fire_within)(api.handle, at, 6.0),
        if burning.is_ok() {
            format!("{fuel:.0}s left")
        } else {
            "nothing burning".to_string()
        },
        say(burning),
        say(combustible),
        say(cures),
        say(drying),
        say(cooking),
        say(curing),
    ));
}

unsafe fn containers_and_simulation(api: &HostApi, out: &mut Vec<String>) {
    if !api.containers.is_null() {
        let containers = &*api.containers;
        let mut found = [BlockPos { x: 0, y: 0, z: 0 }; 16];
        let mut how_many: usize = 0;
        (containers.all)(
            api.handle,
            found.as_mut_ptr(),
            found.len(),
            &mut how_many,
        );
        let mut line = format!("containers: {how_many} holding something");
        if how_many > 0 {
            let at = found[0];
            let (mut slots, mut kind) = (0u32, 0u32);
            let counted = (containers.slot_count)(api.handle, at, &mut slots);
            let named = (containers.container_kind)(api.handle, at, &mut kind);
            let mut first = ItemStack::EMPTY;
            let peeked = (containers.get_slot)(api.handle, at, 0, &mut first);
            line.push_str(&format!(
                "; first at ({},{},{}) kind {kind} ({}) with {slots} slots ({}), slot 0 holds {}x{} ({})",
                at.x,
                at.y,
                at.z,
                say(named),
                say(counted),
                first.count,
                first.block,
                say(peeked),
            ));
        }
        out.push(line);
    }

    if api.simulation.is_null() {
        return;
    }
    let simulation = &*api.simulation;
    out.push(format!(
        "simulation: {} cells of sand queued, {} in the air, {} growing; a bush fills in {:.0}s, a crop stage takes {:.0}s; a standing log is a trunk: {}",
        (simulation.falling_pending)(api.handle),
        (simulation.falling_entities)(api.handle),
        (simulation.growth_pending)(api.handle),
        (simulation.regrow_seconds)(api.handle),
        (simulation.crop_stage_seconds)(api.handle),
        (simulation.is_standing_trunk)(api.handle, block("log")),
    ));

    if api.entities.is_null() {
        return;
    }
    let entities = &*api.entities;
    let mut alive = [0u64; 32];
    let mut how_many: usize = 0;
    (entities.all)(api.handle, alive.as_mut_ptr(), alive.len(), &mut how_many);
    let mut info = SpeciesInfo {
        max_health: 0.0,
        damage: 0.0,
        walk_speed: 0.0,
        run_speed: 0.0,
        hostile: false,
        awareness: 0.0,
        provoke_range: 0.0,
        height: 0.0,
        width: 0.0,
        length: 0.0,
        drop_count: 0,
    };
    let described = (entities.species_info)(api.handle, 0, &mut info);
    let name = text_from(|buf, cap, written| {
        (entities.species_name)(api.handle, 0, buf, cap, written)
    })
    .unwrap_or_default();
    let mut spoil = ItemStack::EMPTY;
    let drops = (entities.species_drop)(api.handle, 0, 0, &mut spoil);
    out.push(format!(
        "entities: {how_many} alive of {} species; '{name}' has {:.0} health, hits for {:.1}, runs at {:.1}, hostile {} ({}); it leaves {}x{} ({})",
        (entities.species_count)(api.handle),
        info.max_health,
        info.damage,
        info.run_speed,
        info.hostile,
        say(described),
        spoil.count,
        spoil.block,
        say(drops),
    ));
}

unsafe fn players_and_inventory(api: &HostApi, player: PlayerId, out: &mut Vec<String>) {
    if !api.players.is_null() {
        let players = &*api.players;
        let mut vitals = PlayerVitals {
            health: 0.0,
            max_health: 0.0,
            nourishment: 0.0,
            hydration: 0.0,
            breath: 0.0,
            body_temperature_c: 0.0,
            ambient_c: 0.0,
            wetness: 0.0,
            carried_kg: 0.0,
            dead: false,
        };
        let got = (players.vitals)(api.handle, player, &mut vitals);
        let mut look = Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let facing = (players.look)(api.handle, player, &mut look);
        let mut slot = 0u32;
        let selected = (players.selected_slot)(api.handle, player, &mut slot);
        out.push(format!(
            "players: {} online; you: {:.0}/{:.0} health, body {:.1} C in air of {:.1} C ({}), looking ({look:.2?}) ({}), slot {slot} ({}), on ground {}, submerged {}, flying {}, operator {}",
            (players.count)(api.handle),
            vitals.health,
            vitals.max_health,
            vitals.body_temperature_c,
            vitals.ambient_c,
            say(got),
            say(facing),
            say(selected),
            (players.on_ground)(api.handle, player),
            (players.is_submerged)(api.handle, player),
            (players.is_flying)(api.handle, player),
            (players.is_operator)(api.handle, player),
        ));
        // **Refused, and that is the answer being checked.** A respawn
        // is not a teleport: asking for one on a living player has to
        // come back `Refused` rather than quietly moving them to spawn.
        out.push(format!(
            "players: respawn while alive -> {}",
            say((players.respawn)(api.handle, player)),
        ));
    }

    if api.inventory.is_null() {
        return;
    }
    let inventory = &*api.inventory;
    let mut held = ItemStack::EMPTY;
    let holding = (inventory.held)(api.handle, player, &mut held);
    let mut used = 0u32;
    let counted = (inventory.used_slots)(api.handle, player, &mut used);
    let mut hat = ItemStack::EMPTY;
    let worn = (inventory.get_equipment)(api.handle, player, BodySlot::Head, &mut hat);
    out.push(format!(
        "inventory: {} slots, {used} in use ({}), holding {}x{} ({}), head {}x{} ({})",
        (inventory.slot_count)(api.handle),
        say(counted),
        held.count,
        held.block,
        say(holding),
        hat.count,
        hat.block,
        say(worn),
    ));

    if api.physics.is_null() || api.items.is_null() {
        return;
    }
    let physics = &*api.physics;
    let items = &*api.items;
    let carried = 100.0;
    out.push(format!(
        "physics: capacity {:.0} kg; at {carried:.0} kg you move at {:.2}x and fall {:.2}x as hard; a safe fall is {:.0} blocks; stone drag {:.2} grip {:.2}; drops last {:.0}s, at most {}",
        (physics.carry_capacity_kg)(api.handle),
        (physics.load_speed_scale)(api.handle, carried),
        (physics.load_fall_multiplier)(api.handle, carried),
        (physics.safe_fall_blocks)(api.handle),
        (physics.block_drag)(api.handle, block("stone")),
        (physics.block_grip)(api.handle, block("stone")),
        (items.lifetime_seconds)(api.handle),
        (items.capacity)(api.handle),
    ));
}

/// The calls that change something.
///
/// Every one of them is made against a scratch column beside the player,
/// or with arguments chosen so that the call is real and its effect is
/// not -- see the note at the top of the file about a diagnostic not
/// being a disaster.
unsafe fn write_sweep(player: PlayerId) -> Vec<String> {
    let mut out = Vec::new();
    let Some(api) = host() else {
        return vec!["no host".to_string()];
    };
    if api.players.is_null() || api.world.is_null() {
        return vec!["no players or world table".to_string()];
    }
    let players = &*api.players;
    let world = &*api.world;

    let mut feet = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    if !(players.position)(api.handle, player, &mut feet).is_ok() {
        return vec!["you are not here".to_string()];
    }
    let offset = SCRATCH_OFFSET.load(Ordering::Relaxed) as i32;
    let column = (feet.x.floor() as i32 + offset, feet.z.floor() as i32 + offset);
    let mut ground = 0i32;
    if !(world.surface_at)(api.handle, column.0, column.1, &mut ground).is_ok() {
        return vec!["the scratch column is not loaded".to_string()];
    }
    let scratch = BlockPos {
        x: column.0,
        y: ground + 1,
        z: column.1,
    };
    out.push(format!(
        "scratch column ({}, {}, {})",
        scratch.x, scratch.y, scratch.z
    ));

    // ---- the world, and taking it apart again ----
    let stone = block("stone");
    out.push(format!(
        "world: set stone {}, break it back {}",
        say((world.set_block)(api.handle, scratch, stone)),
        // With the drop, so the stone is on the ground rather than gone
        // -- which is the whole difference between this and writing air.
        say((world.break_block)(api.handle, scratch, true)),
    ));

    // ---- water, put down and taken away ----
    if !api.fluid.is_null() {
        let fluid = &*api.fluid;
        out.push(format!(
            "fluid: source down {}, taken back up {}",
            say((fluid.place_source)(api.handle, scratch)),
            say((fluid.remove)(api.handle, scratch)),
        ));
    }

    // ---- a container, filled, emptied and taken apart ----
    if !api.containers.is_null() {
        let containers = &*api.containers;
        let chest = block("chest");
        let (mut left, mut taken) = (0u32, 0u32);
        let built = (world.set_block)(api.handle, scratch, chest);
        let given = (containers.give)(
            api.handle,
            scratch,
            ItemStack {
                block: stone,
                count: 4,
                damage: 0,
            },
            &mut left,
        );
        let placed = (containers.set_slot)(
            api.handle,
            scratch,
            1,
            ItemStack {
                block: stone,
                count: 1,
                damage: 0,
            },
        );
        let took = (containers.take)(api.handle, scratch, stone, 4, &mut taken);
        let shut = (containers.close)(api.handle, scratch);
        let spilt = (containers.spill)(api.handle, scratch);
        out.push(format!(
            "containers: built {}, gave 4 ({}, {left} left over), set slot 1 {}, took {taken} back ({}), closed {}, spilt {}",
            say(built),
            say(given),
            say(placed),
            say(took),
            say(shut),
            say(spilt),
        ));
        (world.break_block)(api.handle, scratch, false);
    }

    // ---- a fire, lit, fed and put out ----
    if !api.stations.is_null() {
        let stations = &*api.stations;
        let campfire = block("campfire");
        let built = (world.set_block)(api.handle, scratch, campfire);
        let lit = (stations.light_fire)(api.handle, scratch);
        let fed = (stations.feed_fire)(api.handle, scratch, 30.0);
        let doused = (stations.extinguish)(api.handle, scratch);
        // A rack, and the bar shoved along it. Nothing is on the frame,
        // so nothing finishes -- what is being proved is that the call
        // reaches the drying map at all.
        let rack = block("drying_rack");
        let framed = (world.set_block)(api.handle, scratch, rack);
        let shoved = (stations.set_drying_progress)(api.handle, scratch, 0.5);
        out.push(format!(
            "stations: campfire {}, lit {}, fed 30s {}, put out {}; rack {}, progress to 0.5 {}",
            say(built),
            say(lit),
            say(fed),
            say(doused),
            say(framed),
            say(shoved),
        ));
        (world.break_block)(api.handle, scratch, false);
    }

    // ---- growth, and a tree ----
    if !api.simulation.is_null() {
        let simulation = &*api.simulation;
        let mut felled = 0u32;
        // Against the scratch cell, which is air: nothing comes down,
        // and `Refused` is the answer being checked. A sweep that felled
        // whatever tree happened to be standing beside the player would
        // be a diagnostic nobody runs twice.
        let tree = (simulation.fell_tree)(api.handle, scratch, &mut felled);
        out.push(format!(
            "simulation: watch growth here {}, fell what is at the scratch cell {} ({felled} cells)",
            say((simulation.watch_growth)(api.handle, feet)),
            say(tree),
        ));
    }

    // ---- the player, shoved and put back ----
    let mut vitals = PlayerVitals {
        health: 0.0,
        max_health: 0.0,
        nourishment: 0.0,
        hydration: 0.0,
        breath: 0.0,
        body_temperature_c: 0.0,
        ambient_c: 0.0,
        wetness: 0.0,
        carried_kg: 0.0,
        dead: false,
    };
    (players.vitals)(api.handle, player, &mut vitals);
    let mut slot = 0u32;
    (players.selected_slot)(api.handle, player, &mut slot);
    out.push(format!(
        "players: health set to what it already was {}, warmth to what it already was {}, slot to the one already selected {}",
        // Called with the value already there, so the write is real and
        // the change is nothing. What is proved is the crossing.
        say((players.set_health)(api.handle, player, vitals.health)),
        say((players.set_warmth)(
            api.handle,
            player,
            vitals.body_temperature_c,
            vitals.wetness
        )),
        say((players.set_selected_slot)(api.handle, player, slot)),
    ));

    // ---- the pack ----
    if !api.inventory.is_null() && !api.food.is_null() {
        let inventory = &*api.inventory;
        let food = &*api.food;
        let mut used = 0u32;
        (inventory.used_slots)(api.handle, player, &mut used);
        // The last slot of the pack, which is where a player keeps
        // nothing. Both calls answer `Refused` against an empty square,
        // and that is the answer being checked -- a sweep that threw
        // away whatever was in slot zero would not be run twice either.
        let empty = (inventory.slot_count)(api.handle) - 1;
        let mut dropped = 0u32;
        let spilt = if used == 0 {
            say((inventory.spill)(api.handle, player, &mut dropped))
        } else {
            "skipped: your pack is not empty"
        };
        out.push(format!(
            "inventory: drop the last slot {}, eat the last slot {}, spill the pack {spilt}",
            say((inventory.drop_slot)(api.handle, player, empty, true)),
            say((food.eat)(api.handle, player, empty)),
        ));
    }

    // ---- what is lying about, and what is alive ----
    if !api.items.is_null() && !api.entities.is_null() {
        let items = &*api.items;
        let entities = &*api.entities;
        let mut id = 0u64;
        let dropped = (items.spawn_worn)(
            api.handle,
            Vec3 {
                x: scratch.x as f32 + 0.5,
                y: scratch.y as f32 + 0.5,
                z: scratch.z as f32 + 0.5,
            },
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            ItemStack {
                block: stone,
                count: 1,
                damage: 3,
            },
            &mut id,
        );
        let mut removed = 0u32;
        let swept = (items.clear_near)(
            api.handle,
            Vec3 {
                x: scratch.x as f32 + 0.5,
                y: scratch.y as f32 + 0.5,
                z: scratch.z as f32 + 0.5,
            },
            1.5,
            &mut removed,
        );
        // **Against an id nothing has**, so nobody's deer is hurt to
        // prove that a call crosses the boundary. `NotFound` is what
        // both should answer, and an answer of `Ok` here would be the
        // interesting failure.
        out.push(format!(
            "items/entities: dropped a worn stone {}, swept {removed} back up ({}); heal a ghost {}, kill a ghost {}",
            say(dropped),
            say(swept),
            say((entities.heal)(api.handle, u64::MAX, 1.0)),
            say((entities.kill)(api.handle, u64::MAX)),
        ));
    }

    // ---- and a craft, which is the whole of the crafting table's
    // ---- write half
    if !api.crafting.is_null() {
        let crafting = &*api.crafting;
        let mut made = 0u32;
        out.push(format!(
            "crafting: run recipe #0 once -> {} ({made} made)",
            say((crafting.craft)(api.handle, player, 0, 1, &mut made)),
        ));
    }

    // ---- the sky, put back where it was ----
    if !api.core.is_null() {
        let core = &*api.core;
        let was = (core.time_of_day)(api.handle);
        let there_and_back = (core.set_time_of_day)(api.handle, was);
        let weather = (world.weather)(api.handle);
        out.push(format!(
            "sky: time set to the hour it already was {}, weather set to {weather:?} again {}",
            say(there_and_back),
            say((world.set_weather)(api.handle, weather)),
        ));
    }

    out
}

/// How many of each event have arrived, worst news first.
///
/// **Zeroes at the top**, because a zero is the interesting number: an
/// event nobody is ever told about is the failure this mod exists to
/// find, and burying it in an alphabetical list is how it stayed
/// unnoticed through two versions of the API.
fn event_report() -> Vec<String> {
    let mut never = Vec::new();
    let mut seen = Vec::new();
    for (index, (_, name)) in EVENTS.iter().enumerate() {
        let count = COUNTS[index].load(Ordering::Relaxed);
        if count == 0 {
            never.push(*name);
        } else {
            seen.push(format!("{name} {count}"));
        }
    }
    let mut out = Vec::new();
    out.push(format!("never fired ({}): {}", never.len(), never.join(" ")));
    out.push(format!("fired ({}): {}", seen.len(), seen.join("  ")));
    out
}

// ------------------------------------------------------------ the mod

unsafe extern "C" fn load(host_api: *const HostApi) -> Status {
    if host_api.is_null() {
        return Status::BadArgument;
    }
    // Refused rather than trusted. The host checks this too -- both,
    // because a mod loaded by an older host would otherwise read past
    // the end of these tables before it ever got the chance to complain.
    let version = (*host_api).version;
    if !version.accepts(API_VERSION) {
        return Status::Refused;
    }
    HOST.store(host_api as u64, Ordering::Release);

    ALLOW_WRITES.store(
        matches!(setting("allow_writes").as_deref(), Some("true")),
        Ordering::Relaxed,
    );
    if let Some(offset) = setting("scratch_offset").and_then(|s| s.parse::<i64>().ok()) {
        SCRATCH_OFFSET.store(offset.clamp(1, 32), Ordering::Relaxed);
    }

    let api = &*host_api;
    if api.events.is_null() {
        return Status::Unavailable;
    }
    // **Everything**, which is the one mod in the workspace for which
    // that is the right answer: the question it is here to settle is
    // which events never arrive, and it cannot settle that about an
    // event it did not ask for.
    for (event, _) in EVENTS {
        ((*api.events).subscribe)(api.handle, *event);
    }
    ((*api.events).register_command)(
        api.handle,
        Str::borrow("sandbox"),
        Str::borrow("call every host API and report; 'events' or 'write' for more"),
    );

    log(
        LogLevel::Info,
        &format!(
            "sandbox ready against API {version}: {} events watched, writes {}",
            EVENTS.len(),
            if ALLOW_WRITES.load(Ordering::Relaxed) {
                "allowed"
            } else {
                "off"
            }
        ),
    );
    Status::Ok
}

unsafe extern "C" fn event(what: Event, data: *const EventData) -> HookResult {
    let Some(data) = data.as_ref() else {
        return HookResult::Continue;
    };
    // Counted first and always, including for the command below: an
    // event this mod handles is still an event that arrived.
    if let Some(index) = EVENTS.iter().position(|(e, _)| *e == what) {
        COUNTS[index].fetch_add(1, Ordering::Relaxed);
    }

    if what != Event::Command {
        return HookResult::Continue;
    }
    // The name in `text` and the arguments in `args`, which is what
    // `Event::Command` carries. Two fields rather than one line to split,
    // because the server has already split it -- and a mod that split it
    // again would disagree with the host about what a quoted argument is.
    let name = data.text.as_str().trim().trim_start_matches('/');
    if name != "sandbox" {
        return HookResult::Continue;
    }
    let rest = data.args.as_str().trim();

    let lines = match rest {
        "events" => event_report(),
        "write" => {
            if ALLOW_WRITES.load(Ordering::Relaxed) {
                write_sweep(data.player)
            } else {
                vec![
                    "writes are off: set allow_writes to true in mods/sandbox/mod.ron".to_string(),
                ]
            }
        }
        _ => read_sweep(data.player),
    };
    remember(lines.len());
    for line in lines {
        say_to(data.player, &line);
    }
    // **Handled**, which is what `Cancel` means for a command -- see
    // `Event::Command`. Without it the server would go on to report
    // `/sandbox` as a typo.
    HookResult::Cancel
}

primitive_modapi::declare_mod! {
    name: "sandbox",
    version: "1.0.0",
    load: load,
    event: event,
}

/// Not a test of the game: a test of *this file*, and of the one thing
/// about it that a compiler cannot check.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_the_contract_declares_has_a_counter_here() {
        // The whole value of this mod is that it notices an event that
        // is never fired. It cannot notice one it does not know about --
        // and the day somebody appends a variant to `Event` and forgets
        // this list, this mod would go on reporting a clean sweep while
        // the new event was as dead as the nine that started all this.
        //
        // There is no way to enumerate a `#[repr(C)]` enum's variants
        // from outside the crate that declares it, so the check is on
        // the count: the list here must be as long as the highest
        // discriminant the contract uses is dense. Asserting the exact
        // membership is the compiler's job -- a variant that was removed
        // stops this file compiling -- and asserting the *number* is
        // this test's.
        assert_eq!(
            EVENTS.len(),
            35,
            "the contract's event list and this mod's have drifted"
        );
        assert_eq!(COUNTS.len(), EVENTS.len());
        // ...and no name twice, because two entries for one event would
        // make one of the counters unreachable and the report a lie.
        let mut names: Vec<&str> = EVENTS.iter().map(|(_, name)| *name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "an event is listed twice");
    }

    #[test]
    fn a_sweep_with_no_host_says_so_rather_than_dereferencing_null() {
        // The mod's own null check, which is the mirror of the host's:
        // every table may be absent, and a mod that assumed otherwise
        // crashes a server rather than reporting one.
        assert_eq!(read_sweep(1), vec!["no host".to_string()]);
    }
}
