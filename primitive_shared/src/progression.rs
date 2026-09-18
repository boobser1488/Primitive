//! **The whole ladder, walked.** From bare hands to a steeled iron pick, by
//! the game's own recipes, its own hearths and its own mining times, counting
//! what it costs.
//!
//! ## Why a walk and not a list of prices
//!
//! Each recipe test in `crafting` checks one rung, and a rung can be right
//! while the ladder is wrong: every price reasonable, and the sum a hundred
//! hours of chopping. The player's complaint that started this -- "nothing
//! takes time and the materials are all wrong" -- is a claim about the *sum*,
//! and so is the answer, which has to be "it takes hours, not minutes, and not
//! days". So this runs the path the way a player would, through `craft` and
//! through a hearth loaded by `hearth::next_recipe`, and totals two clocks:
//!
//! * **active** -- seconds with the player's hands on something: mining by
//!   `types::break_seconds_with`, and the walking a find costs, estimated from
//!   how thinly the world scatters it (see the constants);
//! * **furnace** -- seconds hearths are busy with batches, by
//!   `hearth::batch_seconds`. A player is doing something else meanwhile, so
//!   the two do not add up to wall-clock time; both are bounded.
//!
//! Charcoal pits are counted apart, as pits dug and hour-long burns
//! (`pit::CHARCOAL_SECONDS`): they burn beside everything else, and adding
//! each burn's hour onto the hearths' clock would count one afternoon six
//! times over.
//!
//! What it does *not* model: a failed knapping beyond its expected cost (the
//! walk pays the average number of nodules), a blunt edge (every swing here is
//! sharp, so the times are a floor), and the player's route between places,
//! which is a handful of journeys charged once each.
//!
//! Run it with `--nocapture` to read the tally.

use crate::crafting::{craft, Attempt, Crafted, Heat, Recipe, RECIPES};
use crate::hearth::{self, Kind};
use crate::inventory::Inventory;
use crate::types::*;

/// Walking speed on open ground, blocks a second, near enough the game's.
const WALK: f32 = 4.3;
/// How wide a strip of ground a player looking for something scans.
const SIGHT: f32 = 6.0;

/// Seconds of walking to come across one of a thing the world scatters one to
/// `spacing` columns.
fn walk_to_find(spacing: f32) -> f32 {
    spacing / (SIGHT * WALK)
}

/// One-time journeys, in seconds of walking, there and back.
const TO_THE_HILLS: f32 = 2.0 * 250.0 / WALK;
const TO_TIN_COUNTRY: f32 = 2.0 * 600.0 / WALK;
const TO_THE_RIVER: f32 = 2.0 * 120.0 / WALK;
/// A shaft into deep rock and a look along it for a seam: twenty blocks of
/// stone dug with the copper pick, and a minute of searching.
const DEEP_SEAM_SEARCH: f32 = 60.0;
/// Stalking a deer and killing it with a spear, then the three cuts that take
/// its sinew. A deer gives three sinews (`animals`).
const HUNT: f32 = 150.0;
/// Laying a log pile, walling it in and lighting it (`pit`).
const BUILD_A_PIT: f32 = 60.0;

struct Walk {
    pack: Inventory,
    active: f32,
    /// Seconds hearths spent on batches.
    furnace: f32,
    /// Charcoal pits burnt, and how many separate hour-long burns they took.
    /// Kept apart from `furnace`: pits burn beside everything else, so summing
    /// their hours onto the hearths' would count one afternoon six times.
    pits: u32,
    burns: u32,
    /// Raw material taken out of the world, by name.
    taken: std::collections::BTreeMap<&'static str, f32>,
    /// Fuel burnt in hearth fuel slots, not yet paid for.
    coal_burnt: f32,
    logs_burnt: f32,
}

fn named(name: &str) -> &'static Recipe {
    RECIPES
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no recipe called {name:?}"))
}

