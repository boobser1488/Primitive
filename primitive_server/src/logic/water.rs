//! Water that flows.
//!
//! The rules are in `primitive_shared::fluid` and were written down
//! before this existed, for the reason that module gives: a client that
//! draws a surface at one height and a server that thinks the depth is
//! another produce a player swimming through a wall of water that is not
//! where it is drawn. This is the half that owns the world -- it looks
//! cells up, writes them back and reports what changed. It decides
//! nothing about *how much* water ends up where; `fluid::fall_transfer`
//! and `fluid::level_transfer` decide that, and they do it without a
//! world so they can be tested exhaustively.
//!
//! ## The shape
//!
//! A `CellMechanic` like any other (see `logic::simulation`): a queue of
//! coordinates to re-examine, a bounded number of them per pass, and
//! changes handed back for the caller to batch and broadcast. Sand
//! taught the shape; water is the second thing written to it.
//!
//! Two things it does that sand does not:
//!
//! * **It dedupes the queue.** Every cell that changes wakes nine
//!   neighbours, and a flowing front changes hundreds of cells a second
//!   -- so the same coordinate is offered dozens of times before it is
//!   ever looked at. Sand can afford the duplicates because a block edit
//!   wakes two cells; water cannot, and a queue full of the same dozen
//!   coordinates is a queue that is not draining.
//! * **It runs slower than the tick.** Not for the physics -- the rule
//!   is stepwise and has no timestep in it -- but because water that
//!   spreads a block every 50 ms crosses a valley faster than a player
//!   can walk out of it, and because a step that runs a fifth as often
//!   costs a fifth as much. See `FLOW_INTERVAL`.
//!
//! ## Push, not pull -- every change is a transfer
//!
//! **This header used to describe the opposite, and it was the most
//! expensive wrong comment in the file.** It said nothing here moved
//! water: that a cell worked out what depth it *ought* to be from its
//! neighbours and was written if that was not what it already held. That
//! is Minecraft's rule, it is the model this file had before water was
//! made finite, and it has not been the code below since -- `flow_one`
//! calls `fluid::fall_transfer` and `fluid::level_transfer`, both of
//! which answer "how much moves from here to there". The comment
//! survived the rewrite that took the rest of it out, along with a
//! section under it claiming water is not conserved and that a channel
//! cut from the sea does not lower it, which is the exact behaviour the
//! rewrite existed to remove (see `CHANGELOG.md`, "Вода стала
//! конечной", and `GUIDE.md`). Left standing it is an invitation to
//! "restore" the recompute and take draining a pond out of the game
//! again, which is why it is written out here rather than deleted.
//!
//! What the code does: a cell is examined, and whatever it has to give
//! is *handed* to the cell below it or to the lower cells beside it.
//! What one loses another gains, to the eighth. A cell with nowhere to
//! send anything writes nothing, wakes nobody and leaves the queue for
//! good.
//!
//! ## A body of water is one thing
//!
//! There is a third movement here that is not in `fluid`, because it is
//! about the world rather than about two numbers: a cell that has just
//! given water away **draws on whatever is standing within reach of it**
//! -- along the water it is part of, up to `SEARCH_RANGE` cells, taking
//! the same half-difference a neighbour would have given it. See
//! `draw_on_the_body`.
//!
//! Without it the two neighbour rules are a diffusion, and a diffusion
//! is not what water looks like. News of a hole travels one cell a step
//! and half of it is lost at each one, so a lake whose bed is cut
//! through at a corner gives up the ring of cells around the cut and
//! then stops -- 120 eighths of 968 on the fixture in
//! `a_cut_at_the_edge_of_a_lake_drains_it_rather_than_seeping`, and
//! nothing moved again after that, ever. With it, 621 of the 968 leave.
//!
//! **What is deliberately *not* here** is the same idea with the trigger
//! at the other end -- a cell that has nothing to do looking for
//! somewhere shallower to push to. It reads like the same rule and it is
//! not: it can move water in a state the neighbour rules call rest, so
//! `wake` would have to reach `SEARCH_RANGE` in four directions instead
//! of one cell. The whole argument, with what the two ways of paying for
//! it were measured to cost, is on `level_one` -- which is how the wedge
//! that rule was wanted for is levelled now, from a trigger of its own.
//!
//! ## A sheet of water is level
//!
//! And a fourth, which also is not in `fluid` for the same reason: every
//! cell the flow writes goes on a list, and a couple of times a step one
//! of them has the whole connected sheet of water it is part of levelled
//! a little (`level_one`). The neighbour rules cannot split a difference
//! of one, so without it a channel stood as a wedge and two joined ponds
//! at two heights for ever. Its trigger is its own list rather than
//! `wake`, which is the whole reason it could be added without paying
//! what levelling at a distance was measured to cost.
//!
//! ## What conservation asks of the simulation, and how it is paid
//!
//! A rule that moves quantities around can fail to terminate, and that
//! is the whole risk of this model -- two cells trading the same eighth
//! at the tick rate is a shimmer on the surface, a packet and a remesh
//! per hand-over, and the world's edit overlay written on every one and
//! making the autosave rewrite `edits.bin` over a map that is not
//! changing. Both halves of `fluid::level_transfer` -- half the
//! difference, and only when the difference is at least two -- exist to
//! make that impossible, and the tests that say so are in both files:
//! `nothing_is_written_to_the_world_once_the_water_has_settled` here,
//! and `random_ground_comes_to_rest_where_the_rules_call_it_rest`, which
//! sweeps thirty-two random landscapes and checks a brute-force pass
//! finds no work left after the queue has drained.
//!
//! Two consequences worth knowing before editing:
//!
//! * **Still water stays still.** Equal neighbours trade nothing, so an
//!   undisturbed sea costs nothing at all: no writes, no packets, no
//!   overlay rows. It is earned from the rule rather than from a special
//!   case for full cells, which is better -- there is no case to
//!   remember to keep.
//! * **Water *is* conserved, and a full cell is not eternal.** Cut a
//!   channel from a pond and the pond goes down it and is gone. A sea
//!   goes down too; it is simply spread over so many cells that the
//!   difference is under the one eighth the rule can move. See the note
//!   in `fluid` for the whole argument.
//! * **One exception, and only on the server** (`Water::soaking`): a
//!   film of one eighth the flow has left on open ground soaks away
//!   (`soaks_away`), so a pond let out over a meadow does not leave the
//!   meadow ankle-deep for ever. The rain is what puts water back.
//!   `Water::new` conserves to the eighth and every test of conservation
//!   uses it.

use std::collections::{HashSet, VecDeque};

use primitive_shared::fluid;
use primitive_shared::protocol::BlockChange;
use primitive_shared::season;
use primitive_shared::types::{
    blocks_the_sky, can_be_displaced_by_falling, is_air, is_liquid, BlockId, BLOCK_AIR, BLOCK_ICE, CHUNK_SIZE_Y,
};

use crate::logic::falling::BlockWorld;
use crate::logic::rng::Rng;
use crate::logic::simulation::CellMechanic;

/// How long between flow steps, in seconds.
///
/// A quarter of a second is five ticks at the stock rate, and it is the
/// number that decides how fast water moves: a step advances a front by
/// one cell, so this is four cells a second -- a brisk walk, slow enough
/// to run away from and fast enough that a channel fills while you are
/// still looking at it. It is the interval Minecraft uses too, and that
/// is a coincidence about the number rather than about the rule: what
/// moves here is a quantity and what moves there is a distance from a
/// source (see the header). A quarter of a second is simply how fast a
/// front should travel, whichever arithmetic is advancing it.
///
/// It is four fifths of the cost gone as well. The rule does not care
/// how often it is applied (there is no timestep in `fluid`), so the
/// only thing a per-tick step would buy is water that moves twenty cells
/// a second, which is the thing everyone regrets.
///
/// **Faster than a quarter now, and the player asked for it: "воду
/// сделай более текучей".** A front at four cells a second is slower
/// than a walk, so a channel dug from a river arrived behind the person
/// who dug it and a pond emptied at a pace nobody stands and watches.
/// At 0.15 it is between six and seven cells a second -- ahead of a
/// walk, still well short of the twenty a per-tick step would give, and
/// still a *front you can see travelling*, which is the thing this
/// number exists to keep.
///
/// What it costs is proportional and nothing else: the pass does the
/// same work per step and now takes five thirds as many steps, against
/// a budget that is already counted in cells per tick (`MAX_QUEUE` and
/// the mechanic's own allowance) rather than in seconds -- so a busy
/// lake spends its allowance sooner rather than spending more.
const FLOW_INTERVAL: f32 = 0.15;

/// How many flow steps between retries of the cells that stopped at the
/// edge of the loaded world. See `Water::stalled`.
///
/// Twenty is five seconds. Long enough that a border a hundred cells
/// long costs five hundred world reads a *second* at worst, which is
/// less than the drying pass this replaced ran at; short enough that a
/// player walking a chunk into view does not watch dry ground for long.
const RETRY_AFTER_STEPS: u32 = 20;

/// Hard cap on either queue, as for falling blocks: a pathological edit
/// pattern degrades into "some water is slow" rather than into unbounded
/// memory.
const MAX_QUEUE: usize = 64 * 1024;

/// How far a cell that has just given water away may look, along the
/// water it is part of, for something to take a share of.
///
/// **This is the number that turns spreading into flowing**, and it is
/// bought with reads. Falling and levelling only ever look at a
/// neighbour, so a disturbance travels one cell a step and half of it is
/// lost at each one: a lake whose bed is cut through at a corner gives
/// up the ring of cells around the cut and then stops for good. Measured
/// on `a_cut_at_the_edge_of_a_lake_drains_it_rather_than_seeping`, 120
/// of 968 eighths left such a lake, and after that nothing moved again.
///
/// The search is breadth-first through *water*, so what it finds is part
/// of the same body, and it costs at most one read per cell in a diamond
/// of this radius -- 41 cells at four, 85 at six, 145 at eight. What
/// that buys, against what the reference spill pays for it:
///
/// | range | eighths the cut takes, of 968 | spill: ticks, reads |
/// |---|---|---|
/// | no search at all | 120 | 475, 16,999 |
/// | 2 | 223 | 390, 18,160 |
/// | 4 | 480 | 390, 21,434 |
/// | 6 | 621 | 410, 23,759 |
/// | 8 | 697 | 410, 24,000 |
///
/// Eight drains a little more for almost no extra reads *on that
/// fixture*, and six is taken anyway, because the number the fixture
/// does not show is the worst case: `budget` counts cells and not reads,
/// so a tick that examines its whole allowance of five hundred cells,
/// every one of them having just given water away, walks 85 cells per
/// search at six and 145 at eight. Doubling the worst tick in the game
/// to drain a test lake a tenth faster is not a trade worth making.
const SEARCH_RANGE: i32 = 6;

/// How many sheets `level_one` levels a flow step, and the most cells one
/// sheet may be.
///
/// **Two sheets of 256 cells** is a worst case of a few thousand reads a
/// step (three a cell to find its floor and its lid, and one for each dry
/// or solid cell round the edge) -- 2,447 was the worst step measured on
/// the reference spill, against 413 for the flow alone, and a flow step
/// examining its whole budget of cells can walk eighty-five apiece. Paid
/// only while the flow has written something not yet found level; still
/// water is never on the list. A sheet bigger than 256 -- a lake
/// drained through a cut -- is levelled 256 cells at a time from whichever
/// cell comes up, and the overlapping pieces agree in the end, because each
/// pass lowers the same sum of squares whatever piece it is given.
const SHEETS_PER_STEP: usize = 2;
const SHEET_MAX: usize = 256;

/// How many cells of the unlevelled list a step may look at to find its
/// `SHEETS_PER_STEP` -- most of them, in a flood, are still running and go
/// to the back.
const SHEET_LOOKS: usize = 32;

/// How long a film of water the flow has left on open ground stands before
/// it soaks away, in seconds.
///
/// **Twenty, and not a spill's minute and a half** (`SPILL_DRIES_SECONDS`),
/// because a film goes from its edges in: a one soaks, the two beside it
/// levels into the hole and leaves two ones, and *those* wait their own
/// time. At ninety a drained pond eleven cells across was still shining
/// after six minutes, a ring every minute and a half
/// (`a_pond_drained_through_its_bed_leaves_a_film_that_soaks_away`); at
/// twenty the bed is seen wet, then seen drying from the rim, and is dry in
/// a few minutes -- a puddle drying, rather than a puddle that is simply
/// gone one moment.
const SOAK_SECONDS: f32 = 20.0;

/// The deepest water that counts as a film: one eighth. See `soaks_away`.
const SOAKS_AT_OR_BELOW: u8 = 1;

/// The four cells beside one, in a fixed order.
const SIDES: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

type Cell = (i32, i32, i32);

#[derive(Default)]
pub struct Water {
    queue: VecDeque<Cell>,
    /// Exactly the contents of `queue`. See the note above on why water
    /// needs this and sand does not.
    queued: HashSet<Cell>,
    /// Seconds since the last flow step, so the mechanic can run slower
    /// than the tick that drives it.
    since_step: f32,
    /// Everything the flow reached and the loaded world could not
    /// answer for.
    ///
    /// **The one thing the rules cannot answer on their own.** A cell
    /// whose neighbour is in a chunk nobody has generated is a cell
    /// whose answer depends on terrain that does not exist yet, so it is
    /// left alone -- and then it is dropped from the queue, and nothing
    /// ever tells this mechanic that the chunk beyond has since loaded.
    /// What a player saw was a wall of water standing along a straight
    /// line they could not account for.
    ///
    /// **Two kinds of cell get here, and missing the second one left
    /// half the bug in place.**
    ///
    /// * A cell of *water* that could not see past its own border. This
    ///   is the shoreline: the sea is loaded, the land beyond it is not,
    ///   and what has to be looked at again when the chunk lands is what
    ///   is beyond the cell.
    /// * A cell that **was not loaded at all** when the front arrived at
    ///   it. This is a flood that was running with everything loaded
    ///   when the player walked off and the server evicted the ground in
    ///   front of it. The queued cells in the evicted chunk read as
    ///   `None` and used to be dropped -- and nothing else was holding
    ///   the front, because the last loaded cell had already been
    ///   examined, had seen four loaded neighbours, and had left the
    ///   queue. Same symptom as the first case, opposite cause, and only
    ///   the first case was covered.
    ///
    /// Kept in insertion order, and retried rarely (`RETRY_AFTER_STEPS`)
    /// rather than watched for. The alternative -- a `chunk_loaded`
    /// notification on `CellMechanic`, plumbed from the loader and
    /// indexed by chunk -- is strictly better and is three files away
    /// from here; this costs ninety-two world reads a second on a
    /// thirty-cell front that never loads (measured -- see
    /// `a_border_that_never_loads_costs_what_it_was_measured_to_cost`),
    /// which is less than the drying pass it replaces cost, and it is
    /// entirely inside this file.
    ///
    /// A cell whose chunk has still not come back is put back on the
    /// list rather than forgotten. Five seconds of patience is not
    /// patience: a player who leaves a flood running and returns a
    /// minute later is the ordinary case.
    stalled: VecDeque<Cell>,
    stalled_set: HashSet<Cell>,
    /// Flow steps since the last retry pass.
    steps_since_retry: u32,
    /// Scratch for `deeper_within_reach`: the cells the search has
    /// looked at, and how far each one is.
    ///
    /// Kept on the struct rather than made afresh, because the search
    /// runs inside the per-cell loop and a `Vec` allocated and dropped a
    /// few hundred times a step is a measurable cost for something that
    /// never holds more than a few dozen entries. Never read between
    /// calls; it is cleared on entry.
    search: Vec<(Cell, i32)>,
    /// Cells the flow has written, waiting to have the sheet of water they
    /// are part of levelled. See `level_one`.
    ///
    /// `unlevelled_set` is the truth and the deque only an order: a sheet
    /// levelled from one cell takes every other cell of it off the set, and
    /// the entries left behind in the deque are skipped when they come up.
    unlevelled: VecDeque<Cell>,
    unlevelled_set: HashSet<Cell>,
    /// Scratch for `level_one`, for the reason `search` is kept.
    sheet: Vec<Cell>,
    sheet_seen: HashSet<Cell>,
    sheet_depths: Vec<u8>,
    /// Whether a thin film the flow leaves behind soaks away. Off in `new`,
    /// which every test of conservation builds on; on in `soaking`, which
    /// is what the server runs. See `SOAK_SECONDS`.
    soaks: bool,
    /// `SHEETS_PER_STEP`, as a field so the cost of levelling can be
    /// measured against the flow alone in one binary
    /// (`the_reference_spill_timed`). Nothing else sets it.
    sheets_per_step: usize,
    /// Seconds of flow steps so far: the clock `thin` is timed by.
    clock: f32,
    /// Films the flow has written, and the `clock` at which each is looked
    /// at again to see if it is still a film. In that order, because every
    /// one waits the same `SOAK_SECONDS`.
    thin: VecDeque<(Cell, f32)>,
    thin_set: HashSet<Cell>,
}

impl Water {
    /// Water that is conserved to the eighth: nothing made, nothing lost.
    pub fn new() -> Self {
        Self { sheets_per_step: SHEETS_PER_STEP, ..Self::default() }
    }

    /// Water as the server runs it: conserved, except that a film of one
    /// eighth the flow has left on open ground soaks away after
    /// `SOAK_SECONDS`. See `soaks_away`.
    pub fn soaking() -> Self {
        Self { soaks: true, ..Self::new() }
    }

    /// Work still to do: cells to flow, and sheets to level. Not the films
    /// waiting to soak away, which are on a clock rather than a queue --
    /// the way a spill is (`Spills`).
    pub fn pending(&self) -> usize {
        self.queue.len() + self.unlevelled_set.len()
    }

    /// Queue a cell, unless it is outside the world or already waiting.
    fn push(&mut self, (x, y, z): Cell) {
        if y < 0 || y >= CHUNK_SIZE_Y as i32 || self.queue.len() >= MAX_QUEUE {
            return;
        }
        if self.queued.insert((x, y, z)) {
            self.queue.push_back((x, y, z));
        }
    }

    /// Every cell whose water could have been changed by water arriving
    /// at or leaving `p`.
    ///
    /// **This is the exact inverse of what an update reads**, and it is
    /// written out rather than approximated because a missing entry here
    /// is water that stops one cell short of where the rules say it goes
    /// and stays there until something else nearby is disturbed.
    ///
    /// An update of a cell `c` *decides* from `c`, the cell **below**
    /// `c`, and the four **beside** it. That is all: six cells, and the
    /// whole rule is "fall if there is room down there, otherwise level
    /// with whoever is lower".
    ///
    /// **It reads further than it decides, and the difference is the
    /// point.** `draw_on_the_body` walks a diamond of `SEARCH_RANGE`
    /// looking for somewhere to take a share of, so an update can touch
    /// eighty-five cells -- and this still wakes six, on purpose. What
    /// unlocks that search is not anything about the far cells: it is
    /// that *this* cell has just given water away, which is a fact about
    /// `c` and its six. So the fact still travels one cell a step,
    /// exactly as it did, and only the water travels further. Get that
    /// backwards -- write a rule whose *trigger* is a distant cell's
    /// depth -- and this list has to grow to the whole diamond; there is
    /// a note on `level_one` about what that was measured to cost when it
    /// was tried, and what levels a sheet instead.
    ///
    /// Turning it round, a change at `p` matters to:
    ///
    /// * the four cells **beside** `p`, which level against it;
    /// * the cell **above** `p`, which reads `p` as its floor -- new
    ///   under this model, and the reason a waterfall whose foot drains
    ///   away starts running again instead of standing there with
    ///   somewhere to go and no reason to look;
    /// * the cell **below** `p`, which is how water keeps falling: `p`
    ///   drops into it, `set` wakes `p`'s below, and that cell takes its
    ///   own turn and drops further.
    ///
    /// Six cells, and the count is worth guarding. **The old model woke
    /// four more** -- the ones beside the cell under `p` -- because a
    /// cell's depth there depended on whether each *neighbour* had a
    /// floor. Nothing reads that any more: when the cell below actually
    /// changes, `set` is called on it and wakes its own neighbours
    /// itself. Dropping the group was measured on the reference spill at
    /// 20,144 reads before and 16,999 after, with the same ticks, the
    /// same writes, and the random-ground sweep still agreeing that the
    /// queue leaves nothing undone.
    ///
    /// `p` itself is absent too, and only from here. What decides `p`'s
    /// own depth is what is beside and above it, and writing `p` changes
    /// none of that -- so re-offering it would be ten world reads to
    /// write nothing. A *block* edit at `p` is a different matter and
    /// `on_block_changed` queues it.
    ///
    /// The "below each of the four" group is absent as well, and for a
    /// reason rather than by omission: that read asks whether there is a
    /// *floor* there, and a floor is terrain. Water is never a floor and
    /// neither is air, so nothing this mechanic writes can change the
    /// answer. A block edit can, and `on_block_changed` adds it.
    fn wake(&mut self, (x, y, z): Cell) {
        for (dx, dz) in SIDES {
            self.push((x + dx, y, z + dz));
        }
        self.push((x, y - 1, z));
        // **And the cell above, which the old model never needed.**
        // Depth used to be computed from below and beside, so a cell
        // losing water told nobody overhead. Now that water is moved, a
        // cell that has just made room is exactly the cell the one above
        // it can fall into -- and a waterfall whose foot drains away
        // would otherwise stand there with somewhere to go and no reason
        // to look.
        self.push((x, y + 1, z));
    }

