//! What a lit hearth does while nobody is looking at it.
//!
//! ## The things a furnace has to do
//!
//! * **Burn.** Fuel comes out of the fuel slot one item at a time, and
//!   the fire goes out when there is none left. This replaces the
//!   right-click that used to feed a fire: a hearth with a fuel slot
//!   does not need a gesture, and a gesture that stayed would be a
//!   second way to do one thing.
//! * **Cook.** If what is in the input slots adds up to a batch this
//!   hearth can run, it runs it -- over `hearth::batch_seconds`, not
//!   instantly, because the whole reason a furnace is a place rather
//!   than a menu is that it takes time you can spend elsewhere. **And only
//!   while the fire is hot enough for it** (`hearth::needs_degrees`): a
//!   batch in a fire that is too cool waits, with what it has done kept,
//!   rather than running at any heat because the fire happens to be lit.
//! * **Boil, char and leave ash.** A jug of pond water on the fire boils
//!   into good water; supper left in the tray of a fire hot enough to
//!   char it turns to ash a piece at a time; and every second piece of
//!   fuel leaves an ash. See `primitive_shared::hearth` for why each of
//!   those is the rule it is.
//! * **Say so.** Everything visible -- the flame, the heat, the progress
//!   bar, the slots -- goes to whoever has the screen open, and only to
//!   them.
//!
//! ## Why the state is split in three
//!
//! The contents live with the chests (`logic::containers`), because a
//! hearth is a container and everything already written for chests --
//! opening, moving, spilling on break, saving -- applies unchanged. The
//! fuel and the heat live with the fires (`logic::fire`), because that is
//! what decides whether the block is alight and it was already saved
//! beside the world. What is left lives here, and **none of it is saved**:
//!
//! * the cooking progress, because a batch interrupted by a server
//!   restart starting again is the honest answer and a save format for a
//!   progress bar is not;
//! * how long supper has sat in a charring tray, because a restart that
//!   gives a player thirty more seconds to fetch it harms nobody;
//! * the half of an ash a fire is owed, because half an ash is nothing.

use std::collections::HashMap;

use primitive_shared::hearth::{self, Batch, Kind};
use primitive_shared::inventory::Inventory;

use crate::logic::containers::{ChestPos, Chests};
use crate::logic::fire::Fires;

/// How much fuel of the heat in the slot has to be left before the next
/// item is drawn from it.
///
/// Not zero, and the reason is the block update: a fire that runs to
/// nothing goes *out* (see `Fires::step`), which writes the cold block
/// into the world and tells every client about it. Topping up while
/// there is still a moment left means a hearth with fuel in its slot
/// burns continuously rather than flickering out and being relit twice a
/// minute. See `Fires::wants` for what "of the heat" means.
const TOP_UP_BELOW: f32 = 1.0;

/// How much ash one piece of fuel leaves.
///
/// **TerraFirmaCraft's half**, which it rolls for: a coin per log, one
/// ash on heads. Here it is counted instead -- every second piece, exactly
/// -- because the average is the same, the player cannot tell the
/// difference, and a coin in the middle of a hearth is a coin in the
/// middle of every test of one.
const ASH_PER_FUEL: f32 = 0.5;

/// How often a hearth whose contents did not change still tells whoever
/// is watching it what its fire is doing, in seconds.
///
/// **The gauge has to move on its own**, which is the rack's lesson (see
/// the tick loop) learnt again: a temperature climbs for half a minute
/// without a single slot changing, and a screen told only about changes
/// would show the fire at whatever heat it had when the screen opened.
/// Once a second is as often as a player reads a gauge, and it costs
/// nothing for a hearth nobody has open -- `broadcast_chest_state` asks
/// that first.
const REPORT_EVERY: f32 = 1.0;

/// How near a hearth somebody has to be standing to be its cook, in blocks.
///
/// Four: within arm's reach of the tray and a step or two, which is where
/// a person minding a fire stands -- and near enough that the cook of a
/// supper in the hut is never the neighbour on the other side of the
/// wall.
pub const TENDING_REACH: f32 = 4.0;

