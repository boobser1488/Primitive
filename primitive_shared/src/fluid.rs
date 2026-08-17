//! Water, as a thing with a *level* rather than a thing that is either
//! there or not.
//!
//! ## Why the rules live here and not with the simulation
//!
//! Water flows: `primitive_server::logic::water` runs it, a cell at a
//! time, on the server's tick. What that code owns is the *world* --
//! looking cells up, writing them back, telling clients what changed.
//! What it does not own is how much water moves, and that is this
//! module.
//!
//! The split is not tidiness. Flowing water goes wrong in one specific
//! way: the client draws a surface at one height, the server thinks the
//! level is another, and the player swims through a wall of water that
//! is not where it is drawn. That is exactly the class of disagreement
//! `primitive_shared` exists to prevent -- so the mesher, the collider,
//! the drowning check and the simulation all read their answers from the
//! same functions here.
//!
//! Everything below is a pure function of a few small numbers, which is
//! what lets the rules be tested exhaustively -- every level against
//! every level, with no world, no tick loop and no socket.
//!
//! ## The model
//!
//! A cell of water holds a **level** in eighths. It is the number the
//! flow simulation moves between cells, and it decides how far water
//! spreads and where it stops.
//!
//! **Nobody outside the simulation can see it.** `surface_height` is the
//! same for every cell of water there is, and so, therefore, is what the
//! mesher draws, what the collider walks into and what the fog switches
//! on. That is deliberate and it is worth the loss of a physically
//! honest shallow puddle: a level that shows through is a cell of water
//! at a different height from the water beside it, which is a step with
//! a wall down it in the middle of an otherwise flat sea. See
//! `surface_height`.
//!
//! The level rides in the same variant field a layer count does (see
//! `types`), so a partly-filled cell is still one `u16` and a save
//! written before flowing water existed reads back as full. That is not
//! an accident of layout: it is the reason the layer field was made
//! three bits wide rather than two.
//!
//! ## What the flow simulation will need from here
//!
//! * `FLOW_PER_STEP` -- how much level moves in one step, which sets how
//!   fast water spreads and therefore how much work a burst of it costs.
//! * `settle` -- where a unit of water goes from one cell: down first,
//!   then sideways into whatever is lower. Pure, so it can be tested
//!   without a world.
//! * `surface_height` -- how high the drawn and collided surface of a
//!   cell sits. The mesher already asks this.
//!
//! What it must *not* need from here is a world: this module knows about
//! levels, not about neighbours it has to go and look up. The lookups
//! belong to the simulation, which is the thing that owns the world.

use crate::types::{block_layers, is_liquid, BlockId, LAYERS_PER_BLOCK};

/// How far below the top of its cell a full cell of water is drawn.
///
/// A full cube of water and a full cube of stone read as the same solid
/// thing at foot level, and most of any shoreline is one cell deep -- so
/// standing *in* the shallows looked exactly like standing *on* them.
/// Dropping the surface a little is what makes the waterline visible.
///
/// Here rather than in the mesher because the flow simulation will want
/// the same number: a cell that is nearly full has to be drawn nearly
/// full, and "nearly full" has to mean the same thing on both sides.
pub const SURFACE_DROP: f32 = 0.12;

/// How much level moves between cells in one step of the flow
/// simulation, in eighths.
///
/// Two rather than one: water that spreads an eighth at a time takes
/// eight steps to empty a cell, and at twenty ticks a second that is a
/// visible crawl down a hillside. Two is a compromise between that and
/// the thing everyone regrets, which is water that fills a mine faster
/// than a player can run out of it.
pub const FLOW_PER_STEP: u8 = 2;

/// The shallowest level a cell keeps rather than drying up.
///
/// Below this a cell is emptied outright. Without a floor, spreading
/// water leaves an eighth of a level in every cell it ever touched --
/// a film of water over the whole map that never quite disappears and
/// costs a block update every time it is nudged.
pub const MINIMUM_LEVEL: u8 = 1;

/// How full a cell of water is, in eighths. Zero for anything that is
/// not water at all.
#[inline]
pub fn level(block: BlockId) -> u8 {
    if !is_liquid(block) {
        return 0;
    }
    block_layers(block)
}