    /// Can water be in this cell at all?
    ///
    /// The same list that decides what falling sand buries: air, water,
    /// and the things that stand in a cell without filling it. A tuft of
    /// grass does not hold back a flood.
    fn holds_water(block: BlockId) -> bool {
        can_be_displaced_by_falling(block)
    }


    /// Writes a cell and wakes what that can affect.
    fn set(
        &mut self,
        world: &dyn BlockWorld,
        cell: Cell,
        depth: u8,
        changes: &mut Vec<BlockChange>,
    ) {
        let block = fluid::with_depth(depth);
        let before = world.block(cell.0, cell.1, cell.2);
        world.set(cell.0, cell.1, cell.2, block);
        // **Every change of depth is told, because every depth is drawn.**
        //
        // This used to filter: `fluid::surface_height` returned one
        // height for every depth, so a cell going from three eighths to
        // five drew *identically*, and sending it cost a packet, a chunk
        // remesh on every client that could see it (~0.8 ms of their
        // frame budget) and a row in the world's edit overlay, to change
        // nothing. Now each eighth is its own height and the mesher
        // slopes between them, so that same change is a surface visibly
        // settling -- exactly what the player asked to see -- and it has
        // to go out. The remesh cost is real and is paid at
        // `FLOW_INTERVAL`, not at the tick; `visibly_different` is where
        // the filter would go back if a depth ever stopped being drawn.
        if visibly_different(before, block) {
            changes.push(BlockChange {
                global_x: cell.0,
                global_y: cell.1,
                global_z: cell.2,
                block_id: block,
            });
        }
        // Not `cell` itself, and it does not need to be: whatever
        // brought water here took its own turn to do it, and a cell that
        // has just *given* water away has already spent this turn. What
        // did change is what this cell is to its neighbours -- a floor
        // to the one above, a lower side to the four around it -- and
        // those are exactly what `wake` offers.
        self.wake(cell);
        // ...and the sheet it is part of is no longer known to be level,
        // and a film is no longer known not to be one. Only cells the flow
        // has written: water nobody has disturbed -- every sea, every lake
        // the generator laid -- is never looked at by either.
        if depth > 0 && self.unlevelled.len() < MAX_QUEUE && self.unlevelled_set.insert(cell) {
            self.unlevelled.push_back(cell);
        }
        if self.soaks && depth > 0 && depth <= SOAKS_AT_OR_BELOW && self.thin.len() < MAX_QUEUE && self.thin_set.insert(cell) {
            self.thin.push_back((cell, self.clock + SOAK_SECONDS));
        }
    }

    /// Is this a cell whose water is the top of a sheet: water, with a
    /// floor under it that will not take any more, and no water over it?
    /// Returns how deep it is.
    ///
    /// The floor is ground, or a full cell of water: anything else under it
    /// is somewhere it can still fall, which is the flow's business and not
    /// a sheet's. Nothing over it, because a cell with water over it is not
    /// the surface -- its surface is further up. Unloaded counts as neither,
    /// so a sheet stops at the edge of what is loaded rather than guessing.
    ///
    /// **Plain water only.** A drowned snag is liquid and has a depth, and
    /// writing a depth back into its cell is writing water over the wood --
    /// the flow never does that, because a full snag beside full water has
    /// nothing to hand on, but a sheet levelled through it would.
    fn sheet_depth(world: &dyn BlockWorld, (x, y, z): Cell) -> Option<u8> {
        let block = world.block(x, y, z)?;
        let here = fluid::depth(block);
        if here == 0 || fluid::with_depth(here) != block {
            return None;
        }
        let below = world.block(x, y - 1, z)?;
        if Self::holds_water(below) && fluid::depth(below) < fluid::SOURCE_DEPTH {
            return None;
        }
        if is_liquid(world.block(x, y + 1, z)?) {
            return None;
        }
        Some(here)
    }

    /// Levels the sheet of water `start` is part of by one pass of
    /// `fluid::level_sheet`: each of its deepest cells hands an eighth to
    /// one of its shallowest.
    ///
    /// **Why the neighbour rules are not enough, and what this is.** They
    /// cannot split a difference of one, so a surface sloping an eighth a
    /// block is at rest under them -- and that wedge was the last thing
    /// the water did that water does not: a channel poured full at one end
    /// stopped five blocks short of its end, and two ponds joined by a
    /// trench stood three quarters of a block apart for ever. Measured on
    /// `a_channel_poured_at_one_end_levels_along_its_whole_length` before
    /// this existed: `[7, 6, 5, 4, 4, 3, 2, 1, 0, 0, 0, 0, 0]`.
    ///
    /// A sheet is found breadth-first through water at one height, every
    /// cell of it the top of its column (`sheet_depth`), up to `SHEET_MAX`
    /// cells -- so, like `draw_on_the_body`, it only ever moves water
    /// between cells joined by water, never over a bank or across a gap.
    ///
    /// **Rejected, and each was tried in this file:**
    ///
    /// * *Levelling at a distance from inside `flow_one`* -- the note in
    ///   `a_pond_drains_down_to_a_wedge_and_no_further`, the test that
    ///   asserted the wedge before this, said why: a wedge is a rest state of the neighbour rules, so a rule
    ///   that can still move water there needs `wake` to reach the whole
    ///   diamond, and that took the reference spill from 17,000 reads to
    ///   101,000. This is the same idea with a trigger of its own -- a
    ///   list of cells the flow has *written*, looked at a few sheets a
    ///   step -- so `wake` stays six cells and a flow step costs what it
    ///   did.
    /// * *Writing the whole sheet to its mean at once.* Levels it, and
    ///   snaps: a wedge eight deep jumped half a block at each end in one
    ///   step. One eighth a pair a pass is a surface that settles while you
    ///   watch it (`fluid::level_sheet`).
    /// * *Finer units*, which would make the wedge shallower rather than
    ///   gone, and are a save format and a vertex byte besides.
    ///
    /// It stops because every pair moves an eighth from one cell to
    /// another at least two shallower, which lowers the same sum of squares
    /// `level_transfer` lowers; a sheet within an eighth of level moves
    /// nothing, is written nowhere, and puts nothing back on the list.
    fn level_one(&mut self, world: &dyn BlockWorld, start: Cell, changes: &mut Vec<BlockChange>) {
        let Some(depth) = Self::sheet_depth(world, start) else {
            return;
        };
        let mut sheet = std::mem::take(&mut self.sheet);
        let mut depths = std::mem::take(&mut self.sheet_depths);
        sheet.clear();
        depths.clear();
        self.sheet_seen.clear();
        sheet.push(start);
        depths.push(depth);
        self.sheet_seen.insert(start);
        let mut head = 0;
        while head < sheet.len() && sheet.len() < SHEET_MAX {
            let (x, y, z) = sheet[head];
            head += 1;
            for (dx, dz) in SIDES {
                let next = (x + dx, y, z + dz);
                if sheet.len() >= SHEET_MAX || !self.sheet_seen.insert(next) {
                    continue;
                }
                if let Some(there) = Self::sheet_depth(world, next) {
                    sheet.push(next);
                    depths.push(there);
                }
            }
        }
        // Every cell of it has been looked at now, whether it moves or not.
        for cell in &sheet {
            self.unlevelled_set.remove(cell);
        }
        let before = depths.clone();
        if fluid::level_sheet(&mut depths) {
            for (i, &cell) in sheet.iter().enumerate() {
                if depths[i] != before[i] {
                    // `set` puts it back on the list: the next pass looks
                    // again, which is how an eighth a pass becomes level.
                    self.set(world, cell, depths[i], changes);
                    // ...and the cell itself goes to the flow, which `set`
                    // leaves out because a cell the flow writes is one it
                    // has just examined. This one has not been: the edge of
                    // a sheet raised from one eighth to three is an edge
                    // that now pours onto the dry floor beside it, and
                    // without this it stood there at three for ever.
                    self.push(cell);
                }
            }
        }
        self.sheet = sheet;
        self.sheet_depths = depths;
    }

    /// Should this film of water soak away now?
    ///
    /// **One eighth, on ground, under nothing wet, beside nothing deeper
    /// than two.** Conservation leaves films: a pond drained through a hole
    /// keeps an eighth in every cell of its bed, and once `level_one`
    /// spreads a spill as far as its water goes, a pond let out over a
    /// field is a film over the field. An eighth of a block across a
    /// meadow is still water to the collider, the fog and the frost, and it
    /// never went anywhere -- the ankle-deep sea over everything a player
    /// has ever drained, which this file has been through once already.
    ///
    /// *Beside nothing deeper than two* rather than nothing deeper than
    /// one, because a sheet levelled to within an eighth is a mix of ones
    /// and twos, and a one that had to wait for every two beside it would
    /// wait for ever. When it goes, the twos level into the hole, and the
    /// ones they leave go in their turn -- so a sheet a quarter of a block
    /// deep or less on open ground soaks away from its edges, and anything
    /// deeper is a pond and stays. Not ground under it (a fall's kept
    /// eighth) and not water over it (the bottom of a column).
    fn soaks_away(world: &dyn BlockWorld, (x, y, z): Cell) -> bool {
        let Some(block) = world.block(x, y, z) else {
            return false;
        };
        let here = fluid::depth(block);
        if here == 0 || here > SOAKS_AT_OR_BELOW || fluid::with_depth(here) != block {
            return false;
        }
        if world.block(x, y - 1, z).is_none_or(Self::holds_water) || world.block(x, y + 1, z).is_none_or(is_liquid) {
            return false;
        }
        SIDES.iter().all(|(dx, dz)| {
            world
                .block(x + dx, y, z + dz)
                .is_some_and(|beside| fluid::depth(beside) <= SOAKS_AT_OR_BELOW + 1)
        })
    }

    /// Water that has reached a block somebody has been quarrying takes
    /// the rest of that block with it.
    ///
    /// **A bite is a hole in the block, and a hole is all a lake needs.**
    /// The player asked for water to flow into the space a dig opens, and a
    /// cell cannot hold three quarters of a granite block *and* the water
    /// standing in the quarter that is gone: one cell is one id. So the
    /// remainder goes, and the cell is air the flow can fill on the next
    /// pass.
    ///
    /// **And it gives nothing back** -- no drop, no stone in the pack.
    /// That is the decision rather than an omission: cutting into a
    /// cistern from the wet side costs you the block you were half way
    /// through, and walking round to drain it from the dry side first is
    /// the plan the mechanic exists to make you form. It also keeps the
    /// fluid simulation out of the business of spawning items, which is
    /// the one thing in this file that would need a `Context`.
    ///
    /// Only a *bitten* block: a whole one holds a lake back exactly as it
    /// always has, so nothing a player built leaks.
    fn wash_out(&mut self, world: &dyn BlockWorld, cell: Cell, changes: &mut Vec<BlockChange>) {
        world.set(cell.0, cell.1, cell.2, BLOCK_AIR);
        changes.push(BlockChange {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
            block_id: BLOCK_AIR,
        });
        // The cell itself as well as its neighbours: it has just become
        // somewhere water can be, and nothing else is going to notice.
        self.push(cell);
        self.wake(cell);
    }

    /// One cell's worth of work: where does this cell's water go?
    ///
    /// **Water is moved, not recomputed.** The old rule asked what a
    /// cell's depth *should* be given its neighbours, and answered with
    /// a number -- which meant a source was full by definition and no
    /// pond could ever be emptied. Every change here is a transfer:
    /// what this cell loses, another gains, to the unit. See the note
    /// on `fluid::level_transfer`.
    ///
    /// `budget` counts these, not the lookups inside them, and that
    /// mattered less when the most an update could do was six reads --
    /// this cell, the one below it, the four beside it. A cell that has
    /// given water away now also searches (`draw_on_the_body`), and a
    /// search walks up to eighty-five. **A cell is still the unit of
    /// budget anyway**, because the alternative is stopping half way
    /// through one and remembering where, and a mechanic that can leave
    /// a cell half-updated is a mechanic that can lose an eighth. What
    /// the budget therefore has to be read as is "cells, and the reads
    /// they may cost"; the worst tick is measured in
    /// `the_reference_spill_costs_no_more_than_it_was_measured_to_cost`
    /// and the reasoning about the ceiling is on `SEARCH_RANGE`.
    fn flow_one(&mut self, world: &dyn BlockWorld, cell: Cell, changes: &mut Vec<BlockChange>) {
        let (x, y, z) = cell;
        let Some(block) = world.block(x, y, z) else {
            // **Not loaded: leave it alone rather than guess -- but do
            // not forget it.** This is the cell a flood was about to
            // reach when the chunk it lives in went away, and dropping
            // it is how a flood used to stop dead on a straight line for
            // ever. Nothing else is holding the front: the last *loaded*
            // cell was examined before the eviction, saw four loaded
            // neighbours, and left the queue with a clean conscience, so
            // it will never put itself on the slow list either.
            self.mark_stalled(cell);
            return;
        };
        if !Self::holds_water(block) {
            return; // solid ground
        }
        let was = fluid::depth(block);
        if was == 0 {
            // Air with nothing in it. Something else will fall or flow
            // into it and wake it again; there is nothing here to move.
            return;
        }
        let mut here = was;

        // ---- down first, and all of it ----
        //
        // Water does not half-fall. Given somewhere to go down it goes,
        // and only what is left over spreads -- doing it the other way
        // round makes a waterfall that puddles at its own lip.
        let below = (x, y - 1, z);
        match world.block(below.0, below.1, below.2) {
            None => {
                // The floor is not loaded. Guessing here is how water
                // pours into a chunk that turns out to be stone.
                self.mark_stalled(cell);
                return;
            }
            // **A bite in the floor is washed through**, and this tick does
            // nothing else: the cell below becomes air here and the water
            // falls into it on the next pass, which is one flow interval
            // away. Free to ask -- the block is already read.
            // ...but not a turf lip (`dig::is_turf_lip`): the roots hold
            // it, and a stream let down a meadow runs over the slope rather
            // than cutting a gully through the lip of every rise.
            Some(under) if primitive_shared::dig::is_dug(under) && !primitive_shared::dig::is_turf_lip(under) => {
                self.wash_out(world, below, changes);
                return;
            }
            Some(under) if Self::holds_water(under) => {
                let there = fluid::depth(under);
                // **A fed cell keeps its last eighth** -- which is what
                // makes a waterfall a column rather than drops in every
                // other cell. The whole argument, and what was weighed
                // against it, is on `fluid::fall_keeping`. The reads that
                // answer "fed" are paid only when the fall would empty
                // the cell, which is the only case the answer changes.
                let fed = fluid::fall_transfer(here, there) == here && self.is_fed(world, cell);
                let moved = fluid::fall_keeping(here, there, fed);
                if moved > 0 {
                    self.set(world, below, there + moved, changes);
                    here -= moved;
                    self.set(world, cell, here, changes);
                    // ...and then draw on the body behind it. **This is
                    // the cell the whole search exists for**: the one
                    // over the hole, which empties itself downward every
                    // step and is refilled a neighbour at a time. Left
                    // out of this branch -- which is how it was written
                    // first -- the drain is fed by diffusion and the
                    // lake behind it settles into a wedge with nearly
                    // half of itself still standing in it.
                    self.draw_on_the_body(world, cell, here, &[None; 4], changes);
                    // Falling is the whole of this cell's turn
                    // otherwise. What is left spreads on the next one,
                    // by which time the cell below has had its own turn
                    // and may have taken more.
                    return;
                }
            }
            Some(_) => {}
        }

        // ---- then level, with each side in turn ----
        //
        // `here` is spent as it goes, so the second side is offered what
        // is actually left rather than what there was to begin with.
        // Without that a cell with four empty neighbours would promise
        // each of them half of the same water.
        let mut waiting_on_a_chunk = false;
        // What each side holds *now*. The search below is seeded from
        // it, so a cell that ends up looking further afield does not pay
        // to read its own four neighbours a second time -- and it has to
        // be what they hold after this pass rather than before it, or a
        // side this cell has just poured into reads as a place to pour
        // into again.
        let mut sides = [None; 4];
        for (i, (dx, dz)) in SIDES.into_iter().enumerate() {
            let side = (x + dx, y, z + dz);
            match world.block(side.0, side.1, side.2) {
                None => waiting_on_a_chunk = true,
                // ...and a bite in the wall beside it, on the same terms.
                // The side is left out of this pass's arithmetic -- it is
                // rock until the wash lands -- so nothing is promised to a
                // cell that is not yet somewhere water can be.
                Some(there_block)
                    if primitive_shared::dig::is_dug(there_block) && !primitive_shared::dig::is_turf_lip(there_block) =>
                {
                    self.wash_out(world, side, changes);
                }
                Some(there_block) if Self::holds_water(there_block) => {
                    let there = fluid::depth(there_block);
                    let moved = fluid::level_transfer(here, there);
                    if moved > 0 {
                        self.set(world, side, there + moved, changes);
                        here -= moved;
                    }
                    sides[i] = Some(there + moved);
                }
                Some(_) => {}
            }
        }
        if here != was {
            self.set(world, cell, here, changes);
        }

        // ---- and what has just emptied draws on what is behind it ----
        //
        // **This is the difference between water that spreads and water
        // that flows.** Falling and levelling only ever look at a
        // neighbour, so news of a hole travels one cell a step and half
        // of it is lost at each one: a lake whose bed is cut through at
        // a corner empties at a rate that has nothing to do with how
        // much water is standing behind the cut, and a channel dug out
        // of a pond runs for four blocks and stops. What the model was
        // missing is not a rule about holes -- it is that a body of
        // water is *one thing*, and taking water out of one part of it
        // draws on the rest.
        //
        // So a cell that has just given water away -- fallen down a
        // hole, or levelled into a gap -- looks along the water it is
        // part of for somewhere deeper within `SEARCH_RANGE`, and takes
        // the same half-difference a neighbour would have given it. The
        // draw then spreads outward at `SEARCH_RANGE` cells a step
        // rather than one, because every cell it takes from has itself
        // given water away and draws in its turn.
        //
        // **"Has just given water away" is what makes this provable, and
        // it is not a cheap trick.** Levelling at a distance sounds
        // like the same rule and is not: a wedge -- a surface sloping
        // one eighth per block -- is a *rest state* under the neighbour
        // rules, and a rule that could still move water there would need
        // `wake` to reach `SEARCH_RANGE` in every direction rather than
        // one. Written that way, the sweep in
        // `random_ground_comes_to_rest_where_the_rules_call_it_rest`
        // found the queue draining with work left to do on the second
        // seed it tried. Water having moved *here* is local knowledge,
        // the six-cell `wake` already delivers it exactly, and at rest
        // no cell has moved, so this cannot fire -- the fixed point is
        // the one the neighbour rules had, unchanged.
        //
        // The two guards in front of it are the cost control, and they
        // are answered out of numbers this pass already had: still water
        // moves nothing and so never looks, which is every cell of every
        // ocean; and a cell within one eighth of full has no room for
        // half of any difference.
        if here < was {
            self.draw_on_the_body(world, cell, here, &sides, changes);
        }
        // Only water waits on a chunk. An empty cell beside an unloaded
        // one has nothing to resume doing, and putting those on the list
        // would multiply it by the volume of air along the border.
        if here > 0 && waiting_on_a_chunk {
            self.mark_stalled(cell);
        }
    }

    /// Will something hand this cell more water: the cell above holding
    /// any, or a side that would level into it once it is down to one
    /// eighth?
    ///
    /// **Strict on purpose**, and `fluid::fall_keeping` says why: a side
    /// that merely *has* water is how a drained lake's last eighth at the
    /// lip would hold a whole fall up in the air for ever. A side that
    /// would pour is a side that is not at rest, so a kept eighth always
    /// has a neighbour about to move. An unloaded neighbour feeds nothing:
    /// the cell that could not see past it is on the slow list already.
    fn is_fed(&self, world: &dyn BlockWorld, (x, y, z): Cell) -> bool {
        if world.block(x, y + 1, z).is_some_and(|above| fluid::depth(above) > 0) {
            return true;
        }
        SIDES.into_iter().any(|(dx, dz)| {
            world
                .block(x + dx, y, z + dz)
                .is_some_and(|side| fluid::level_transfer(fluid::depth(side), 1) > 0)
        })
    }

    /// Takes a share of whatever is standing within reach, into a cell
    /// that has just given water away.
    ///
    /// `sides`, where the caller has them, is what the four neighbours
    /// hold *now*; `[None; 4]` means "read them yourself", which is what
    /// the falling branch passes because it never looked sideways.
    fn draw_on_the_body(
        &mut self,
        world: &dyn BlockWorld,
        cell: Cell,
        here: u8,
        sides: &[Option<u8>; 4],
        changes: &mut Vec<BlockChange>,
    ) {
        // A cell within one eighth of full has no room for half of any
        // difference, so there is nothing to look for.
        if here + 2 > fluid::SOURCE_DEPTH {
            return;
        }
        let Some((from, deeper)) = self.deeper_within_reach(world, cell, here, sides) else {
            return;
        };
        let moved = fluid::level_transfer(deeper, here);
        if moved > 0 {
            self.set(world, from, deeper - moved, changes);
            self.set(world, cell, here + moved, changes);
        }
    }