/// Somebody who might be minding a fire: where they stand, and the state
/// they would cook in.
///
/// Read by the tick loop under each player's own lock, before the fires
/// and the chests are taken -- the player lock is never taken inside
/// those two (see `heat_within_reach` for the deadlock the other order
/// is), so this is a copy and not a borrow.
#[derive(Debug, Clone, Copy)]
pub struct Cook {
    pub at: (f32, f32, f32),
    pub maker: primitive_shared::quality::Maker,
}

/// Who cooked what came off this hearth: whoever is standing nearest it,
/// within [`TENDING_REACH`], at the moment the batch is done.
///
/// **A hearth has no maker -- the fire does the work -- so this decides
/// who it is**, and three people were candidates:
///
/// * **Whoever put the food in**, remembered on the hearth. It is the
///   first answer and it is wrong about what cooking is: loading a spit
///   and walking off to the mine is not cooking, and the supper that
///   came off unattended was nobody's work. It also needs a memory per
///   hearth threaded through four different gestures (a drag, a
///   shift-click, a move-all, a sort), each of which could forget it.
/// * **Whoever takes it out.** Judged at the tray, hours later, by a
///   person who may have slept in between: a supper cooked by a starving
///   cook comes out fine because a rested one lifted it off -- and the
///   whole tray is one stack by then, so the taker's roll lands on meat
///   they never saw cook. The judgement has to be made when the work is.
/// * **Whoever is minding the fire when it comes off** (this). One
///   moment, one place in the code, and a decision for the player rather
///   than a chore: stay by the fire and the meat is as good as the cook
///   is today -- which, tired and cold, can be *worse* than plain -- or
///   leave it and it comes out unmarked, exactly as it always did.
///
/// Nearest rather than "best of those present", so two people at one
/// fire cannot choose the rested one's roll by standing still.
pub fn cook_at(cooks: &[Cook], at: ChestPos) -> Option<primitive_shared::quality::Maker> {
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    cooks
        .iter()
        .map(|cook| {
            let (dx, dy, dz) = (cook.at.0 - centre.0, cook.at.1 - centre.1, cook.at.2 - centre.2);
            ((dx * dx + dy * dy + dz * dz).sqrt(), cook.maker)
        })
        .filter(|&(distance, _)| distance <= TENDING_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, maker)| maker)
}

/// What one hearth is doing, for the screen.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Progress {
    /// Seconds of the current batch already done.
    pub cooked: f32,
    /// Seconds one batch takes here, or zero if nothing is cooking.
    pub batch: f32,
    /// How hot the batch wants the fire, or zero if there is no batch.
    pub needs: f32,
}

impl Progress {
    /// How far along, 0..1. Zero when nothing is cooking.
    pub fn fraction(self) -> f32 {
        if self.batch <= 0.0 {
            return 0.0;
        }
        (self.cooked / self.batch).clamp(0.0, 1.0)
    }
}

/// What a tick of the hearths did, for the tick loop to pass on.
#[derive(Debug, Default)]
pub struct Stepped {
    /// Hearths whose contents changed: fuel drawn, a batch done, a jug
    /// boiled, a piece of supper charred. Everyone watching one is sent
    /// its contents.
    pub changed: Vec<ChestPos>,
    /// Of those, the ones that finished a batch.
    ///
    /// **Apart from `changed`**, because the mods' `SmeltingFinished` is
    /// about a batch coming out and not about a log going in. When the two
    /// were one list, every piece of fuel a hearth drew told every mod that
    /// something had been smelted.
    pub finished: Vec<ChestPos>,
    /// Hearths whose contents did not change but whose fire did, once a
    /// second, so the gauge on an open screen moves. See [`REPORT_EVERY`].
    pub warming: Vec<ChestPos>,
}

/// Every hearth that is part way through a batch.
#[derive(Default)]
pub struct Smelting {
    progress: HashMap<ChestPos, f32>,
    /// Seconds cooked food has sat in a tray hot enough to char it.
    scorch: HashMap<ChestPos, f32>,
    /// The fraction of an ash each fire has earned and not yet made.
    ash_owed: HashMap<ChestPos, f32>,
    /// Seconds since the gauges were last sent.
    since_report: f32,
    stepped: Stepped,
}

impl Smelting {
    pub fn new() -> Self {
        Self::default()
    }