/// Whether this cell holds water with room for more.
#[inline]
pub fn has_room(block: BlockId) -> bool {
    is_liquid(block) && level(block) < LAYERS_PER_BLOCK
}

/// How high the surface of this cell sits above its floor, in blocks.
///
/// **The same height for every cell of water there is**, less the
/// surface drop. A cell either holds water or it does not.
///
/// This used to return the cell's own level, so a half-full cell was
/// drawn and collided at half height. That is the physically honest
/// answer and it is the wrong one, for a reason that has nothing to do
/// with physics: **water has to look the same everywhere**. Break one
/// block under a lake and the cell that fills is, for a while and
/// sometimes for ever, at a different height from the water around it
/// -- a step, with a wall drawn down it, in the middle of a surface that
/// is otherwise flat. One cell out of place is the whole of what makes a
/// sea look broken, and a sea is the thing a player looks at longest.
///
/// So the level stops being something anybody can see. It is still the
/// number the flow simulation moves about -- it is what decides how far
/// water spreads and where it stops -- but it is now *internal to the
/// simulation*, and the mesher, the collider and the fog all get one
/// answer: water is water.
///
/// The cost is honest and small: a film spreading across a floor is
/// drawn full depth rather than shallow. What is bought is a surface
/// with no seams in it anywhere.
#[inline]
pub fn surface_height(block: BlockId) -> f32 {
    if is_liquid(block) {
        1.0 - SURFACE_DROP
    } else {
        0.0
    }
}

/// Does the water in this cell cover a point `height` above the cell's
/// own floor?
///
/// **The one question everything that is not the mesher asks about
/// water**, and the reason it is here rather than in three places. The
/// collider asks it of the feet, the waist and the eyes; the server asks
/// it of the eyes to decide whether a player is drowning; the fog asks
/// it to decide whether the view is under water. All three used to ask
/// `is_liquid` on the cell instead, which is the same answer only while
/// every cell of water is full: an ankle-deep film read as deep enough
/// to swim in, and -- the one a player would actually notice -- the
/// underwater fog came on a hand's breadth *above* the surface the
/// mesher had drawn, because a full cell stops short of the top (see
/// `SURFACE_DROP`) and nothing but the mesher knew it.
#[inline]
pub fn covers(block: BlockId, height: f32) -> bool {
    is_liquid(block) && height < surface_height(block)
}

/// How high the surface of this cell sits, given what is in the cell
/// directly above it.
///
/// **Water with water on top of it has no surface**, and the drop only
/// belongs to the cell where the air starts. The mesher has always known
/// this -- it lifts the top face of a submerged cell to 1.0, or every
/// layer of a deep lake would be drawn with a seam across it and a
/// half-full cell under a full one would open a slot through the middle
/// of a waterfall -- but it knew it privately, in a two-line `if` of its
/// own, and nothing else did.
///
/// So everything that asked `covers` about a point deep under water got
/// the shoreline answer: a band `SURFACE_DROP` thick at the top of every
/// submerged cell that read as *above the water*. Twelve per cent of all
/// heights, which is not a rare edge case -- it is one eye position in
/// eight, at any depth, anywhere in the ocean. A player standing on the
/// sea floor with their head in the band had their breath restored every
/// tick: the meter jittered, and drowning at depth was luck.
///
/// It lives here, next to `surface_height`, because that is the whole
/// bargain of this module: the mesher, the collider, the anti-cheat and
/// the drowning check must not each have their own idea of where the top
/// of the water is.
#[inline]
pub fn surface_height_with_above(block: BlockId, above: BlockId) -> f32 {
    if is_liquid(block) && is_liquid(above) {
        1.0
    } else {
        surface_height(block)
    }
}

/// `covers`, for callers that can see the cell above as well.
///
/// The one to reach for whenever the neighbour above is available:
/// `covers` is the shoreline case, and this is the same question asked
/// in a way that is also right underneath a surface. See
/// `surface_height_with_above`.
#[inline]
pub fn covers_with_above(block: BlockId, above: BlockId, height: f32) -> bool {
    is_liquid(block) && height < surface_height_with_above(block, above)
}

