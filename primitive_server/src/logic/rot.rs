//! Food going off: the clock, and the pass that runs it over every
//! pack, every chest and every stack lying on the ground.
//!
//! ## What this is for
//!
//! Hunger without rot has a hole in it, and a player finds the hole on
//! the first good evening: one boar is eight cuts, eight cuts cooked is
//! eighty points of bar, and eighty points in a chest is the end of ever
//! having to hunt again. The rules for *what* keeps and for how long are
//! in [`primitive_shared::food`] -- raw meat a day, cooked meat two,
//! dried meat and bread for ever, which is the reason to build a rack.
//! This module is only the clock that applies them, and the walk over
//! everywhere a stack can be.
//!
//! ## Why a pass on a coarse clock and not a timer per stack
//!
//! A stack's age is three bits in its block id (see `food::rot_stage`),
//! not a timestamp, so there is nothing to compare against a clock per
//! slot. Instead the world's food is aged **one step at a time, all of
//! it together**, [`primitive_shared::food::ROT_STEPS_PER_DAY`] times a
//! day. That is a quarter of a game day between steps -- two and a half
//! real minutes by default -- which is coarse on purpose: the finest
//! thing a player can see is one stage of eight, and stepping finer
//! than the thing being stepped would be spending ticks to move a
//! number nobody can watch.
//!
//! The cost of the pass is what it walks: forty slots per connected
//! player, forty per chest that holds anything, one closure per dropped
//! stack -- and one climate sample per chest and per drop that actually
//! has something perishable in it, which is why the pass looks before
//! it samples. At two and a half minutes apart it does not register.
//!
//! ## Cold keeps
//!
//! Below freezing nothing ages. It is the same temperature the racks
//! stop curing at and it is read off the same sample the player's own
//! warmth comes from ([`crate::logic::climate`]), so the tundra, a
//! winter night and a cave under the snow are all a larder -- and a
//! chest in the cold is a *reason to site a chest*, which is what turns
//! "put food somewhere" into a decision. A player's pack is judged by
//! the ambient already on their state, sampled twice a second for the
//! body; a chest and a drop are sampled where they sit, so a haunch by
//! the hearth goes off at the hearth's temperature and not the night's.
//!
//! **Cool slows it**, between the frost and [`COOL_BELOW_C`]: a stack there
//! ages at half its pace ([`Keeping`]). That is the cellar -- the ground
//! holds the year's average (`climate::earth_shares`), and in temperate
//! country the year's average is cellar-cold. The carcasses on the ground
//! are not given the band: they are a clock of their own (`carrion`) and a
//! kill left in a cave is not somebody's larder.
//!
//! Rain is deliberately **not** a factor here, though it is for the
//! racks: wet stops drying, and it would be reasonable for wet to
//! hasten rot, but a player has no way to keep a *drop* out of the rain
//! and a mechanic that punishes what cannot be answered is a chore.
//!
//! ## What it does not do
//!
//! - **A chest in a chunk nobody has loaded is not aged.** The same
//!   rule every periodic pass on this server follows, for the same
//!   reason: reading the world through `cached_block` and only through
//!   it is what stops a background step from generating terrain. It is
//!   also the same honesty the racks have -- the world stands still
//!   where nobody is -- and the gift runs the other way from the
//!   racks': meat in a far camp waits for you. Let it.
//! - **The clock's phase is not saved.** A restart loses at most a
//!   quarter of a day of ageing, in the player's favour, and nothing
//!   else: the stage itself is in the id, and the id is what every
//!   container save already writes. A `rot.bin` for one float was not
//!   worth a file format.

use std::sync::Arc;

use primitive_shared::food::{self, ROT_STEPS_PER_DAY};
use primitive_shared::inventory::{Inventory, Stack};
use primitive_shared::types::BLOCK_AIR;

use crate::logic::climate::Ambient;
use crate::logic::containers::{ChestPos, Chests};
use crate::logic::items::{Item, Items};