    /// The nearest cell this one could draw on: somewhere at the same
    /// height, reachable through water, holding at least two eighths
    /// more than `here`. Returns it and how deep it is.
    ///
    /// **Breadth-first through water, and only through water.** The path
    /// matters as much as the far end: a search that walked through air
    /// would find a puddle on the other side of a bank and drag it over
    /// the bank, which is water going through a wall. Every cell on the
    /// path holds water, so what comes back is part of the same body --
    /// and moving an eighth from one part of a connected body of water
    /// to another is what a body of water *is*.
    ///
    /// Nearest rather than deepest, which is a deliberate difference
    /// from the mod this behaviour is named after. Deepest needs the
    /// whole diamond walked every time; nearest stops at the first ring
    /// with an answer, and the two agree in the end anyway, because the
    /// cell this one draws from has itself given water away and draws in
    /// its turn on the step after.
    ///
    /// The four sides are seeded from what the levelling pass has
    /// already read and are tested as sources without a read of their
    /// own, so this costs nothing at all for the ring it starts from --
    /// which is where the answer is most of the time.
    fn deeper_within_reach(
        &mut self,
        world: &dyn BlockWorld,
        (x, y, z): Cell,
        here: u8,
        sides: &[Option<u8>; 4],
    ) -> Option<(Cell, u8)> {
        // Taken out of `self` and put back at the end, because `set`
        // wants `&mut self` and a scratch buffer borrowed across it
        // would not compile. Nothing reads it between calls.
        let mut seen = std::mem::take(&mut self.search);
        seen.clear();
        seen.push(((x, y, z), 0));
        let mut found = None;
        for (i, (dx, dz)) in SIDES.into_iter().enumerate() {
            let Some(there) = sides[i] else { continue };
            if there == 0 {
                // A dry side is not on the path and cannot be drawn on.
                continue;
            }
            if found.is_none() && there >= here + 2 {
                found = Some(((x + dx, y, z + dz), there));
            }
            seen.push(((x + dx, y, z + dz), 1));
        }

        let mut head = 0;
        while head < seen.len() && found.is_none() {
            let (at, distance) = seen[head];
            head += 1;
            if distance >= SEARCH_RANGE {
                continue;
            }
            for (dx, dz) in SIDES {
                let next = (at.0 + dx, y, at.2 + dz);
                if seen.iter().any(|(cell, _)| *cell == next) {
                    continue;
                }
                let Some(block) = world.block(next.0, next.1, next.2) else {
                    // Not loaded: impassable, and nothing more. It is
                    // deliberately *not* put on the slow list --
                    // `flow_one` lists a cell whose own neighbour it
                    // could not read, and listing every cell whose
                    // search merely brushed a border would multiply that
                    // list by the area of the search instead of by the
                    // length of the shore.
                    continue;
                };
                if !Self::holds_water(block) {
                    continue;
                }
                let there = fluid::depth(block);
                if there == 0 {
                    continue;
                }
                if there >= here + 2 {
                    found = Some((next, there));
                    break;
                }
                seen.push((next, distance + 1));
            }
        }
        self.search = seen;
        found
    }

    /// Puts a cell on the slow queue, if it is not already on it.
    fn mark_stalled(&mut self, cell: Cell) {
        if self.stalled.len() >= MAX_QUEUE {
            return;
        }
        if self.stalled_set.insert(cell) {
            self.stalled.push_back(cell);
        }
    }

    /// One waiting cell: has the chunk it was stopped by arrived?
    ///
    /// Nothing is computed here -- the cell is simply re-offered to the
    /// ordinary machinery, along with whichever of its neighbours can be
    /// seen now. A cell that was already right computes to itself and
    /// drops straight out of the queue, so the steady cost of a border
    /// that never loads is six reads per waiting cell per retry.
    ///
    /// **The cell itself is offered as well as its sides**, and the
    /// difference matters for the two ways a cell gets on this list.
    /// One is a cell of water that could not see past its own border:
    /// what has to be re-examined is what is *beyond* it, so the sides
    /// are the point. The other is a cell that was not loaded at all
    /// when the front reached it -- there the cell itself is the thing
    /// that has never been worked out, and offering only its sides
    /// would look at four cells that are all already right and leave
    /// the hole in the middle.
    fn retry_one(&mut self, world: &dyn BlockWorld, cell: Cell) {
        let (x, y, z) = cell;
        let Some(block) = world.block(x, y, z) else {
            // Still nothing there. **Keep it.** Forgetting a cell whose
            // chunk has not come back yet is the same bug as never
            // listing it: a player who walks away from a flood and
            // returns a minute later is the ordinary case, not the
            // exotic one, and five seconds of patience is not enough
            // patience.
            self.mark_stalled(cell);
            return;
        };
        self.push(cell);
        if !is_liquid(block) && !Self::holds_water(block) {
            return; // solid ground now: nothing here can ever flow
        }
        let mut still_waiting = false;
        for (dx, dz) in SIDES {
            let side = (x + dx, y, z + dz);
            match world.block(side.0, side.1, side.2) {
                None => still_waiting = true,
                Some(_) => self.push(side),
            }
        }
        // Only water goes back on the list. An empty cell beside an
        // unloaded one has nothing to resume doing, and keeping those
        // would multiply the list by the volume of air along the
        // border -- the same rule `flow_one` applies at the end of an
        // ordinary update.
        if still_waiting && is_liquid(block) {
            self.mark_stalled(cell);
        }
    }
}

/// Rain, as far as standing water is concerned.
///
/// ## Why there is a spring in a game that took the springs out
///
/// The endless source went for a good reason and is not coming back:
/// a pond that refills itself is a pond that cannot be drained, and
/// draining one is most of what a player wants to do with water. But a
/// world where every eighth is accounted for and none is ever added is
/// a world that only ever gets drier -- every channel dug, every bucket
/// carried and every hole in a lake bed is one-way, and after a long
/// enough game the map is a record of everywhere somebody has been.
///
/// So the sky puts water back, and everything about how it does it is
/// chosen to keep the pond drainable:
///
/// * **It only deepens water that is already there.** See
///   `fluid::rain_transfer` for why -- briefly, the sky reaches every
///   open cell in the world, and one eighth is drawn exactly like
///   eight, so a rule that wet dry ground would flood the landscape on
///   screen the moment it started raining.
/// * **It refuses any cell that has somewhere to send its water** --
///   anything that can fall, anything that can level into a neighbour.
///   That is what makes a drain win: while a pond is running out
///   through a cut, every cell of it is a cell water is leaving, and
///   the sky cannot touch one. Rain fills a pond that is *standing*,
///   which is the only kind of pond there is anything to fill.
/// * **It is slow, and it is local.** Four drops every two seconds
///   around each player, so the sky adds two eighths a second to a
///   world in which one cut passes eight. A player who wants a pond
///   emptied in the rain digs a second channel; a player who wants one
///   kept digs a roof.
///
/// ## Why it is driven from outside
///
/// It needs to know where the players are, and this file knows nothing
/// about players. The tick loop hands in their positions and whether it
/// is raining, and gets back the cells it changed so it can tell the
/// flow simulation about them -- which it must, because water that
/// arrives and is never examined sits there until something else nearby
/// is disturbed.
///
/// Nothing is broadcast. A cell going from three eighths to four draws
/// identically (`fluid::surface_height`), so there is nothing for a
/// client to redraw until the water it feeds actually moves somewhere,
/// and that goes out through the flow simulation like any other change.
pub struct Rainfall {
    since: f32,
    rng: Rng,
}

/// How long between showers, in seconds.
///
/// Not the tick, and not `FLOW_INTERVAL` either. What this decides is
/// how fast the sky can fill a pond, and the number that matters is how
/// it compares with what a drain passes: a one-block cut moves up to
/// eight eighths a flow step, which is thirty-two a second, and this is
/// two. The sky is sixteen times slower than a hole, and that ratio is
/// the whole of why `a_pond_in_the_rain_still_empties_through_a_cut`
/// holds.
const RAIN_INTERVAL: f32 = 2.0;

/// How many columns each player's neighbourhood gets per shower.
const DROPS_PER_PLAYER: usize = 4;

/// How far from a player rain is simulated, in blocks.
///
/// Rain is only ever simulated where somebody can see it, for the same
/// reason chunks are only loaded there. Weather over an empty continent
/// changing the world is a cost with nobody to notice it -- and, worse,
/// a cost that grows with the size of the map rather than with the
/// number of people playing.
const RAIN_RADIUS: i32 = 48;

/// How far above and below a player a drop is looked for.
///
/// **A drop falls from the sky, so the honest search is the whole
/// column** -- and a whole column is two hundred and fifty-six reads,
/// four times a shower, per player. Bounding it to the storeys around
/// the player costs the case of rain filling a pond on a mountain top
/// while the player stands in the valley, which is rain nobody is
/// looking at, and buys back three quarters of the reads.
const RAIN_ABOVE: i32 = 32;
const RAIN_BELOW: i32 = 32;

impl Default for Rainfall {
    fn default() -> Self {
        Self::new()
    }
}

impl Rainfall {
    pub fn new() -> Self {
        Self {
            since: 0.0,
            rng: Rng::from_clock(),
        }
    }

    /// The same, repeatable, for tests.
    pub fn seeded(seed: u64) -> Self {
        Self {
            since: 0.0,
            rng: Rng::seeded(seed),
        }
    }

    /// Has enough time passed for another shower?
    ///
    /// Separate from `fall` so that the tick loop only pays for
    /// collecting everybody's position on the tick it is going to use
    /// them -- one in forty, and none at all when the sky is clear.
    pub fn due(&mut self, dt: f32, raining: bool) -> bool {
        if !raining {
            // The clock does not run while it is dry, so the first
            // shower of a downpour is a shower and not a backlog.
            self.since = 0.0;
            return false;
        }
        self.since += dt;
        if self.since < RAIN_INTERVAL {
            return false;
        }
        // Not zeroed, for the same reason `Water::step` does not zero
        // its own: a server running a hair over its tick budget would
        // otherwise lose the remainder every time and drift slow.
        self.since = (self.since - RAIN_INTERVAL).min(RAIN_INTERVAL);
        true
    }

    /// One shower. Returns the cells it deepened.
    pub fn fall(&mut self, world: &dyn BlockWorld, around: &[(i32, i32, i32)]) -> Vec<Cell> {
        let mut wetted = Vec::new();
        let span = (RAIN_RADIUS * 2 + 1) as u32;
        for &(px, py, pz) in around {
            for _ in 0..DROPS_PER_PLAYER {
                let x = px + self.rng.below(span) as i32 - RAIN_RADIUS;
                let z = pz + self.rng.below(span) as i32 - RAIN_RADIUS;
                let Some(cell) = Self::where_the_drop_lands(world, x, py, z) else {
                    continue;
                };
                // Two cells in one shower can be the same cell, and one
                // of them would then be deciding from a depth that is
                // already stale. Cheap to rule out at four drops.
                if wetted.contains(&cell) {
                    continue;
                }
                let Some(block) = world.block(cell.0, cell.1, cell.2) else {
                    continue;
                };
                let here = fluid::depth(block);
                let added = fluid::rain_transfer(here);
                if added == 0 || !Self::standing(world, cell, here) {
                    continue;
                }
                world.set(cell.0, cell.1, cell.2, fluid::with_depth(here + added));
                wetted.push(cell);
            }
        }
        wetted
    }

    /// Where a drop falling down this column lands, if it lands in
    /// water.
    ///
    /// Walks down from the sky and stops at the first cell that is
    /// either water -- rain -- or something that blocks the sky -- a
    /// roof, and no rain. Air, tall grass and leaves are neither, so
    /// rain falls through a canopy and not through a plank floor, which
    /// is the same answer `climate::has_roof` gives a player standing
    /// there. Two rules for what a roof is would be a game that shelters
    /// a pond in a place it shelters nobody.
    fn where_the_drop_lands(world: &dyn BlockWorld, x: i32, from_y: i32, z: i32) -> Option<Cell> {
        let top = (from_y + RAIN_ABOVE).min(CHUNK_SIZE_Y as i32 - 1);
        let bottom = (from_y - RAIN_BELOW).max(0);
        for y in (bottom..=top).rev() {
            // An unloaded cell is not rained on. Guessing here would be
            // rain landing on terrain that has not been generated yet.
            let block = world.block(x, y, z)?;
            if is_liquid(block) {
                return Some((x, y, z));
            }
            if blocks_the_sky(block) {
                return None;
            }
        }
        None
    }