/// How much level may actually move sideways from a cell holding `here`
/// into a neighbour holding `there`, when the step would like to move
/// `wanted`.
///
/// Two limits, and each of them is a way a flow simulation fails to go
/// quiet:
///
/// * **Never more than fits.** The receiving cell has a ceiling, and
///   water that "moves" into a full cell has been created out of
///   nothing.
/// * **Never past halfway.** Move enough to make the neighbour the
///   deeper of the two and it hands the water straight back next step,
///   which is two cells trading the same eighth for ever -- a block
///   update per step, per pair, for the rest of the session. Stopping at
///   the midpoint means a difference of a single eighth is simply left
///   alone: the surface of a settled pool is flat to within one layer,
///   which is finer than it is drawn.
///
/// `settle` decides *whether* and *roughly how much*; this decides what
/// a particular pair of cells can do about it. Both are pure, and the
/// simulation that owns the world does the looking up.
#[inline]
pub fn sideways(here: u8, there: u8, wanted: u8) -> u8 {
    if here <= there {
        return 0;
    }
    let room = LAYERS_PER_BLOCK.saturating_sub(there);
    let halfway = (here - there) / 2;
    wanted.min(room).min(halfway)
}

/// Where a unit of water in a cell of `here` wants to go, given what is
/// under it and what is beside it.
///
/// The one rule of every flowing-water simulation, written down once:
/// **down before sideways, and sideways only into something lower.**
/// Returns how much level leaves this cell for the cell below and how
/// much for each lower neighbour beside it.
///
/// Pure on purpose. The expensive and error-prone half of a flow
/// simulation is deciding what a cell does; the cheap half is walking
/// the world to find its neighbours. Keeping the first half a function
/// of numbers means it can be tested exhaustively without a world, a
/// tick loop or a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flow {
    /// Level leaving for the cell below.
    pub down: u8,
    /// Level leaving for *each* neighbour listed as lower.
    pub sideways: u8,
}

/// How many flow steps a stranded cell keeps its water before losing an
/// eighth of it.
///
/// **Draining has to be much slower than flowing, and that is the whole
/// design of it.** A cell being filled from the sea is, for a moment
/// between one step and the next, indistinguishable from a cell that
/// nothing feeds: the water arrives in eighths, so a pit halfway through
/// filling is full of cells whose neighbours are no higher than they
/// are. Drain them at the speed water moves and filling a hole becomes
/// a tug of war that never settles -- cells drying out while the sea
/// pushes more in, for ever, with a block update every time.
///
/// Eight steps is two seconds at `FLOW_INTERVAL`, against two eighths a
/// step arriving. Anything with a source behind it wins that race by a
/// factor of sixteen and fills; anything with nothing behind it loses
/// every time and goes.
pub const DRY_AFTER_STEPS: u32 = 8;

/// Is this cell cut off from anything that could be feeding it?
///
/// **What water does when its source is taken away.** Break the dam of a
/// channel and the water in it used to stay exactly where it was, for
/// ever: a river bed full of standing water with nothing running into
/// it, which is the one thing water never does. The flow rules alone
/// cannot fix that, because they only ever *move* water -- a puddle
/// that has finished levelling is at equilibrium and equilibrium is
/// where they stop.
///
/// A cell is fed if water can reach it: from above, where it would fall
/// in, or from a neighbour standing higher than it, where it would run
/// down. A **full** cell is never stranded whatever is around it -- that
/// is what makes the sea the sea, and it is the same rule that stops
/// strict conservation turning the ocean into a wave pool (see
/// `logic::water`).
#[inline]
pub fn stranded(here: u8, fed_from_above: bool, highest_side: u8) -> bool {
    here > 0 && here < LAYERS_PER_BLOCK && !fed_from_above && highest_side <= here
}