/// At or below this, in the degrees `body` measures in, nothing ages.
///
/// Zero, the same line `drying::NO_CURE_BELOW_C` draws: freezing is
/// freezing, and a player who has learned that the rack stops in the
/// frost should be able to guess that the meat does too.
pub const KEEPS_BELOW_C: f32 = 0.0;

/// At or below this, and above [`KEEPS_BELOW_C`], food ages at **half** its
/// pace: a step of the clock in two passes over it.
///
/// **Twelve, because of the rule of ten.** The things that spoil food are
/// living things, and like most of life they slow by about half for every
/// ten degrees colder: meat on a summer's evening at twenty-odd degrees and
/// meat in a cellar at eight to ten differ by that half, which is the whole
/// reason cellars were dug. Twelve puts a temperate root cellar -- the
/// year's average of a meadow, eight or nine degrees, which is what the
/// ground holds (`climate::earth_shares`) -- inside the line with room to
/// spare, and keeps a cellar in the tropics outside it, where the ground's
/// average is twenty-five and salt and the rack are the only larder there is.
///
/// That split is the decision. In the north a hole in a hillside is a
/// larder; in the south it is a hole, and the salt has to be fetched.
///
/// Rejected: a continuous rate, ten per cent slower per degree. It is the
/// truer curve, and it needs a remainder carried per stack, which is the
/// field `food`'s note on ageing already refused. Two bands a player can
/// name -- "the frost keeps it, the cellar slows it" -- are also two things
/// a player can plan around, where a curve is a number they cannot see.
pub const COOL_BELOW_C: f32 = 12.0;

/// How the weather where a stack sits treats it on a step of the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keeping {
    /// Frozen: nothing ages.
    Frozen,
    /// Cellar-cool: ages on every second step.
    Cool,
    /// Everything else: ages at its own pace.
    Warm,
}

impl Keeping {
    pub fn of(ambient: &Ambient) -> Keeping {
        if ambient.temperature_c <= KEEPS_BELOW_C {
            Keeping::Frozen
        } else if ambient.temperature_c <= COOL_BELOW_C {
            Keeping::Cool
        } else {
            Keeping::Warm
        }
    }

    /// The step of the food's own clock this step of the world's is, or
    /// `None` if the food does not age on it at all.
    ///
    /// **Cool halves the step number rather than skipping odd steps**, and
    /// that difference is the bug it avoids: the larder ages on every
    /// fourth, twenty-fourth or seventy-second step (`food::rot_every`),
    /// all of them even, so "skip the odd ones" would slow the fresh meat
    /// and leave the salted meat exactly where it was. Halving the count
    /// slows every food by the same half.
    pub fn clock(self, step: u64) -> Option<u64> {
        match self {
            Keeping::Frozen => None,
            Keeping::Warm => Some(step),
            Keeping::Cool => step.is_multiple_of(2).then_some(step / 2),
        }
    }
}

/// The clock. One per server, owned by the tick loop the way its
/// rainfall is, because it is a counter and nothing else has any
/// business reading it.
#[derive(Default)]
pub struct Rot {
    /// Seconds since the last step.
    since: f32,
    /// How many steps this pass has taken, which is what tells the larder
    /// from the hunt: a salted haunch ages on one step in four and a dried
    /// one on one in twenty-four (`food::rot_every`), and a pass that asked
    /// `food::aged` alone aged everything on every step -- so drying bought
    /// nothing at all, and a month in a pack turned the rack's meat to rot.
    step: u64,
}

impl Rot {
    pub fn new() -> Self {
        Self::default()
    }

    /// How long one step is, in real seconds, for a day of this length.
    ///
    /// Derived from the day rather than fixed, so an operator who makes
    /// the day an hour gets meat that lasts an hour: the promise is
    /// "raw meat keeps a day", and a day is whatever the server says.
    pub fn step_seconds(day_length_seconds: f32) -> f32 {
        // A floor of a second, so a nonsense day length in a settings
        // file (zero, negative, NaN) cannot make every tick a step.
        let day = if day_length_seconds.is_finite() {
            day_length_seconds.max(0.0)
        } else {
            0.0
        };
        (day / ROT_STEPS_PER_DAY as f32).max(1.0)
    }