    /// How far along the hearth at `at` is, and how hot it needs to be.
    pub fn progress(&self, at: ChestPos, contents: &Inventory, kind: Kind) -> Progress {
        let cooked = self.progress.get(&at).copied().unwrap_or(0.0);
        match hearth::next_batch(kind, contents) {
            Some(batch) => Progress {
                cooked,
                batch: batch.seconds(kind),
                needs: batch.degrees(),
            },
            None => Progress {
                cooked,
                batch: 0.0,
                needs: 0.0,
            },
        }
    }

    /// One tick of every hearth with any heat in it.
    ///
    /// Takes the two stores rather than reaching for them, because the
    /// lock order matters: the tick loop holds the fire map and the
    /// container store together in exactly one place, which is the only
    /// way to be sure it is always the same order.
    pub fn step(
        &mut self,
        fires: &mut Fires,
        chests: &mut Chests,
        block_at: impl Fn(ChestPos) -> Option<primitive_shared::types::BlockId>,
        cooks: &[Cook],
        dt: f32,
    ) -> Stepped {
        self.stepped = Stepped::default();
        // **Every hearth with heat in it, not only the burning ones.** A
        // kiln that has just burnt through its fuel is still hot, and the
        // batch in it is still cooking if it is hot enough -- see
        // `Fires::heated_cells`. A copy of the list, because feeding one
        // takes the fire map mutably.
        let heated: Vec<ChestPos> = fires.heated_cells();
        for &at in &heated {
            let Some(block) = block_at(at) else {
                continue; // the chunk is not loaded; it is nobody's business
            };
            let Some(kind) = Kind::of(block) else {
                continue; // a fire map entry for something that is not a hearth
            };
            let contents = chests.contents(at);
            self.feed_from_slot(fires, chests, at, &contents);
            let degrees = fires.degrees(at);
            self.cook(chests, at, kind, &contents, degrees, cooks, dt);
            self.char_supper(chests, at, &contents, degrees, dt);
        }
        // A hearth that has gone cold is not cooking, charring or owed
        // anything, and none of it is kept: relighting starts the batch
        // again, which is what happens to a pot left to go cold.
        //
        // **Burning *or* hot, not hot alone.** A fire struck a moment ago
        // is alight at nought degrees -- it has not caught yet -- and the
        // first version of this asked only about the heat, so the half an
        // ash owed for the first log it drew was thrown away on the same
        // tick it was earned, and a fire fed from cold never made ash.
        let alive = |at: &ChestPos| fires.fuel_left(*at).is_some() || fires.degrees(*at) > 0.0;
        self.progress.retain(|at, _| alive(at));
        self.scorch.retain(|at, _| alive(at));
        self.ash_owed.retain(|at, _| alive(at));

        self.since_report += dt;
        if self.since_report >= REPORT_EVERY {
            self.since_report = 0.0;
            let changed = &self.stepped.changed;
            self.stepped.warming = heated.into_iter().filter(|at| !changed.contains(at)).collect();
        }
        std::mem::take(&mut self.stepped)
    }

    /// Draws one item out of the fuel slot when the fire wants it.
    ///
    /// Asked of the copy the tick already took, and only then of the
    /// store, so a hearth whose slot has nothing the fire wants -- which is
    /// nearly every hearth on nearly every tick -- costs no edit at all.
    fn feed_from_slot(
        &mut self,
        fires: &mut Fires,
        chests: &mut Chests,
        at: ChestPos,
        contents: &Inventory,
    ) {
        let Some(fuel) = contents.block_in(hearth::FUEL_SLOT) else {
            return;
        };
        let (Some(seconds), Some(degrees)) = (hearth::fuel_seconds(fuel), hearth::fuel_degrees(fuel))
        else {
            return;
        };
        if !fires.wants(at, degrees, TOP_UP_BELOW) {
            return;
        }
        // Re-checked inside the edit: the copy is from the top of this
        // tick, and the fire is only fed if the item is really still there
        // to be spent.
        let fed = chests.edit(at, |contents| {
            if contents.block_in(hearth::FUEL_SLOT) != Some(fuel)
                || !fires.feed_with(at, seconds, degrees)
            {
                return false;
            }
            contents.take_from(hearth::FUEL_SLOT, 1);
            true
        });
        if !fed {
            return;
        }
        self.stepped.changed.push(at);

        let owed = self.ash_owed.entry(at).or_insert(0.0);
        *owed += ASH_PER_FUEL;
        if *owed >= 1.0 {
            *owed -= 1.0;
            // Into the ash slot, or nowhere if it is full: a hearth whose
            // ash nobody empties has ash blowing off it, which is what a
            // real one does, and nothing stops.
            chests.edit(at, |contents| {
                contents.add_within(
                    hearth::ASH_SLOT..hearth::ASH_SLOT + 1,
                    primitive_shared::types::BLOCK_ASH,
                    1,
                )
            });
        }
    }