impl Walk {
    fn new() -> Self {
        Self {
            // **A chest's worth, because this is a ledger and not a
            // pack.** The walk adds up everything gathered over several
            // in-game hours -- forty-odd kinds of thing, none of which
            // is ever put down -- and it is an `Inventory` only because
            // that is a convenient bag of stacks with the right stack
            // limits in it. When the player's pack was halved
            // (`inventory::STORAGE_ROWS`) this began overflowing at the
            // sinew, which said nothing about the ladder and everything
            // about the container.
            pack: Inventory::chest(),
            active: 0.0,
            furnace: 0.0,
            pits: 0,
            burns: 0,
            taken: Default::default(),
            coal_burnt: 0.0,
            logs_burnt: 0.0,
        }
    }

    /// Takes `count` of `item` out of the world, each costing `each` seconds.
    fn take(&mut self, item: BlockId, count: u32, each: f32) {
        assert_eq!(self.pack.add(item, count), 0, "the pack is full taking {}", block_name(item));
        self.active += count as f32 * each;
        *self.taken.entry(block_name(item)).or_default() += count as f32;
    }

    /// Mines `count` of a block with a tool, walking `walk` seconds to each.
    fn mine(&mut self, block: BlockId, drops: BlockId, count: u32, tool: Option<BlockId>, walk: f32) {
        let seconds = break_seconds_with(block, tool)
            .unwrap_or_else(|| panic!("{} cannot be mined with that", block_name(block)));
        self.take(drops, count, seconds + walk);
    }

    fn hands(&mut self, name: &str, times: u32) {
        let recipe = named(name);
        for _ in 0..times {
            assert_eq!(
                craft(&mut self.pack, recipe, Heat::NONE, Attempt::Succeeds),
                Crafted::Made,
                "could not make {name} by hand: {:?}",
                crate::crafting::feasibility(&self.pack, recipe, Heat::NONE)
            );
        }
    }

    /// A workshop row, run standing at its workshop.
    fn at(&mut self, station: crate::crafting::Station, name: &str, times: u32) {
        let recipe = named(name);
        assert_eq!(recipe.station, station, "{name} is not made at {station:?}");
        let beside = Heat::NONE.with_workshop(station);
        for _ in 0..times {
            assert_eq!(
                craft(&mut self.pack, recipe, beside, Attempt::Succeeds),
                Crafted::Made,
                "could not make {name} at {station:?}: {:?}",
                crate::crafting::feasibility(&self.pack, recipe, beside)
            );
        }
    }

    /// A knapping row run as often as it takes to succeed `successes` times
    /// on average, the failures spending their inputs.
    fn knap(&mut self, name: &str, successes: u32) {
        let recipe = named(name);
        let attempts = (successes as f32 / (1.0 - recipe.failure)).ceil() as u32;
        for attempt in 0..attempts {
            let roll = if attempt < attempts - successes { Attempt::Fails } else { Attempt::Succeeds };
            let done = craft(&mut self.pack, recipe, Heat::NONE, roll);
            assert_ne!(done, Crafted::Refused, "short of what {name} needs on attempt {attempt}");
        }
    }

    /// Loads a hearth with exactly what `name` asks for, lets it run, and
    /// takes everything out again. Pays the batch's time and its fuel.
    fn fire(&mut self, name: &str, kind: Kind, times: u32) {
        let recipe = named(name);
        for _ in 0..times {
            let mut hearth = Inventory::new();
            for &(block, count) in recipe.inputs {
                assert!(
                    self.pack.take_exact(block, count),
                    "short of {} for {name}",
                    block_name(block)
                );
                hearth.add_within(hearth::INPUT_SLOTS, block, count);
            }
            let (_, found) = hearth::next_recipe(kind, &hearth)
                .unwrap_or_else(|| panic!("a {} loaded for {name} does nothing", kind.name()));
            assert_eq!(found.name, name, "a {} loaded for {name} made {}", kind.name(), found.name);
            assert!(hearth::complete(&mut hearth, found));

            // The cheapest fuel that reaches the heat: wood when it does,
            // charcoal when only charcoal does.
            let needs = hearth::needs_degrees(found).expect("a hearth row names its heat");
            let seconds = hearth::batch_seconds(found, kind);
            if kind.reaches(BLOCK_LOG).is_some_and(|heat| heat >= needs) {
                self.logs_burnt += seconds / hearth::fuel_seconds(BLOCK_LOG).unwrap();
            } else {
                assert!(kind.reaches(BLOCK_COAL).is_some_and(|heat| heat >= needs), "{name} is out of reach");
                self.coal_burnt += seconds / hearth::fuel_seconds(BLOCK_COAL).unwrap();
            }
            self.furnace += seconds;

            for stack in hearth.slots()[..hearth::USED_SLOTS].iter().flatten() {
                assert_eq!(self.pack.add_worn(stack.block, stack.count, stack.damage), 0, "the pack is full");
            }
        }
    }