    /// Has enough time passed for another step?
    ///
    /// `dt` is clamped to a second, as every interval on this server
    /// clamps its own: a frame that took five seconds must not be five
    /// seconds of rot applied at once. The remainder is kept rather
    /// than zeroed, for the reason `water::Rainfall::due` keeps its own
    /// -- a server a hair over its tick budget would otherwise drift
    /// slow for ever.
    pub fn due(&mut self, dt: f32, day_length_seconds: f32) -> bool {
        self.since += dt.clamp(0.0, 1.0);
        let step = Self::step_seconds(day_length_seconds);
        if self.since < step {
            return false;
        }
        self.since = (self.since - step).min(step);
        self.step = self.step.wrapping_add(1);
        true
    }

    /// Does food keep in this weather?
    pub fn keeps(ambient: &Ambient) -> bool {
        ambient.temperature_c <= KEEPS_BELOW_C
    }

    /// Is there anything in here that could go off?
    ///
    /// Asked before a climate sample is paid for. Most chests in the
    /// world are stone and tools, and most of the cost of this pass
    /// would otherwise be sampling the weather over them.
    pub fn holds_perishables(inventory: &Inventory) -> bool {
        inventory
            .slots()
            .iter()
            .flatten()
            .any(|stack| food::is_perishable(stack.block))
    }

    /// One step of the clock over one container. Answers whether
    /// anything in it changed.
    ///
    /// Every perishable stack is rewritten **in its own slot at its own
    /// count**: a stack that went off is a stack of rot where the meat
    /// was, not a stack of rot wherever `add` found room. The slot is
    /// where the player left it, and food that jumps around the pack
    /// when it turns is food the player cannot keep an eye on.
    pub fn age_inventory(inventory: &mut Inventory, step: u64) -> bool {
        let mut changed = false;
        for slot in 0..inventory.slots().len() {
            let Some(stack) = inventory.slots()[slot] else {
                continue;
            };
            let next = food::aged_on(stack.block, step);
            if next == stack.block {
                continue;
            }
            // **Something cooked well keeps better**, and the coin is
            // flipped here rather than stored anywhere: the ages
            // themselves are three bits of a block id (`food`, "going
            // off") and a per-stack rate would need a per-stack clock,
            // which is the field that note refused. See
            // `quality::keeping_chance`.
            //
            // The "coin" is a hash of the step and the slot and not a
            // generator, for `minigame::target`'s reason: a stream with
            // state would have to be stepped in the same order on both
            // sides of a restart, and a hash of what the situation
            // already is gives the same answer wherever it is asked.
            if Self::kept(stack, step, slot) {
                continue;
            }
            inventory.take_slot(slot);
            // A slot just emptied takes the whole stack: rot stacks as
            // high as anything that becomes it (there is a test in
            // `food` that says so), and a perishable has no wear to
            // carry over. If that test is ever wrong, the tail goes to
            // `add` rather than to nowhere -- moved is better than gone.
            if let Some(left) = inventory.put_in_slot(slot, Stack::new(next, stack.count)) {
                inventory.add(left.block, left.count);
            }
            changed = true;
        }
        changed
    }

    /// Did this stack's quality carry it past one step of the clock?
    ///
    /// Never for an unjudged stack: `keeping_chance` is zero at the middle
    /// of the scale, so every crumb in every world written before quality
    /// existed goes off exactly as fast as it always did.
    fn kept(stack: Stack, step: u64, slot: usize) -> bool {
        let chance = primitive_shared::quality::keeping_chance(stack.quality());
        if chance <= 0.0 {
            return false;
        }
        // One round of the integer hash `minigame::target` uses.
        let mut h = (step as u32)
            .wrapping_mul(0x9E37_79B9)
            ^ (slot as u32).wrapping_mul(0x85EB_CA6B)
            ^ u32::from(stack.block);
        h ^= h >> 16;
        h = h.wrapping_mul(0x7FEB_352D);
        h ^= h >> 15;
        h = h.wrapping_mul(0x846C_A68B);
        h ^= h >> 16;
        (h % 1000) as f32 / 1000.0 < chance
    }

