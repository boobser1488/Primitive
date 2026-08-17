//! Where a mechanic that watches the world lives.
//!
//! ## What this is for
//!
//! Two of these run today -- sand falling and water flowing -- and the
//! first of them taught the shape the hard way: a queue of coordinates
//! to re-examine, a bounded amount of work per tick, and changes handed
//! back as data for the caller to broadcast. Water that flows, fire that
//! spreads, grass that creeps back over bare earth and crops that grow
//! are all the same shape with different rules, and every one of them
//! goes wrong in the same two ways if it is written afresh: it does
//! unbounded work on a tick when a player does something drastic, and it
//! reaches into the world from inside a lock the tick loop is already
//! holding.
//!
//! So the shape is written down once, here, and the next mechanic is a
//! `CellMechanic` and a `register` call rather than another hand-rolled
//! loop wired into the tick by hand.
//!
//! ## The contract, and why each half of it is there
//!
//! * **`on_block_changed` is a notification, not work.** It is called
//!   from the edit path, which is on a player's connection task and
//!   holds locks. Queue the cell; do not look at the world.
//! * **`step` is given a budget and must respect it.** A player
//!   hollowing out a desert queues thousands of cells, and a tick that
//!   drains all of them is a tick that takes a second. Draining slower
//!   is not a failure mode -- it is the design.
//! * **`step` returns changes rather than sending them.** The caller
//!   knows which players can see which chunk, how to batch a tick's
//!   worth of edits into one message, and what else has to happen to a
//!   cell that changed (a plant on top of it falls, sand under it is
//!   now unsupported). A mechanic that sent its own messages would have
//!   to know all of that too.
//!
//! ## What it deliberately is not
//!
//! It is not a scheduler with priorities, threads or ordering
//! guarantees. Mechanics are stepped in registration order, each with
//! its own share of the budget, on the tick thread. Anything cleverer
//! would be a guess about mechanics that do not exist yet, and the one
//! that does exist has never needed it.

use primitive_shared::protocol::BlockChange;

use crate::logic::falling::{BlockWorld, FallingBlocks};

/// How many cells a mechanic may examine in one tick, before the
/// per-mechanic share is worked out.
///
/// The falling-sand simulation ran at this figure alone for its whole
/// life, and it is a couple of cached block lookups per cell -- so this
/// is a budget that has been in production rather than a number picked
/// to look reasonable.
pub const DEFAULT_TICK_BUDGET: usize = 512;

/// A mechanic that watches cells and changes them.
///
/// Object-safe on purpose: the whole point is that the tick loop holds a
/// list of these without knowing what any of them are.
pub trait CellMechanic: Send {
    /// What to call this in `/stats` and in the log.
    fn name(&self) -> &'static str;

    /// A cell changed. Queue whatever that might affect; do not touch
    /// the world here (see the note above about locks).
    fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32);

    /// Do at most `budget` cells' worth of work and hand back what
    /// changed.
    fn step(&mut self, world: &dyn BlockWorld, dt: f32, budget: usize) -> Vec<BlockChange>;

    /// How much work is waiting. Reported in `/stats`, and the reason a
    /// growing queue is visible rather than mysterious.
    fn pending(&self) -> usize;
}

/// Every mechanic the server is running, stepped together.
#[derive(Default)]
pub struct Mechanics {
    mechanics: Vec<Box<dyn CellMechanic>>,
    budget: usize,
}

impl Mechanics {
    pub fn new() -> Self {
        Self {
            mechanics: Vec::new(),
            budget: DEFAULT_TICK_BUDGET,
        }
    }

    /// Adds a mechanic. Order is registration order, and nothing here
    /// depends on it.
    pub fn register(&mut self, mechanic: Box<dyn CellMechanic>) {
        self.mechanics.push(mechanic);
    }

    pub fn is_empty(&self) -> bool {
        self.mechanics.is_empty()
    }

    /// Names and queue lengths, for `/stats`.
    pub fn pending(&self) -> Vec<(&'static str, usize)> {
        self.mechanics
            .iter()
            .map(|m| (m.name(), m.pending()))
            .collect()
    }

    /// Tells every mechanic that a cell changed.
    ///
    /// Broadcast rather than routed: which mechanics care about a cell
    /// of sand is the mechanics' business, and a router here would be a
    /// second place that has to know what each of them is watching.
    pub fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        for mechanic in &mut self.mechanics {
            mechanic.on_block_changed(gx, gy, gz);
        }
    }

    /// Steps everything, splitting the budget between them.
    ///
    /// An even split rather than a shared pool, because a shared pool is
    /// first-come-first-served: one mechanic with a long queue would
    /// starve every other one for as long as it took to drain, and the
    /// symptom -- "fire stops spreading while sand is falling" -- is the
    /// sort of thing that gets diagnosed as a bug in fire.
    pub fn step(&mut self, world: &dyn BlockWorld, dt: f32) -> Vec<BlockChange> {
        if self.mechanics.is_empty() {
            return Vec::new();
        }
        let share = (self.budget / self.mechanics.len()).max(1);
        let mut changes = Vec::new();
        for mechanic in &mut self.mechanics {
            changes.extend(mechanic.step(world, dt, share));
        }
        changes
    }
}