    /// Fells `count` logs with an axe.
    fn fell(&mut self, count: u32, axe: BlockId) {
        // A standing oak, and the walk from one trunk to the next shared out
        // over the six or so logs a tree gives.
        self.mine(BLOCK_LOG, BLOCK_LOG, count, Some(axe), 8.0 / 6.0);
    }

    /// Burns `coal` charcoal in pits: two logs a lump, eight logs a pit, the
    /// pits side by side so the hour is waited once.
    fn char_in_pits(&mut self, coal: u32, axe: BlockId) {
        let logs = coal * 2;
        self.fell(logs, axe);
        assert!(self.pack.take_exact(BLOCK_LOG, logs));
        let pits = logs.div_ceil(crate::pit::PILE_LOGS_MAX as u32);
        self.active += pits as f32 * BUILD_A_PIT;
        self.pits += pits;
        self.burns += 1;
        let made: u32 = (0..pits)
            .map(|pit| {
                let in_this = (logs - pit * crate::pit::PILE_LOGS_MAX as u32).min(crate::pit::PILE_LOGS_MAX as u32);
                crate::pit::charcoal_from(in_this as u8) as u32
            })
            .sum();
        assert_eq!(self.pack.add(BLOCK_COAL, made), 0);
    }

    /// Pays for the fuel the hearths have burnt so far: logs felled for the
    /// wood, pits for the charcoal.
    fn settle_fuel(&mut self, axe: BlockId) {
        let logs = self.logs_burnt.ceil() as u32;
        if logs > 0 {
            self.fell(logs, axe);
            assert!(self.pack.take_exact(BLOCK_LOG, logs));
            self.logs_burnt = 0.0;
        }
        let coal = self.coal_burnt.ceil() as u32;
        if coal > 0 {
            self.char_in_pits(coal, axe);
            assert!(self.pack.take_exact(BLOCK_COAL, coal));
            self.coal_burnt = 0.0;
        }
    }

    fn minutes(&self) -> (f32, f32) {
        (self.active / 60.0, self.furnace / 60.0)
    }

    fn report(&self, rung: &str) {
        let (active, furnace) = self.minutes();
        let taken: Vec<String> = self.taken.iter().map(|(name, n)| format!("{name} {n:.0}")).collect();
        eprintln!(
            "{rung:>18}: {active:6.1} min active, {furnace:6.1} min of hearth, {} pits in {} hour-long burns | {}",
            self.pits,
            self.burns,
            taken.join(", ")
        );
    }
}