    /// Is this water standing rather than running?
    ///
    /// **The whole of what keeps a pond drainable**, and it is exactly
    /// the question `flow_one` asks: has this cell anywhere to send what
    /// it holds? If it has, the sky leaves it alone, so every cell of a
    /// pond that is emptying through a cut is out of the rain's reach
    /// until it has finished emptying. Six reads, on a cell the sky has
    /// already decided to rain on, a few times a second.
    ///
    /// An unloaded neighbour counts as somewhere the water might go, and
    /// therefore as a refusal. Rain is the one thing here that makes
    /// water rather than moving it, so where the rules are unsure it
    /// does nothing.
    fn standing(world: &dyn BlockWorld, (x, y, z): Cell, here: u8) -> bool {
        match world.block(x, y - 1, z) {
            None => return false,
            Some(under) => {
                if Water::holds_water(under) && fluid::fall_transfer(here, fluid::depth(under)) > 0 {
                    return false;
                }
            }
        }
        for (dx, dz) in SIDES {
            match world.block(x + dx, y, z + dz) {
                None => return false,
                Some(side) => {
                    if Water::holds_water(side)
                        && fluid::level_transfer(here, fluid::depth(side)) > 0
                    {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Winter closing a bay, and summer opening it again.
///
/// ## Why there is anything here at all
///
/// The generator lays ice where water never thaws (`worldgen::freezes`,
/// `season::water_never_thaws`), and that is all it can honestly lay: a
/// chunk is rebuilt from the seed whenever it is loaded, so anything it
/// decides is decided for the life of the world. Before this existed the
/// generator drew the line at the *yearly mean* instead, which froze every
/// pond in the north for ever while the player's own thermometer read
/// twenty degrees in July -- two temperatures for one place, which is the
/// bug this file's half of the fix answers.
///
/// The other half is the season, and a season needs a clock. So: the world
/// owns the permafrost and this owns the year. A bay at sixty degrees is
/// open water in July, freezes over in the winter, and opens again -- and a
/// player who wants the far shore in February walks and in July rows. That
/// is the decision the old permanent ice was not.
///
/// ## What freezes, and what does not
///
/// **The lid on standing water and nothing else**, in both directions:
///
/// * A cell of water full to the brim (`fluid::SOURCE_DEPTH`) with air over
///   it freezes. Full, because a trickle an eighth deep is a film running
///   over the ground rather than a surface to stand on, and a lid on one
///   would be a sheet of ice a player falls through into nothing.
/// * A cell of ice with air over it and something under it thaws, back into
///   a full cell of water.
///
/// The two are deliberately each other's inverse, which is what makes the
/// mechanic reversible: everything it can freeze it can thaw, and every
/// cell it thaws is one it would freeze again in November. Ice with a block
/// over it is never touched, which is also how a player keeps some: an ice
/// store with a roof on it is theirs, and an ice bridge over a lake is a
/// *winter* bridge. That is the mechanic rather than a hole in it.
///
/// Rejected: **thawing only ice with water under it.** It sounds safer --
/// it could never touch anything a player laid on dry ground -- and it is
/// not reversible: a pond one cell deep freezes (water, air over it) and
/// then has rock under its ice, so it would be frozen for the rest of the
/// world's life. A rule that can make a state it cannot unmake is the bug
/// this whole change is fixing, one cell smaller.
///
/// Rejected: **melting into air.** Ice would vanish rather than become
/// water, which is a block a player could have carried turning into
/// nothing, and it would quietly drain a lake by one layer a year.
///
/// ## What it costs
///
/// A pass every [`FROST_INTERVAL`] over [`COLUMNS_PER_PLAYER`] columns
/// picked at random within [`FROST_RADIUS`] of each player -- the shape
/// [`Rainfall`] uses and for its reasons: work proportional to where the
/// players are rather than to how much world has ever existed, and never
/// more than a fixed handful of reads a tick whatever the world is doing.
///
/// **Random columns rather than a sweep of the loaded ones.** A sweep is
/// the obvious way and it is the expensive one: it is proportional to the
/// render distance squared, it costs the same on the day nothing changes,
/// and it lands every cell of a bay in one tick -- a lake that turns white
/// in a single frame, which is not a season. Sampling covers a bay in
/// proportion to its size, so the ice closes over a few minutes of the
/// autumn and the player watches it happen.
///
/// **And the ice does not flicker at the shore.** The line the decision is
/// made on moves with `season::ambient_offset_c`, a cosine over the whole
/// year and nothing else -- no hour in it, no weather, no noise -- so a
/// column crosses it twice a year rather than twice a day. What could still
/// twitch is a column sitting exactly on it, which is why a cell has to be
/// [`THAW_MARGIN`] clear of the line before it opens: the freeze and the
/// thaw are a hair apart, and a column on the line stays whichever it
/// already was.
pub struct Frost {
    since: f32,
    rng: Rng,
    /// How far from a player the frost reaches, in blocks: `FROST_RADIUS`
    /// until the server says how far its players see (`reach_view`).
    radius: i32,
}

/// How long between frost passes, in seconds.
///
/// **Two seconds, the rain's interval**, and for the same reason: it is
/// often enough that the player sees the ice creep and rare enough that the
/// per-player cost is a rounding error. A season is forty-five real minutes
/// at the default clock, so this is about thirteen hundred passes a season
/// -- the ice has a whole autumn to close, not a tick.
const FROST_INTERVAL: f32 = 1.0;

/// How many columns each player's neighbourhood is offered a pass.
///
/// A hundred and twenty-eight, which with [`FROST_INTERVAL`] is the whole
/// of the pace. The square of [`FROST_RADIUS`] holds thirty-seven thousand
/// columns, so a bay anywhere in it is 29 per cent closed after a minute,
/// 87 per cent after five and all but shut after ten -- and a whole season
/// is forty-five. That is an autumn: the player sees the ice come, and the
/// bay is crossable well before the winter ends.
///
/// What it costs is the walk down each column, which stops at the first
/// thing that blocks the sky: about thirty-four cached block reads, so a
/// hundred and twenty-eight of them a second is four thousand reads -- a
/// third of what the flow simulation alone is budgeted
/// (`simulation::DEFAULT_TICK_BUDGET`, twenty times a second).
const COLUMNS_PER_PLAYER: usize = 128;

/// The most columns one player is offered a pass, whatever the reach: two
/// thousand and forty-eight, which covers twenty-four chunks of view at the
/// near pace. About seventy thousand cached reads a second -- more than the
/// frost used to spend and still a few milliseconds of a core in a second.
const MAX_COLUMNS_PER_PLAYER: usize = 2048;

/// How far a column's own freezing line is moved, on the generator's 0..1
/// climate scale: up to a sixtieth either way, about a degree.
///
/// **So that ice comes as a front with a ragged edge, and not as the
/// order the dice fell in.** Every column of a bay used to cross the line on
/// the same day, and what decided which froze first was only which the
/// random pass happened to reach -- white speckles appearing over open
/// water. A degree of the column's own, fixed by where it is, puts the
/// shallows and the coves a few days ahead of the open water and gives the
/// ice an edge that holds still from one pass to the next.
const COLUMN_JITTER: f32 = 1.0 / 60.0;

/// A column's fixed nudge to its freezing line, in `-COLUMN_JITTER ..
/// COLUMN_JITTER`.
fn column_jitter(x: i32, z: i32) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    ((h >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * COLUMN_JITTER
}

/// How far from a player ice forms, in blocks. Twice the rain's, because a
/// player sees much further than the rain they are standing in: a bay that
/// froze only within forty-eight blocks would have a visible white circle
/// round the player, moving with them.
const FROST_RADIUS: i32 = 96;

/// How far above and below a player a surface is looked for. The rain's
/// bounds, for the rain's reason.
const FROST_ABOVE: i32 = 32;
const FROST_BELOW: i32 = 32;

/// How far the wrong side of the line a cell has to be before ice opens
/// again, on the generator's 0..1 climate scale.
///
/// A hundredth, which is half a degree of the fifty-six the scale spans:
/// far enough that a column cannot be on both sides of the line at once,
/// small enough that the thaw is still the spring and not a week later.
const THAW_MARGIN: f32 = 0.01;

impl Default for Frost {
    fn default() -> Self {
        Self::new()
    }
}

impl Frost {
    pub fn new() -> Self {
        Self { since: 0.0, rng: Rng::from_clock(), radius: FROST_RADIUS }
    }

    /// Reaches as far as a player is sent chunks, `view_chunks` of them.
    ///
    /// **The square was visible** ("лёд появляется в радиусе квадрата от
    /// игрока со времени"): ninety-six blocks is six chunks, and a player
    /// sees twenty-four. Everything past the square stayed open water while
    /// the square filled in, so a frozen bay had a straight white edge a
    /// hundred blocks out that walked along with the player. The frost now
    /// covers every chunk a player is sent, and offers columns in proportion
    /// to the area (`columns_for`), so the far water freezes at the pace the
    /// near water does and there is no edge to see.
    pub fn reach_view(&mut self, view_chunks: i32) {
        self.radius = (view_chunks.max(1) * primitive_shared::types::CHUNK_SIZE_X as i32).max(FROST_RADIUS);
    }

    /// Columns a pass offers each player at this reach: `COLUMNS_PER_PLAYER`
    /// per `FROST_RADIUS` square of area, so the pace a column is visited at
    /// is the same at any reach -- capped, because a server told to send
    /// sixty-four chunks should not spend a core on ice.
    fn columns_for(radius: i32) -> usize {
        let scale = (f64::from(radius) / f64::from(FROST_RADIUS)).powi(2);
        ((COLUMNS_PER_PLAYER as f64 * scale) as usize).clamp(COLUMNS_PER_PLAYER, MAX_COLUMNS_PER_PLAYER)
    }

    /// The same, repeatable, for tests.
    pub fn seeded(seed: u64) -> Self {
        Self { since: 0.0, rng: Rng::seeded(seed), radius: FROST_RADIUS }
    }

    /// Has enough time passed for another pass?
    ///
    /// Separate from `pass` for `Rainfall::due`'s reason: the tick loop
    /// pays for collecting everybody's position only on the tick it is
    /// going to use them, which is one in forty.
    pub fn due(&mut self, dt: f32) -> bool {
        self.since += dt;
        if self.since < FROST_INTERVAL {
            return false;
        }
        // Not zeroed, so a server running a hair over its tick budget does
        // not lose the remainder every time and drift slow.
        self.since = (self.since - FROST_INTERVAL).min(FROST_INTERVAL);
        true
    }

    /// One pass. Returns the cells it froze or thawed.
    ///
    /// `climate` answers the generator's 0..1 warmth at a cell and
    /// `latitude` the degrees north of a row -- both handed in rather than
    /// read off a generator here, because this file knows nothing about
    /// worldgen and because the tests need a climate they can choose. They
    /// are the same two questions `climate::Ambient` asks of the world,
    /// which is what keeps the ice and the thermometer one answer.
    pub fn pass(
        &mut self,
        world: &dyn BlockWorld,
        around: &[(i32, i32, i32)],
        world_time: f32,
        climate: impl Fn(i32, i32, i32) -> f32,
        latitude: impl Fn(i32) -> Option<f32>,
    ) -> Vec<BlockChange> {
        let mut changed = Vec::new();
        let radius = self.radius;
        let span = (radius * 2 + 1) as u32;
        let columns = Self::columns_for(radius);
        for &(px, py, pz) in around {
            for _ in 0..columns {
                let x = px + self.rng.below(span) as i32 - radius;
                let z = pz + self.rng.below(span) as i32 - radius;
                let Some((cell, block)) = Self::surface(world, x, py, z) else {
                    continue;
                };
                let swing = season::seasonal_swing(latitude(z));
                let warmth = climate(cell.0, cell.1, cell.2) + column_jitter(cell.0, cell.2);
                let id = if block == BLOCK_ICE {
                    // **Minus the margin, and it was plus.** A cell thaws once
                    // it is the margin *warmer* than the line; with the plus,
                    // a cell a hair on the cold side froze on one pass and
                    // thawed on the next, for ever. Nothing sat in that band
                    // while every column shared one line -- the column's own
                    // nudge (`column_jitter`) filled it, and the shore strobed.
                    if season::water_freezes_with_swing(warmth - THAW_MARGIN, world_time, swing) {
                        continue; // still winter here
                    }
                    fluid::with_depth(fluid::SOURCE_DEPTH)
                } else {
                    if !season::water_freezes_with_swing(warmth, world_time, swing) {
                        continue; // still summer here
                    }
                    BLOCK_ICE
                };
                world.set(cell.0, cell.1, cell.2, id);
                changed.push(BlockChange {
                    global_x: cell.0,
                    global_y: cell.1,
                    global_z: cell.2,
                    block_id: id,
                });
            }
        }
        changed
    }

    /// The cell in this column the frost may act on: the first thing under
    /// the sky that is either a full cell of water or a cell of ice with
    /// something under it.
    ///
    /// Walks down from the sky exactly as a raindrop does
    /// (`Rainfall::where_the_drop_lands`), and stops at the first cell that
    /// blocks the sky -- so ice under a roof is never touched, which is how
    /// an ice store is kept, and a pond under a canopy still freezes.
    fn surface(world: &dyn BlockWorld, x: i32, from_y: i32, z: i32) -> Option<(Cell, BlockId)> {
        let top = (from_y + FROST_ABOVE).min(CHUNK_SIZE_Y as i32 - 1);
        let bottom = (from_y - FROST_BELOW).max(0);
        for y in (bottom..=top).rev() {
            // An unloaded cell is left alone: guessing here would be ice
            // forming on terrain nobody has generated.
            let block = world.block(x, y, z)?;
            if block == BLOCK_ICE {
                // Never a sheet hanging in the air: something has to hold
                // it, or a thaw drops a hole into whatever is below.
                return world
                    .block(x, y - 1, z)
                    .filter(|&under| under != BLOCK_AIR)
                    .map(|_| ((x, y, z), block));
            }
            if is_liquid(block) {
                return (fluid::depth(block) >= fluid::SOURCE_DEPTH).then_some(((x, y, z), block));
            }
            if blocks_the_sky(block) {
                return None;
            }
        }
        None
    }
}

/// Would a client draw these two cells differently?
///
/// Every depth is drawn at its own height (`fluid::surface_height`), so
/// any change of the block is a change on screen -- water arriving,
/// leaving, or settling by an eighth. This used to answer "only
/// arriving or leaving", when all depths drew alike; it is kept as a
/// function, next to the reasoning in `set`, because it is the one
/// place the filter would come back if that were ever true again.
#[inline]
fn visibly_different(before: Option<BlockId>, after: BlockId) -> bool {
    match before {
        // Nothing known about the cell -- say yes and let the client
        // sort it out when the chunk lands.
        None => true,
        Some(before) => before != after,
    }
}

impl CellMechanic for Water {
    fn name(&self) -> &'static str {
        "water"
    }

    fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.push((gx, gy, gz));
        self.wake((gx, gy, gz));
        // ...and the four cells beside the one *above* this, which is
        // the group `wake` leaves out on purpose. A block appearing or
        // going here changes whether their water still has a floor under
        // it -- and a cell that has lost its floor has somewhere to fall
        // that it had no way of hearing about. Only a real block edit
        // can do that; water never can, because water cannot put stone
        // anywhere. So it is paid for here and not on every eighth that
        // moves.
        //
        // This is the entry that dries a stream up when the player digs
        // the floor out from under it, and it has a test by that name.
        for (dx, dz) in SIDES {
            self.push((gx + dx, gy + 1, gz + dz));
        }
    }

    fn step(&mut self, world: &dyn BlockWorld, dt: f32, budget: usize) -> Vec<BlockChange> {
        self.since_step += dt;
        if self.since_step < FLOW_INTERVAL {
            return Vec::new();
        }
        // Not zeroed: a server running a hair over its tick budget would
        // otherwise lose the remainder every time and drift slow.
        // Clamped, because a server that has stalled for a second must
        // not then run five steps in five ticks to catch up -- the water
        // would visibly lurch.
        self.since_step = (self.since_step - FLOW_INTERVAL).min(FLOW_INTERVAL);

        let mut changes = Vec::new();
        for _ in 0..budget.min(self.queue.len()) {
            let Some(cell) = self.queue.pop_front() else {
                break;
            };
            self.queued.remove(&cell);
            self.flow_one(world, cell, &mut changes);
        }

        // ...and, far less often, the cells that stopped at the edge of
        // the loaded world.
        self.steps_since_retry += 1;
        if self.steps_since_retry >= RETRY_AFTER_STEPS {
            self.steps_since_retry = 0;
            for _ in 0..budget.min(self.stalled.len()) {
                let Some(cell) = self.stalled.pop_front() else {
                    break;
                };
                self.stalled_set.remove(&cell);
                self.retry_one(world, cell);
            }
        }

        // ...then a few sheets levelled, once the step's flowing is done,
        // so what they write is looked at by the next step's.
        //
        // **Only from a cell the flow has finished with.** A cell still on
        // the flow's queue is part of water that is still running, and the
        // neighbour rules are already levelling it faster than a pass over
        // the sheet would -- walking its sheet then is two hundred reads to move
        // what the next step moves anyway. Measured on the reference spill:
        // 58,229 reads and 3,293 writes levelling from every cell as it came
        // up, 48,576 and 2,706 with this rule. Such a
        // cell goes to the back of the list rather than off it, and the
        // look is bounded (`SHEET_LOOKS`) so a flood that keeps every cell
        // busy costs a few set lookups a step and nothing more.
        let (mut levelled, mut looked) = (0, 0);
        while levelled < self.sheets_per_step && looked < SHEET_LOOKS {
            let Some(cell) = self.unlevelled.pop_front() else {
                break;
            };
            looked += 1;
            // Already taken off by a sheet levelled from another of its
            // cells: nothing to do, and nothing spent.
            if !self.unlevelled_set.contains(&cell) {
                continue;
            }
            if self.queued.contains(&cell) {
                self.unlevelled.push_back(cell);
                continue;
            }
            self.unlevelled_set.remove(&cell);
            self.level_one(world, cell, &mut changes);
            levelled += 1;
        }

        // ...and the films whose time is up.
        if self.soaks {
            self.clock += FLOW_INTERVAL;
            let mut looked = 0;
            while looked < budget && self.thin.front().is_some_and(|&(_, due)| due <= self.clock) {
                let Some((cell, _)) = self.thin.pop_front() else {
                    break;
                };
                self.thin_set.remove(&cell);
                looked += 1;
                if self.queued.contains(&cell) {
                    // Still being worked on: it has not stood as a film for
                    // any time at all yet. Its time starts again.
                    self.thin_set.insert(cell);
                    self.thin.push_back((cell, self.clock + SOAK_SECONDS));
                } else if Self::soaks_away(world, cell) {
                    self.set(world, cell, 0, &mut changes);
                }
            }
        }
        changes
    }

    fn pending(&self) -> usize {
        Water::pending(self)
    }
}

// ---- a vessel knocked over ----
//
// **"сделай небольшое подтопление при уничтожении бочки с водой"**: a
// barrel of water broken used to lose its water with its staves, and a jug
// of water set down and knocked over came back full. Now what they held is
// on the floor, spreads a little, and goes.
//
// **Three ways the water could have gone, and why this one:**
//
// * *Nowhere* -- what it did. The one way a barrel of river water is
//   destroyed without anything happening, and the one thing a player
//   filling a barrel indoors is never made to think about.
// * *Water handed to the flow and left there.* The flow conserves water to
//   the eighth and levels only a difference of two (`fluid::level_transfer`),
//   so a spill spread thin is a film of single eighths that never moves
//   again -- a permanent puddle on a plank floor from one broken barrel, and
//   a house that floods a little more with every one.
// * *Written, flowed and then dried* (chosen). The spill is real water for
//   the flow to move -- down a step, into a hole, out of a doorway -- and
//   the place it was spilled remembers it for `SPILL_DRIES_SECONDS`, after
//   which whatever is still standing shallow round it soaks away. Nothing is
//   a source and nothing is refilled, so a spill is finite twice over: in
//   what it is written as, and in how long any of it is left.
//
// Only shallow water that is not part of anything deeper dries: a spill that
// ran into a pond is the pond's now, and a lake's shallow shore beside a
// broken barrel is not taken with it (`dries_away`).

/// Eighths of a cell a jug of water is when it is spilled.
///
/// **Two, which is not a jug's volume and is not meant to be.** An eighth is
/// the smallest water the world holds, and it is 125 litres of a metre cube;
/// a jug is a few. A spill scaled honestly would be nothing at all, which is
/// the complaint. Two eighths a jug makes a full barrel (`BARREL_JUGS`) a
/// floor of water round it -- the "small flooding" asked for -- and a jug a
/// wet patch the size of its cell.
pub const SPILL_EIGHTHS_PER_JUG: u32 = 2;

/// The most a single spill writes, however much it held: two cells' worth.
pub const SPILL_MAX_EIGHTHS: u32 = 2 * fluid::SOURCE_DEPTH as u32;

/// How far from the vessel's cell a spill is written, in steps along the
/// floor. Two: the cell, the ring round it and the ring round that.
pub const SPILL_REACH: i32 = 2;

/// The most a spill writes into any one cell, in eighths: an ankle of water,
/// which is a spill and not a pool.
const SPILL_CELL_EIGHTHS: u8 = 3;

/// How long a spill stands before what is left of it soaks away, in seconds.
///
/// A minute and a half: long enough to see the water run and to step in it,
/// short enough that nobody comes back to a flooded room.
pub const SPILL_DRIES_SECONDS: f32 = 90.0;

/// The deepest water a drying spill takes, in eighths. A spill is written at
/// most `SPILL_CELL_EIGHTHS` a cell, and water that has gathered deeper than
/// that has found a hollow and is a pond.
const DRIES_AT_OR_BELOW: u8 = SPILL_CELL_EIGHTHS;

/// Writes `eighths` of water out from `at` -- the cell a vessel stood in,
/// which is empty now -- across the floor, and returns the cells written.
///
/// Breadth first along cells that are empty air, never through a wall, and
/// only onto a floor: a cell over nothing is passed through, not written as
/// water hanging in the air. Each cell takes at most `SPILL_CELL_EIGHTHS`,
/// the nearest first, so a jug wets its own cell and a barrel reaches the
/// rings round it. What does not fit within `SPILL_REACH` is not written: a
/// spill has a size, and it is this.
pub fn spill(world: &dyn BlockWorld, at: Cell, eighths: u32) -> Vec<(Cell, BlockId)> {
    let mut left = eighths.min(SPILL_MAX_EIGHTHS);
    let mut written = Vec::new();
    let open = |(x, y, z): Cell| world.block(x, y, z).is_some_and(is_air);
    let floored = |(x, y, z): Cell| world.block(x, y - 1, z).is_some_and(|below| !can_be_displaced_by_falling(below));
    if left == 0 || !open(at) {
        return written;
    }
    let mut seen = HashSet::from([at]);
    let mut ring = vec![at];
    for step in 0..=SPILL_REACH {
        let mut next = Vec::new();
        for &cell in &ring {
            if left > 0 && floored(cell) {
                let depth = left.min(u32::from(SPILL_CELL_EIGHTHS)) as u8;
                let block = fluid::with_depth(depth);
                world.set(cell.0, cell.1, cell.2, block);
                written.push((cell, block));
                left -= u32::from(depth);
            }
            if step < SPILL_REACH {
                for (dx, dz) in SIDES {
                    let beside = (cell.0 + dx, cell.1, cell.2 + dz);
                    if open(beside) && seen.insert(beside) {
                        next.push(beside);
                    }
                }
            }
        }
        ring = next;
    }
    written
}

/// Should this cell round a spill soak away?
///
/// Shallow water with a floor under it, nothing on it, and nothing deeper
/// beside it: the thin end of a spill. Water on water is a column, and water
/// beside deeper water is the edge of a pond -- neither is a spill's to take.
fn dries_away(world: &dyn BlockWorld, (x, y, z): Cell) -> bool {
    let Some(block) = world.block(x, y, z) else {
        return false;
    };
    let here = fluid::depth(block);
    if here == 0 || here > DRIES_AT_OR_BELOW {
        return false;
    }
    if world.block(x, y - 1, z).is_none_or(is_liquid) || world.block(x, y + 1, z).is_none_or(is_liquid) {
        return false;
    }
    SIDES
        .iter()
        .all(|(dx, dz)| world.block(x + dx, y, z + dz).is_some_and(|beside| fluid::depth(beside) <= DRIES_AT_OR_BELOW))
}

/// The spills standing in the world, and how long each has left.
///
/// **Not saved.** A server stopped with a spill on the floor comes back with
/// water that will not dry; it is a few eighths, in the one place a barrel
/// was broken, and a save format for it would outlive every spill it kept.
#[derive(Default)]
pub struct Spills {
    standing: Vec<(Cell, f32)>,
}

/// The most spills timed at once. Past it the oldest is dried at the next
/// pass: a player breaking barrels in a row is not owed a queue without end.
const MAX_SPILLS: usize = 256;

impl Spills {
    pub fn new() -> Self {
        Self::default()
    }

    /// A spill has been written at `at`.
    pub fn spilled(&mut self, at: Cell) {
        if self.standing.len() >= MAX_SPILLS {
            self.standing[0].1 = 0.0;
        }
        self.standing.push((at, SPILL_DRIES_SECONDS));
    }

    pub fn is_empty(&self) -> bool {
        self.standing.is_empty()
    }

    /// Advances every spill by `dt` and dries the ones whose time is up,
    /// returning the cells emptied.
    ///
    /// The neighbourhood dried is the one written, two rings further for
    /// what ran, and the level under it for what poured down a step.
    pub fn dry(&mut self, world: &dyn BlockWorld, dt: f32) -> Vec<(Cell, BlockId)> {
        let mut dried = Vec::new();
        let reach = SPILL_REACH + 2;
        let mut i = 0;
        while i < self.standing.len() {
            self.standing[i].1 -= dt;
            if self.standing[i].1 > 0.0 {
                i += 1;
                continue;
            }
            let ((ax, ay, az), _) = self.standing.swap_remove(i);
            for y in ay - 1..=ay {
                for x in ax - reach..=ax + reach {
                    for z in az - reach..=az + reach {
                        if dries_away(world, (x, y, z)) {
                            world.set(x, y, z, BLOCK_AIR);
                            dried.push(((x, y, z), BLOCK_AIR));
                        }
                    }
                }
            }
        }
        dried
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use crate::logic::rng::Rng;
    use primitive_shared::types::{
        block_kind, BLOCK_AIR, BLOCK_STONE, BLOCK_TALL_GRASS, BLOCK_WATER,
    };
    use primitive_shared::season;

    const TICK: f32 = 1.0 / 20.0;

    // ---- the year, and the lid it puts on a bay ----

    /// Midsummer and midwinter as world times, which is what `Frost` is
    /// handed. Written out rather than computed at each use, because a
    /// season the test got wrong would look like a frost rule that did.
    fn midwinter() -> f32 {
        season::MIDWINTER_DAY_OF_YEAR - season::WORLD_OPENS_ON_DAY
    }

    fn midsummer() -> f32 {
        season::MIDSUMMER_WORLD_TIME
    }

    /// A cold temperate lake: a floor of stone, a pool of full water on it,
    /// open sky above. The climate is handed in whole, so what is under
    /// test is the season and never a seed.
    fn a_pool() -> TestWorld {
        let world = floored();
        for x in -4..=4 {
            for z in -4..=4 {
                world.put(x, 1, z, BLOCK_WATER);
            }
        }
        world
    }

    /// Runs `passes` of weather over the pool. Returns how many cells
    /// changed in all.
    ///
    /// **`SEASON` passes is the number a season is**, and it is not a round
    /// figure: the frost offers [`COLUMNS_PER_PLAYER`] columns of the
    /// thirty-seven thousand in its square each time, so the chance a
    /// *particular* cell has not been offered one yet falls as
    /// `exp(-passes * 128 / 37249)`. At 2700 that is one in ten thousand,
    /// which is what makes "the whole bay" an assertion rather than a
    /// gamble. In game time it is forty-five minutes -- one season.
    const SEASON: usize = 2_700;

    fn weather_on(frost: &mut Frost, world: &TestWorld, warmth: f32, time: f32, passes: usize) -> usize {
        let mut changed = 0;
        for _ in 0..passes {
            changed += frost
                .pass(world, &[(0, 1, 0)], time, |_, _, _| warmth, |_| Some(45.0))
                .len();
        }
        changed
    }

    fn ice_cells(world: &TestWorld) -> usize {
        let mut n = 0;
        for x in -4..=4 {
            for z in -4..=4 {
                n += usize::from(world.get(x, 1, z) == BLOCK_ICE);
            }
        }
        n
    }

    /// The whole mechanic in one sentence, and the answer to "на севере вся
    /// вода заледеневшая хотя температура 20 градусов": a bay closes for
    /// the winter and opens again, rather than being frozen for the life of
    /// the world.
    #[test]
    fn a_bay_freezes_over_in_winter_and_opens_again_in_summer() {
        // A cool northern column: a tenth of the climate scale over the
        // generator's freezing line, so it is open water at the equinox and
        // the winter reaches it. `season::CLIMATE_SPAN_C` is fifty-six
        // degrees, so a tenth is five and a half of them.
        let warmth = primitive_shared::worldgen::CLIMATE_FREEZING + 0.10;
        let world = a_pool();
        let mut frost = Frost::seeded(7);

        // High summer: nothing happens, over and over.
        assert_eq!(weather_on(&mut frost, &world, warmth, midsummer(), SEASON), 0);
        assert_eq!(ice_cells(&world), 0, "a lake froze in July");

        // Midwinter closes it.
        weather_on(&mut frost, &world, warmth, midwinter(), SEASON);
        assert_eq!(ice_cells(&world), 81, "the bay did not close: {} of 81 cells", ice_cells(&world));

        // ...and the summer opens it again, back into water rather than
        // into nothing: what a thaw leaves has to be what a freeze took.
        weather_on(&mut frost, &world, warmth, midsummer(), SEASON);
        assert_eq!(ice_cells(&world), 0, "the bay did not open again");
        for x in -4..=4 {
            for z in -4..=4 {
                assert_eq!(
                    fluid::depth(world.get(x, 1, z)),
                    fluid::SOURCE_DEPTH,
                    "the thaw at {x},{z} left {}",
                    world.get(x, 1, z)
                );
            }
        }
    }

    /// **It is a season, not a switch.** The player has to watch the ice
    /// close over an autumn; a bay that turns white in one tick is a
    /// texture change rather than a thing that happened. See `Frost`'s note
    /// on why the columns are sampled rather than swept.
    #[test]
    fn the_ice_closes_over_a_bay_gradually_rather_than_in_one_pass() {
        let warmth = primitive_shared::worldgen::CLIMATE_FREEZING + 0.10;
        let world = a_pool();
        let mut frost = Frost::seeded(11);
        // A tenth of a season -- four and a half minutes of play, and about
        // a game day and a half. Enough that the ice is plainly coming and
        // nowhere near enough that it is finished.
        weather_on(&mut frost, &world, warmth, midwinter(), SEASON / 10);
        let so_far = ice_cells(&world);
        assert!(so_far > 0, "a day and a half of midwinter froze nothing at all");
        assert!(so_far < 81, "the whole bay closed in a tenth of a season");
    }

    /// **The shore does not twitch.** The line ice is decided on moves with
    /// the season and nothing else, and a column sitting exactly on it is
    /// held where it is by `THAW_MARGIN`. Without the margin a cell on the
    /// line freezes and thaws on alternate passes, which at two seconds a
    /// pass is a shoreline that strobes.
    #[test]
    fn a_column_exactly_on_the_freezing_line_does_not_strobe() {
        let world = a_pool();
        let mut frost = Frost::seeded(3);
        // The day the shift is zero is the day the world opens, so the line
        // is the generator's own -- and this column is on it to the digit.
        let on_the_line = primitive_shared::worldgen::CLIMATE_FREEZING;
        let mut changes = 0;
        for _ in 0..SEASON {
            changes += frost
                .pass(&world, &[(0, 1, 0)], 0.0, |_, _, _| on_the_line, |_| Some(45.0))
                .len();
        }
        // Every cell may settle once -- the water is on the cold side of
        // the line, so it freezes -- and none of them may change twice.
        assert!(changes <= 81, "the shore changed {changes} times over 81 cells");
    }

    /// A tropical lagoon has a winter of about two degrees
    /// (`season::seasonal_swing`), and no amount of it puts a lid on the
    /// water. The same column at forty-five degrees freezes, which is what
    /// makes this a test of the latitude rather than of the threshold.
    #[test]
    fn a_tropical_lagoon_keeps_its_winter_to_itself() {
        let warmth = primitive_shared::worldgen::CLIMATE_FREEZING + 0.10;
        for (latitude, want_ice) in [(8.0f32, false), (45.0, true)] {
            let world = a_pool();
            let mut frost = Frost::seeded(5);
            for _ in 0..SEASON {
                frost.pass(&world, &[(0, 1, 0)], midwinter(), |_, _, _| warmth, |_| Some(latitude));
            }
            assert_eq!(
                ice_cells(&world) > 0,
                want_ice,
                "at {latitude} degrees the midwinter pool has {} frozen cells",
                ice_cells(&world)
            );
        }
    }

    /// **An ice store is the player's.** Ice with a block over it is never
    /// looked at, which is how somebody keeps a block they carried there --
    /// and it is the same rule that lets a pond freeze under a canopy,
    /// since leaves do not block the sky (`blocks_the_sky`).
    #[test]
    fn the_frost_reaches_as_far_as_a_player_sees_at_the_same_pace_and_a_columns_line_is_its_own() {
        let mut frost = Frost::seeded(1);
        frost.reach_view(24);
        assert_eq!(frost.radius, 24 * 16, "the frost stops short of the chunks a player is sent");
        // The pace a column is visited at does not fall with the reach...
        let near = COLUMNS_PER_PLAYER as f64 / f64::from((FROST_RADIUS * 2 + 1).pow(2));
        let far = Frost::columns_for(frost.radius) as f64 / f64::from((frost.radius * 2 + 1).pow(2));
        assert!(far > near * 0.9, "far water is visited {far} per column a pass against {near} near");
        // ...and a reach nobody asked for is not paid for.
        frost.reach_view(1);
        assert_eq!(frost.radius, FROST_RADIUS);
        // A column's line is fixed by where it is, and small.
        assert_eq!(column_jitter(12, -40), column_jitter(12, -40));
        let spread: Vec<f32> = (0..200).map(|i| column_jitter(i, i * 7)).collect();
        assert!(spread.iter().all(|j| j.abs() <= COLUMN_JITTER));
        assert!(spread.iter().any(|&j| j > 0.0) && spread.iter().any(|&j| j < 0.0), "every column's line moved the same way");
    }

    #[test]
    fn ice_under_a_roof_survives_the_summer_and_ice_under_leaves_is_weather() {
        let warmth = primitive_shared::worldgen::CLIMATE_FREEZING + 0.10;
        let world = floored();
        // A block of ice on the ground with a plank of stone over it.
        world.put(0, 1, 0, BLOCK_ICE);
        world.put(0, 2, 0, BLOCK_STONE);
        // ...and one under tall grass, which the sky reaches through.
        world.put(3, 1, 0, BLOCK_ICE);
        world.put(3, 2, 0, BLOCK_TALL_GRASS);
        let mut frost = Frost::seeded(13);
        for _ in 0..SEASON {
            frost.pass(&world, &[(0, 1, 0)], midsummer(), |_, _, _| warmth, |_| Some(45.0));
        }
        assert_eq!(world.get(0, 1, 0), BLOCK_ICE, "a roofed store melted");
        assert_eq!(
            fluid::depth(world.get(3, 1, 0)),
            fluid::SOURCE_DEPTH,
            "ice under grass did not thaw: {}",
            world.get(3, 1, 0)
        );
    }

    /// A film of water an eighth deep is not a surface: a lid on one would
    /// be ice a player falls through into the mud under it. Only a cell
    /// full to the brim freezes.
    #[test]
    fn a_puddle_an_eighth_deep_grows_no_ice() {
        let world = floored();
        world.put(0, 1, 0, flowing(1));
        world.put(2, 1, 0, BLOCK_WATER);
        let mut frost = Frost::seeded(17);
        for _ in 0..SEASON {
            frost.pass(
                &world,
                &[(0, 1, 0)],
                midwinter(),
                |_, _, _| primitive_shared::worldgen::CLIMATE_FREEZING + 0.10,
                |_| Some(45.0),
            );
        }
        assert_eq!(world.get(0, 1, 0), flowing(1), "a film of water grew a lid");
        assert_eq!(world.get(2, 1, 0), BLOCK_ICE, "the full cell beside it did not freeze");
    }

    /// Unloaded terrain is left alone, on the rule every mechanic in this
    /// file keeps: where it cannot see, it does nothing.
    #[test]
    fn the_frost_leaves_a_chunk_that_has_not_arrived_alone() {
        let world = a_pool();
        for x in -4..=4 {
            for z in -4..=4 {
                world.missing_cell(x, 1, z);
            }
        }
        let mut frost = Frost::seeded(19);
        let changed = weather_on(
            &mut frost,
            &world,
            primitive_shared::worldgen::CLIMATE_FREEZING + 0.10,
            midwinter(),
            SEASON / 10,
        );
        assert_eq!(changed, 0, "the frost worked on terrain nobody had loaded");
    }

    /// The clock, which is what keeps a pass off thirty-nine ticks in
    /// forty. `Rainfall::due`'s property, asked of the frost because the
    /// frost has no sky to be switched off by.
    #[test]
    fn a_frost_pass_is_due_on_its_own_interval_and_not_every_tick() {
        let mut frost = Frost::seeded(23);
        let mut due = 0;
        for _ in 0..(20 * 10) {
            due += usize::from(frost.due(TICK));
        }
        // Ten seconds at a one-second interval. Nine or ten rather than
        // exactly ten: the remainder is carried rather than dropped, so the
        // last pass can land a tick either side of the boundary.
        assert!((9..=10).contains(&due), "ten seconds gave {due} passes at a one-second interval");
    }

    /// A source, and the thin end of a spill, spelled out so the
    /// fixtures read as what they mean.
    fn flowing(depth: u8) -> BlockId {
        fluid::with_depth(depth)
    }

    /// Ticks until the water stops moving, or until `limit` runs out.
    fn settle(sim: &mut Water, world: &TestWorld, limit: usize) -> Vec<BlockChange> {
        let mut all = Vec::new();
        for _ in 0..limit {
            if sim.pending() == 0 {
                break;
            }
            all.extend(sim.step(world, TICK, 4096));
        }
        all
    }

    /// How far the fixtures reach. Comfortably further than the water in
    /// any of them can spread, so a test that says "it all ended up
    /// somewhere" has looked everywhere it could have gone.
    const REACH: i32 = 16;

    /// A floor of stone at y = 0 across the whole area a test uses.
    fn floored() -> TestWorld {
        let world = TestWorld::default();
        for x in -REACH..=REACH {
            for z in -REACH..=REACH {
                world.put(x, 0, z, BLOCK_STONE);
            }
        }
        world
    }

    /// Every eighth of water in the world, wherever it is.
    fn total_water(world: &TestWorld) -> u32 {
        let mut total = 0;
        for x in -REACH..=REACH {
            for z in -REACH..=REACH {
                for y in 0..12 {
                    total += fluid::depth(world.get(x, y, z)) as u32;
                }
            }
        }
        total
    }

    /// Pours full cells into a box and tells the simulation about them.
    ///
    /// **The fixture the finite model needs and the old one did not.**
    /// A single cell used to be an endless spring, so a test that wanted
    /// a supply wrote one cell and got a river. Now a cell is eight
    /// eighths -- a cupful -- and a test about what a *stream* does has
    /// to pour a stream. That is how the world does it too: what feeds a
    /// river is the rest of the river.
    fn pour(sim: &mut Water, world: &TestWorld, xs: [i32; 2], y: i32, zs: [i32; 2]) {
        for x in xs[0]..=xs[1] {
            for z in zs[0]..=zs[1] {
                world.put(x, y, z, BLOCK_WATER);
                sim.on_block_changed(x, y, z);
            }
        }
    }

    /// Every pair of neighbours in a settled body differs by at most one
    /// eighth, which is as level as whole units can be.
    fn is_level(world: &TestWorld, y: i32, reach: i32) {
        for x in -reach..=reach {
            for z in -reach..=reach {
                let here = fluid::depth(world.get(x, y, z));
                for (dx, dz) in SIDES {
                    let side = fluid::depth(world.get(x + dx, y, z + dz));
                    assert!(
                        here <= side + 1,
                        "({x}, {z}) is {here} deep beside {side} and stopped anyway",
                    );
                }
            }
        }
    }

    #[test]
    fn a_cupful_of_water_spreads_out_and_is_still_a_cupful() {
        // **The number the whole model is built around, and it is no
        // longer seven.** The old rule spent an eighth of depth per
        // block of travel, so one source made a diamond seven blocks
        // across -- out of nothing, and for ever. Here a cell holds
        // eight eighths and that is all there is: it spreads until it
        // is level, and then it stops, and the sum is what was poured.
        //
        // What replaces "seven blocks" as the number a player can hold
        // in their head is "as much as you poured".
        let world = floored();
        world.put(0, 1, 0, BLOCK_WATER);
        let before = total_water(&world);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 4_000);

        assert_eq!(
            total_water(&world),
            before,
            "the puddle is not the water that made it"
        );
        let wet = (-8..=8i32)
            .flat_map(|x| (-8..=8i32).map(move |z| (x, z)))
            .filter(|(x, z)| fluid::depth(world.get(*x, 1, *z)) > 0)
            .count();
        assert!(wet > 1, "the water never left the cell it was poured into");
        is_level(&world, 1, 9);
        assert_eq!(sim.pending(), 0, "the queue never drained");
    }

    #[test]
    fn water_fills_the_bite_a_digger_opened_and_takes_the_rest_of_the_block() {
        // **The player asked for water to flow into the space a dig
        // opens.** A cell cannot hold three quarters of a stone block and
        // the water standing in the quarter that is gone -- one cell is one
        // id -- so what a bite means to a lake is that the block has
        // stopped being watertight. The remainder is washed out and the
        // water takes the cell.
        //
        // And a *whole* block beside the same pool holds it back exactly as
        // it always did, which is the half of this that says nothing a
        // player built leaks.
        use primitive_shared::dig;
        let world = floored();
        let bitten = dig::next_bite(BLOCK_STONE, dig::Side::PosX).unwrap();
        world.put(1, 1, 0, bitten);
        world.put(-1, 1, 0, BLOCK_STONE);
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 4_000);

        assert_eq!(
            world.get(-1, 1, 0),
            BLOCK_STONE,
            "a whole block beside the pool was washed away"
        );
        assert!(
            !dig::is_dug(world.get(1, 1, 0)),
            "the bitten block held the water out: {:#x}",
            world.get(1, 1, 0)
        );
        assert!(
            fluid::depth(world.get(1, 1, 0)) > 0,
            "the water never reached the cell the bite opened"
        );
    }

    #[test]
    fn water_runs_over_a_turf_lip_and_leaves_it_where_it_was() {
        // The generator's lip on a slope (`dig::is_turf_lip`) is a bite by
        // shape and turf by kind, and the roots are what decide it: a pool
        // beside one and a spill falling onto one leave it standing, where a
        // cut bite in the same places is washed out (above).
        use primitive_shared::dig;
        use primitive_shared::types::BLOCK_GRASS;
        let world = floored();
        let lip = dig::lowered(BLOCK_GRASS, 2);
        world.put(1, 1, 0, lip);
        world.put(0, 1, 1, lip);
        world.put(0, 0, 0, lip);
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 4_000);

        for cell in [(1, 1, 0), (0, 1, 1), (0, 0, 0)] {
            assert_eq!(world.get(cell.0, cell.1, cell.2), lip, "the water washed out the lip at {cell:?}");
        }
    }

    #[test]
    fn water_over_snow_on_a_turf_lip_leaves_the_lip_where_it_was() {
        // Snow on a lip lies in the cell over it (`types::rest_drop`), where
        // the water runs; the lip under the snow is the same turf the water
        // ran over bare, and the roots still hold it.
        use primitive_shared::dig;
        use primitive_shared::types::{BLOCK_GRASS, BLOCK_SNOW_COVER};
        let world = floored();
        let lip = dig::lowered(BLOCK_GRASS, 1);
        for x in 1..=3 {
            world.put(x, 1, 0, lip);
            world.put(x, 2, 0, BLOCK_SNOW_COVER);
        }
        world.put(0, 2, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 2, 0);
        settle(&mut sim, &world, 4_000);

        for x in 1..=3 {
            assert_eq!(world.get(x, 1, 0), lip, "the water washed out the snowy lip at x {x}");
        }
    }

    #[test]
    fn a_pool_is_used_up_by_what_runs_out_of_it() {
        // **The half of Minecraft's model this one refuses, and the
        // whole reason it was rewritten.** There a source is endless:
        // a pond cannot be drained, and a channel cut from a lake runs
        // for ever out of nothing. It is a rule a player holds in their
        // head after seeing it once, and what it costs is conservation.
        //
        // Here a full cell is full, not eternal. Let it spread and it is
        // no longer full, because the water is somewhere else -- and
        // that "somewhere else" is the whole of it.
        let world = floored();
        world.put(0, 1, 0, BLOCK_WATER);
        let before = total_water(&world);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 4_000);

        assert!(
            fluid::depth(world.get(0, 1, 0)) < fluid::SOURCE_DEPTH,
            "the cell stayed full while water ran out of it, which is water from nothing",
        );
        assert_eq!(total_water(&world), before, "and none of it went missing either");
    }

    #[test]
    fn water_falls_before_it_spreads() {
        // The one rule, through the world rather than through the pure
        // functions: water that reaches a hole goes down it and does not
        // carry on past it, and what falls is a curtain rather than a
        // cone.
        //
        // A shelf with a slot right across it -- a hole one block wide
        // is walked *round* rather than over, which is right and is a
        // different test -- and a pool standing on the shelf a couple of
        // blocks back from the slot.
        let world = floored();
        for x in -3..=3 {
            for z in -3..=3 {
                world.put(x, 5, z, BLOCK_STONE);
            }
        }
        for z in -3..=3 {
            world.put(2, 5, z, BLOCK_AIR);
        }

        let mut sim = Water::new();
        pour(&mut sim, &world, [-1, 0], 6, [-1, 1]);
        let before = total_water(&world);
        settle(&mut sim, &world, 4_000);

        assert_eq!(total_water(&world), before, "the shelf ate some of it");
        assert!(
            fluid::depth(world.get(2, 1, 0)) > 0,
            "it never reached the floor"
        );
        // Nothing stands over a hole: the cells above the slot spend
        // themselves downward before they spread, so the far half of the
        // shelf stays dry.
        assert_eq!(
            fluid::depth(world.get(2, 6, 0)),
            0,
            "water puddled at the lip of the fall"
        );
        assert_eq!(
            fluid::depth(world.get(3, 6, 0)),
            0,
            "it walked straight over the slot"
        );
        // ...and what falls is a curtain one block thick.
        for y in 2..=4 {
            for x in [1, 3] {
                assert_eq!(
                    fluid::depth(world.get(x, y, 0)),
                    0,
                    "the falling water wet the air beside it at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn water_does_not_get_tired_on_the_way_down() {
        // **However far it falls, all of it arrives.** The old model
        // measured a spill in blocks of reach and had to be told
        // separately that a fall costs none of them. Here the statement
        // is simpler and stronger, because an amount is the thing being
        // tracked: what leaves the top is what lands, from two storeys
        // or from nine.
        let mut landed = Vec::new();
        for height in [2i32, 9] {
            // A stone chimney, open at the bottom: the water can only go
            // down, so what reaches the floor is a fall and nothing else.
            let world = floored();
            for y in 2..=height {
                for (dx, dz) in SIDES {
                    world.put(dx, y, dz, BLOCK_STONE);
                }
            }

            let mut sim = Water::new();
            pour(&mut sim, &world, [0, 0], height, [0, 0]);
            let before = total_water(&world);
            settle(&mut sim, &world, 8_000);

            assert_eq!(
                total_water(&world),
                before,
                "a fall from {height} lost water on the way"
            );
            assert!(
                fluid::depth(world.get(0, 1, 0)) > 0,
                "a fall from {height} never reached the floor"
            );
            landed.push(total_water(&world));
        }
        assert_eq!(
            landed[0], landed[1],
            "the height of the fall changed how much water there was"
        );
    }

    #[test]
    fn taking_the_water_away_takes_away_only_the_water_you_took() {
        // **The old model's cleanest property, and the one that had to
        // go.** There, water was a function of its sources, so removing
        // the source removed the spill: every cell in turn recomputed to
        // nothing. It is a lovely rule, and it is exactly why a lake
        // could not be drained -- the same arithmetic that tidies a
        // spill away refills one.
        //
        // Here water that has left is gone from where it was and present
        // where it went, so breaking the cell it came from costs that
        // cell's worth and not a drop more. What is on the floor stays
        // on the floor.
        let world = floored();
        world.put(0, 1, 0, BLOCK_WATER);
        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 4_000);

        let spread = total_water(&world);
        let took = fluid::depth(world.get(0, 1, 0)) as u32;
        assert!(spread > 0, "there was no spill to take from");

        world.put(0, 1, 0, BLOCK_AIR);
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 4_000);

        assert_eq!(
            total_water(&world),
            spread - took,
            "breaking one cell took more than that cell held"
        );
        assert_eq!(sim.pending(), 0, "the queue never drained");
    }

    #[test]
    fn digging_the_floor_out_from_under_a_stream_empties_it_down_the_hole() {
        // **The one entry in `wake` that only a block edit can reach**,
        // and therefore the one that is easy to leave out and hard to
        // notice missing. A cell stops holding water up when the floor
        // under *it* is taken away -- so the cells that have to be
        // re-examined are the ones beside the cell *above* the block
        // that went, and nothing water does by itself can ever put them
        // on the queue.
        //
        // Left out, this is a stream running placidly over a hole it is
        // visibly standing on, until something else nearby is disturbed.
        let world = floored();
        for x in -2..=10 {
            for z in -2..=2 {
                world.put(x, -1, z, BLOCK_STONE);
            }
        }
        for x in -1..=9i32 {
            for z in [-1, 1] {
                world.put(x, 1, z, BLOCK_STONE);
            }
        }
        world.put(-1, 1, 0, BLOCK_STONE);
        world.put(9, 1, 0, BLOCK_STONE);

        let mut sim = Water::new();
        pour(&mut sim, &world, [0, 3], 1, [0, 0]);
        let before = total_water(&world);
        settle(&mut sim, &world, 4_000);
        let along: u32 = (0..=8)
            .map(|x| fluid::depth(world.get(x, 1, 0)) as u32)
            .sum();
        assert!(along > 24, "the channel never filled: it holds {along}");

        world.put(4, 0, 0, BLOCK_AIR);
        sim.on_block_changed(4, 0, 0);
        settle(&mut sim, &world, 4_000);

        assert_eq!(
            fluid::depth(world.get(4, 0, 0)),
            fluid::SOURCE_DEPTH,
            "nothing went down the hole"
        );
        let left: u32 = (0..=8)
            .map(|x| fluid::depth(world.get(x, 1, 0)) as u32)
            .sum();
        assert_eq!(
            left,
            along - fluid::SOURCE_DEPTH as u32,
            "the stream ran on over the hole without noticing it"
        );
        assert_eq!(total_water(&world), before, "the hole ate water rather than held it");
    }

    #[test]
    fn water_never_climbs() {
        // A source in a pit wets the pit and not the rim. Nothing reads
        // downward for water, which is why `wake` does not queue the
        // cell above the one that changed.
        let world = floored();
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                if x.abs() == 2 || z.abs() == 2 {
                    for y in 1..=3 {
                        world.put(x, y, z, BLOCK_STONE);
                    }
                }
            }
        }
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 4_000);

        for x in -1..=1 {
            for z in -1..=1 {
                assert_eq!(
                    fluid::depth(world.get(x, 2, z)),
                    0,
                    "water stood a storey above its source at ({x}, {z})"
                );
            }
        }
    }