    /// Advances the batch, and finishes it if it is done.
    // One hearth's tick, and every argument is the tick's own copy of
    // something a lock already holds; a struct of them would be a type
    // made only to hide a count.
    #[allow(clippy::too_many_arguments)]
    fn cook(
        &mut self,
        chests: &mut Chests,
        at: ChestPos,
        kind: Kind,
        contents: &Inventory,
        degrees: f32,
        cooks: &[Cook],
        dt: f32,
    ) {
        let Some(batch) = hearth::next_batch(kind, contents) else {
            // Nothing to do: the progress goes back to zero rather than
            // waiting, or a hearth would keep half a batch of something
            // it is no longer making and hand it to whatever was loaded
            // next.
            self.progress.remove(&at);
            return;
        };
        if degrees < batch.degrees() {
            // **Too cool: it waits, and keeps what it has done.** Not
            // reset, because the fire dipping for a moment -- a shower, a
            // log going on under charcoal -- is not the batch being taken
            // off the heat, and a player who fixed it should not have to
            // wait the whole batch again.
            return;
        }
        let cooked = self.progress.entry(at).or_insert(0.0);
        *cooked += dt;
        if *cooked < batch.seconds(kind) {
            return;
        }
        *cooked = 0.0;
        // Re-checked inside the edit: `next_batch` was asked of a copy,
        // and between then and now a player at the screen may have taken
        // the ingredients out -- or swapped them for something that wants
        // a hotter fire than this one is.
        // Judged here, as it comes off, out of whoever is minding it: see
        // `cook_at`. The roll is the server's, as every roll is.
        let quality = cook_at(cooks, at)
            .map(|maker| maker.judge(crate::logic::rng::Rng::from_clock().range(0.0, 1.0)));
        if let Some(done) = chests.edit(at, |contents| finish(contents, kind, degrees, quality)) {
            self.stepped.changed.push(at);
            if matches!(done, Batch::Recipe(_)) {
                self.stepped.finished.push(at);
            }
        }
    }

    /// Chars one piece of supper left in the tray of a fire hot enough.
    fn char_supper(
        &mut self,
        chests: &mut Chests,
        at: ChestPos,
        contents: &Inventory,
        degrees: f32,
        dt: f32,
    ) {
        if !hearth::is_charring(contents, degrees) {
            // Taken out, or the fire fell below orange: the clock starts
            // again. A piece that sat twenty-nine seconds on a fire and was
            // rescued is rescued, not owed.
            self.scorch.remove(&at);
            return;
        }
        let sat = self.scorch.entry(at).or_insert(0.0);
        *sat += dt;
        if *sat < hearth::CHAR_SECONDS {
            return;
        }
        *sat = 0.0;
        if chests.edit(at, hearth::char_one) {
            self.stepped.changed.push(at);
        }
    }
}