#[test]
fn the_ladder_from_bare_hands_to_steel_takes_hours_and_not_days() {
    let mut w = Walk::new();
    let grass = break_seconds_with(BLOCK_TALL_GRASS, None).unwrap_or(0.3);

    // ---- the stone kit: a flint knife, a wedged axe and a wedged pick ----
    //
    // Seven hafts over the whole walk (knife, axe, pick, the pegs, and the
    // copper, bronze and steel picks), each whittled with a flake; a knife head
    // is two flakes and fails half the time. So four good strikes of flakes.
    w.take(BLOCK_FLINT, 7, walk_to_find(60.0));
    w.knap("flint flakes", 4);
    w.take(BLOCK_STICK, 11, walk_to_find(10.0));
    w.hands("worked stick", 7);
    w.knap("knife head", 1);
    // Three cords of grass: a tuft gives fibre one time in two.
    w.take(BLOCK_FIBER, 18, 2.0 * (grass + walk_to_find(3.0)));
    w.hands("cord", 3);
    w.take(BLOCK_PEBBLE, 13, walk_to_find(26.0));
    w.hands("knapped stone", 2);
    w.hands("axe head", 1);
    w.hands("pick head", 1);
    w.hands("flint knife", 1);
    w.hands("stone axe", 1);
    w.hands("stone pick", 1);
    w.hands("pegs", 1);
    w.hands("wedged axe", 1);
    w.hands("wedged pick", 1);
    w.report("stone kit");
    let axe = BLOCK_WEDGED_AXE;
    let pick = BLOCK_WEDGED_PICKAXE;

    // ---- a chest and a stool, pegged ----
    //
    // The stone age's furniture, on the way and not after it: a joined chest
    // is a pegged frame boarded in, and the stool is three legs wedged into a
    // slab. Seven worked sticks -- four rails and three for the eleven pegs
    // -- so three good strikes of flakes, and two logs of boards.
    w.take(BLOCK_FLINT, 5, walk_to_find(60.0));
    w.knap("flint flakes", 3);
    w.take(BLOCK_STICK, 10, walk_to_find(10.0));
    w.hands("worked stick", 7);
    w.hands("pegs", 3);
    w.hands("pegged frame", 1);
    w.fell(2, axe);
    w.hands("planks", 2);
    w.hands("chest", 1);
    w.hands("stool", 1);
    assert_eq!((w.pack.count(BLOCK_CHEST), w.pack.count(BLOCK_STOOL)), (1, 1), "no furniture before metal");
    w.report("pegged chest");

    // ---- a bench, and a door hung at it ----
    //
    // A second frame boarded over, which is what turns the shelter the chest
    // stands in into a room that keeps its smoke and its wolves apart. On
    // the stone-age side of the ladder, because a house is -- and hung at a
    // joiner's bench, because a door two cells tall racks unless its frame
    // is cut square (`crafting::Station::Bench`). The bench is a log on
    // pegged legs; at it the frame is plain sticks and pegs, so the flint a
    // field frame would have eaten goes into the bench's pegs instead.
    w.take(BLOCK_FLINT, 2, walk_to_find(60.0));
    w.knap("flint flakes", 1);
    w.take(BLOCK_STICK, 10, walk_to_find(10.0));
    w.hands("worked stick", 2);
    w.hands("pegs", 2);
    w.fell(2, axe);
    w.hands("workbench", 1);
    w.at(crate::crafting::Station::Bench, "bench frame", 1);
    w.hands("planks", 1);
    w.at(crate::crafting::Station::Bench, "door", 1);
    assert_eq!(w.pack.count(BLOCK_DOOR), 1, "no door before metal");
    w.report("bench and door");

    // ---- the fire, the clay and the kiln ----
    w.mine(BLOCK_STONE, BLOCK_COBBLESTONE, 3 + 4, Some(pick), 0.5);
    w.hands("campfire", 1);
    w.active += TO_THE_RIVER;
    w.mine(BLOCK_CLAY, BLOCK_CLAY, 8 + 4 + 3 + 4 + 16, None, 0.5);
    w.take(BLOCK_SAND, 1, 2.0);
    w.hands("kiln", 1);
    w.hands("clay vessel", 1);
    w.hands("ingot mould", 1);
    w.hands("clay jug", 1);
    w.fire("fire vessel", Kind::Kiln, 1);
    w.fire("fire mould", Kind::Kiln, 1);
    w.fire("fire jug", Kind::Kiln, 1);
    w.settle_fuel(axe);
    w.report("kiln and pots");

    // ---- copper: ore in the hills, and a pick that opens iron ----
    w.active += TO_THE_HILLS;
    w.mine(BLOCK_COPPER_ORE, BLOCK_COPPER_ORE, 3 * 7, Some(pick), 1.0);
    w.char_in_pits(2 * 7, axe);
    w.fire("copper ingot", Kind::Campfire, 3);
    w.fire("pick casting", Kind::Kiln, 1);
    // Two deer: a sinew for the copper pick, one for the bronze and two for
    // the steel.
    w.take(BLOCK_SINEW, 6, HUNT / 3.0);
    w.hands("copper pick", 1);
    w.settle_fuel(axe);
    w.report("copper pick");
    let (copper_active, copper_fire) = w.minutes();

    // ---- bronze: a walk to tin country and a pan along its river ----
    w.fire("copper ingot", Kind::Campfire, 4);
    w.active += TO_TIN_COUNTRY;
    // Walking a bank is walking the strip the pebbles lie in, so a find is
    // its spacing in blocks of walk.
    w.mine(BLOCK_STREAM_TIN, BLOCK_TIN_ORE, 2, None, 16.0 / WALK);
    w.char_in_pits(1, axe);
    w.fire("tin ingot", Kind::Campfire, 1);
    w.fire("bronze ingot", Kind::Kiln, 1);
    w.fire("bronze pick", Kind::Kiln, 1);
    w.settle_fuel(axe);
    w.report("bronze pick");
    let (bronze_active, bronze_fire) = w.minutes();
    assert_eq!(w.pack.count(BLOCK_BRONZE_PICKAXE), 1);

    // ---- iron: a shaft, a bloomery, a kiln to steel it in ----
    let copper_pick = Some(BLOCK_COPPER_PICKAXE);
    w.active += DEEP_SEAM_SEARCH;
    w.mine(BLOCK_STONE, BLOCK_COBBLESTONE, 8, copper_pick, 0.5);
    // Four blooms: three bars for the steel and one cut into nails.
    w.mine(BLOCK_IRON_ORE, BLOCK_IRON_ORE, 3 * 4, copper_pick, 1.0);
    w.hands("raw bricks", 4);
    w.fire("fire bricks", Kind::Kiln, 4);
    w.hands("brickwork", 4);
    w.hands("bloomery", 1);
    // Charcoal for four blooms, four bars and three steelings.
    w.char_in_pits(4 * 4 + 4 + 3 * 2, axe);
    w.fire("iron bloom", Kind::Bloomery, 4);
    w.fire("wrought iron", Kind::Bloomery, 4);
    // ...and the fourth bar a nailed chest and a nailed frame: iron where the
    // stone age spent flint.
    w.fire("nails", Kind::Kiln, 1);
    w.fell(2, axe);
    w.hands("planks", 2);
    w.hands("nailed chest", 1);
    w.take(BLOCK_STICK, 4, walk_to_find(10.0));
    w.hands("nailed frame", 1);
    assert_eq!(w.pack.count(BLOCK_CHEST), 2, "the nailed chest was not made");
    assert_eq!(w.pack.count(BLOCK_FRAME), 1, "the nailed frame was not made");
    w.fire("steel", Kind::Kiln, 3);
    w.active += TO_THE_RIVER;
    // The fired jug, filled at the river.
    assert!(w.pack.take_exact(BLOCK_JUG, 1), "the jug went missing");
    w.take(jug_of(crate::body::Water::Fresh), 1, 5.0);
    w.fire("steel pick", Kind::Kiln, 1);
    w.settle_fuel(axe);
    w.report("steeled iron pick");
    let (steel_active, steel_fire) = w.minutes();

    let steel = w
        .pack
        .slots()
        .iter()
        .flatten()
        .find(|s| block_kind(s.block) == BLOCK_IRON_PICKAXE)
        .copied()
        .expect("no iron pick at the end of the walk");
    assert!(crate::tools::is_hardened(steel.block), "the pick at the end is not steeled");
    let logs = w.taken.get("log").copied().unwrap_or(0.0);
    eprintln!("logs felled over the whole walk: {logs:.0}; slag left in the pack: {}", w.pack.count(BLOCK_SLAG));

    // **Hours of fire, not minutes.** The ladder has to take time at the
    // hearths above all, which is what the old eight- and twenty-second
    // batches did not: a copper pick was a minute and a half of hearth, and
    // an iron one about three.
    assert!(copper_fire >= 8.0, "a copper pick is only {copper_fire:.1} minutes of hearth");
    assert!(bronze_fire > copper_fire && steel_fire > bronze_fire);
    assert!(steel_fire >= 45.0, "the iron age is only {steel_fire:.0} minutes of hearth");
    // **And not days.** Hands-on time to a steeled pick stays inside an
    // afternoon's play, and the hearths inside an evening of it; a player
    // loads a hearth and does the next thing, so the two clocks are not
    // added together.
    assert!(copper_active < 60.0, "{copper_active:.0} minutes of hands-on work to a copper pick");
    assert!(bronze_active < 90.0, "{bronze_active:.0} minutes of hands-on work to a bronze pick");
    assert!(steel_active < 180.0, "{steel_active:.0} minutes of hands-on work to a steeled pick");
    assert!(steel_fire < 240.0, "{steel_fire:.0} minutes of hearth to a steeled pick");
    assert!(w.burns <= 8, "{} separate hour-long charcoal burns: that is a week of waiting", w.burns);
    // The iron age eats a wood, and should: but a wood, not a forest.
    assert!((40.0..=250.0).contains(&logs), "{logs:.0} logs felled on the way to steel");
}