    /// One step over every chest that holds something perishable.
    ///
    /// `ambient_at` answers the weather over a chest, or `None` for one
    /// in a chunk nobody has loaded -- which is left exactly as it is,
    /// see the note at the top. Answers which chests changed, so the
    /// caller can tell whoever has one open; a chest that only ages is
    /// a chest whose screen has to be redrawn.
    pub fn age_chests(
        chests: &mut Chests,
        step: u64,
        mut ambient_at: impl FnMut(ChestPos) -> Option<Ambient>,
    ) -> Vec<ChestPos> {
        let mut changed = Vec::new();
        for at in chests.positions() {
            // `contents` clones forty slots; done once, and only the
            // perishable ones go on to pay for a climate sample.
            if !Self::holds_perishables(&chests.contents(at)) {
                continue;
            }
            let Some(ambient) = ambient_at(at) else {
                continue;
            };
            let Some(step) = Keeping::of(&ambient).clock(step) else {
                continue;
            };
            if chests.edit(at, |contents| Self::age_inventory(contents, step)) {
                changed.push(at);
            }
        }
        changed
    }

    /// One step over every stack on the ground. Answers how many
    /// changed.
    ///
    /// The ground rots by the pack's rules, and not more slowly: a drop
    /// that kept better than a chest would make throwing your dinner in
    /// the grass the right way to store it.
    pub fn age_items(
        items: &mut Items,
        step: u64,
        mut ambient_at: impl FnMut((f32, f32, f32)) -> Ambient,
    ) -> usize {
        items.rewrite_stacks(|item: &Item| {
            if !food::is_perishable(item.block) {
                return None;
            }
            let ambient = ambient_at(primitive_shared::geometry::narrow(item.position));
            let step = Keeping::of(&ambient).clock(step)?;
            Some(food::aged_on(item.block, step))
        })
    }