/// The worked example, and the one mechanic the game ships with.
///
/// Sand was written before this trait existed and needed nothing
/// changed to fit it, which is the strongest thing that can be said for
/// a shape: it was taken from the one implementation there was.
impl CellMechanic for FallingBlocks {
    fn name(&self) -> &'static str {
        "falling blocks"
    }

    fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        FallingBlocks::on_block_changed(self, gx, gy, gz);
    }

    fn step(&mut self, world: &dyn BlockWorld, dt: f32, budget: usize) -> Vec<BlockChange> {
        FallingBlocks::step_budgeted(self, world, dt, budget)
    }

    fn pending(&self) -> usize {
        FallingBlocks::pending(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{BLOCK_SAND, BLOCK_STONE};

    /// A mechanic that turns whatever it is told about into stone, one
    /// cell per step. Enough to prove the contract without a world.
    #[derive(Default)]
    struct Petrify {
        queue: Vec<(i32, i32, i32)>,
    }

    impl CellMechanic for Petrify {
        fn name(&self) -> &'static str {
            "petrify"
        }
        fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
            self.queue.push((gx, gy, gz));
        }
        fn step(&mut self, world: &dyn BlockWorld, _dt: f32, budget: usize) -> Vec<BlockChange> {
            let mut changes = Vec::new();
            for (gx, gy, gz) in self.queue.drain(..).take(budget).collect::<Vec<_>>() {
                world.set(gx, gy, gz, BLOCK_STONE);
                changes.push(BlockChange {
                    global_x: gx,
                    global_y: gy,
                    global_z: gz,
                    block_id: BLOCK_STONE,
                });
            }
            changes
        }
        fn pending(&self) -> usize {
            self.queue.len()
        }
    }

    #[test]
    fn a_registered_mechanic_hears_about_edits_and_gets_stepped() {
        let world = TestWorld::default();
        let mut mechanics = Mechanics::new();
        mechanics.register(Box::new(Petrify::default()));
        assert!(!mechanics.is_empty());

        mechanics.on_block_changed(4, 5, 6);
        assert_eq!(mechanics.pending(), vec![("petrify", 1)]);

        let changes = mechanics.step(&world, 0.05);
        assert_eq!(changes.len(), 1);
        assert_eq!(world.get(4, 5, 6), BLOCK_STONE);
        assert_eq!(mechanics.pending(), vec![("petrify", 0)]);
    }

    #[test]
    fn nothing_registered_costs_nothing() {
        let world = TestWorld::default();
        let mut mechanics = Mechanics::new();
        mechanics.on_block_changed(0, 0, 0);
        assert!(mechanics.step(&world, 0.05).is_empty());
        assert!(mechanics.pending().is_empty());
    }

    #[test]
    fn one_long_queue_does_not_starve_everything_else() {
        // The reason the budget is split rather than shared. A mechanic
        // with thousands of cells queued must not stop the others from
        // running at all while it drains.
        let world = TestWorld::default();
        let mut mechanics = Mechanics::new();
        let mut greedy = Petrify::default();
        for i in 0..10_000 {
            greedy.on_block_changed(i, 40, 0);
        }
        mechanics.register(Box::new(greedy));
        mechanics.register(Box::new(Petrify::default()));
        mechanics.on_block_changed(1, 1, 1);

        let changes = mechanics.step(&world, 0.05);
        assert!(
            changes.iter().any(|c| (c.global_x, c.global_y, c.global_z) == (1, 1, 1)),
            "the second mechanic never ran"
        );
        assert!(changes.len() < 10_000, "the budget was ignored");
    }

    #[test]
    fn sand_is_a_mechanic_like_any_other() {
        // Falling sand through the trait rather than through its own
        // type: if the shape did not fit the one mechanic it was taken
        // from, it fits nothing.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 6, 0, BLOCK_SAND);

        let mut mechanics = Mechanics::new();
        mechanics.register(Box::new(FallingBlocks::new()));
        mechanics.on_block_changed(0, 6, 0);
        assert_eq!(mechanics.pending(), vec![("falling blocks", 2)]);

        for _ in 0..200 {
            mechanics.step(&world, 1.0 / 20.0);
        }
        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "sand did not fall");
    }
}