    #[test]
    fn a_hole_dug_in_a_lake_bed_fills_from_the_lake_and_the_lake_pays_for_it() {
        // **The complaint this model exists to answer, the other way
        // round from the last one.** A hole dug under a lake fills --
        // and the water that fills it comes out of the lake, because
        // there is nowhere else for water to come from any more. Under
        // the old rule the hole filled from a field of eternal sources
        // and the lake was untouched, which is the same arithmetic that
        // made a pond impossible to drain.
        //
        // A lake this size does not visibly care: three cells' worth out
        // of a hundred and twenty-one is a level nobody can see move.
        // That is the point -- conservation costs the look of a lake
        // nothing, and buys the draining of one.
        let world = floored();
        const EDGE: i32 = 6;
        // A floor under the bed as well as the bed: what goes down the
        // hole has to land somewhere the sum can still see it.
        for x in -EDGE..=EDGE {
            for z in -EDGE..=EDGE {
                world.put(x, -1, z, BLOCK_STONE);
            }
        }
        for x in -EDGE..=EDGE {
            for z in -EDGE..=EDGE {
                let wall = x.abs() == EDGE || z.abs() == EDGE;
                for y in 1..=3 {
                    world.put(x, y, z, if wall { BLOCK_STONE } else { BLOCK_WATER });
                }
            }
        }
        let before = total_water(&world);

        let mut sim = Water::new();
        for x in 2..=4 {
            world.put(x, 0, 0, BLOCK_AIR);
            sim.on_block_changed(x, 0, 0);
        }
        settle(&mut sim, &world, 20_000);

        for x in 2..=4 {
            assert!(
                fluid::depth(world.get(x, 0, 0)) > 0,
                "the hole at {x} never filled"
            );
        }
        assert_eq!(
            total_water(&world),
            before,
            "the hole filled with water that did not come from the lake"
        );
        // ...and the lake is still a lake: nothing has drained out of
        // it, only down into the bed it stands on.
        let mut dry = 0;
        for x in -EDGE + 1..EDGE {
            for z in -EDGE + 1..EDGE {
                if fluid::depth(world.get(x, 1, z)) == 0 {
                    dry += 1;
                }
            }
        }
        assert_eq!(dry, 0, "{dry} cells of the lake bed came out dry");
    }