/// `below_room` is how much the cell underneath can still take,
/// `lower_sides` how many neighbours beside this one are lower than it,
/// and `fed_from_above` whether water is falling into it from over its
/// head.
///
/// **Why a cell with something falling into it keeps a floor.** The
/// middle of a falling column receives `FLOW_PER_STEP` from above and
/// hands exactly `FLOW_PER_STEP` down, so without this the arithmetic
/// came out at nothing left -- and nothing left is not a thinner cell of
/// water, it is `BLOCK_AIR` (see `logic::water::water_at`). The cell
/// then emptied to air, the source above filled it again the same step,
/// and it did that four times a second for as long as the water fell:
/// a waterfall that visibly flickered, a swimmer in it dropped out of
/// `swimming` between frames, and -- the expensive part -- an *edit*
/// written into the world's overlay every step, so the autosave rewrote
/// the whole of `edits.bin` every interval while any water anywhere was
/// falling.
///
/// The floor is the same idea as `MINIMUM_LEVEL` in the sideways case
/// below and is here for the same reason: a cell that is *doing*
/// something must not be allowed to arrive at a state its own source
/// will immediately undo. What it costs is one eighth held back in each
/// cell of a column, which nobody can see -- every cell of water is
/// drawn at one height (see `surface_height`).
///
/// The rejected alternative was to let the cell empty and simply not
/// broadcast it. That fixes the flicker on screen and none of the rest:
/// the cell really is air on the server between the two halves of the
/// step, so a player swimming up a waterfall really does fall out of it,
/// and the overlay really does grow a row every step.
pub fn settle(here: u8, below_room: u8, lower_sides: u8, fed_from_above: bool) -> Flow {
    if here == 0 {
        return Flow { down: 0, sideways: 0 };
    }
    // Down first, and as much as fits: water does not spread across a
    // floor it could be falling through.
    let mut down = here.min(below_room).min(FLOW_PER_STEP);
    if fed_from_above {
        down = down.min(here.saturating_sub(MINIMUM_LEVEL));
    }
    let left = here - down;
    if down > 0 || left <= MINIMUM_LEVEL || lower_sides == 0 {
        return Flow { down, sideways: 0 };
    }
    // What is left, shared out -- never enough to empty this cell below
    // the level of the neighbours it is filling, which is what would
    // make two cells swap water back and forth for ever.
    let spare = left - MINIMUM_LEVEL;
    let sideways = (spare / lower_sides.max(1)).min(FLOW_PER_STEP);
    Flow { down, sideways }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{with_layers, BLOCK_STONE, BLOCK_WATER};

    #[test]
    fn a_cell_that_is_not_water_has_no_level() {
        assert_eq!(level(BLOCK_STONE), 0);
        assert_eq!(level(crate::types::BLOCK_AIR), 0);
        assert_eq!(surface_height(BLOCK_STONE), 0.0);
        assert!(!has_room(BLOCK_STONE));
    }

    #[test]
    fn a_full_cell_is_full_and_drawn_a_little_low() {
        assert_eq!(level(BLOCK_WATER), LAYERS_PER_BLOCK);
        assert!(!has_room(BLOCK_WATER));
        assert!((surface_height(BLOCK_WATER) - (1.0 - SURFACE_DROP)).abs() < 1e-6);
    }

    #[test]
    fn a_partly_filled_cell_keeps_its_level_and_hides_it() {
        // The level rides in the same field a layer count does, so this
        // is the same encoding -- and a save written before water could
        // flow still reads as full.
        //
        // **And it stops there.** The simulation reads the level; the
        // surface does not. Every cell of water is drawn and collided at
        // one height, so that a cell filling in after a player breaks a
        // block is not visibly a different cell from the water around
        // it. See `surface_height`.
        let full = surface_height(BLOCK_WATER);
        for n in 1..LAYERS_PER_BLOCK {
            let cell = with_layers(BLOCK_WATER, n);
            assert_eq!(level(cell), n, "level {n} did not survive the round trip");
            assert!(has_room(cell));
            assert_eq!(
                surface_height(cell),
                full,
                "level {n} shows through as a different surface height"
            );
        }
    }

    #[test]
    fn water_goes_down_before_it_goes_sideways() {
        // The one rule. A cell with somewhere to fall does not spread.
        let falling = settle(LAYERS_PER_BLOCK, LAYERS_PER_BLOCK, 4, false);
        assert!(falling.down > 0);
        assert_eq!(falling.sideways, 0, "it spread while it had room to fall");
    }

    #[test]
    fn water_on_a_floor_spreads_into_whatever_is_lower() {
        let spreading = settle(LAYERS_PER_BLOCK, 0, 2, false);
        assert_eq!(spreading.down, 0);
        assert!(spreading.sideways > 0);
        // ...and nowhere at all if nothing beside it is lower.
        assert_eq!(settle(LAYERS_PER_BLOCK, 0, 0, false).sideways, 0);
    }

    #[test]
    fn a_puddle_never_empties_itself_below_the_floor_it_keeps() {
        // Two cells trading the last eighth of water back and forth for
        // ever is the classic way a flow simulation never goes quiet --
        // and a simulation that never goes quiet is a block update per
        // tick for every puddle in the world.
        for here in 0..=LAYERS_PER_BLOCK {
            let flow = settle(here, 0, 4, false);
            let leaving = flow.sideways * 4;
            assert!(leaving <= here.saturating_sub(MINIMUM_LEVEL), "level {here} over-drained");
            if here <= MINIMUM_LEVEL {
                assert_eq!(flow.sideways, 0, "a puddle this shallow should sit still");
            }
        }
    }

    #[test]
    fn a_cell_covers_exactly_what_it_is_drawn_as() {
        // The collider and the fog read this; the mesher draws
        // `surface_height`. They have to be the same line, or the fog
        // comes on above the water.
        assert!(covers(BLOCK_WATER, 0.0));
        assert!(covers(BLOCK_WATER, 1.0 - SURFACE_DROP - 0.01));
        assert!(!covers(BLOCK_WATER, 1.0 - SURFACE_DROP + 0.01));
        assert!(!covers(BLOCK_STONE, 0.5), "stone is not something you swim in");

        // A cell that is only an eighth full is still a cell of water,
        // and is covered exactly as deeply as any other. That is the
        // point: what the collider walks into is what the mesher drew,
        // and neither of them can see a level.
        let barely = with_layers(BLOCK_WATER, 1);
        assert!(covers(barely, 0.05));
        assert!(covers(barely, 1.0 - SURFACE_DROP - 0.01));
        assert!(!covers(barely, 1.0 - SURFACE_DROP + 0.01));
    }

    #[test]
    fn a_cell_with_water_over_it_is_covered_all_the_way_to_the_top() {
        // **The band that let a submerged player breathe.** The drop
        // belongs to the cell where the air starts and to no other, so
        // the top `SURFACE_DROP` of a cell with more water above it is
        // under water like the rest of it. Without this, one eye height
        // in eight -- twelve per cent, at any depth -- read as "head
        // above water", and the meter refilled every time a player's
        // eyes drifted into the band.
        let deep = 1.0 - SURFACE_DROP + 0.01;
        assert!(!covers(BLOCK_WATER, deep), "this is the shoreline answer");
        assert!(covers_with_above(BLOCK_WATER, BLOCK_WATER, deep));
        assert!(covers_with_above(BLOCK_WATER, BLOCK_WATER, 0.999));

        // The very top of the cell is the floor of the next one up, and
        // belongs to it. Anything else and the two cells would overlap.
        assert!(!covers_with_above(BLOCK_WATER, BLOCK_WATER, 1.0));

        // With air above, it is `covers` exactly -- the surface is a
        // surface again.
        for height in [0.0f32, 0.5, 1.0 - SURFACE_DROP - 0.01, deep, 0.999] {
            assert_eq!(
                covers_with_above(BLOCK_WATER, crate::types::BLOCK_AIR, height),
                covers(BLOCK_WATER, height),
                "the top cell of a column stopped agreeing with `covers` at {height}"
            );
        }
        // ...and stone is not water whatever is stacked on it.
        assert!(!covers_with_above(BLOCK_STONE, BLOCK_WATER, 0.5));

        // A partly-filled cell under a full one is covered to the top
        // like any other: the level is invisible, and a seam through the
        // middle of a waterfall is exactly what this prevents.
        let barely = with_layers(BLOCK_WATER, 1);
        assert!(covers_with_above(barely, BLOCK_WATER, deep));
        assert_eq!(surface_height_with_above(barely, BLOCK_WATER), 1.0);
    }

    #[test]
    fn water_never_moves_sideways_past_the_midpoint() {
        // The property that stops two cells trading the same eighth for
        // ever: after the move, the giver is still at least as deep as
        // the receiver.
        for here in 0..=LAYERS_PER_BLOCK {
            for there in 0..=LAYERS_PER_BLOCK {
                for wanted in 0..=LAYERS_PER_BLOCK {
                    let moved = sideways(here, there, wanted);
                    assert!(moved <= wanted);
                    assert!(moved <= here, "gave away more than it had");
                    assert!(there + moved <= LAYERS_PER_BLOCK, "overfilled a cell");
                    // Uphill is not a move at all, so there is nothing
                    // to say about the pair afterwards.
                    if moved > 0 {
                        assert!(
                            here - moved >= there + moved,
                            "{here} into {there} inverted the two"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn water_still_runs_into_an_empty_neighbour() {
        // ...and the midpoint rule must not be so cautious that nothing
        // ever flows at all.
        assert!(sideways(LAYERS_PER_BLOCK, 0, FLOW_PER_STEP) > 0);
        assert_eq!(sideways(4, 4, FLOW_PER_STEP), 0, "a flat surface should sit still");
        assert_eq!(sideways(4, 5, FLOW_PER_STEP), 0, "water does not flow uphill");
    }

    #[test]
    fn water_with_nothing_behind_it_is_stranded() {
        // A puddle at rest: nothing above, nothing beside it standing
        // higher. Nothing is running into this, so it is going to go.
        assert!(stranded(4, false, 0));
        assert!(stranded(4, false, 4), "a neighbour at the same level feeds nothing");
        assert!(stranded(1, false, 1));
    }

    #[test]
    fn water_with_a_source_behind_it_stays() {
        // Fed from above -- something is falling into it.
        assert!(!stranded(4, true, 0));
        // Fed from the side: a neighbour standing higher runs downhill
        // into this cell, which is exactly what `settle` will do next
        // step.
        assert!(!stranded(4, false, 5));
        assert!(!stranded(4, false, LAYERS_PER_BLOCK));
    }

    #[test]
    fn the_sea_is_never_stranded() {
        // **The rule the ocean depends on.** A full cell is a source by
        // definition here and in `logic::water`; if drying could touch
        // one, the sea would evaporate from the top down whatever was
        // beside it.
        for side in 0..=LAYERS_PER_BLOCK {
            for above in [false, true] {
                assert!(
                    !stranded(LAYERS_PER_BLOCK, above, side),
                    "a full cell dried out with {side} beside it"
                );
            }
        }
        // ...and an empty cell has nothing to lose.
        assert!(!stranded(0, false, 0));
    }

    #[test]
    fn nothing_ever_moves_more_than_a_step_at_a_time() {
        for here in 0..=LAYERS_PER_BLOCK {
            for room in 0..=LAYERS_PER_BLOCK {
                for sides in 0..=4 {
                    for fed in [false, true] {
                        let flow = settle(here, room, sides, fed);
                        assert!(flow.down <= FLOW_PER_STEP);
                        assert!(flow.sideways <= FLOW_PER_STEP);
                        assert!(flow.down <= here, "a cell gave away more than it had");
                        assert!(flow.down <= room, "more went down than fits");
                    }
                }
            }
        }
    }

    #[test]
    fn a_cell_with_water_falling_into_it_never_empties_itself() {
        // **The waterfall bug, as arithmetic.** A cell in the middle of
        // a falling column takes `FLOW_PER_STEP` from above and passes
        // `FLOW_PER_STEP` down, and "nothing left" is not a thinner cell
        // of water: it is air. The cell went to air, the source refilled
        // it, and the pair did that four times a second for ever.
        for here in 1..=LAYERS_PER_BLOCK {
            for room in 0..=LAYERS_PER_BLOCK {
                let flow = settle(here, room, 4, true);
                assert!(
                    here - flow.down >= MINIMUM_LEVEL,
                    "level {here} with {room} below emptied itself while being filled"
                );
            }
        }
        // ...and the same cell with nothing over it is free to drain
        // completely, which is what lets the *bottom* of a column empty
        // out once the source above it is gone.
        assert_eq!(settle(FLOW_PER_STEP, LAYERS_PER_BLOCK, 0, false).down, FLOW_PER_STEP);
    }

    #[test]
    fn a_fed_cell_still_passes_water_on() {
        // The floor must not be so cautious that a waterfall stops being
        // a waterfall: the steady state of a column is "hold one eighth,
        // pass the rest down", not "hold everything".
        let flow = settle(MINIMUM_LEVEL + FLOW_PER_STEP, LAYERS_PER_BLOCK, 0, true);
        assert_eq!(flow.down, FLOW_PER_STEP, "the column stopped flowing");
    }
}