    /// The whole pass, from the tick loop: advances the clock by `dt`
    /// and, on the tick a step falls due, ages every pack, chest and
    /// drop. Answers the chests that changed, for the caller to
    /// broadcast -- the players' packs are sent from here, because the
    /// pack's own dirty flag and `send_inventory` already know how.
    ///
    /// **Lock order.** The player's state is taken alone and let go;
    /// then the fires and the chests, in that order, which is the order
    /// the racks take them; then the fires and the items. Nothing here
    /// holds two of a player's state, the chests and the items at once,
    /// so this pass cannot be the second half of a deadlock with any of
    /// the paths that take them singly.
    pub fn pass(&mut self, ctx: &Arc<crate::Context>, dt: f32) -> Vec<ChestPos> {
        if !self.due(dt, ctx.clock.day_length_seconds()) {
            return Vec::new();
        }

        // ---- the packs ----
        //
        // Judged by the ambient already on the player: it is the number
        // their body is being warmed or chilled by this tick, so the
        // meat in the pack and the person carrying it agree about how
        // cold it is.
        for handle in ctx.registry.handles() {
            let changed = {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(step) = Keeping::of(&state.ambient).clock(self.step) {
                    let changed = Self::age_inventory(&mut state.inventory, step);
                    if changed {
                        state.inventory_dirty = true;
                    }
                    changed
                } else {
                    false
                }
            };
            if changed {
                crate::send_inventory(&handle);
            }
        }

        // The weather and the hour are read once and used for every
        // container, so the chests and the drops are looking at the
        // same sky.
        let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
        // The calendar, not the hour: `Ambient::of` reads the season
        // off the day count.
        let time_of_day = ctx.clock.world_days();

        // ---- the chests ----
        let mut changed_chests = {
            let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
            let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
            Self::age_chests(&mut chests, self.step, |at| {
                // Cache only: a chest in a chunk nobody has is left
                // alone rather than generated for.
                ctx.world.cached_block(at.0, at.1, at.2)?;
                Some(Ambient::of(
                    &ctx.world,
                    &fires,
                    (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5),
                    time_of_day,
                    weather,
                ))
            })
        };

        // ---- the ground ----
        //
        // A drop in an unloaded column gets the neutral answer from
        // `Ambient::of` and ages, which is right: drops only exist near
        // players, and one that has just crossed a chunk edge nobody
        // has yet is not thereby in a fridge.
        {
            let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
            let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
            Self::age_items(&mut items, self.step, |at| {
                Ambient::of(&ctx.world, &fires, at, time_of_day, weather)
            });
        }

        // ---- the kills ----
        //
        // On this clock rather than one of its own, because a carcass
        // *is* meat and the promise "a day is a day" should mean the
        // same thing on the ground as in a pack. See `logic::carrion`
        // for where the age is kept and why it is not in the block.
        //
        // The locks are taken and let go one at a time, as everywhere
        // else in this pass: fires with the world to read the cold,
        // then carrion alone to age, then items alone to leave the
        // bones. What comes back is written into the world outside all
        // three -- `set_block` and `broadcast_block` take their own.
        let spoiled = {
            let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
            let mut carrion = ctx.carrion.lock().unwrap_or_else(|e| e.into_inner());
            carrion.step(|at| {
                let block = ctx.world.cached_block(at.0, at.1, at.2)?;
                let ambient = Ambient::of(
                    &ctx.world,
                    &fires,
                    (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5),
                    time_of_day,
                    weather,
                );
                Some((block, Self::keeps(&ambient)))
            })
        };
        for gone in spoiled {
            let at = gone.at;
            // **The skeleton takes the carcass's place**, rather than
            // the cell going to air: what is left of an animal nobody
            // came back for is the part that does not rot, and it stays
            // where the animal fell. Air only if the species is unknown,
            // which is the id-off-a-socket case rather than a real one.
            let becomes = gone.bones.unwrap_or(BLOCK_AIR);
            if !ctx.world.set_block(at.0, at.1, at.2, becomes) {
                continue;
            }
            crate::notify_mechanics(ctx, at.0, at.1, at.2);
            crate::broadcast_block(ctx, at, becomes);
            // **What a body was carrying is filtered after the block has
            // actually changed, and that order is chosen rather than
            // fallen into.** Both orders have a window; they are not
            // worth the same. This way, a write the world refuses leaves
            // an untouched body -- and the gap where the cell is already
            // bones holding leather nobody has taken out yet is a gift
            // to whoever is standing there, measured in microseconds.
            // The other way round, the same refusal would leave a body
            // that looks fresh with the soft half already gone out of
            // it, which is a loss nothing in the game could explain.
            if gone.player_body {
                let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
                if chests.edit(at, crate::logic::carrion::what_the_ground_takes) {
                    // Anyone standing at it is looking at a screen that
                    // is now a lie. The same broadcast a chest that only
                    // *aged* gets, and for the same reason.
                    changed_chests.push(at);
                }
            }
            let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
            for (block, count) in gone.leaves {
                items.spawn(
                    block,
                    count,
                    (f64::from(at.0) + 0.5, f64::from(at.1) + 0.5, f64::from(at.2) + 0.5),
                    (0.0, 0.0, 0.0),
                    None,
                    std::time::Instant::now(),
                );
            }
        }

        // ---- the traps ----
        //
        // On this clock for the carcass's reason: "a day" should be one
        // thing, whether it is the meat going off in a pack or the fish
        // going into a basket in the river. See `logic::fishing`.
        crate::fill_traps(ctx, time_of_day, weather);

        changed_chests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::food::{rot_stage, ROT_STEPS_PER_DAY};
    use primitive_shared::types::{
        block_kind, BLOCK_BREAD, BLOCK_COOKED_MEAT, BLOCK_DRIED_MEAT, BLOCK_RAW_MEAT,
        BLOCK_ROTTEN, BLOCK_STONE,
    };
    use std::time::Instant;

    const AT: ChestPos = (3, 40, -9);

    /// A mild evening: warm enough that nothing keeps.
    fn mild() -> Ambient {
        Ambient {
            temperature_c: 18.0,
            ..Ambient::default()
        }
    }

    /// Hard frost.
    fn freezing() -> Ambient {
        Ambient {
            temperature_c: -6.0,
            ..Ambient::default()
        }
    }

    fn a_day_of_steps_from(inventory: &mut Inventory, step: &mut u64) {
        for _ in 0..ROT_STEPS_PER_DAY {
            *step += 1;
            Rot::age_inventory(inventory, *step);
        }
    }

    #[test]
    fn a_pack_of_raw_meat_left_for_a_day_is_a_pack_of_rot() {
        let mut step = 0u64;
        // The headline promise, and the one the constants in `food`
        // were chosen backwards from.
        let mut pack = Inventory::new();
        pack.put_in_slot(2, Stack::new(BLOCK_RAW_MEAT, 5));
        pack.put_in_slot(7, Stack::new(BLOCK_STONE, 12));

        // Part way through the day it is older meat, in the same slot,
        // at the same count -- not rot yet, and not moved.
        assert!(Rot::age_inventory(&mut pack, 0));
        let older = pack.block_in(2).expect("still in its slot");
        assert_eq!(block_kind(older), BLOCK_RAW_MEAT, "it went off after one step");
        assert!(rot_stage(older) > 0, "a step passed and nothing aged");
        assert_eq!(pack.count_in(2), 5);

        for _ in 1..ROT_STEPS_PER_DAY {
            Rot::age_inventory(&mut pack, 0);
        }
        assert_eq!(pack.block_in(2), Some(BLOCK_ROTTEN), "a day passed and it is not rot");
        assert_eq!(pack.count_in(2), 5, "the count changed on the way to rot");
        assert_eq!(pack.count(BLOCK_RAW_MEAT), 0);
        // ...and the stone beside it is stone.
        assert_eq!(pack.block_in(7), Some(BLOCK_STONE));
        assert_eq!(pack.count_in(7), 12);
        // Rot is the end: another day changes nothing.
        a_day_of_steps_from(&mut pack, &mut step);
        assert_eq!(pack.block_in(2), Some(BLOCK_ROTTEN));
        assert!(!Rot::age_inventory(&mut pack, 0), "a pack of rot and stone reported a change");
    }

    #[test]
    fn a_chest_in_the_cold_keeps_meat() {
        // The decision the mechanic exists to create: where the chest
        // is matters. A frozen chest is a larder; a mild one is not.
        let mut chests = Chests::new();
        chests.edit(AT, |contents| {
            contents.put_in_slot(0, Stack::new(BLOCK_RAW_MEAT, 4));
        });
        for _ in 0..ROT_STEPS_PER_DAY * 3 {
            let changed = Rot::age_chests(&mut chests, 0, |_| Some(freezing()));
            assert!(changed.is_empty(), "a frozen chest reported a change");
        }
        assert_eq!(
            chests.contents(AT).block_in(0),
            Some(BLOCK_RAW_MEAT),
            "three days of frost and the meat aged"
        );

        // The thaw. One mild step and it is a step older; a day of
        // them and it is rot, and the chest says so each time so the
        // screen can be redrawn.
        let changed = Rot::age_chests(&mut chests, 0, |_| Some(mild()));
        assert_eq!(changed, vec![AT], "a mild chest did not report the change");
        let older = chests.contents(AT).block_in(0).expect("meat");
        assert_eq!(block_kind(older), BLOCK_RAW_MEAT);
        assert!(rot_stage(older) > 0);
        for _ in 1..ROT_STEPS_PER_DAY {
            Rot::age_chests(&mut chests, 0, |_| Some(mild()));
        }
        assert_eq!(chests.contents(AT).block_in(0), Some(BLOCK_ROTTEN));
        assert_eq!(chests.contents(AT).count_in(0), 4);
    }

    #[test]
    fn a_chest_nobody_has_loaded_is_left_as_it_was() {
        // The same rule the racks keep, and the same reason: a pass
        // that reads only the cache cannot generate terrain, and the
        // world stands still where nobody is.
        let mut chests = Chests::new();
        chests.edit(AT, |contents| {
            contents.put_in_slot(0, Stack::new(BLOCK_RAW_MEAT, 1));
        });
        for _ in 0..ROT_STEPS_PER_DAY * 3 {
            assert!(Rot::age_chests(&mut chests, 0, |_| None).is_empty());
        }
        assert_eq!(chests.contents(AT).block_in(0), Some(BLOCK_RAW_MEAT));
    }

    #[test]
    fn a_chest_of_stone_costs_no_weather() {
        // The look-before-you-sample rule: a chest with nothing
        // perishable in it must not pay for a climate sample, because
        // most chests are that chest.
        let mut chests = Chests::new();
        chests.edit(AT, |contents| {
            contents.put_in_slot(0, Stack::new(BLOCK_STONE, 30));
            // Bread, not dried meat: since the larder learned salting, dried
            // meat goes off too -- slowly (`food::rot_every`) -- and a chest
            // with it in is a chest the weather is asked about.
            contents.put_in_slot(1, Stack::new(BLOCK_BREAD, 6));
        });
        let mut samples = 0;
        let changed = Rot::age_chests(&mut chests, 0, |_| {
            samples += 1;
            Some(mild())
        });
        assert_eq!(samples, 0, "the weather was sampled over a chest of stone");
        assert!(changed.is_empty());
    }

    #[test]
    fn the_racks_dried_meat_outlasts_the_hunt() {
        let mut step = 0u64;
        // The whole argument for building a rack: what came off it
        // keeps. A pack of dried meat, bread and a cooked haunch is
        // stepped for a month, and only the haunch is gone.
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_DRIED_MEAT, 8));
        pack.put_in_slot(1, Stack::new(BLOCK_BREAD, 3));
        pack.put_in_slot(2, Stack::new(BLOCK_COOKED_MEAT, 2));
        for _ in 0..30 {
            a_day_of_steps_from(&mut pack, &mut step);
        }
        // Still dried meat after a month, older but nowhere near rot: it ages
        // on one step in twenty-four, where the hunt it came from ages on
        // every one, and salting it first buys three times longer again
        // (`food::rot_every`).
        assert_eq!(
            pack.block_in(0).map(primitive_shared::types::block_kind),
            Some(BLOCK_DRIED_MEAT),
            "the rack's meat went off"
        );
        assert_eq!(pack.count_in(0), 8);
        assert_eq!(pack.block_in(1), Some(BLOCK_BREAD), "the bread went off");
        assert_eq!(pack.block_in(2), Some(BLOCK_ROTTEN), "a cooked haunch kept for a month");
        // ...and once everything left is what keeps, the pass has
        // nothing to say -- which is what stops it marking every chest
        // dirty and resending every pack four times a day for ever. The
        // dried meat comes out first: it keeps, but not for ever, so on the
        // steps it ages on the pass does have something to say.
        pack.take_slot(0);
        assert!(!Rot::age_inventory(&mut pack, 1));
    }

    #[test]
    fn a_cool_cellar_keeps_every_food_twice_as_long() {
        let cellar = Ambient {
            temperature_c: 9.0,
            ..Ambient::default()
        };
        assert_eq!(Keeping::of(&cellar), Keeping::Cool);
        assert_eq!(Keeping::of(&mild()), Keeping::Warm);
        assert_eq!(Keeping::of(&freezing()), Keeping::Frozen);

        // How many of the world's steps each food lasts, warm and cool.
        let lasts = |block, keeping: Keeping| {
            let mut pack = Inventory::new();
            pack.add(block, 1);
            let mut step = 0u64;
            while !pack.slots().iter().flatten().any(|s| s.block == BLOCK_ROTTEN) {
                step += 1;
                assert!(step < 10_000, "{block} never went off");
                if let Some(own) = keeping.clock(step) {
                    Rot::age_inventory(&mut pack, own);
                }
            }
            step
        };
        // The fresh meat and the salted, which ages only on every fourth
        // step: skipping odd steps would have left the salted one as fast
        // in the cellar as out of it.
        for food in [BLOCK_RAW_MEAT, BLOCK_COOKED_MEAT, primitive_shared::types::BLOCK_SALTED_MEAT] {
            let warm = lasts(food, Keeping::Warm);
            let cool = lasts(food, Keeping::Cool);
            assert!(
                cool >= warm * 2 - 2 && cool <= warm * 2 + 2,
                "{food} lasted {warm} steps warm and {cool} in the cellar"
            );
        }
    }

    #[test]
    fn items_on_the_ground_rot_like_items_in_a_pack() {
        
        // Or the grass is the best larder in the game.
        let mut items = Items::new();
        let now = Instant::now();
        assert!(items.spawn(BLOCK_RAW_MEAT, 3, (0.5, 40.0, 0.5), (0.0, 0.0, 0.0), None, now));
        assert!(items.spawn(BLOCK_STONE, 2, (4.5, 40.0, 0.5), (0.0, 0.0, 0.0), None, now));

        let mut samples = 0;
        for _ in 0..ROT_STEPS_PER_DAY {
            Rot::age_items(&mut items, 0, |_| {
                samples += 1;
                mild()
            });
        }
        assert_eq!(samples, ROT_STEPS_PER_DAY as usize, "the stone was sampled for weather");
        let meat = items
            .iter()
            .find(|item| item.count == 3)
            .expect("the meat is still on the ground");
        assert_eq!(meat.block, BLOCK_ROTTEN, "a day on the ground and it is not rot");
        assert_eq!(meat.position, (0.5, 40.0, 0.5), "ageing moved it");
        let stone = items.iter().find(|item| item.count == 2).expect("the stone");
        assert_eq!(stone.block, BLOCK_STONE);

        // ...and in the frost it lies there as it was.
        let mut frozen = Items::new();
        frozen.spawn(BLOCK_RAW_MEAT, 1, (0.5, 40.0, 0.5), (0.0, 0.0, 0.0), None, now);
        for _ in 0..ROT_STEPS_PER_DAY * 3 {
            assert_eq!(Rot::age_items(&mut frozen, 0, |_| freezing()), 0);
        }
        assert_eq!(frozen.iter().next().expect("meat").block, BLOCK_RAW_MEAT);
    }

    #[test]
    fn the_clock_steps_four_times_a_day_whatever_the_day_is() {
        // The promise is "raw meat keeps a day", and a day is whatever
        // the server says it is.
        for day in [600.0f32, 60.0, 3600.0] {
            let mut rot = Rot::new();
            let mut steps = 0;
            // Twenty ticks a second, the server's own rate, for one day
            // and a second over -- the second is there so that the
            // rounding of twelve thousand additions of 0.05 cannot leave
            // the last step a hair short of due.
            let ticks = (day * 20.0) as u32 + 20;
            for _ in 0..ticks {
                if rot.due(0.05, day) {
                    steps += 1;
                }
            }
            assert_eq!(steps, ROT_STEPS_PER_DAY, "a {day}s day stepped {steps} times");
        }
        // A stalled tick is worth a second and not five: a server that
        // hung must not rot a pack in one frame.
        let mut rot = Rot::new();
        assert!(!rot.due(1000.0, 600.0), "a five-second hang was a whole step");
        // ...and a nonsense day length is not a step every tick.
        let mut broken = Rot::new();
        assert!(!broken.due(0.05, 0.0));
        assert!(!broken.due(0.05, f32::NAN));
    }
}