    #[test]
    fn a_flat_lake_never_moves_and_never_reports_a_change() {
        // Still water is free. Every cell of a sea is a source, a source
        // computes to itself, and a cell that computes to itself writes
        // nothing and wakes nobody -- so an undisturbed ocean costs no
        // packets, no remeshes and no rows in the edit overlay.
        let world = floored();
        for x in -3i32..=3 {
            for z in -3i32..=3 {
                let wall = x.abs() == 3 || z.abs() == 3;
                world.put(x, 1, z, if wall { BLOCK_STONE } else { BLOCK_WATER });
            }
        }

        let mut sim = Water::new();
        for x in -2..=2 {
            for z in -2..=2 {
                sim.on_block_changed(x, 1, z);
            }
        }
        let changes = settle(&mut sim, &world, 2_000);

        assert!(changes.is_empty(), "a flat lake moved {} times", changes.len());
    }

    #[test]
    fn nothing_is_written_to_the_world_once_the_water_has_settled() {
        // **The autosave bug, as a property.** Every write goes into the
        // world's edit overlay, so a simulation that keeps writing after
        // it has stopped changing anything makes the autosave rewrite
        // `edits.bin` every interval over a map nobody is touching. It
        // has happened three times, always as two cells trading the same
        // eighth; there is nothing left to trade now, and this is the
        // test that says so.
        let world = floored();
        world.put(0, 1, 0, BLOCK_WATER);
        world.put(4, 0, 0, BLOCK_AIR); // a hole, so there is a fall in it too
        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        sim.on_block_changed(4, 0, 0);
        settle(&mut sim, &world, 8_000);
        assert_eq!(sim.pending(), 0, "it never came to rest");

        // Now offer it every cell it could possibly care about, and it
        // must decline all of them.
        let counted = Counted::new(&world);
        for x in -9..=9 {
            for z in -9..=9 {
                for y in 0..4 {
                    sim.push((x, y, z));
                }
            }
        }
        let mut changes = Vec::new();
        for _ in 0..4_000 {
            changes.extend(sim.step(&counted, TICK, 4096));
            if sim.pending() == 0 {
                break;
            }
        }
        assert_eq!(sim.pending(), 0, "a full sweep did not drain");
        assert_eq!(counted.writes.get(), 0, "settled water wrote to the world");
        assert!(changes.is_empty(), "settled water broadcast a change");
    }

    #[test]
    fn a_waterfall_leaves_nothing_hanging_in_the_air_behind_it() {
        // **What the old model had to be told and this one cannot get
        // wrong.** There, the cells of a fall were recomputed from their
        // neighbours every pass, and two of them disagreeing by an
        // eighth was a column that flickered between depths for ever --
        // a shimmer on screen, a packet per client per tick, and an
        // autosave rewriting the world at every interval.
        //
        // Here water in the air has somewhere to be and goes there. When
        // it has all come down, the column is empty, the sum is what was
        // poured, and nothing writes anything again.
        let world = floored();
        let mut sim = Water::new();
        pour(&mut sim, &world, [0, 0], 8, [0, 0]);
        let before = total_water(&world);
        settle(&mut sim, &world, 4_000);

        assert_eq!(total_water(&world), before, "the fall lost water");
        for y in 2..=8 {
            assert_eq!(
                fluid::depth(world.get(0, y, 0)),
                0,
                "water was left standing in the air at y = {y}"
            );
        }
        assert!(
            fluid::depth(world.get(0, 1, 0)) > 0,
            "it never landed at all"
        );
        // ...and it is finished, not merely quiet for a moment.
        assert_eq!(sim.pending(), 0, "the queue never drained");
        let after = settle(&mut sim, &world, 200);
        assert!(after.is_empty(), "the settled fall was still writing: {after:?}");
    }

    /// A lake on a shelf with a notch cut through its lip, falling eight
    /// blocks into a walled lowland -- `water_repro::what_running_water_looks_like`
    /// at a size the tests can afford, and the fixture the ladder of slabs
    /// was photographed on.
    ///
    /// The shelf is stone over x in -12..=-1 to y = 8; the lake stands two
    /// deep (y = 7, 8) over x in -10..=-3, z in -3..=3; the notch is
    /// x in -2..=-1 at z = 0; the fall is the column x = 0, z = 0; the
    /// lowland round its foot is walled three high so every eighth stays
    /// where `total_water` counts it. Returns the notch cells, so a test
    /// can close it again.
    fn a_lake_cut_over_a_cliff(sim: &mut Water) -> (TestWorld, Vec<Cell>) {
        let world = floored();
        for x in -12..=-1 {
            for z in -6..=6 {
                for y in 1..=8 {
                    let lake = (-10..=-3).contains(&x) && (-3..=3).contains(&z) && y >= 7;
                    world.put(x, y, z, if lake { BLOCK_WATER } else { BLOCK_STONE });
                }
            }
        }
        for x in 0..=12 {
            for z in -7i32..=7 {
                if x == 12 || z.abs() == 7 {
                    for y in 1..=3 {
                        world.put(x, y, z, BLOCK_STONE);
                    }
                }
            }
        }
        let mut notch = Vec::new();
        for x in -2..=-1 {
            for y in 7..=8 {
                world.put(x, y, 0, BLOCK_AIR);
                sim.on_block_changed(x, y, 0);
                notch.push((x, y, 0));
            }
        }
        (world, notch)
    }

    /// The cells of the fall's column, from the lowland floor up to the
    /// lip, that are dry with water both somewhere above them and
    /// somewhere below them. Each one is a gap a player sees as air
    /// between two slabs.
    fn gaps_in_the_fall(world: &TestWorld) -> Vec<i32> {
        let wet: Vec<bool> = (1..=8).map(|y| fluid::depth(world.get(0, y, 0)) > 0).collect();
        (0..wet.len())
            .filter(|&i| !wet[i] && wet[..i].iter().any(|w| *w) && wet[i + 1..].iter().any(|w| *w))
            .map(|i| i as i32 + 1)
            .collect()
    }

    #[test]
    fn a_fall_from_a_cut_lake_is_one_unbroken_column_while_it_runs() {
        // **The ladder of slabs.** A lake cut through its lip used to fall
        // as drops in every other cell -- `w3, air, w3, air` -- because the
        // lip handed its water to the cell below one step and heard that
        // the cell had room again only the step after. Every depth is drawn
        // at its own height, so that data was a stack of thin plates
        // hanging in the air with nothing between them, and the player
        // called the water broken. See `fluid::fall_keeping`.
        let mut sim = Water::new();
        let (world, _) = a_lake_cut_over_a_cliff(&mut sim);
        let before = total_water(&world);
        let counted = Counted::new(&world);
        let (mut running, mut steps, mut broken) = (0, 0, Vec::new());
        while sim.pending() > 0 && steps < 4_000 {
            sim.step(&counted, 1.0, 4096);
            steps += 1;
            assert_eq!(total_water(&world), before, "step {steps} made or lost water");
            let gaps = gaps_in_the_fall(&world);
            if !gaps.is_empty() {
                broken.push((steps, gaps));
            }
            if (2..=6).all(|y| fluid::depth(world.get(0, y, 0)) > 0) {
                running += 1;
            }
        }
        println!(
            "FALL: steps={steps} reads={} writes={} whole={running} broken={}",
            counted.reads.get(),
            counted.writes.get(),
            broken.len()
        );
        assert_eq!(sim.pending(), 0, "the cut lake never came to rest");
        assert!(broken.is_empty(), "the fall had air in it: (step, heights) {:?}", &broken[..broken.len().min(8)]);
        // ...and it did run: a column that is never there is never broken.
        assert!(running >= 20, "the fall was a whole column for only {running} steps");
    }

    #[test]
    fn a_fall_whose_lake_is_walled_off_again_runs_down_and_is_gone() {
        // The other half of keeping a film in a falling cell: it is kept
        // *while something is feeding it*, and not a step longer. Close the
        // notch and the column has to empty itself into the lowland --
        // a fall left standing in the air over a lake that no longer feeds
        // it is the bug this rule would be if "fed" were read loosely.
        let mut sim = Water::new();
        let (world, notch) = a_lake_cut_over_a_cliff(&mut sim);
        for _ in 0..40 {
            sim.step(&world, 1.0, 4096);
        }
        assert!(
            (2..=6).all(|y| fluid::depth(world.get(0, y, 0)) > 0),
            "the fall was not running when the notch was closed"
        );
        for &(x, y, z) in &notch {
            world.put(x, y, z, BLOCK_STONE);
            sim.on_block_changed(x, y, z);
        }
        // Counted after the stone went in: what stood in the notch was
        // replaced along with the air, and that is the player's doing.
        let before = total_water(&world);
        for _ in 0..4_000 {
            if sim.pending() == 0 {
                break;
            }
            sim.step(&world, 1.0, 4096);
        }
        assert_eq!(sim.pending(), 0, "the closed fall never came to rest");
        assert_eq!(total_water(&world), before, "closing the notch made or lost water");
        for x in 0..=11 {
            for z in -6..=6 {
                for y in 2..=9 {
                    assert_eq!(
                        fluid::depth(world.get(x, y, z)),
                        0,
                        "water still stands in the air at ({x}, {y}, {z}) after the notch was closed"
                    );
                }
            }
        }
    }

    #[test]
    fn water_does_not_flow_into_solid_ground_or_unloaded_chunks() {
        let world = floored();
        world.put(1, 1, 0, BLOCK_STONE);
        world.missing_cell(-1, 1, 0);
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 2_000);