/// **The trap is a first-evening thing and the rod is a copper-age thing**,
/// walked the way the ladder is: out of the world by hand, through `craft`.
///
/// The trap from a bare-handed player at a riverbank -- reeds pulled by hand,
/// one of them split for the cord's fibre -- with no knife, no fire and no
/// metal in the pack at any point. The rod refused to the same player, and
/// refused for the right reason: its hook is made at a forge and nowhere else,
/// so there is no road to a rod that does not go through copper. See
/// `fishing` for why the rod waits.
#[test]
fn a_fish_trap_needs_only_the_riverbank_and_a_rod_waits_for_copper() {
    let mut w = Walk::new();
    let reed = break_seconds_with(BLOCK_REEDS, None).expect("reeds are pulled by hand");
    w.active += TO_THE_RIVER;
    // Six for the basket and three split into the six fibre of its cord.
    w.take(BLOCK_REEDS, 9, reed + walk_to_find(4.0));
    w.hands("reed fibre", 3);
    w.hands("cord", 1);
    w.hands("fish trap", 1);
    w.report("fish trap");
    assert_eq!(w.pack.count(BLOCK_FISH_TRAP), 1, "no trap at the river");
    let (active, fire) = w.minutes();
    assert_eq!(fire, 0.0, "a trap spent time at a hearth");
    assert!(active < 5.0, "{active:.1} minutes to a trap: it is meant for the first evening");
    for slot in w.pack.slots().iter().flatten() {
        assert!(
            ![BLOCK_FLINT_KNIFE, BLOCK_COPPER_INGOT, BLOCK_COPPER_HOOK].contains(&block_kind(slot.block)),
            "the trap's road went through {}",
            block_name(slot.block)
        );
    }

    // The rod, with everything but the hook in hand, is refused...
    let rod = named("fishing rod");
    let mut pack = Inventory::new();
    assert_eq!(pack.add(BLOCK_WORKED_STICK, 1), 0);
    assert_eq!(pack.add(BLOCK_CORD, 2), 0);
    assert_ne!(craft(&mut pack, rod, Heat::NONE, Attempt::Succeeds), Crafted::Made, "a rod without a hook");
    // ...and the hook is made in one place, which is a forge, from copper.
    let hooks: Vec<&Recipe> = RECIPES.iter().filter(|r| block_kind(r.output.0) == BLOCK_COPPER_HOOK).collect();
    assert_eq!(hooks.len(), 1, "a second road to a hook");
    assert_eq!(hooks[0].station, crate::crafting::Station::Forge);
    assert!(hooks[0].inputs.iter().any(|&(b, _)| b == BLOCK_COPPER_INGOT), "a hook with no copper in it");
}