/// Runs whatever batch the hearth is loaded for *now*, if the fire is hot
/// enough for it, and says which it was.
///
/// **What is loaded now, not what was loaded when it started.** This used
/// to compare the two and then run the new one either way, which is the
/// same thing said twice. What it did not do was ask about heat, and with
/// heat in the rules that is the case that matters: a player who swaps
/// the meat on a wood fire for ore in the last second of the batch must
/// not get an ingot out of a fire that could never have melted it.
fn finish(
    contents: &mut Inventory,
    kind: Kind,
    degrees: f32,
    quality: Option<primitive_shared::quality::Quality>,
) -> Option<Batch> {
    let batch = hearth::next_batch(kind, contents)?;
    if degrees < batch.degrees() {
        return None;
    }
    let done = match batch {
        // **Raw pottery is rolled for, a piece at a time**, against how wet
        // it went in (`clay::crack_chance`). The server's dice, as every
        // roll is; one generator for the batch, so four bricks are four
        // throws and not one throw read four times.
        Batch::Recipe(recipe) if recipe.inputs.iter().any(|&(block, _)| primitive_shared::clay::is_raw_pottery(block)) => {
            let mut dice = crate::logic::rng::Rng::from_clock();
            hearth::complete_fired(contents, recipe, quality, || dice.range(0.0, 1.0)).is_some()
        }
        Batch::Recipe(recipe) => hearth::complete_made(contents, recipe, quality),
        Batch::Boil => hearth::boil(contents),
    };
    done.then_some(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::body::Water;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{
        jug_of, vessel_water, BlockId, BLOCK_ASH, BLOCK_CAMPFIRE_LIT, BLOCK_COAL,
        BLOCK_COOKED_MEAT, BLOCK_COPPER_INGOT, BLOCK_DRIED_PEAT, BLOCK_MOULD, BLOCK_NATIVE_COPPER,
        BLOCK_RAW_MEAT, BLOCK_VESSEL,
    };

    const AT: ChestPos = (3, 21, 4);

    fn campfire() -> impl Fn(ChestPos) -> Option<BlockId> {
        |_| Some(BLOCK_CAMPFIRE_LIT)
    }

    fn a_campfire_in_the_world() -> TestWorld {
        let world = TestWorld::default();
        world.put(AT.0, AT.1, AT.2, BLOCK_CAMPFIRE_LIT);
        world
    }

    fn loaded(chests: &mut Chests, slots: &[(usize, BlockId, u32)]) {
        chests.edit(AT, |contents| {
            for &(slot, block, count) in slots {
                contents.put_in_slot(slot, Stack::new(block, count));
            }
        });
    }

    /// A campfire struck and left to come up to the heat of its kindling.
    fn a_fire_come_up_to_heat(world: &TestWorld) -> Fires {
        let mut fires = Fires::new();
        fires.light(AT);
        fires.step(world, 30.0, 16);
        assert_eq!(fires.degrees(AT), crate::logic::fire::LAID_FIRE_C);
        fires
    }

    /// The fires and the hearths together, a tick at a time, the way the
    /// server runs them.
    fn run(
        fires: &mut Fires,
        chests: &mut Chests,
        smelting: &mut Smelting,
        world: &TestWorld,
        seconds: f32,
    ) {
        let mut left = seconds;
        while left > 1e-4 {
            let dt = left.min(0.05);
            fires.step(world, dt, 16);
            smelting.step(fires, chests, campfire(), &[], dt);
            left -= dt;
        }
    }

    #[test]
    fn a_lit_hearth_cooks_what_is_in_it() {
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        loaded(&mut chests, &[(0, BLOCK_RAW_MEAT, 4)]);

        let mut smelting = Smelting::new();
        // Half a batch: nothing yet.
        smelting.step(&mut fires, &mut chests, campfire(), &[], Kind::Campfire.cook_seconds() * 0.5);
        assert_eq!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT),
            0,
            "it cooked before the time was up"
        );

        // ...and the rest of it.
        let stepped = smelting.step(&mut fires, &mut chests, campfire(), &[], Kind::Campfire.cook_seconds());
        assert_eq!(stepped.changed, vec![AT], "nobody was told the batch finished");
        assert_eq!(stepped.finished, vec![AT], "the mods were not told the batch finished");
        assert!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT) > 0,
            "the meat never cooked"
        );
    }

    /// One batch of meat on a fire, with these people about; what came off.
    fn supper_with(cooks: &[Cook]) -> Stack {
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        loaded(&mut chests, &[(0, BLOCK_RAW_MEAT, 1)]);
        let mut smelting = Smelting::new();
        smelting.step(&mut fires, &mut chests, campfire(), cooks, Kind::Campfire.cook_seconds());
        let contents = chests.contents(AT);
        hearth::OUTPUT_SLOTS
            .filter_map(|slot| contents.slots()[slot])
            .find(|stack| stack.block == BLOCK_COOKED_MEAT)
            .expect("the meat never cooked")
    }

    #[test]
    fn meat_cooked_with_somebody_minding_the_fire_is_their_work() {
        use primitive_shared::quality::Maker;
        let beside = (AT.0 as f32 + 1.5, AT.1 as f32, AT.2 as f32 + 0.5);
        let rested = supper_with(&[Cook { at: beside, maker: Maker::RESTED }]);
        assert!(rested.quality().is_marked(), "a cook stood at the fire and the meat is nobody's");
        assert!(rested.quality().fraction() > 0.8, "a rested cook made middling meat");
        // ...and a cook who is finished makes worse than a rested one:
        // it is the person, not the fire.
        let spent = Maker { fatigue: 1.0, comfort: 0.0, health: 0.3, hunger: 1.0, ..Maker::RESTED };
        let tired = supper_with(&[Cook { at: beside, maker: spent }]);
        assert!(tired.quality().fraction() < rested.quality().fraction());
    }

    #[test]
    fn meat_left_to_cook_itself_comes_off_unmarked() {
        use primitive_shared::quality::Maker;
        assert!(!supper_with(&[]).quality().is_marked(), "an empty camp cooked a graded supper");
        // Somebody in the world, but not at this fire.
        let far = (AT.0 as f32 + 20.0, AT.1 as f32, AT.2 as f32);
        let meat = supper_with(&[Cook { at: far, maker: Maker::RESTED }]);
        assert!(!meat.quality().is_marked(), "a cook twenty blocks off minded the fire");
    }

    #[test]
    fn the_cook_is_whoever_stands_nearest_the_fire() {
        use primitive_shared::quality::Maker;
        let spent = Maker { fatigue: 1.0, comfort: 0.0, health: 0.3, hunger: 1.0, ..Maker::RESTED };
        let near = (AT.0 as f32 + 1.0, AT.1 as f32, AT.2 as f32 + 0.5);
        let further = (AT.0 as f32 + 3.0, AT.1 as f32, AT.2 as f32 + 0.5);
        let cooks = [Cook { at: further, maker: Maker::RESTED }, Cook { at: near, maker: spent }];
        assert_eq!(cook_at(&cooks, AT), Some(spent), "the better cook's roll was chosen from further off");
    }

    #[test]
    fn a_cold_hearth_does_nothing_at_all() {
        let mut fires = Fires::new();
        let mut chests = Chests::new();
        loaded(&mut chests, &[(0, BLOCK_RAW_MEAT, 4)]);
        let mut smelting = Smelting::new();
        smelting.step(&mut fires, &mut chests, campfire(), &[], 600.0);
        assert_eq!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT),
            0,
            "an unlit fire cooked"
        );
    }

    #[test]
    fn meat_waits_for_the_fire_to_come_up_to_cooking_heat() {
        // Struck flint is a flame, not a hot fire. A batch put on it at
        // once has to wait for the fire to catch -- and does not lose what
        // it has done while it waits.
        let world = a_campfire_in_the_world();
        let mut fires = Fires::new();
        fires.light(AT);
        let mut chests = Chests::new();
        loaded(&mut chests, &[(0, BLOCK_RAW_MEAT, 1)]);
        let mut smelting = Smelting::new();

        smelting.step(&mut fires, &mut chests, campfire(), &[], Kind::Campfire.cook_seconds() * 2.0);
        assert_eq!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT),
            0,
            "meat cooked on a fire at {} degrees",
            fires.degrees(AT)
        );

        run(&mut fires, &mut chests, &mut smelting, &world, 20.0);
        assert!(fires.degrees(AT) >= hearth::COOKING_C);
        assert_eq!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT),
            1,
            "the meat never cooked once the fire was hot"
        );
    }

    #[test]
    fn copper_will_not_pour_on_a_wood_fire_and_does_on_charcoal() {
        // The decision the fuel slot is. The same crucible, the same
        // nuggets, the same campfire: on its kindling it melts nothing
        // however long it is left, and a lump of charcoal in the slot
        // pours the ingot.
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        loaded(
            &mut chests,
            &[(0, BLOCK_NATIVE_COPPER, 2), (1, BLOCK_VESSEL, 1), (2, BLOCK_MOULD, 1)],
        );
        let mut smelting = Smelting::new();

        run(&mut fires, &mut chests, &mut smelting, &world, 60.0);
        assert_eq!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COPPER_INGOT),
            0,
            "copper poured on a wood fire"
        );
        assert_eq!(
            smelting.progress(AT, &chests.contents(AT), Kind::Campfire).needs,
            hearth::COPPER_MELTS_C,
            "the screen would not say what the batch is waiting for"
        );

        loaded(&mut chests, &[(hearth::FUEL_SLOT, BLOCK_COAL, 1)]);
        run(&mut fires, &mut chests, &mut smelting, &world, 60.0);
        assert_eq!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COPPER_INGOT),
            1,
            "charcoal did not pour the copper"
        );
    }

    #[test]
    fn a_cooler_fuel_waits_until_the_fire_is_nearly_out() {
        let mut fires = Fires::new();
        let mut chests = Chests::new();
        fires.light(AT);
        // Peat burns cooler than the laid kindling, so it is the fire's
        // next hour rather than a way to cool it now.
        loaded(&mut chests, &[(hearth::FUEL_SLOT, BLOCK_DRIED_PEAT, 2)]);

        let mut smelting = Smelting::new();
        smelting.step(&mut fires, &mut chests, campfire(), &[], 0.0);
        let in_slot = |chests: &Chests| {
            chests
                .contents(AT)
                .count_within(hearth::FUEL_SLOT..hearth::FUEL_SLOT + 1, BLOCK_DRIED_PEAT)
        };
        assert_eq!(in_slot(&chests), 2, "it ate the fuel while it had plenty");

        // Now run it right down and step again.
        fires.burn_for_test(AT, 1e6);
        smelting.step(&mut fires, &mut chests, campfire(), &[], 0.0);
        assert_eq!(in_slot(&chests), 1, "it did not draw from the fuel slot");
        assert!(
            fires.fuel_left(AT).is_some_and(|left| left > TOP_UP_BELOW),
            "the fire was not topped up"
        );
    }

    #[test]
    fn charcoal_goes_onto_a_wood_fire_at_once() {
        // A player loading a campfire for copper must not watch four
        // minutes of kindling burn before the charcoal in the slot is
        // touched.
        let mut fires = Fires::new();
        let mut chests = Chests::new();
        fires.light(AT);
        loaded(&mut chests, &[(hearth::FUEL_SLOT, BLOCK_COAL, 3)]);
        let mut smelting = Smelting::new();
        smelting.step(&mut fires, &mut chests, campfire(), &[], 0.0);
        smelting.step(&mut fires, &mut chests, campfire(), &[], 0.0);
        assert_eq!(
            chests
                .contents(AT)
                .count_within(hearth::FUEL_SLOT..hearth::FUEL_SLOT + 1, BLOCK_COAL),
            2,
            "one lump went on, or it waited, or it took the whole slot"
        );
    }

    #[test]
    fn every_second_piece_of_fuel_leaves_an_ash() {
        let mut fires = Fires::new();
        let mut chests = Chests::new();
        fires.light(AT);
        loaded(&mut chests, &[(hearth::FUEL_SLOT, BLOCK_COAL, 4)]);
        let mut smelting = Smelting::new();
        for _ in 0..4 {
            smelting.step(&mut fires, &mut chests, campfire(), &[], 0.0);
            fires.burn_for_test(AT, 1e6);
        }
        let contents = chests.contents(AT);
        assert!(contents.block_in(hearth::FUEL_SLOT).is_none(), "the fuel was not all drawn");
        assert_eq!(
            contents.count_within(hearth::ASH_SLOT..hearth::ASH_SLOT + 1, BLOCK_ASH),
            2,
            "four pieces of fuel left the wrong amount of ash"
        );
        assert_eq!(
            contents.count_within(hearth::OUTPUT_SLOTS, BLOCK_ASH),
            0,
            "the ash went into the output tray, where it jams the next batch"
        );
    }

    #[test]
    fn a_jug_of_pond_water_comes_off_the_fire_good_to_drink() {
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        loaded(&mut chests, &[(0, jug_of(Water::Standing), 1)]);
        let mut smelting = Smelting::new();

        run(&mut fires, &mut chests, &mut smelting, &world, hearth::BOIL_SECONDS * 0.5);
        assert_eq!(
            chests.contents(AT).block_in(0).map(vessel_water),
            Some(Water::Standing),
            "it boiled before the time was up"
        );
        run(&mut fires, &mut chests, &mut smelting, &world, hearth::BOIL_SECONDS);
        assert_eq!(
            chests.contents(AT).block_in(0).map(vessel_water),
            Some(Water::Fresh),
            "boiling did not make the pond water safe, or moved the jug"
        );
    }

    #[test]
    fn supper_left_on_a_charcoal_fire_turns_to_ash_a_piece_at_a_time_and_a_wood_fire_keeps_it() {
        let cooked = |chests: &Chests| {
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT)
        };

        // On the kindling: two minutes, and supper is all still there.
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        loaded(&mut chests, &[(hearth::OUTPUT_SLOTS.start, BLOCK_COOKED_MEAT, 3)]);
        let mut smelting = Smelting::new();
        run(&mut fires, &mut chests, &mut smelting, &world, 120.0);
        assert_eq!(cooked(&chests), 3, "a wood fire charred the supper");

        // On charcoal: the heat has to come up to orange, and then it
        // costs a piece every half minute -- not the whole stack at once.
        loaded(&mut chests, &[(hearth::FUEL_SLOT, BLOCK_COAL, 1)]);
        let mut waited = 0.0;
        while fires.degrees(AT) < hearth::CHAR_C {
            run(&mut fires, &mut chests, &mut smelting, &world, 1.0);
            waited += 1.0;
            assert!(waited < 60.0, "charcoal never brought the fire to orange");
        }
        run(&mut fires, &mut chests, &mut smelting, &world, hearth::CHAR_SECONDS + 1.0);
        assert_eq!(cooked(&chests), 2, "it charred the wrong number of pieces");
        assert_eq!(
            chests.contents(AT).block_in(hearth::ASH_SLOT),
            Some(BLOCK_ASH),
            "the charred piece left no ash"
        );
    }

    #[test]
    fn a_hearth_that_has_gone_out_finishes_what_it_is_still_hot_enough_for() {
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        loaded(&mut chests, &[(0, BLOCK_RAW_MEAT, 1)]);
        let mut smelting = Smelting::new();

        // Out of fuel, on the tick it goes out.
        fires.burn_for_test(AT, 1e6);
        run(&mut fires, &mut chests, &mut smelting, &world, 0.1);
        assert!(fires.fuel_left(AT).is_none(), "it did not go out");

        run(&mut fires, &mut chests, &mut smelting, &world, Kind::Campfire.cook_seconds() + 1.0);
        assert_eq!(
            chests.contents(AT).count_within(hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT),
            1,
            "the embers of a fire at cooking heat did not finish the meat"
        );
    }

    #[test]
    fn an_open_screen_is_told_the_heat_once_a_second_without_anything_changing() {
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        let mut smelting = Smelting::new();
        let first = smelting.step(&mut fires, &mut chests, campfire(), &[], 0.5);
        assert!(first.warming.is_empty(), "it reported twice a second");
        let second = smelting.step(&mut fires, &mut chests, campfire(), &[], 0.6);
        assert_eq!(second.warming, vec![AT], "the gauge was never sent");
        assert!(second.finished.is_empty(), "a heat report told the mods something was smelted");
    }

    #[test]
    fn taking_the_ingredients_out_mid_batch_produces_nothing() {
        let world = a_campfire_in_the_world();
        let mut fires = a_fire_come_up_to_heat(&world);
        let mut chests = Chests::new();
        loaded(&mut chests, &[(0, BLOCK_RAW_MEAT, 4)]);

        let mut smelting = Smelting::new();
        smelting.step(&mut fires, &mut chests, campfire(), &[], Kind::Campfire.cook_seconds() * 0.9);
        chests.edit(AT, |contents| {
            contents.take_within(hearth::INPUT_SLOTS, BLOCK_RAW_MEAT, 4);
        });
        smelting.step(&mut fires, &mut chests, campfire(), &[], Kind::Campfire.cook_seconds());
        assert!(
            chests.contents(AT).is_empty(),
            "it made something out of an empty hearth"
        );
    }
}