        assert_eq!(world.get(1, 1, 0), BLOCK_STONE, "it went through the wall");
        assert_eq!(
            fluid::depth(world.get(-1, 1, 0)),
            0,
            "it flowed into a chunk nobody has generated"
        );
    }

    #[test]
    fn water_stopped_by_an_unloaded_chunk_starts_again_when_the_chunk_arrives() {
        // **The gap this file used to carry as a known bug**, in its own
        // words: "a flood that reaches the edge of what the server has
        // cached dams itself against it, comes to rest, is dropped from
        // the queue -- and stays dammed for ever". What a player saw was
        // a wall of water along a straight line they could not account
        // for.
        //
        // Closing it is cheap: a cell that saw an unloaded neighbour
        // goes on a slow list, and a retry is nothing more than offering
        // it and its visible neighbours to the ordinary queue again. See
        // `Water::stalled`.
        let world = floored();
        // A channel, so "how far did it get" is one number.
        for x in -4..=7i32 {
            for z in [-1, 1] {
                world.put(x, 1, z, BLOCK_STONE);
            }
        }
        world.put(-4, 1, 0, BLOCK_STONE);
        world.put(7, 1, 0, BLOCK_STONE);
        for x in 1..=6 {
            world.missing_cell(x, 1, 0);
        }

        let mut sim = Water::new();
        pour(&mut sim, &world, [-3, 0], 1, [0, 0]);
        settle(&mut sim, &world, 2_000);
        assert_eq!(
            fluid::depth(world.get(1, 1, 0)),
            0,
            "it flowed into terrain that did not exist yet"
        );
        assert!(!sim.stalled.is_empty(), "nothing noticed it was waiting");

        // The chunk lands.
        for x in 1..=6 {
            world.load_cell(x, 1, 0);
        }
        // Nobody tells the mechanic; it has to notice by itself, and it
        // is allowed to take a few seconds over it.
        for _ in 0..(RETRY_AFTER_STEPS as usize + 4) * 5 * 12 {
            sim.step(&world, TICK, 4096);
        }
        assert!(
            fluid::depth(world.get(3, 1, 0)) > 0,
            "the water never noticed the chunk had arrived"
        );
    }

    #[test]
    fn a_flood_whose_ground_is_evicted_under_it_carries_on_when_it_comes_back() {
        // **The half of the unloaded-chunk case the retry list did not
        // cover, and the one a player is most likely to meet.** The test
        // above starts with the far chunk already missing, so the cell
        // that stops against it is *examined while loaded*, sees a
        // `None` beside it and puts itself on the slow list. That is the
        // shoreline case and it works.
        //
        // The other order does not. A flood is running with everything
        // loaded, so the cells in front of it are queued in the ordinary
        // way; then the player walks off and the server evicts the chunk
        // those cells are in. `flow_one` looks one of them up, gets
        // `None`, and drops it -- and nobody is left holding anything:
        // the last *loaded* cell was examined before the eviction, saw
        // four loaded neighbours, and was taken off the queue with a
        // clean conscience. Nothing re-examines it, so it never reaches
        // the slow list either. The flood stands still on a straight
        // line for ever, which is exactly the symptom the list was added
        // to remove.
        let world = floored();
        for x in -4..=12 {
            for z in [-1, 1] {
                world.put(x, 1, z, BLOCK_STONE);
            }
        }
        world.put(-4, 1, 0, BLOCK_STONE);

        let mut sim = Water::new();
        pour(&mut sim, &world, [-3, 0], 1, [0, 0]);
        let before = total_water(&world);
        // Far enough that the front is running and the cells beyond it
        // are queued; not far enough to have finished.
        for _ in 0..10 {
            sim.step(&world, TICK, 4096);
        }
        let front = (1..=10).filter(|x| fluid::depth(world.get(*x, 1, 0)) > 0).count();
        assert!(
            (1..8).contains(&front),
            "the fixture has to catch the flood mid-flow, and it was {front} blocks along"
        );

        // The chunk in front of it goes away, queued cells and all.
        for x in (front as i32 + 1)..=12 {
            for y in 0..=2 {
                for z in -1..=1 {
                    world.missing_cell(x, y, z);
                }
            }
        }
        settle(&mut sim, &world, 2_000);

        // ...and comes back.
        for x in (front as i32 + 1)..=12 {
            for y in 0..=2 {
                for z in -1..=1 {
                    world.load_cell(x, y, z);
                }
            }
        }
        for _ in 0..(RETRY_AFTER_STEPS as usize + 4) * 5 * 12 {
            sim.step(&world, TICK, 4096);
        }

        let reached = (1..=12).filter(|x| fluid::depth(world.get(*x, 1, 0)) > 0).count();
        assert!(
            reached > front,
            "the flood never finished: it is still {front} blocks along"
        );
        assert_eq!(total_water(&world), before, "the eviction lost water");
    }

    #[test]
    fn a_border_that_never_loads_costs_what_it_was_measured_to_cost() {
        // **The price of remembering**, kept as a number so it cannot
        // creep. Every cell the front reached and could not read is on
        // the slow list now (see `flow_one`), and it stays there while
        // the chunk is away -- so a shore against terrain nobody ever
        // generates pays for that patience once every
        // `RETRY_AFTER_STEPS` flow steps, for ever.
        //
        // A waiting cell whose chunk is still away costs one read: the
        // retry looks it up, finds nothing, and puts it straight back.
        // The ones that *can* be read cost the ordinary update.
        // Measured on a thirty-cell front over sixty seconds of game
        // time -- twelve retry passes: 5,498 reads, ninety-two a
        // second, and no writes at all. The drying pass this whole
        // mechanism replaced cost more than that on a map with no
        // water on it.
        let world = floored();
        for x in 1..=10 {
            for z in -10..=10 {
                world.missing_cell(x, 1, z);
            }
        }
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 2_000);
        assert!(!sim.stalled.is_empty(), "nothing noticed it was waiting");

        // Sixty seconds of game time with nothing to do.
        let counted = Counted::new(&world);
        for _ in 0..(60.0 / TICK) as usize {
            assert!(
                sim.step(&counted, TICK, 4096).is_empty(),
                "a border that never loaded wrote to the world"
            );
        }
        let per_second = counted.reads.get() as f64 / 60.0;
        println!(
            "STALLED: {} waiting, {} reads in 60 s ({per_second:.0}/s)",
            sim.stalled.len(),
            counted.reads.get()
        );
        assert_eq!(counted.writes.get(), 0, "it kept editing a world it cannot see");
        assert!(per_second <= 600.0, "{per_second:.0} reads a second while idle");
    }

    #[test]
    fn water_washes_plants_away() {
        // Same list that decides what falling sand buries: a tuft of
        // grass does not hold back a flood.
        let world = floored();
        world.put(1, 1, 0, BLOCK_TALL_GRASS);
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 2_000);

        assert_ne!(
            block_kind(world.get(1, 1, 0)),
            block_kind(BLOCK_TALL_GRASS),
            "the tuft held the water back"
        );
    }

    #[test]
    fn a_drowned_snag_keeps_its_wood_and_its_water_while_the_pool_round_it_settles() {
        // **The other side of the tuft.** A snag's foot in a swamp pool is
        // water by its row (`types::BLOCK_DROWNED_BOUGH`), and on the list
        // water moves through it was the first cell a ripple wrote plain
        // water over -- the wood gone, and its eighths handed to the pool.
        // A pool walled in stone with the foot in the middle and one cell a
        // few eighths short, so the water has somewhere to move.
        use primitive_shared::types::{branch, drowned, is_liquid};
        let world = floored();
        for x in -2..=2i32 {
            for z in -2..=2i32 {
                let wall = x.abs() == 2 || z.abs() == 2;
                world.put(x, 1, z, if wall { BLOCK_STONE } else { BLOCK_WATER });
            }
        }
        let foot = drowned(branch(12));
        world.put(0, 1, 0, foot);
        world.put(1, 1, 1, primitive_shared::fluid::with_depth(3));
        let before = total_water(&world);

        let mut sim = Water::new();
        for x in -1..=1 {
            for z in -1..=1 {
                sim.on_block_changed(x, 1, z);
            }
        }
        settle(&mut sim, &world, 2_000);

        assert_eq!(world.get(0, 1, 0), foot, "the pool wrote over the snag standing in it");
        assert_eq!(total_water(&world), before, "water went into or out of the snag");
        for x in -1..=1 {
            for z in -1..=1 {
                assert!(is_liquid(world.get(x, 1, z)), "the pool round the snag has a hole at {x},{z}");
            }
        }
    }

    #[test]
    fn it_runs_slower_than_the_tick_that_drives_it() {
        let world = floored();
        world.put(0, 1, 0, BLOCK_WATER);
        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);

        // **Stated in terms of the interval rather than in ticks.** The
        // point is that water runs on a clock of its own and not on the
        // tick that drives it -- so the test has to say that, and not
        // "four ticks", which was true only while `FLOW_INTERVAL` was a
        // quarter of a second and went red the day it became 0.15.
        // Strictly *under*: the step fires on the tick that reaches the
        // interval, so the last quiet tick is the one before it.
        let ticks_under = (FLOW_INTERVAL / TICK).ceil() as usize - 1;
        assert!(ticks_under >= 2, "the interval is now within a tick of the tick");
        for tick in 0..ticks_under {
            assert!(
                sim.step(&world, TICK, 512).is_empty(),
                "it stepped at tick {tick}, before its own interval was up"
            );
        }
        assert!(!sim.step(&world, TICK, 512).is_empty(), "it never stepped");
    }

    #[test]
    fn the_queue_dedupes_and_stays_bounded() {
        let mut sim = Water::new();
        for _ in 0..50 {
            sim.on_block_changed(0, 5, 0);
        }
        assert_eq!(
            sim.pending(),
            11,
            "the same cell was queued once per notification"
        );

        for i in 0..(MAX_QUEUE as i32 + 5_000) {
            sim.on_block_changed(i, 30, 0);
        }
        assert!(sim.pending() <= MAX_QUEUE);
    }

    #[test]
    fn a_change_of_depth_is_broadcast_because_it_is_drawn() {
        // A cell going from one eighth to three used to draw identically
        // and was filtered out of the broadcast. Every depth is its own
        // height now, so a cell that deepens is a surface a client can
        // see settle, and it has to be told.
        let world = floored();
        let mut sim = Water::new();
        for x in -1..=7 {
            for z in [-1, 1] {
                world.put(x, 1, z, BLOCK_STONE);
            }
        }
        world.put(7, 1, 0, BLOCK_STONE);
        world.put(-1, 1, 0, BLOCK_STONE);
        // The far cell is already wet, at a depth the water arriving
        // later revises upward: it was water before and it is water
        // after, and neither change is visible.
        world.put(2, 1, 0, flowing(1));
        pour(&mut sim, &world, [0, 1], 1, [0, 0]);
        let changes = settle(&mut sim, &world, 2_000);

        let ended = fluid::depth(world.get(2, 1, 0));
        assert!(ended > 1, "the water never got that far, so nothing was proved");
        assert!(
            changes.iter().any(|change| {
                (change.global_x, change.global_y, change.global_z) == (2, 1, 0)
                    && fluid::depth(change.block_id) == ended
            }),
            "the far cell went from one eighth to {ended} and nobody was told"
        );
    }

    #[test]
    fn arriving_and_leaving_are_still_broadcast() {
        // The other half: the changes that *are* visible must survive
        // the filter, or water spreads on the server and nowhere else.
        let world = floored();
        let mut sim = Water::new();
        world.put(0, 1, 0, BLOCK_WATER);
        sim.on_block_changed(0, 1, 0);
        let changes = settle(&mut sim, &world, 2_000);
        assert!(
            changes.iter().any(|c| is_liquid(c.block_id)),
            "water spread on the server and told nobody"
        );

        // Water leaving is the half the finite model made real: knock
        // the floor out and the puddle goes down the hole rather than
        // being recomputed away, and every cell it leaves is a cell a
        // client has to be told about.
        world.put(0, 0, 0, BLOCK_AIR);
        world.put(0, -1, 0, BLOCK_STONE);
        sim.on_block_changed(0, 0, 0);
        let changes = settle(&mut sim, &world, 2_000);
        assert!(
            changes.iter().any(|c| !is_liquid(c.block_id)),
            "water drained away on the server and told nobody"
        );
    }

    #[test]
    fn work_per_pass_is_bounded() {
        let world = floored();
        let mut sim = Water::new();
        for i in 0..2_000 {
            world.put(i, 5, 0, BLOCK_WATER);
            sim.on_block_changed(i, 5, 0);
        }
        let before = sim.pending();
        // Enough ticks for one step, and one step only.
        for _ in 0..5 {
            sim.step(&world, TICK, 64);
        }
        assert!(sim.pending() > before - 512, "a single pass drained far too much");
    }

    #[test]
    fn it_flows_through_the_real_world_and_the_real_registry() {
        // The unit tests above prove the rules. This proves the wiring:
        // the server's own sharded, overlay-backed `World` as the
        // `BlockWorld`, and the mechanic reached through `Mechanics`
        // exactly as the tick loop reaches it. Everything between the
        // two -- the trait impl, the registration, the budget split --
        // is code no unit test here touches.
        use crate::logic::simulation::Mechanics;
        use crate::logic::world::World;

        let world = World::new(4242, 64);
        let chunk = world.generate(primitive_shared::types::ChunkPos::new(0, 0));
        world.insert(chunk);

        // A basin of stone with a source in the air over it.
        for y in 36..=41 {
            for x in 2..=6 {
                for z in 2..=6 {
                    let wall = x == 2 || x == 6 || z == 2 || z == 6;
                    world.set_block(x, y, z, if wall { BLOCK_STONE } else { BLOCK_AIR });
                }
            }
        }
        for x in 2..=6 {
            for z in 2..=6 {
                world.set_block(x, 36, z, BLOCK_STONE);
            }
        }
        world.set_block(4, 40, 4, BLOCK_WATER);

        let mut mechanics = Mechanics::new();
        mechanics.register(Box::new(Water::new()));
        mechanics.on_block_changed(4, 40, 4);
        assert_eq!(mechanics.pending(), vec![("water", 11)]);

        let mut changes = Vec::new();
        for _ in 0..2_000 {
            changes.extend(mechanics.step(&world, TICK));
        }

        assert!(!changes.is_empty(), "nothing was reported to the clients");
        assert!(
            fluid::depth(world.cached_block(4, 37, 4).unwrap()) > 0,
            "it never reached the shelf"
        );
    }

    #[test]
    fn a_chunk_edge_is_not_an_edge_to_water() {
        // Chunks are sixteen wide, and every coordinate in this file is
        // global -- so a border ought to be invisible here. "Ought to"
        // is what a test is for: the failure mode is water that stops
        // dead on a line at x = 16 and looks like a bug in the mesher.
        use crate::logic::simulation::Mechanics;
        use crate::logic::world::World;
        use primitive_shared::types::{ChunkPos, CHUNK_SIZE_X};

        let world = World::new(77, 64);
        for cx in 0..=1 {
            let chunk = world.generate(ChunkPos::new(cx, 0));
            world.insert(chunk);
        }
        // A stone shelf straddling the border at x = CHUNK_SIZE_X, with
        // walls, so the only thing being tested is the crossing.
        let border = CHUNK_SIZE_X as i32;
        let (from, to) = (border - 4, border + 4);
        for x in from..=to {
            for z in 3..=5 {
                world.set_block(x, 40, z, BLOCK_STONE);
                world.set_block(x, 41, z, if z == 4 { BLOCK_AIR } else { BLOCK_STONE });
                world.set_block(x, 42, z, BLOCK_AIR);
            }
        }
        // Sealed at both ends: an open channel leaks into whatever the
        // generator put there, and then this is a test about that.
        for y in 41..=42 {
            world.set_block(from - 1, y, 4, BLOCK_STONE);
            world.set_block(to + 1, y, 4, BLOCK_STONE);
        }
        let mut mechanics = Mechanics::new();
        mechanics.register(Box::new(Water::new()));
        for x in from..=from + 2 {
            world.set_block(x, 41, 4, BLOCK_WATER);
            mechanics.on_block_changed(x, 41, 4);
        }

        for _ in 0..4_000 {
            mechanics.step(&world, TICK);
        }

        for x in border..=border + 1 {
            let depth = fluid::depth(world.cached_block(x, 41, 4).unwrap());
            assert!(
                depth > 0,
                "at x = {x} the water is {depth} deep: it stopped at the border ({border})"
            );
        }
    }

    /// The lake in `a_cut_at_the_edge_of_a_lake_drains_it_rather_than_seeping`:
    /// eleven cells square and full, walled, standing on a bed with a
    /// chamber under it for whatever gets through the bed.
    ///
    /// Fenced well outside the lake, because the test world has no floor
    /// past `REACH`: water that ran off the edge of the fixture would
    /// fall out of the range `total_water` counts and read as water gone
    /// missing when nothing of the kind had happened.
    ///
    /// **The chamber is seven storeys deep and that is not decoration.**
    /// A shallow one fills: what comes through the hole piles up under
    /// it into a cone sloping one eighth per block, because that is
    /// where the neighbour rules stop, and once the cone reaches the
    /// hole the lake stops draining for want of anywhere to drain *to*.
    /// A fixture like that measures the size of the cone rather than the
    /// speed of the drain, which is not the question being asked.
    const LAKE_Y: i32 = 9;
    fn walled_lake() -> TestWorld {
        const LAKE: i32 = 5;
        const FENCE: i32 = 12;
        let world = floored();
        for x in -FENCE..=FENCE {
            for z in -FENCE..=FENCE {
                world.put(x, LAKE_Y - 1, z, BLOCK_STONE); // the bed
                if x.abs() == FENCE || z.abs() == FENCE {
                    for y in 1..=LAKE_Y + 1 {
                        world.put(x, y, z, BLOCK_STONE);
                    }
                } else if x.abs() == LAKE + 1 || z.abs() == LAKE + 1 {
                    world.put(x, LAKE_Y, z, BLOCK_STONE);
                } else if x.abs() <= LAKE && z.abs() <= LAKE {
                    world.put(x, LAKE_Y, z, BLOCK_WATER);
                }
            }
        }
        world
    }

    /// Every eighth still standing in the lake itself.
    fn water_in_the_lake(world: &TestWorld) -> u32 {
        let mut total = 0;
        for x in -5..=5 {
            for z in -5..=5 {
                total += fluid::depth(world.get(x, LAKE_Y, z)) as u32;
            }
        }
        total
    }

    #[test]
    fn a_cut_at_the_edge_of_a_lake_drains_it_rather_than_seeping() {
        // **The difference between water that spreads and water that
        // flows.** Falling and levelling only ever look at a neighbour,
        // so news of a hole travels one cell a step and half of it is
        // lost at each one: a lake whose bed is cut through at a corner
        // gives up the ring of cells around the cut and then stops, with
        // the rest of itself standing in a wedge nothing can move. A
        // player who digs the hole and watches sees a damp patch and
        // concludes the lake is not draining, and for any length of time
        // they are prepared to watch, it is not.
        //
        // What makes it a flow is `draw_on_the_body`: a cell that has
        // just given water away takes a share of whatever is standing
        // within `SEARCH_RANGE` of it, along the water it is part of.
        // The news still travels a cell a step -- `wake` is unchanged --
        // but the *water* travels six.
        //
        // Measured on this fixture, 121 full cells with one block of the
        // bed taken out at a corner:
        //
        // | rule | after 30 s | where it stops |
        // |---|---|---|
        // | falling and levelling | 848 of 968 | 848 |
        // | ...and drawing on the body | 692 of 968 | 347 |
        // | ...and levelling the sheet | 450 of 968 | 213 |
        //
        // (The middle row's 692 was at the old `FLOW_INTERVAL`; at 0.15 it
        // is 599.) **All of them stop**, the first two at a wedge the
        // neighbour rules cannot split and the last at a film of one or two
        // eighths over the bed: an eighth of the lake left through the cut
        // with the neighbour rules alone, and four fifths of it leaves now.
        // This runs `Water::new`, without the soaking that takes the last
        // film -- see
        // `a_pond_drained_through_its_bed_leaves_a_film_that_soaks_away`
        // for the lake as the server runs it.
        let world = walled_lake();
        let before = total_water(&world);
        let inside = water_in_the_lake(&world);
        assert_eq!(inside, 968, "the fixture is not the lake it used to be");

        let mut sim = Water::new();
        world.put(5, LAKE_Y - 1, 5, BLOCK_AIR);
        sim.on_block_changed(5, LAKE_Y - 1, 5);
        let mut thirty_seconds = 0;
        for tick in 1..=8_000 {
            sim.step(&world, TICK, 4096);
            if tick == 600 {
                thirty_seconds = water_in_the_lake(&world);
            }
        }
        let left = water_in_the_lake(&world);
        println!("LAKE: {thirty_seconds} of {inside} after 30 s, {left} where it stops");
        assert_eq!(total_water(&world), before, "the cut ate some of the lake");
        assert_eq!(sim.pending(), 0, "the lake never came to rest");
        assert!(
            thirty_seconds * 4 < inside * 3,
            "half a minute and {thirty_seconds} of {inside} eighths are still in \
             the lake: it is seeping rather than draining"
        );
        assert!(
            left * 2 < inside,
            "the cut stopped with {left} of {inside} eighths still in the lake"
        );
    }

    #[test]
    fn a_channel_poured_at_one_end_levels_along_its_whole_length() {
        // **This test used to be called
        // `a_pond_drains_down_to_a_wedge_and_no_further`**, and it asserted
        // the wedge: `level_transfer` will not move a difference of one, so
        // a channel poured full at one end settled as
        // `[7, 6, 5, 4, 4, 3, 2, 1, 0, 0, 0, 0, 0]` -- thirty-two eighths
        // that would cover thirteen cells two and a half deep stopped after
        // eight of them, and the last five stayed dry. It was written down
        // as the one honest limit of whole eighths, with the fixes that had
        // been tried and why each went (moving the last eighth: a shimmer
        // for ever; levelling at a distance inside the flow: 101,000 reads
        // where 17,000 had done; finer units: a save format). `level_one`
        // is the fix none of them was -- see it for why -- and this is the
        // same channel with the opposite assertion.
        const FAR: i32 = 12;
        let world = floored();
        for x in -1..=FAR + 1 {
            for z in [-1, 1] {
                world.put(x, 1, z, BLOCK_STONE);
            }
        }
        world.put(-1, 1, 0, BLOCK_STONE);
        world.put(FAR + 1, 1, 0, BLOCK_STONE);

        let mut sim = Water::new();
        pour(&mut sim, &world, [0, 3], 1, [0, 0]);
        let before = total_water(&world);
        settle(&mut sim, &world, 20_000);
        assert_eq!(total_water(&world), before, "the channel lost water");
        assert_eq!(sim.pending(), 0, "the channel never came to rest");

        let profile: Vec<u8> = (0..=FAR).map(|x| fluid::depth(world.get(x, 1, 0))).collect();
        println!("CHANNEL: {profile:?}");
        let (low, high) = (profile.iter().min().unwrap(), profile.iter().max().unwrap());
        assert!(*low >= 2 && high - low <= 1, "the channel did not level out along its length: {profile:?}");
    }

    #[test]
    fn two_ponds_joined_by_a_trench_come_to_one_level() {
        // A full pond and an empty one, five cells apart, and a trench cut
        // between them. The neighbour rules alone left them standing three
        // quarters of a block apart with a wedge in the trench.
        let world = floored();
        for x in -2..=14 {
            for z in -2..=2 {
                world.put(x, 1, z, BLOCK_STONE);
            }
        }
        // Two 3x3 ponds, and a one-wide trench from one to the other.
        for x in -1..=13 {
            for z in -1..=1 {
                let pond = x <= 1 || x >= 11;
                if pond || z == 0 {
                    world.put(x, 1, z, BLOCK_AIR);
                }
            }
        }
        let mut sim = Water::new();
        pour(&mut sim, &world, [-1, 1], 1, [-1, 1]);
        let before = total_water(&world);
        settle(&mut sim, &world, 20_000);
        assert_eq!(total_water(&world), before);
        assert_eq!(sim.pending(), 0, "the ponds never came to rest");
        let near = fluid::depth(world.get(0, 1, 0));
        let far = fluid::depth(world.get(12, 1, 0));
        println!("PONDS: near {near}, far {far}");
        assert!(near.abs_diff(far) <= 1, "two joined ponds rested {near} and {far} eighths deep");
    }

    #[test]
    fn a_pond_drained_through_its_bed_leaves_a_film_that_soaks_away() {
        // The bed cut at one corner: the lake runs down the hole, and what
        // conservation leaves behind -- an eighth or two over the bed, which
        // the header of `fluid` used to list as a known fault -- dries from
        // its rim in. Measured a minute apart:
        // `[968, 261, 179, 152, 119, 84, 46, 16, 1, 0]` -- the lake gone in
        // the first minute, the film in the next seven. With `Water::new` it
        // would stand for ever, and that is what every other test here
        // relies on.
        let world = walled_lake();
        let mut sim = Water::soaking();
        world.put(5, LAKE_Y - 1, 5, BLOCK_AIR);
        sim.on_block_changed(5, LAKE_Y - 1, 5);
        let steps = (900.0 / TICK) as usize;
        let mut left_at = Vec::new();
        for tick in 0..steps {
            sim.step(&world, TICK, 4096);
            if tick % 1200 == 0 {
                left_at.push(water_in_the_lake(&world));
            }
        }
        let left = water_in_the_lake(&world);
        println!("SOAKED: {left_at:?} -> {left}");
        assert_eq!(left, 0, "the drained lake kept {left} eighths standing in its bed");
    }

    #[test]
    fn a_film_in_a_hollow_beside_deeper_water_does_not_soak_away() {
        // The edge of a pond is not a film: a one beside a three is the
        // shore of something, and the pond would be eaten from its edges.
        let world = floored();
        world.put(0, 1, 0, fluid::with_depth(1));
        world.put(1, 1, 0, fluid::with_depth(3));
        assert!(!Water::soaks_away(&world, (0, 1, 0)));
        world.put(1, 1, 0, fluid::with_depth(2));
        assert!(Water::soaks_away(&world, (0, 1, 0)));
        // Water over it, or nothing under it, is a column and a fall.
        world.put(0, 2, 0, fluid::with_depth(1));
        assert!(!Water::soaks_away(&world, (0, 2, 0)), "a film over water soaked away");
        assert!(!Water::soaks_away(&world, (0, 1, 0)), "the bottom of a column soaked away");
    }

    // ---- the rain ------------------------------------------------------

    /// Runs showers until `count` of them have fallen, telling the flow
    /// simulation about everything the sky changed, and letting the
    /// water settle in between exactly as the tick loop does.
    fn rain_on(
        rain: &mut Rainfall,
        sim: &mut Water,
        world: &TestWorld,
        around: &[(i32, i32, i32)],
        count: usize,
    ) {
        for _ in 0..count {
            while !rain.due(TICK, true) {
                sim.step(world, TICK, 4096);
            }
            for cell in rain.fall(world, around) {
                sim.on_block_changed(cell.0, cell.1, cell.2);
            }
            settle(sim, world, 400);
        }
    }

    #[test]
    fn a_dry_world_stays_dry_however_long_it_rains() {
        // **The bound on the one rule that makes water.** The sky
        // reaches every open cell there is, so a rule that wet dry
        // ground would turn a meadow into a lake in a downpour -- and it
        // would look like one, because one eighth is drawn exactly like
        // eight. Rain deepens water that is already standing; it never
        // starts any.
        let world = floored();
        let mut sim = Water::new();
        let mut rain = Rainfall::seeded(4);
        rain_on(&mut rain, &mut sim, &world, &[(0, 1, 0)], 200);

        assert_eq!(total_water(&world), 0, "it rained a puddle onto dry stone");
    }

    #[test]
    fn rain_deepens_standing_water_and_does_nothing_to_water_that_is_already_full() {
        // **The sky puts water back, and it has a ceiling.** A sheet of
        // water drawn down to a film is what a channel that has since
        // been blocked up leaves behind -- and it is drawn as the film it
        // is. Left open it comes back.
        //
        // The same sheet full to the brim takes nothing at all, however
        // long it rains, which is the ceiling: `fluid::rain_transfer`
        // refuses a full cell, so what the sky can do to a body of water
        // is bounded by the size of the body and not by the length of
        // the downpour.
        //
        // Two runs of the same fixture, because the interesting number
        // is the difference between them. The sheet is wide because the
        // sky is: rain falls on a `RAIN_RADIUS` square around a player
        // and only a fraction of it lands on any one pond, which is
        // realistic and makes for a slow test if the pond is a puddle.
        const EDGE: i32 = 12;
        let sheet = |depth: u8| {
            let world = floored();
            for x in -EDGE..=EDGE {
                for z in -EDGE..=EDGE {
                    let wall = x.abs() == EDGE || z.abs() == EDGE;
                    world.put(x, 1, z, if wall { BLOCK_STONE } else { flowing(depth) });
                }
            }
            world
        };

        let film = sheet(1);
        let before = total_water(&film);
        let mut sim = Water::new();
        let mut rain = Rainfall::seeded(11);
        rain_on(&mut rain, &mut sim, &film, &[(0, 1, 0)], 120);
        let after = total_water(&film);
        println!("FILM: {before} -> {after}");
        assert!(after > before, "the sky did nothing to a sheet standing open");

        let brimming = sheet(fluid::SOURCE_DEPTH);
        let was = total_water(&brimming);
        let mut sim = Water::new();
        let mut rain = Rainfall::seeded(11);
        rain_on(&mut rain, &mut sim, &brimming, &[(0, 1, 0)], 120);
        assert_eq!(
            total_water(&brimming),
            was,
            "the rain overfilled water that had no room in it"
        );
    }

    #[test]
    fn a_roof_keeps_the_rain_out_of_what_is_under_it() {
        // The same pond with a lid on. It has to be the *same* answer
        // `climate::has_roof` gives a player standing there, or the game
        // shelters a pond in a place it shelters nobody -- so both read
        // `types::blocks_the_sky`.
        let world = floored();
        for x in -1..=1 {
            for z in -1..=1 {
                world.put(x, 1, z, flowing(1));
                world.put(x, 4, z, BLOCK_STONE); // the lid
            }
        }
        let before = total_water(&world);

        let mut sim = Water::new();
        let mut rain = Rainfall::seeded(2);
        rain_on(&mut rain, &mut sim, &world, &[(0, 1, 0)], 300);

        assert_eq!(total_water(&world), before, "the rain came through the roof");
    }

    #[test]
    fn a_pond_in_the_rain_still_empties_through_a_cut() {
        // **The property the endless source was taken out of the game to
        // get, tested against the one rule that was put back in.** A sky
        // that filled a pond faster than a cut emptied it would be the
        // endless source under another name, and a pond would be
        // undrainable again -- in the rain, which is exactly when a
        // player is standing there watching it.
        //
        // Two things make it hold, both in `Rainfall`: the sky never
        // touches a cell that has anywhere to send its water, so every
        // cell of a pond that is running out is out of its reach until
        // it has finished running out; and even where it does land it
        // is sixteen times slower than what a one-block cut passes.
        //
        // The same pond is cut twice, once under a clear sky and once in
        // the rain, because the number that means anything is the
        // difference. Where it stops is the same either way.
        const EDGE: i32 = 3;
        let pond = || {
            let world = floored();
            for x in -EDGE..=EDGE {
                for z in -EDGE..=EDGE {
                    let wall = x.abs() == EDGE || z.abs() == EDGE;
                    for y in 1..=2 {
                        world.put(x, y, z, if wall { BLOCK_STONE } else { BLOCK_WATER });
                    }
                }
            }
            world
        };
        let inside = |world: &TestWorld| -> u32 {
            let mut total = 0;
            for x in -EDGE + 1..EDGE {
                for z in -EDGE + 1..EDGE {
                    for y in 1..=2 {
                        total += fluid::depth(world.get(x, y, z)) as u32;
                    }
                }
            }
            total
        };
        let cut = |world: &TestWorld, sim: &mut Water| {
            for y in 1..=2 {
                world.put(EDGE, y, 0, BLOCK_AIR);
                sim.on_block_changed(EDGE, y, 0);
            }
        };

        let dry = pond();
        let before = inside(&dry);
        let mut sim = Water::new();
        cut(&dry, &mut sim);
        settle(&mut sim, &dry, 20_000);
        let under_a_clear_sky = inside(&dry);

        let wet = pond();
        let mut sim = Water::new();
        let mut rain = Rainfall::seeded(77);
        cut(&wet, &mut sim);
        rain_on(&mut rain, &mut sim, &wet, &[(0, 2, 0)], 200);
        let in_the_rain = inside(&wet);

        println!("CUT: {before} -> {under_a_clear_sky} dry, {in_the_rain} in the rain");
        // Not "empty": what a cut leaves is the wedge, and on a pond
        // this size the wedge is nearly half of it. The fixture only has
        // to drain enough that being held up would show.
        assert!(
            under_a_clear_sky * 4 < before * 3,
            "the fixture does not drain even under a clear sky: {under_a_clear_sky} of {before}"
        );
        assert!(
            in_the_rain <= under_a_clear_sky + fluid::SOURCE_DEPTH as u32,
            "the rain held the pond up: {in_the_rain} in it against {under_a_clear_sky} dry"
        );
    }

    #[test]
    fn the_sky_does_nothing_when_it_is_not_raining() {
        let world = floored();
        world.put(0, 1, 0, flowing(4));
        let before = total_water(&world);

        let mut rain = Rainfall::seeded(9);
        for _ in 0..2_000 {
            assert!(!rain.due(TICK, false), "a clear sky came due for a shower");
        }
        assert_eq!(total_water(&world), before);

        // ...and the first shower of a downpour is a shower, not a
        // backlog of everything that did not fall while it was dry.
        assert!(!rain.due(TICK, true), "the clock was running under a clear sky");
    }

    // ---- the sweep over random ground ----------------------------------

    /// Builds the same lumpy terrain twice from one seed: stone floor,
    /// random pillars and overhangs, and a few sources standing in it.
    fn random_ground(seed: u64, world: &TestWorld) -> Vec<Cell> {
        /// The last cell of open ground; the walls stand one further out.
        const SIDE: i32 = 5;
        const TOP: i32 = 6;
        let mut rng = Rng::seeded(seed);
        for x in -SIDE - 1..=SIDE + 1 {
            for z in -SIDE - 1..=SIDE + 1 {
                world.put(x, 0, z, BLOCK_STONE);
                // Walled in, so that everything the water does happens
                // inside the box the comparison looks at. An open box
                // would test what happens at the edge of the fixture,
                // which is nothing anybody plays.
                let wall = x.abs() > SIDE || z.abs() > SIDE;
                for y in 1..=TOP + 1 {
                    if wall || rng.next_f32() < 0.30 {
                        world.put(x, y, z, BLOCK_STONE);
                    }
                }
            }
        }
        let mut sources = Vec::new();
        while sources.len() < 3 {
            let x = (rng.next_u64() % (SIDE as u64 * 2 + 1)) as i32 - SIDE;
            let z = (rng.next_u64() % (SIDE as u64 * 2 + 1)) as i32 - SIDE;
            let y = 1 + (rng.next_u64() % TOP as u64) as i32;
            if world.get(x, y, z) == BLOCK_AIR {
                world.put(x, y, z, BLOCK_WATER);
                sources.push((x, y, z));
            }
        }
        sources
    }

    /// The box the sweep and the comparison both cover: the walled
    /// ground, and one cell of margin all round it.
    const BOX_SIDE: i32 = 7;
    const BOX_TOP: i32 = 9;

    /// Sweeps every cell in the box over and over until nothing
    /// changes, and returns how many rounds that took.
    ///
    /// Deliberately the dumbest possible driver: no queue, no wake, no
    /// order worth the name. What it checks is not the rules -- `fluid`
    /// tests those exhaustively -- but everything this file adds around
    /// them. Run over a world the queue has already finished with, a
    /// return of zero is the sweep saying "there was nothing left to
    /// do", which is the only honest way to ask whether a wake missed
    /// somebody now that the arrangement itself is order-dependent.
    fn brute_force(world: &TestWorld) -> u32 {
        let mut sim = Water::new();
        let mut ignored = Vec::new();
        for round in 0..500u32 {
            let mut changed = false;
            for x in -BOX_SIDE..=BOX_SIDE {
                for z in -BOX_SIDE..=BOX_SIDE {
                    for y in 0..BOX_TOP {
                        let before = world.get(x, y, z);
                        sim.flow_one(world, (x, y, z), &mut ignored);
                        if world.get(x, y, z) != before {
                            changed = true;
                        }
                        // Whatever it wanted to wake is the queue's job,
                        // and the queue is the thing this is a reference
                        // for. Sweeping everything is the point.
                        sim.queue.clear();
                        sim.queued.clear();
                        sim.stalled.clear();
                        sim.stalled_set.clear();
                        ignored.clear();
                    }
                }
            }
            if !changed {
                return round;
            }
        }
        panic!("the brute-force sweep never reached a fixed point");
    }

    #[test]
    fn random_ground_comes_to_rest_where_the_rules_call_it_rest() {
        // **The sweep, and it has earned its keep.** The previous
        // model's worst bug -- two cells trading the same eighth for
        // ever, invisibly, while the autosave rewrote the world every
        // interval -- was found here and nowhere else, on the
        // twenty-somethingth seed, as a queue that never reached zero.
        //
        // What it compares changed with the model, and the new
        // comparison is the stronger one. Water used to be a *function*
        // of the sources, so there was one right answer per fixture and
        // the sweep could be asked for it cell by cell. Now water is
        // moved, so where each eighth ends up depends on the order the
        // cells were visited in -- and the queue's order is not the
        // sweep's, legitimately. Asking for the same picture would be
        // asking the model to be something it is not.
        //
        // So the sweep is asked the question that survives: *is this
        // finished?* It is run over the world the queue left behind, and
        // it has to find nothing to do. That catches the failure the
        // cell-by-cell comparison was really there for -- a wake that
        // misses a neighbour, so the queue empties while the world still
        // has somewhere for water to go -- and it catches it without
        // caring which of several equally settled arrangements came out.
        //
        // Three properties, each a different way the scheduling around
        // the rules can be wrong:
        //
        // * it goes quiet, in a bounded number of steps;
        // * where it goes quiet, the rules agree there is nothing left
        //   to do;
        // * not one eighth was created or lost getting there.
        for seed in 1..=32u64 {
            let world = TestWorld::default();
            let sources = random_ground(seed, &world);
            let before = total_water(&world);

            let mut sim = Water::new();
            for &cell in &sources {
                sim.on_block_changed(cell.0, cell.1, cell.2);
            }
            let mut ticks = 0;
            while sim.pending() > 0 && ticks < 20_000 {
                sim.step(&world, TICK, 4096);
                ticks += 1;
            }
            assert_eq!(sim.pending(), 0, "seed {seed} never came to rest");
            assert_eq!(
                total_water(&world),
                before,
                "seed {seed} finished with water it was not given"
            );

            let rounds = brute_force(&world);
            assert_eq!(
                rounds, 0,
                "seed {seed}: the queue stopped while the rules still had work \
                 to do, and a dumb sweep found it in {rounds} rounds"
            );
            // ...and no sheet anywhere is left unlevelled: the list that
            // drives `level_one` missed nobody either.
            let mut probe = Water::new();
            let mut moved = Vec::new();
            for x in -BOX_SIDE..=BOX_SIDE {
                for z in -BOX_SIDE..=BOX_SIDE {
                    for y in 0..BOX_TOP {
                        probe.level_one(&world, (x, y, z), &mut moved);
                    }
                }
            }
            assert!(moved.is_empty(), "seed {seed}: a sheet was left sloping: {moved:?}");
        }
    }

    // ---- what it costs -------------------------------------------------

    /// A `BlockWorld` that counts what a mechanic asks of it.
    struct Counted<'a> {
        inner: &'a TestWorld,
        reads: std::cell::Cell<u64>,
        writes: std::cell::Cell<u64>,
    }
    impl<'a> Counted<'a> {
        fn new(inner: &'a TestWorld) -> Self {
            Self { inner, reads: 0.into(), writes: 0.into() }
        }
    }
    impl BlockWorld for Counted<'_> {
        fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            self.reads.set(self.reads.get() + 1);
            self.inner.block(gx, gy, gz)
        }
        fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId) {
            self.writes.set(self.writes.get() + 1);
            self.inner.set(gx, gy, gz, block)
        }
    }

    #[test]
    fn the_reference_spill_costs_no_more_than_it_was_measured_to_cost() {
        // **The measurement, kept as a test so it cannot quietly get
        // worse.** The fixture is the ordinary thing a player does: a
        // walled pond two deep on an open floor, with one block of its
        // wall knocked out.
        //
        // Four models have now been measured on it, and the numbers say
        // plainly what each one bought and what it cost:
        //
        // | model | ticks | reads | writes | sent |
        // |---|---|---|---|---|
        // | levels, with a search to get unstuck | 765 | 292,926 | 2,798 | -- |
        // | levels, pulled from the neighbours | 45 | 2,239 | 95 | -- |
        // | amounts, moved between cells | 475 | 16,999 | 1,571 | 55 |
        // | ...and drawing on the body within reach | 410 | 23,759 | 2,474 | 237 |
        // | ...and a fed fall keeping its last eighth | 282 | 22,367 | 2,171 | 2,171 |
        // | ...and sheets levelled, an eighth a pair | 339 | 66,238 | 3,399 | 3,399 |
        // | ...two eighths a pair, from settled cells | 264 | 48,576 | 2,706 | 2,706 |
        //
        // (The last three rows are at `FLOW_INTERVAL` 0.15, where the row
        // above them measures 246 ticks rather than 410; reads and writes do
        // not depend on the interval. See `fluid::fall_keeping` for what the
        // first of them bought -- a waterfall with no air in it.)
        //
        // **What the last row bought and what it cost.** The spill no
        // longer stops at a wedge: it spreads until it is level
        // (`level_one`), which is more cells wetted -- the writes -- and a
        // walk of the sheet every pass -- most of the reads. The worst single
        // step went from 413 reads to 2,447, which is what two walks of a
        // 256-cell sheet cost and is bounded by `SHEETS_PER_STEP` and
        // `SHEET_MAX` whatever the water does. In wall time, from an
        // optimised build (`the_reference_spill_timed`, same binary, both
        // ways): 2.3-2.6 ms for the whole spill without the sheets, 5.5-5.6 ms
        // with them. Still water still costs nothing: it is never on the
        // list.
        //
        // **The second one is cheaper than any of these and it is not
        // coming back**, because what it was cheap at was deciding that
        // a pond has not changed. It never moved any water: a cell's
        // depth was a function of its neighbours, so the whole spill
        // reached its answer in a couple of passes and a source refilled
        // itself for ever. Draining a pond was not slow there -- it was
        // impossible.
        //
        // **What the last line bought and what it cost.** The search
        // (`draw_on_the_body`) is forty per cent more reads and it
        // settles the spill *sooner*, in 410 ticks rather than 475,
        // because the water goes where it is going instead of diffusing
        // there. The reads are the price of looking: a cell that has
        // just given water away walks a diamond of `SEARCH_RANGE`
        // through the water it is part of, and a spill is nearly all
        // cells that have just given water away. Still water never
        // looks, which is why an ocean still costs nothing.
        //
        // The one number that went up more than the rest is what is
        // *sent*: 237 broadcasts rather than 55, because the spill now
        // genuinely wets and unwets four times as many cells. That is
        // water arriving somewhere, which is the change a client has to
        // be told about; the filter is still doing its job, and the
        // assertion below says so.
        //
        // The ceilings below are roughly half again on top of what it
        // takes, so they catch a regression rather than an edit.
        let world = floored();
        const EDGE: i32 = 3;
        for x in -EDGE..=EDGE {
            for z in -EDGE..=EDGE {
                let wall = x.abs() == EDGE || z.abs() == EDGE;
                for y in 1..=2 {
                    world.put(x, y, z, if wall { BLOCK_STONE } else { BLOCK_WATER });
                }
            }
        }
        let counted = Counted::new(&world);
        let mut sim = Water::new();
        for y in 1..=2 {
            world.put(EDGE, y, 0, BLOCK_AIR);
            sim.on_block_changed(EDGE, y, 0);
        }
        let mut ticks = 0u32;
        let mut broadcast = 0usize;
        let mut worst_step = 0u64;
        for _ in 0..200_000 {
            ticks += 1;
            let before = counted.reads.get();
            broadcast += sim.step(&counted, TICK, 4096).len();
            worst_step = worst_step.max(counted.reads.get() - before);
            if sim.pending() == 0 {
                break;
            }
        }
        println!(
            "SPILL: ticks={ticks} reads={} writes={} broadcast={broadcast} worst step={worst_step}",
            counted.reads.get(),
            counted.writes.get()
        );
        assert_eq!(sim.pending(), 0, "the reference spill never settled");
        // A tenth of the writes are visible and sent. The bar is a
        // quarter rather than an eighth because the search legitimately
        // moved that ratio -- water that travels wets more cells -- and
        // a bar set at what it happens to be today fails on the next
        // honest change instead of on the day the filter is deleted,
        // which is what it is here to catch.
        // Every write is a change of depth and every depth is drawn, so
        // every write goes out: 2474 broadcasts for 2474 writes on this
        // fixture. This used to assert the opposite -- fewer than a
        // quarter sent -- when all depths drew alike and the rest were
        // filtered as invisible. If the two counts ever part again,
        // either a depth has stopped being drawn or a write is being
        // lost on the way to the clients; both want looking at.
        assert_eq!(
            broadcast,
            counted.writes.get() as usize,
            "{broadcast} of {} writes were sent to the clients",
            counted.writes.get()
        );
        assert!(ticks <= 1_000, "the spill took {ticks} ticks");
        assert!(counted.reads.get() <= 73_000, "{} reads", counted.reads.get());
        assert!(counted.writes.get() <= 4_100, "{} writes", counted.writes.get());
        assert!(worst_step <= 6_000, "the worst step read the world {worst_step} times");
    }

    /// The reference spill in wall-clock time, with the sheets levelled and
    /// without, in one binary. Ignored because a time is only worth reading
    /// from an optimised build:
    /// `cargo test --release -p primitive_server --lib the_reference_spill_timed -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_reference_spill_timed() {
        for sheets in [0, SHEETS_PER_STEP] {
            let (mut total, mut worst, mut runs) = (0u128, 0u128, 0u32);
            for _ in 0..20 {
                let world = floored();
                const EDGE: i32 = 3;
                for x in -EDGE..=EDGE {
                    for z in -EDGE..=EDGE {
                        let wall = x.abs() == EDGE || z.abs() == EDGE;
                        for y in 1..=2 {
                            world.put(x, y, z, if wall { BLOCK_STONE } else { BLOCK_WATER });
                        }
                    }
                }
                let mut sim = Water { sheets_per_step: sheets, ..Water::new() };
                for y in 1..=2 {
                    world.put(EDGE, y, 0, BLOCK_AIR);
                    sim.on_block_changed(EDGE, y, 0);
                }
                // The flow's own settling, which is where the two agree to
                // stop being comparable: without sheets the list never
                // empties, so the queue is what is waited on.
                for _ in 0..2_000 {
                    let start = std::time::Instant::now();
                    sim.step(&world, TICK, 4096);
                    let took = start.elapsed().as_nanos();
                    total += took;
                    worst = worst.max(took);
                    if sim.queue.is_empty() && (sheets == 0 || sim.pending() == 0) {
                        break;
                    }
                }
                runs += 1;
            }
            println!("TIMED sheets={sheets}: {} us a spill, worst step {} us", total / u128::from(runs) / 1000, worst / 1000);
        }
    }

    // ---- a vessel knocked over ----

    #[test]
    fn a_broken_barrel_floods_a_few_cells_and_the_flood_is_gone_when_it_dries() {
        use primitive_shared::types::BARREL_JUGS;
        let world = floored();
        let full = u32::from(BARREL_JUGS) * SPILL_EIGHTHS_PER_JUG;
        let written = spill(&world, (0, 1, 0), full);
        // Finite: exactly what the barrel held, up to the cap.
        assert_eq!(total_water(&world), full.min(SPILL_MAX_EIGHTHS), "the spill is not what the barrel held");
        // Small: a few cells, none further than the reach, all on the floor.
        assert!(written.len() > 1 && written.len() <= 8, "a barrel wet {} cells", written.len());
        for ((x, y, z), _) in &written {
            assert_eq!(*y, 1, "a spill was written off the floor");
            assert!(x.abs() + z.abs() <= SPILL_REACH, "a spill reached ({x}, {z})");
        }
        // Let it run: the flow moves it and makes none.
        let mut sim = Water::new();
        for ((x, y, z), _) in &written {
            sim.on_block_changed(*x, *y, *z);
        }
        settle(&mut sim, &world, 4000);
        assert!(total_water(&world) <= full, "the flow made water out of a spill");
        // ...then let it dry: not early, and then all of it.
        let mut spills = Spills::new();
        spills.spilled((0, 1, 0));
        assert!(spills.dry(&world, SPILL_DRIES_SECONDS * 0.5).is_empty(), "a spill dried at half its time");
        assert!(!spills.dry(&world, SPILL_DRIES_SECONDS).is_empty(), "nothing dried");
        assert_eq!(total_water(&world), 0, "a spill left water standing after it dried");
        assert!(spills.is_empty(), "a dried spill is still timed");
    }

    #[test]
    fn a_spill_beside_a_pond_dries_and_leaves_the_pond() {
        let world = floored();
        // A pond two cells east of the spill, full and walled so it stays put.
        for x in 3..=5 {
            world.put(x, 1, 0, BLOCK_WATER);
            world.put(x, 1, 1, BLOCK_STONE);
            world.put(x, 1, -1, BLOCK_STONE);
        }
        world.put(6, 1, 0, BLOCK_STONE);
        let before = total_water(&world);
        spill(&world, (0, 1, 0), SPILL_EIGHTHS_PER_JUG);
        let mut spills = Spills::new();
        spills.spilled((0, 1, 0));
        spills.dry(&world, SPILL_DRIES_SECONDS + 1.0);
        assert_eq!(total_water(&world), before, "drying a jug's spill took water out of the pond, or left the spill");
    }

    #[test]
    fn a_spill_is_not_written_through_a_wall_or_over_a_hole() {
        let world = floored();
        for (dx, dz) in SIDES {
            world.put(dx, 1, dz, BLOCK_STONE);
        }
        assert_eq!(spill(&world, (0, 1, 0), SPILL_MAX_EIGHTHS).len(), 1, "a spill went through a wall");
        let world = floored();
        world.put(0, 0, 0, BLOCK_AIR);
        assert!(
            spill(&world, (0, 1, 0), SPILL_EIGHTHS_PER_JUG).iter().all(|(cell, _)| *cell != (0, 1, 0)),
            "water was written hanging over a hole"
        );
    }
}


