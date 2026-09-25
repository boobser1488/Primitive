//! Cutting the base of a tree brings the tree down.
//!
//! ## Why this exists
//!
//! Chopping the bottom block out of a trunk used to leave the rest of it
//! standing in the air, and the canopy floating over that. Every player
//! who has ever seen it knows what it looks like, and it is the one
//! thing about trees that nothing in the world explains: a tuft of grass
//! falls when you dig the soil out from under it (see
//! `unsupported_run`), sand falls, a cactus comes down in one piece --
//! and a five-metre trunk hangs there.
//!
//! So it comes down. And because it *falls*, what is left is not the
//! trunk that was standing:
//!
//! - **It is every log it was.** A standing log is a log whether a player
//!   cuts it down piece by piece or brings the tree down from its foot, and
//!   a fall that handed back less taught the player to climb a tree and cut
//!   it from the top. It used to keep two thirds ([`felled_length`] was a
//!   rule of its own, "the rest shattered"), and a player who cut a big
//!   tree down reported what that came to: "с огромного дерева падает 3-4
//!   бревна". What does not fit on the ground beside the stump is dropped
//!   there as logs rather than lost.
//! - **It keeps its bark.** What lands is the wood that was standing --
//!   an oak lands as oak and a birch as birch. It used to land as
//!   `BLOCK_STRIPPED_LOG` on the argument that a trunk hitting the
//!   ground loses its bark, and that was a rule nobody asked for: it
//!   threw away which tree you had cut, and the block it left was
//!   softer than the deadfall the generator scatters, so felling a tree
//!   yourself gave you *different wood* from finding one already down.
//!   Stripping a log is a thing you do to it afterwards (there is a
//!   recipe), not a thing gravity does.
//! - **It lies down.** The logs are written along a horizontal axis, so
//!   they are deadfall: breakable by hand, exactly like the fallen
//!   trunks the generator scatters (see `blocks::BlockDef::felled`).
//!
//! ## A dead tree falls too
//!
//! `Biome::DeadForest` plants bare poles with no crown at all, and for a
//! while felling left them standing: chopping one out at the foot cleared
//! seven blocks and laid three, and "в мёртвом лесу деревья при рубке
//! пропадают" was answered by not felling a crownless trunk at all. That
//! answered the wrong half. The trunk vanished because the fall *kept two
//! thirds of it*, not because it fell; and a pole left standing on air over
//! the cut is the picture this module was written to remove, as the next
//! report said in so many words -- "в мёртвом лесу деревья вообще не
//! падают". Now that a fall lays every log it was, a dead tree comes down
//! exactly as a living one does, and all of it is on the ground beside the
//! stump where a player can see where it went.
//!
//! ## An old tree falls when its whole bole is cut
//!
//! An old tree's bole is two columns by two (`worldgen::place_old_tree`),
//! and its limbs are logs lying out of the bole in the air. Felled a column
//! at a time, each cut took its own column and a canopy box three wide, and
//! left three columns standing on nothing, the limbs hanging off them and
//! the far half of the crown in the air. So a bole is felled as one trunk:
//! **a cut into a bole with a column still standing on the ground is only a
//! cut** -- a tree a quarter cut through does not fall -- and the cut that
//! takes the last foot brings all four columns down, every limb lying off
//! them, and the crown out to the old tree's reach. See [`bole_at`].
//!
//! ## Which way it goes
//!
//! Not "the first free direction", which is what this used to answer and
//! which made every tree in an open field fall north. A tree goes over
//! the way a tree goes over, and [`fell`] scores the four directions in
//! this order:
//!
//! 1. **Room.** How much of the trunk fits before it hits something.
//!    Dominant, because a tree lies as straight as it can and the rest
//!    of it shatters.
//! 2. **Ground.** How many of those cells have something solid under
//!    them. A trunk that lies out over a pit or off a cliff edge is a
//!    trunk hanging in the air, and given the choice it goes the other
//!    way.
//! 3. **Lean.** Which side the crown is heavier on. This is the one that
//!    makes felling *readable*: the canopy is the part a player can see
//!    from the ground, so a tree that leans east and falls east is a
//!    tree they can stand clear of.
//! 4. **Whoever swung the axe.** Only as a tie-break, and deliberately
//!    last: a tree that always fell away from you would be a tree that
//!    can never hurt you, and it can -- see the crushing in
//!    `lib::fell_tree`. What this rule buys is that it never picks your
//!    cell when it had an equally good one to pick.
//!
//! Everything above it is a fact about the world, so the same tree in
//! the same place still falls the same way every time -- which is what
//! makes felling something a player can aim rather than something that
//! happens to them.
//!
//! ### A leaning palm goes the way it leans, before all four
//!
//! "падение пальм не зависит от их наклона". A palm leans towards the
//! lowest of its neighbours (`worldgen::palm_lean`), which on a beach is
//! the sea -- so the lane under its lean is the one lane whose sand falls
//! away from the stump's height, and **Ground** marked it down every time:
//! the trunk went along the shore, or inland, whichever way the crown was
//! not. The crown's own weight (**Lean**) was right and ranked third.
//!
//! So a palm whose course leans half a column or more
//! ([`palm_lean`], read off the same slices it is drawn and walked into as)
//! goes that way whatever the room, the ground or the feller say. What does
//! not fit shatters into spare timber, as any tree's does against a wall;
//! a leaning trunk that could be talked out of its lean by a stone would be
//! the arbitrariness this module removes. **And it lies down the beach**:
//! each log of a palm may sit one lower than the one before, where the sand
//! drops, so the trunk it leaned over the slope lies on the slope rather
//! than hanging a block over it -- and stops at the water, the rest spare.
//! Rejected: *weighing the canopy harder*, which still loses to Ground on
//! every beach; and *putting the lean after Room*, which on an open beach
//! decides nothing Room does not, and on a crowded one sends a palm
//! sideways through the gap for a reason nobody standing under it can see.
//! A straight palm, and every other tree, falls by the four as before.
//!
//! ## Why it is a pure function
//!
//! Everything interesting about felling is a *place*: a tree against a
//! cliff, a tree over a hole, a tree in a forest with no room to fall, a
//! tree at the roof of the world. Each of those is a line here and would
//! be a fixture apiece against a real world -- so [`fell`] takes the
//! world as a closure, exactly as `backpack_cell` does, and hands back a
//! plan rather than performing one.
//!
//! `look` answering `None` means "not loaded", and that is a refusal
//! rather than "empty": a block written into a chunk nobody has is a
//! block the world will regenerate over.
//!
//! ## Cost
//!
//! Bounded and paid once, by the player who swung the axe. A trunk is at
//! most [`MAX_TRUNK`] tall and the canopy search is a box around it, so
//! the worst case is a few hundred cell reads on the one tick somebody
//! finishes chopping -- against the thousands the same tick spends on
//! water and sand.

use std::collections::HashMap;

use primitive_shared::types::{
    block_kind, is_air, is_branch, is_collidable, is_liquid, oriented, Axis, BlockId,
    BLOCK_ACACIA_LEAVES, BLOCK_AIR, BLOCK_BIRCH_LEAVES, BLOCK_BIRCH_LOG, BLOCK_LEAVES, BLOCK_LOG,
    BLOCK_PALM_TRUNK,
    CHUNK_SIZE_Y,
};
use primitive_shared::worldgen::ACACIA_REACH;

/// The tallest trunk this will walk up.
///
/// Taller than anything the generator makes, and short enough that a
/// column of logs a player stacked by hand is felled rather than walked
/// for ever.
///
/// **Thirty-six, and it was twenty-four** while the tallest tree was an old
/// tree of sixteen. Trees are as tall as their kind at the Earth's scale
/// (`worldgen::Scale`): an old tree's bole reaches thirty, and a walk that
/// stopped at twenty-four left its top six logs and its whole crown standing
/// in the air over the stump.
pub const MAX_TRUNK: i32 = 36;

/// How far from the trunk the canopy is cleared.
///
/// The generator's own `MAX_CANOPY_RADIUS`. Leaves outside that belong
/// to a different tree and are not this one's to take.
pub const CANOPY_RADIUS: i32 = 3;

/// ...and how far above the last log a canopy may reach.
pub const CANOPY_ABOVE: i32 = 3;
/// What felling one tree does to the world.
///
/// Returned rather than performed, so the caller owns the broadcasting,
/// the drops and the order -- and so the whole decision can be checked
/// without a server. Every cell in `cleared` becomes air; every
/// `(cell, block)` in `laid` is written as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Felled {
    /// Trunk and canopy cells that are now empty.
    pub cleared: Vec<(i32, i32, i32)>,
    /// The trunk on the ground, already oriented.
    pub laid: Vec<((i32, i32, i32), BlockId)>,
    /// Logs there was no room to lay. The caller drops these as items --
    /// **a tree that falls into a wall still gives you its wood.**
    pub spare: u32,
    /// What wood it was, for those drops. Zero when nothing fell.
    ///
    /// Here rather than worked out again by the caller, because the
    /// caller would have to ask the world what *used* to be standing at
    /// a cell this plan has already emptied.
    pub wood: BlockId,
    /// How many cells of leaves came down with it, so the caller can
    /// decide what they yield.
    pub leaves: u32,
    /// How many twigs came down with it -- the thin pieces of a tree of
    /// branches, which are sticks rather than timber. See
    /// [`sticks_from_twigs`].
    pub twigs: u32,
    /// The wild hives that were stuck to this trunk, and what was in each
    /// (`bees::honey_in`). Their cells are in `cleared` with the rest of the
    /// tree; this is what they held, so the caller can pay for them.
    ///
    /// **Listed apart from the leaves and the twigs because a hive is not a
    /// yield, it is a raid.** A tree that came down with a hive on it has
    /// put a player's hand in the comb whether they meant it or not, and
    /// the caller stings them for it (`lib::sting_from_bees`). Without that
    /// the axe would be the way to take honey without paying the bees, and
    /// the whole of what a hive costs is the bees (`bees`).
    pub hives: Vec<((i32, i32, i32), BlockId)>,
}

impl Felled {
    pub fn is_empty(&self) -> bool {
        self.cleared.is_empty() && self.laid.is_empty() && self.spare == 0
    }
}

/// Is this a standing trunk -- a log with its axis upright?
///
/// The axis is what tells a tree from deadfall everywhere else in the
/// game (see `types::break_seconds_with`), and it is what tells one here
/// too: a log lying on the ground is already fallen and felling it again
/// would be a way to turn one tree into two.
///
/// Any wood's log (`wood::is_log`): a fir and a saxaul are felled as an oak
/// is.
#[inline]
pub fn is_standing_trunk(block: BlockId) -> bool {
    primitive_shared::wood::is_log(block) && primitive_shared::types::block_axis(block) == Axis::Y
}

/// Is this the canopy that belongs to a given wood?
///
/// A predicate rather than the leaf itself, because **one wood can wear
/// more than one canopy**: an apple tree is an ordinary broadleaf trunk
/// with apple leaves on it (see `types::BLOCK_APPLE_LEAVES`), and a
/// function returning a single id would fell an orchard tree and leave
/// its crown hanging in the air.
fn is_canopy_of(wood: BlockId, block: BlockId) -> bool {
    match block_kind(wood) {
        BLOCK_BIRCH_LOG => block_kind(block) == BLOCK_BIRCH_LEAVES,
        // A fir's needles and a saxaul's twigs: one crown each.
        primitive_shared::types::BLOCK_FIR_LOG => block_kind(block) == primitive_shared::types::BLOCK_FIR_NEEDLES,
        primitive_shared::types::BLOCK_SAXAUL_LOG => block_kind(block) == primitive_shared::types::BLOCK_SAXAUL_LEAVES,
        primitive_shared::types::BLOCK_PINE_LOG => block_kind(block) == primitive_shared::types::BLOCK_PINE_NEEDLES,
        primitive_shared::types::BLOCK_WILLOW_LOG => block_kind(block) == primitive_shared::types::BLOCK_WILLOW_LEAVES,
        _ => matches!(
            block_kind(block),
            BLOCK_LEAVES
                | primitive_shared::types::BLOCK_APPLE_LEAVES
                | primitive_shared::types::BLOCK_APPLE_LEAVES_FRUIT
                // ...and an acacia, which is an oak's timber under a
                // savanna leaf. See `types::BLOCK_ACACIA_LEAVES`.
                | BLOCK_ACACIA_LEAVES
                // ...and a maple, an oak's timber under a red crown.
                | primitive_shared::types::BLOCK_MAPLE_LEAVES
                // ...and a palm's crown, fruiting or not, and the moss
                // hanging under a swamp tree's: a palm's pieces are
                // timber as an oak's are (`fell_branches`), and moss left
                // under a felled crown is moss hanging from nothing.
                | primitive_shared::types::BLOCK_PALM_FRONDS
                | primitive_shared::types::BLOCK_PALM_COCONUTS
                | primitive_shared::types::BLOCK_HANGING_MOSS
        ),
    }
}

// ---- savanna ----

/// How far above the top of a straight stem an acacia's crown can start.
///
/// The tallest savanna trunk is seven, the stem leaves the vertical two
/// or more blocks up, and the plate sits on the last log -- see
/// `worldgen::place_acacia`. So the plate's lowest leaf is at most six
/// above the bend.
const CROWN_ABOVE_THE_BEND: i32 = 6;

/// The logs of this wood standing one block above `cell`: straight up, or
/// one column across -- the step a crooked stem takes.
fn limbs_above(
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
    (x, y, z): (i32, i32, i32),
    wood: BlockId,
) -> Vec<(i32, i32, i32)> {
    let mut limbs = Vec::new();
    for dz in -1..=1 {
        for dx in -1..=1 {
            let at = (x + dx, y + 1, z + dz);
            let same_wood = |block: BlockId| {
                is_standing_trunk(block) && block_kind(block) == block_kind(wood)
            };
            if look(at.0, at.1, at.2).is_some_and(same_wood) {
                limbs.push(at);
            }
        }
    }
    limbs
}

/// Is there an acacia's crown over this stem, within the reach of one?
fn acacia_crown_over(
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
    (x, y, z): (i32, i32, i32),
) -> bool {
    (1..=CROWN_ABOVE_THE_BEND).any(|dy| {
        (-ACACIA_REACH..=ACACIA_REACH).any(|dz| {
            (-ACACIA_REACH..=ACACIA_REACH).any(|dx| {
                look(x + dx, y + dy, z + dz)
                    .is_some_and(|block| block_kind(block) == BLOCK_ACACIA_LEAVES)
            })
        })
    })
}

/// How many logs a fall of `pieces` cells of timber gives: all of them.
///
/// **It was two thirds**, on the argument that a tree is a taper and what
/// you carry away is the straight part. Nobody standing at the stump could
/// see the argument; what they saw was that cutting a tree down gave a third
/// less than cutting the same tree apart from a ladder, which makes felling
/// the wrong way to take a tree -- a chore with one right answer, and the
/// wrong one is the one the axe suggests. Kept as a function so the rule is
/// stated in one place and its test says it.
pub fn felled_length(pieces: i32) -> i32 {
    pieces.max(0)
}

/// The four ways a tree can go over, in a fixed order.
///
/// Fixed rather than random: the same tree in the same place falls the
/// same way every time, which is what makes felling something a player
/// can aim rather than something that happens to them. The order runs
/// clockwise from north, and it is the **last** thing consulted -- see
/// the module note for the three that come first.
const DIRECTIONS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// How one direction scored, ranked in the order the fields are written.
///
/// A tuple would do the same comparison and say none of it. The order of
/// the fields **is** the rule, and `derive(Ord)` on a struct compares
/// them top to bottom -- so the rule is stated once, here, rather than
/// spelt out again in a comparison function that can drift from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Lane {
    /// This is the way a leaning palm leans. Above everything, and false
    /// for every lane of every other tree -- see the module note.
    leaning: bool,
    /// How much of the trunk fits. Dominant among the rest.
    room: usize,
    /// How many of those cells have something solid under them.
    supported: usize,
    /// How heavy the crown is on this side.
    lean: u32,
    /// False if the feller is standing in the way. Ranked as a boolean
    /// rather than a distance: this is a tie-break, not a repulsion.
    clear_of_feller: bool,
}

/// Works out what happens when the base of a tree at `stump` is cut.
///
/// `stump` is the cell the player broke -- it is already gone by the
/// time this runs, and this plans what happens to everything *above*
/// it. Returns an empty plan for anything that is not the bottom of a
/// standing trunk, so the caller can call it unconditionally.
///
/// `feller` is the cell whoever swung the axe is standing in, if
/// anybody did. It is the last of the four things that decide which way
/// the tree goes -- see the module note -- and `None` (a plugin, a mod,
/// a command) simply leaves that tie-break out.
///
/// **Whatever the biome.** This took a `has_canopy` flag, false in the
/// dead forest, and a crownless trunk was not felled at all -- see the
/// module note for why that answered the wrong half of the report.
pub fn fell(
    stump: (i32, i32, i32),
    look: impl Fn(i32, i32, i32) -> Option<BlockId>,
    feller: Option<(i32, i32)>,
) -> Felled {
    // **A tree of branches comes down by what holds it up**, not by a walk
    // up its trunk -- see `fell_branches`. Asked first: a piece with
    // nothing under it falls in a dead wood as surely as in a live one.
    if let Some(plan) = fell_branches(stump, &look, feller) {
        return plan;
    }
    let mut plan = Felled::default();
    let (x, y, z) = stump;

    // **A hive comes down with the log it was stuck to even when no tree
    // does.** The cut that takes the top log of a trunk, or one column of a
    // bole the rest of which still stands, fells nothing -- and the comb on
    // that log was still left hanging on a cell that is now air. Written at
    // each way out rather than once at the end, because three of the ways
    // out are returns and the last one has to come *after* the lane is
    // chosen (see `take_the_hives`).
    let hive_on_the_cut = |plan: &mut Felled| take_the_hives(plan, std::iter::once(&stump), &look);

    // What is standing on the cut. One log above the cut is a tree; none
    // is a single block somebody placed, and felling that would be a
    // block that turns into a different block for no reason.
    let Some(wood) = look(x, y + 1, z).filter(|&b| is_standing_trunk(b)) else {
        hive_on_the_cut(&mut plan);
        return plan;
    };

    // ---- an old tree's bole ----
    //
    // The columns this trunk is one of: itself alone, or the four of an old
    // tree. A column of them still standing on the ground holds the tree
    // up, and this cut is only the log it took.
    let bole = bole_at(&look, stump, wood);
    let old = bole.len() > 1;
    if bole
        .iter()
        .any(|&(bx, bz)| (bx, bz) != (x, z) && stands_on_the_ground(&look, (bx, y, bz), wood))
    {
        hive_on_the_cut(&mut plan);
        return plan;
    }

    let mut trunk: Vec<(i32, i32, i32)> = Vec::new();
    // The tallest column's run, which is how long the tree lies.
    let mut tallest = 0usize;
    for &(bx, bz) in &bole {
        let before = trunk.len();
        for step in 1..=MAX_TRUNK {
            let at = (bx, y + step, bz);
            if at.1 >= CHUNK_SIZE_Y as i32 {
                break;
            }
            match look(at.0, at.1, at.2) {
                Some(block) if is_standing_trunk(block) && block_kind(block) == block_kind(wood) => trunk.push(at),
                // `None` is an unloaded chunk: stop rather than guess, and
                // fell what is certainly there. A tree that straddles the
                // edge of what is loaded is a tree that comes down in two
                // goes, which is strange once and harmless.
                _ => break,
            }
        }
        tallest = tallest.max(trunk.len() - before);
    }
    if trunk.is_empty() {
        hive_on_the_cut(&mut plan);
        return plan;
    }

    // ---- a crooked trunk ----
    //
    // An acacia's stem leaves the vertical two or three blocks up and
    // carries on across a corner (`worldgen::place_acacia`), so the walk
    // above stops at the bend with the whole crown still standing on the
    // limbs. From there it is followed one block up and at most one
    // column across at a time, which is exactly the step the generator
    // takes.
    //
    // **Only where an acacia crown is over it**, and that condition is
    // the whole safety of it. A gable a player stacks out of logs steps
    // up across corners just as a limb does, and felling the end of a
    // house by its bottom log is the one thing this module must never
    // do; the crown is what tells a tree from a wall. Rejected: following
    // every corner step in every tree, which is simpler and takes the
    // gable; and a timber of its own for the acacia, which would say
    // "crooked tree" in the block itself -- for the price of a third
    // wood, a third plank and a third stack in every chest.
    //
    // The limbs are looked for before the crown, because the limbs are
    // nine reads and the crown is hundreds: an oak's top has leaves
    // across its corners rather than logs, so no oak pays for the scan.
    let mut reach = CANOPY_RADIUS;
    let bend = trunk[trunk.len() - 1];
    if !old && !limbs_above(&look, bend, wood).is_empty() && acacia_crown_over(&look, bend) {
        reach = ACACIA_REACH;
        let mut frontier = vec![bend];
        while let Some(cell) = frontier.pop() {
            for limb in limbs_above(&look, cell, wood) {
                let within = (limb.0 - x).abs() <= ACACIA_REACH
                    && (limb.2 - z).abs() <= ACACIA_REACH
                    && limb.1 - y <= MAX_TRUNK
                    && limb.1 < CHUNK_SIZE_Y as i32;
                if within && !trunk.contains(&limb) {
                    trunk.push(limb);
                    frontier.push(limb);
                }
            }
        }
    }

    // ---- an old tree's limbs ----
    //
    // Logs lying in the air off the bole, followed face to face out of it.
    // Only for a bole: nothing else the generator grows has a lying log up
    // a tree, and a player's lean-to against a trunk is lying logs too.
    let limbs = if old { limbs_of_bole(&look, stump, &trunk, wood) } else { Vec::new() };
    if old {
        reach = primitive_shared::worldgen::OLD_TREE_REACH + 1;
    }

    // ---- the canopy ----
    //
    // Cleared with the trunk, because leaves with nothing under them are
    // the other half of the bug this fixes. Only *this* wood's leaves
    // and only within the radius the generator plants them in, so a tree
    // felled in a wood does not strip its neighbours -- the acacia's
    // reach for an acacia, whose plate hangs beside its root rather than
    // over it.
    let top = trunk.iter().map(|c| c.1).max().unwrap_or(y);
    for ly in (y + 1)..=(top + CANOPY_ABOVE).min(CHUNK_SIZE_Y as i32 - 1) {
        for lz in -reach..=reach {
            for lx in -reach..=reach {
                let at = (x + lx, ly, z + lz);
                if look(at.0, at.1, at.2).is_some_and(|block| is_canopy_of(wood, block)) {
                    plan.cleared.push(at);
                    plan.leaves += 1;
                }
            }
        }
    }
    plan.cleared.extend(trunk.iter().copied());
    plan.cleared.extend(limbs.iter().copied());

    // ---- where it lands ----
    //
    // As long as it stood, and every other log of it -- a bole's other
    // columns, its limbs -- beside the stump as spare. A bole laid out
    // four logs wide would be a wall across the wood.
    let total = felled_length((trunk.len() + limbs.len()) as i32) as u32;
    let length = if old { tallest } else { trunk.len() };
    plan.wood = block_kind(wood);
    lay_timber(&mut plan, stump, &look, feller, length, total, None);
    // **The hives on it come down with it** -- *after* the lane is chosen,
    // for two reasons: `lay_timber` weighs the crown by counting `cleared`
    // (`lean_towards`), and a comb counted as a leaf would tip a tree a cell
    // the wrong way; and the hive is still standing in the world while the
    // lane is measured, so a comb at the stump's own height would block the
    // fall as the trunk it hangs on does. See [`take_the_hives`].
    take_the_hives(&mut plan, trunk.iter().chain(&limbs), &look);
    // ...and the cut itself, which is not in the trunk: the walk starts a
    // cell above it. A hive hung on the very log the axe took is the tree's
    // as much as one two cells higher.
    hive_on_the_cut(&mut plan);
    plan
}

/// The wild hives stuck to the cells of a trunk that is coming down, added
/// to the plan.
///
/// **A hive is not held up by anything** (`blocks`, "Hung on a trunk, not
/// stood on anything": there is air under a hive, and that is where the bees
/// go in), so nothing in the world took it down when the tree under it went
/// -- and felling an oak with a hive on it left the comb hanging five cells
/// up in an empty clearing, still filling on the growth clock and still
/// stinging whoever reached it. Nothing said what tree it had been on.
///
/// Rejected: **making a hive propped, so the general support pass takes it.**
/// That pass looks *down*, and a hive's support is the wall beside it; giving
/// it `types::support_at`'s second direction would mean the generator, the
/// placing rules and the collapse pass all learning a third kind of support
/// for one block that is never placed by hand. The tree that carried the hive
/// is the one thing that knows it was there, and it is here.
///
/// Rejected too: **leaving the comb and clearing nothing**, on the argument
/// that a hive is worth more standing. A player who fells the tree has felled
/// the hive; a comb floating over a stump is the picture this whole module
/// exists to remove.
fn take_the_hives<'a>(
    plan: &mut Felled,
    wood: impl Iterator<Item = &'a (i32, i32, i32)>,
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
) {
    for &(tx, ty, tz) in wood {
        for (dx, dz) in DIRECTIONS {
            let at = (tx + dx, ty, tz + dz);
            let Some(hive) = look(at.0, at.1, at.2).filter(|&b| primitive_shared::bees::is_hive(b)) else {
                continue;
            };
            // **Only a hive stuck to *this* cell.** The side is in the id
            // (`types::hive_side`, the direction toward the bark), so a comb
            // on a neighbouring tree whose cell happens to touch this trunk
            // is left where it is -- felling one tree must not rob the tree
            // beside it.
            if primitive_shared::types::hive_side(hive).step() != (-dx, -dz) {
                continue;
            }
            if plan.hives.iter().any(|&(cell, _)| cell == at) {
                continue;
            }
            plan.cleared.push(at);
            plan.hives.push((at, hive));
        }
    }
}

/// The columns of the trunk standing on the cut at `(x, y, z)`: the four of
/// an old tree's bole if this is one, or the cut's own column.
///
/// **A bole is a square of four columns all standing a log over the cut's
/// height**, and nothing else is. Two trees rooted side by side are not one
/// (the generator's spacing puts two trunks on neighbouring columns now and
/// then, and felling one must not wait for the other), and a square of four
/// is what `worldgen::place_old_tree` draws and nothing else does.
fn bole_at(
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
    (x, y, z): (i32, i32, i32),
    wood: BlockId,
) -> Vec<(i32, i32)> {
    let trunk_at = |cx: i32, cz: i32| {
        look(cx, y + 1, cz).is_some_and(|b| is_standing_trunk(b) && block_kind(b) == block_kind(wood))
    };
    for (ox, oz) in [(0, 0), (-1, 0), (0, -1), (-1, -1)] {
        let square = [(x + ox, z + oz), (x + ox + 1, z + oz), (x + ox, z + oz + 1), (x + ox + 1, z + oz + 1)];
        if square.iter().all(|&(cx, cz)| trunk_at(cx, cz)) {
            return square.to_vec();
        }
    }
    vec![(x, z)]
}

/// Does the trunk of this wood at `cell` run down, unbroken, to something
/// solid that is not more of it -- is this column still on its feet?
///
/// Asked of a bole's other columns at the cut's height. A column whose foot
/// was cut already, at this height or below it, is standing on air and does
/// not hold the tree.
fn stands_on_the_ground(
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
    (x, y, z): (i32, i32, i32),
    wood: BlockId,
) -> bool {
    let is_this_wood = |b: BlockId| is_standing_trunk(b) && block_kind(b) == block_kind(wood);
    for down in 0..=MAX_TRUNK {
        match look(x, y - down, z) {
            Some(b) if is_this_wood(b) => {}
            // Not loaded: a refusal, as everywhere here -- it holds.
            None => return true,
            Some(b) => return down > 0 && is_collidable(b),
        }
    }
    true
}

/// The most cells of limb one bole's walk follows. An old tree has three or
/// four limbs of two or three logs (`worldgen::place_old_tree`).
const MOST_LIMBS: usize = 32;

/// An old tree's limbs: the logs of its wood lying in the air, joined face
/// to face to its trunk or to each other, no further from the stump than an
/// old tree reaches.
///
/// **Not a log lying on something solid**, which is deadfall and not this
/// tree's -- or a log a player laid on a wall. A limb has air or leaves under
/// it.
fn limbs_of_bole(
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
    (x, y, z): (i32, i32, i32),
    trunk: &[(i32, i32, i32)],
    wood: BlockId,
) -> Vec<(i32, i32, i32)> {
    let reach = primitive_shared::worldgen::OLD_TREE_REACH + 1;
    let lying = |b: BlockId| {
        block_kind(b) == block_kind(wood) && primitive_shared::types::block_axis(b) != Axis::Y
    };
    let mut limbs: Vec<(i32, i32, i32)> = Vec::new();
    let mut frontier: Vec<(i32, i32, i32)> = trunk.to_vec();
    while let Some((cx, cy, cz)) = frontier.pop() {
        for (dx, dy, dz) in FACES {
            let near = (cx + dx, cy + dy, cz + dz);
            if (near.0 - x).abs() > reach || (near.2 - z).abs() > reach || near.1 <= y + 1 {
                continue;
            }
            if limbs.contains(&near) || trunk.contains(&near) {
                continue;
            }
            if !look(near.0, near.1, near.2).is_some_and(lying) {
                continue;
            }
            let resting = look(near.0, near.1 - 1, near.2)
                .is_some_and(|under| is_collidable(under) && !lying(under));
            if resting {
                continue;
            }
            limbs.push(near);
            if limbs.len() >= MOST_LIMBS {
                return limbs;
            }
            frontier.push(near);
        }
    }
    limbs
}

/// Lays `length` logs of `plan.wood` along the best of the four lanes out
/// of the stump, and counts the rest of `total` -- what there was no room
/// for, and every log of the tree past its length -- as spare.
///
/// Split out of `fell` so a tree of log cubes and a tree of branches
/// land by the same four rules -- see the module note -- rather than by two
/// copies of them that drift. `heading` is a leaning palm's lean
/// ([`palm_lean`]): that lane wins, and the palm's logs follow the sand down.
fn lay_timber(
    plan: &mut Felled,
    (x, y, z): (i32, i32, i32),
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
    feller: Option<(i32, i32)>,
    length: usize,
    total: u32,
    heading: Option<(i32, i32)>,
) {
    let wood = plan.wood;
    let timber_along = |axis: Axis| oriented(wood, axis);
    let palm = heading.is_some();

    // Every direction is scored and the best one wins -- see `Lane` for
    // what "best" is and the module note for why. Scored rather than
    // short-circuited on the first that fits, because "it fits" is the
    // least interesting thing about where a tree lands.
    // The cells a lane's logs would take, stump outwards.
    type Cells = Vec<(i32, i32, i32)>;
    let mut best: Option<(Lane, (i32, i32), Cells)> = None;
    for &(dx, dz) in &DIRECTIONS {
        let mut cells: Vec<(i32, i32, i32)> = Vec::with_capacity(length);
        let mut supported = 0usize;
        let mut height = y;
        for step in 1..=length {
            let (cx, cz) = (x + dx * step as i32, z + dz * step as i32);
            // **A palm's log may lie one lower than the one before**, where
            // the cell it would take and the one under it are both open: a
            // palm leans over the fall of a beach, and its lane is that fall.
            // One a step, so a trunk lies along a slope and never pours down
            // a cliff face.
            if palm && look(cx, height, cz).is_some_and(is_air) && look(cx, height - 1, cz).is_some_and(is_air) {
                height -= 1;
            }
            // The cell the trunk would occupy has to be empty, and for every
            // other tree it is measured at the stump's own height: a felled
            // tree lies on the ground it was rooted in, not on whatever
            // slope is next to it.
            match look(cx, height, cz) {
                Some(block) if is_air(block) => {}
                _ => break,
            }
            // ...and what is under it decides whether it is lying on
            // anything. Water counts as nothing: a trunk floating over a
            // pond is the same picture as one floating over a hole -- and a
            // palm, which never loses its lane to that, stops at the water
            // rather than laying a jetty out over the sea.
            match look(cx, height - 1, cz) {
                Some(under) if !is_air(under) && !is_liquid(under) => supported += 1,
                Some(under) if palm && is_liquid(under) => break,
                _ => {}
            }
            cells.push((cx, height, cz));
        }
        let lane = Lane {
            leaning: heading == Some((dx, dz)),
            room: cells.len(),
            supported,
            lean: lean_towards(&plan.cleared, (x, z), (dx, dz)),
            clear_of_feller: feller != Some((x + dx, z + dz)),
        };
        if best.as_ref().is_none_or(|(current, _, _)| lane > *current) {
            best = Some((lane, (dx, dz), cells));
        }
    }
    let Some((_, (dx, dz), cells)) = best else {
        plan.spare = total;
        return;
    };
    let room = cells.len();
    // The axis the logs lie along, which is the one they are travelling
    // down. `Axis::of_normal` is the same mapping the mesher uses for a
    // log a player places against a face, so a felled trunk and a placed
    // one are drawn the same way round.
    let axis = Axis::of_normal(dx, 0, dz).unwrap_or(Axis::X);
    for cell in cells {
        plan.laid.push((cell, timber_along(axis)));
    }
    // ...and the stump's own cell takes the butt of it, if it is free.
    // That is what makes a felled tree read as having come *from* here.
    if room < length && look(x, y, z).is_some_and(is_air) {
        plan.laid.push(((x, y, z), timber_along(axis)));
    }
    plan.spare = total.max(length as u32).saturating_sub(plan.laid.len() as u32);
}

// ---- trees of branches ----

/// The six cells sharing a face with one.
const FACES: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];

/// The most pieces of branch one walk will follow before it gives up and
/// calls what it is walking held.
///
/// A grown tree of branches is under fifty pieces (`worldgen::branches`).
/// The bound is not for one tree: crowns that touch make a wood one piece of
/// wood to a walk, and one cut must not walk a forest -- nor fell one.
const MOST_PIECES: usize = 256;

/// What cutting a cell does to the pieces of branch beside it, or `None` if
/// no piece of branch falls.
///
/// **Whatever no longer stands on anything comes down.** Each piece next to
/// the cut is walked through the pieces it joins; a walk that reaches one
/// standing on solid ground (not another piece, not a leaf) is held, and
/// one that does not is falling. That single rule is every case there is:
///
/// * the foot of a trunk -- the whole tree falls, and its crown with it;
/// * a limb cut through -- the limb beyond the cut falls and the tree stands;
/// * a sapling's foot, by hand -- the stem falls, as sticks;
/// * a twig off the end of a limb -- only the tip past it.
///
/// **Rejected: the trunk walk `fell` does for logs.** It walks up a
/// column from the cut and clears a box of canopy, which knows nothing of a
/// limb reaching sideways out of the box, and nothing of a cut *into* a
/// limb -- the part beyond it would hang in the air, which is the bug this
/// module exists to remove. Pieces of branch are not placeable, so every
/// one was grown and none is a player's wall; the danger the acacia's crown
/// check guards against does not arise.
///
/// What falls: every piece is cleared; the leaves within a column of a
/// falling piece, from one under it to two over (the clumps sit on the
/// tips); and when a grown tree falls from its foot -- a bough among the
/// pieces, and the piece over the cut among them -- the canopy box
/// `fell` clears for a log tree as well. Boughs are timber and land
/// by `lay_timber` from a cut on the ground, or as spare from a cut in the
/// air, where there is no ground at the stump's height to lie along. Twigs
/// are counted for `sticks_from_twigs`.
fn fell_branches(
    stump: (i32, i32, i32),
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
    feller: Option<(i32, i32)>,
) -> Option<Felled> {
    let (x, y, z) = stump;
    let starts: Vec<(i32, i32, i32)> = FACES
        .iter()
        .map(|&(dx, dy, dz)| (x + dx, y + dy, z + dz))
        .filter(|&(nx, ny, nz)| look(nx, ny, nz).is_some_and(is_branch))
        .collect();
    if starts.is_empty() {
        return None;
    }
    // Which walk reached each piece first. A walk that runs into another
    // walk's piece has joined a piece of wood that walk stopped in -- a walk
    // that finishes has visited all of its wood, so the other one stopped,
    // and it stopped because it was held.
    let mut owner: HashMap<(i32, i32, i32), usize> = HashMap::new();
    let mut falling: Vec<(i32, i32, i32)> = Vec::new();
    for (walk, start) in starts.into_iter().enumerate() {
        if owner.contains_key(&start) {
            continue;
        }
        owner.insert(start, walk);
        let mut pieces = vec![start];
        let mut frontier = vec![start];
        let mut held = false;
        'walk: while let Some((cx, cy, cz)) = frontier.pop() {
            match look(cx, cy - 1, cz) {
                Some(under) if !is_branch(under) && is_collidable(under) => {
                    held = true;
                    break;
                }
                // Not loaded: a refusal, as everywhere in this module.
                None => {
                    held = true;
                    break;
                }
                _ => {}
            }
            for (dx, dy, dz) in FACES {
                let near = (cx + dx, cy + dy, cz + dz);
                if near == stump {
                    continue;
                }
                match look(near.0, near.1, near.2) {
                    Some(block) if is_branch(block) => match owner.get(&near) {
                        Some(&other) if other != walk => {
                            held = true;
                            break 'walk;
                        }
                        Some(_) => {}
                        None => {
                            owner.insert(near, walk);
                            pieces.push(near);
                            frontier.push(near);
                            if pieces.len() > MOST_PIECES {
                                held = true;
                                break 'walk;
                            }
                        }
                    },
                    None => {
                        held = true;
                        break 'walk;
                    }
                    _ => {}
                }
            }
        }
        if !held {
            falling.extend(pieces);
        }
    }
    if falling.is_empty() {
        return None;
    }

    let mut plan = Felled::default();
    let boughs = falling
        .iter()
        .filter(|&&(fx, fy, fz)| {
            look(fx, fy, fz).is_some_and(|b| {
                primitive_shared::types::is_bough(b) || block_kind(b) == BLOCK_PALM_TRUNK
            })
        })
        .count();
    // **A palm's crown reaches further than a column from its trunk**: its
    // fronds run out `worldgen::PALM_FROND_REACH` from the top piece, which
    // leans off the root, so neither the column round each piece nor the
    // box round the stump holds all of them. Asked of the pieces that fall,
    // so a palm felled beside another does not strip its neighbour.
    let palm = falling
        .iter()
        .any(|&(fx, fy, fz)| look(fx, fy, fz).is_some_and(|b| block_kind(b) == BLOCK_PALM_TRUNK));
    let around = if palm { primitive_shared::worldgen::PALM_FROND_REACH } else { 1 };
    // ...and which way it leans, asked before the pieces are sorted into the
    // plan, while the world still holds every one of them.
    let heading = if palm { palm_lean(&falling, stump, look) } else { None };
    // An acacia's plate, followed whole below. Asked of the leaf sitting on a
    // falling piece, which is where `branches::acacia_branch_cells` lays it.
    let acacia = falling
        .iter()
        .any(|&(fx, fy, fz)| look(fx, fy + 1, fz).is_some_and(|b| block_kind(b) == BLOCK_ACACIA_LEAVES));
    plan.twigs = (falling.len() - boughs) as u32;
    // **A birch's pieces are birch**, in bark and in timber
    // (`types::birch_branch`): what lands is birch logs, and what is cleared
    // with it is the birch's own leaves. Asked before the leaves are
    // gathered, because an oak's canopy rule would leave a felled birch's
    // whole crown hanging in the air.
    // ...and so is every other bark's (`types::piece_log`): a willow's limbs
    // land as willow and take the willow's leaves with them.
    let wood = falling
        .iter()
        .find_map(|&(fx, fy, fz)| look(fx, fy, fz).and_then(primitive_shared::types::piece_log).filter(|&log| log != BLOCK_LOG))
        .unwrap_or(BLOCK_LOG);

    // Was the cut itself on the ground? Asked here rather than beside the
    // timber it used to serve, because the canopy now turns on it as
    // well -- see `from_the_foot` below.
    let on_the_ground =
        look(x, y - 1, z).is_some_and(|under| !is_branch(under) && is_collidable(under));

    let mut leaves: Vec<(i32, i32, i32)> = Vec::new();
    let mut take = |at: (i32, i32, i32)| {
        if look(at.0, at.1, at.2).is_some_and(|b| is_canopy_of(wood, b)) && !leaves.contains(&at) {
            leaves.push(at);
        }
    };
    for &(fx, fy, fz) in &falling {
        for ly in fy - 1..=fy + 2 {
            for lz in -around..=around {
                for lx in -around..=around {
                    take((fx + lx, ly, fz + lz));
                }
            }
        }
    }
    // **Whether the whole tree came down, rather than a limb off one.**
    //
    // This asked `boughs > 0`, and that was the bug in "ты когда
    // ломаешься маленькое дерево снизу... листва остаётся, снизу она
    // пропадает". A bough is eight sixteenths of wood and up
    // (`types::branch`), and a young tree is thinner than that
    // everywhere: `branches::young_cells(5, 6, ..)` -- stage 1, the five
    // block stem a sapling grows into -- tapers from six to four, so
    // every piece of it is a twig and the count was zero. The box was
    // therefore skipped and the only leaves taken were the ones within a
    // column of a falling piece, which is the stem and whichever two or
    // three sides a limb happens to point at. The rest of the crown --
    // the far side of the mass over the top, two columns out where no
    // limb reached -- stayed in the air over a hole, and the near side
    // vanished. Two halves of one crown behaving differently is exactly
    // what the player described, and it is worse than either half alone.
    //
    // The honest question is not "is there timber in it" but "did this
    // cut take the tree off its feet", and that is `on_the_ground`: the
    // cell that was cut was standing on something solid that is not more
    // wood, and the piece straight above it is falling. A limb cut
    // through never satisfies it -- a limb grows only over air
    // (`branches::young_cells`'s `over_air`, two clear blocks under every
    // piece), so the cell under a limb piece is not ground.
    //
    // Rejected: *widening the column round each piece to the canopy's
    // reach*. It takes the same leaves for a tree felled at the foot and
    // also strips a neighbour's crown when one twig is snapped off a
    // limb, which is the mistake `a_neighbouring_tree_keeps_its_leaves`
    // exists to catch. The box is bounded by the *stump*, so it can only
    // ever reach as far as the tree that stood on it.
    let from_the_foot = on_the_ground && falling.contains(&(x, y + 1, z));
    if from_the_foot {
        // **And the box is the tree's own reach, not the log broadleaf's.**
        // `CANOPY_RADIUS` is `worldgen::MAX_CANOPY_RADIUS`, three, which is
        // how far the canopy of a tree built out of log cubes goes. A tree
        // of branches reaches `BRANCH_REACH` -- five -- because its limbs
        // run three columns out and each carries a mass of leaves two more
        // (`branches::branch_tree_cells`), and five is the number the
        // generator walks its border at. Felled through the narrower box,
        // every grown broadleaf in the world left the outer two columns of
        // each limb's clump hanging over the hole where it had been: the
        // same picture as the young tree above, at the other end of the
        // tree's life, and the reason the count of leaves it reported was
        // short as well.
        //
        // Bounded by the **stump**, so it can never walk: the worst it can
        // reach is exactly as far as a tree rooted here can plant. A
        // neighbour whose crown hangs within five columns of this root
        // loses the part that does, and that is the lenient mistake to
        // make of the two -- a crown the axe trimmed grows back
        // (`logic::growth`), a crown left floating never does.
        let reach = primitive_shared::worldgen::BRANCH_REACH;
        let top = falling.iter().map(|c| c.1).max().unwrap_or(y);
        for ly in (y + 1)..=(top + CANOPY_ABOVE).min(CHUNK_SIZE_Y as i32 - 1) {
            for lz in -reach..=reach {
                for lx in -reach..=reach {
                    take((x + lx, ly, z + lz));
                }
            }
        }
    }
    // **An acacia's plate is taken whole.** It lies on the tip of its limb as
    // wide as the savanna's canopy allows (`branches::acacia_branch_cells`,
    // up to `ACACIA_REACH - 1` round the tip), and the stem is too thin to
    // have a bough among its pieces, so neither the column round each piece
    // nor the canopy box above reaches its rim: felled, a branch acacia left
    // the rim of its plate hanging over the savanna. Nobody saw it while the
    // ordinary world grew its acacias out of logs; the real-acacia felling
    // test did the day it grew them out of pieces. A wider column was tried
    // first and still left a rim -- the plate's radius is the biome's, not a
    // constant here. So the leaves are followed from the one on a falling
    // piece, face to face, no further from the root than an acacia reaches: a
    // flood of this tree's plate, which a neighbour's meets only if they touch.
    if acacia {
        let reach = primitive_shared::worldgen::ACACIA_REACH;
        let mut frontier: Vec<(i32, i32, i32)> = falling.iter().map(|&(fx, fy, fz)| (fx, fy + 1, fz)).collect();
        let mut seen: std::collections::HashSet<(i32, i32, i32)> = std::collections::HashSet::new();
        while let Some(cell) = frontier.pop() {
            if (cell.0 - x).abs() > reach || (cell.2 - z).abs() > reach || !seen.insert(cell) {
                continue;
            }
            if !look(cell.0, cell.1, cell.2).is_some_and(|b| block_kind(b) == BLOCK_ACACIA_LEAVES) {
                continue;
            }
            if !leaves.contains(&cell) {
                leaves.push(cell);
            }
            for (dx, dy, dz) in FACES {
                frontier.push((cell.0 + dx, cell.1 + dy, cell.2 + dz));
            }
        }
    }
    // Sorted, so the same cut sends the same changes in the same order.
    leaves.sort_unstable();
    falling.sort_unstable();
    plan.leaves = leaves.len() as u32;
    plan.cleared = leaves;
    // Copied rather than moved: the pieces are read again below, to find the
    // hives that were stuck to them.
    plan.cleared.extend(falling.iter().copied());

    // An oak's timber, or a birch's: the pieces wear its bark.
    plan.wood = wood;
    let length = felled_length(boughs as i32) as usize;
    if length > 0 {
        if on_the_ground {
            lay_timber(&mut plan, stump, look, feller, length, length as u32, heading);
        } else {
            plan.spare = length as u32;
        }
    }
    // ...and the hives, last, for the reason the log path takes them last:
    // the lane is already chosen. **A tree of branches carries them too** --
    // `worldgen::is_trunk`, which is what `place_hives` asks of the column
    // it hangs one on, says yes to a bough as well as to a log -- so a hive
    // left out of this path is a comb over a cleared crown in every wood the
    // generator grows out of pieces.
    take_the_hives(&mut plan, falling.iter().chain(std::iter::once(&stump)), look);
    Some(plan)
}

/// How far a palm's course has to lean off its stump, in columns, before
/// the lean decides where it falls.
///
/// **Half a column**: a crown over the stump's own column has no side to go
/// to, and the four rules pick one as they pick for an oak. Every palm the
/// generator grows leans one or two columns (`worldgen::palm_cells` steps
/// once or twice), so none of them is near the line.
const PALM_LEANING: f32 = 0.5;

/// **Which way a felled palm leans**: the side it goes over to, or `None`
/// for one that stands straight.
///
/// Read off the course its bark follows -- `palm::trunk_slices`, the same
/// slices it was drawn and walked into as -- from the middle of the stump's
/// column to where the course comes out of the top of its highest piece.
/// Direction and size both: the size is what tells a lean from a straight
/// trunk (`PALM_LEANING`), and the larger of its two parts is the side.
///
/// Rejected: *the column of the top piece alone*. It is the same answer for
/// every palm the generator grows today, and the wrong one for a trunk whose
/// last step comes back -- the course is what a player sees leaning, and a
/// number read from somewhere else is a second opinion that can drift.
fn palm_lean(
    falling: &[(i32, i32, i32)],
    (x, _, z): (i32, i32, i32),
    look: &impl Fn(i32, i32, i32) -> Option<BlockId>,
) -> Option<(i32, i32)> {
    let &(tx, ty, tz) = falling
        .iter()
        .filter(|&&(fx, fy, fz)| look(fx, fy, fz).is_some_and(|b| block_kind(b) == BLOCK_PALM_TRUNK))
        .max_by_key(|&&(fx, fy, fz)| (fy, fx, fz))?;
    let top = look(tx, ty, tz)?;
    let near = |dx: i32, dy: i32, dz: i32| look(tx + dx, ty + dy, tz + dz).unwrap_or(BLOCK_AIR);
    let [ox, oz] = primitive_shared::palm::trunk_slices(top, near).last()?.offset;
    let (lx, lz) = ((tx - x) as f32 + ox, (tz - z) as f32 + oz);
    if lx.hypot(lz) < PALM_LEANING {
        return None;
    }
    Some(if lx.abs() >= lz.abs() { (lx.signum() as i32, 0) } else { (0, lz.signum() as i32) })
}

/// What the twigs of a felled tree of branches leave behind: one stick for
/// every two, rounded up.
///
/// **Rounded up so a sapling is always worth pulling.** The twig a hand
/// breaks gives its own stick (`blocks`, the twig's `drop`); the one or two
/// above it that fall with it give one more. More than one a twig and a
/// grown tree's forty pieces would be a pack of sticks for one cut -- the
/// mess `sticks_from` refuses for leaves.
pub fn sticks_from_twigs(twigs: u32) -> u32 {
    twigs.div_ceil(2)
}

/// How heavy the crown is on one side, in cells.
///
/// Counted off the canopy this fall is *already* taking down, which is
/// the only honest measure of the tree's own lean: leaves outside the
/// radius belong to a neighbour and leaves of another wood belong to
/// another tree, and both were filtered out when the plan was built.
///
/// The test is the sign of the dot product, so a cell counts for the two
/// directions it is between rather than only for the nearer axis. A
/// crown hanging to the north-east should make *both* north and east
/// better than south, and it does.
fn lean_towards(canopy: &[(i32, i32, i32)], (x, z): (i32, i32), (dx, dz): (i32, i32)) -> u32 {
    canopy
        .iter()
        .filter(|(cx, _, cz)| (cx - x) * dx + (cz - z) * dz > 0)
        .count() as u32
}

/// What a cleared canopy leaves behind, given how many cells came down.
///
/// One stick per handful of leaves, which is what a fallen crown is
/// actually good for and is the same thing pulling leaves apart by hand
/// gives. Deliberately not one per cell: a five-metre tree is a couple
/// of hundred cells of canopy, and a fistful of a hundred sticks is not
/// a reward, it is a mess to sort out.
pub fn sticks_from(leaves: u32) -> u32 {
    leaves / 16
}

// ---- a palm's crown ----

/// A frond, fruiting or picked, or a cluster of coconuts: what a palm's crown
/// is made of. The picked frond is the frond's kind with a variant, so the
/// kind answers for both.
fn is_palm_crown(block: BlockId) -> bool {
    matches!(
        block_kind(block),
        primitive_shared::types::BLOCK_PALM_FRONDS | primitive_shared::types::BLOCK_PALM_COCONUTS
    )
}

/// The most cells of crown one walk follows before it calls what it walked
/// held. A crown is under thirty cells (`worldgen::palm_cells`); the bound is
/// for a grove whose crowns touch, which one broken frond must not walk.
const MOST_CROWN: usize = 128;

/// **The cells of a palm's crown that nothing holds up any more**, after the
/// cell at `broken` was emptied -- the crown that has to come down.
///
/// "пальмовые листья не осыпаются после уничтожения основы листа". A crown is
/// held by its **heart**: the cell of it sitting on the top piece of the trunk,
/// out of which the star of fronds and the coconuts grow. Felling handles a
/// trunk cut below the top (`fell_branches` takes the crown with the pieces),
/// and nothing handled the other two ways a crown loses its footing: the heart
/// itself broken, or the top piece under it -- which has a piece under it and
/// so is not a fall to `fell_branches`. Either way the fronds and the
/// coconuts stayed in the air round a hole, which is the picture felling was
/// written to remove from every other tree.
///
/// The rule: the crown cells touching the broken cell are walked through the
/// crown cells they touch, **across edges and corners too** -- a frond's tip
/// hangs one lower than the arm it droops from, and meets it only at an edge.
/// A walk that reaches a cell sitting on a palm's trunk is held; so is one
/// that reaches ground nobody has loaded (a refusal, as everywhere in this
/// module), runs further from the break than two frond-reaches, or runs into
/// cells another walk already stopped in. Everything a free walk visited
/// falls.
///
/// Three rules were weighed:
///
/// * *A box of fronds round the heart, cleared when the heart goes.* Right for
///   the palm the generator grows today, and wrong the day its star changes
///   shape -- and a frond that drooped one row lower than the box would hang.
/// * *Anything within a frond's reach of a trunk stays.* Breaking the heart
///   would then leave the whole star standing on a trunk that is still there,
///   which is exactly the report.
/// * **A walk to a heart (chosen)**, the shape `fell_branches` already takes
///   for a tree of branches. It asks the crown what it is joined to rather than
///   where it is expected to be. A neighbour's crown that touches this one holds
///   both, which is the lenient mistake: a frond left up is a frond a player
///   can still break, a neighbour's crown stripped is a tree ruined.
pub fn unheld_palm_crown(
    broken: (i32, i32, i32),
    look: impl Fn(i32, i32, i32) -> Option<BlockId>,
) -> Vec<(i32, i32, i32)> {
    let (bx, by, bz) = broken;
    let across = 2 * primitive_shared::worldgen::PALM_FROND_REACH;
    let upright = primitive_shared::worldgen::PALM_FROND_REACH;
    let around = || {
        (-1..=1).flat_map(|dy| (-1..=1).flat_map(move |dz| (-1..=1).map(move |dx| (dx, dy, dz))))
    };
    let mut owner: HashMap<(i32, i32, i32), usize> = HashMap::new();
    let mut falling: Vec<(i32, i32, i32)> = Vec::new();
    for (walk, (dx, dy, dz)) in around().enumerate() {
        let start = (bx + dx, by + dy, bz + dz);
        if start == broken || owner.contains_key(&start) || !look(start.0, start.1, start.2).is_some_and(is_palm_crown) {
            continue;
        }
        owner.insert(start, walk);
        let mut crown = vec![start];
        let mut frontier = vec![start];
        let mut held = false;
        'walk: while let Some((cx, cy, cz)) = frontier.pop() {
            match look(cx, cy - 1, cz) {
                Some(under) if block_kind(under) == BLOCK_PALM_TRUNK => {
                    held = true;
                    break;
                }
                None => {
                    held = true;
                    break;
                }
                _ => {}
            }
            for (nx, ny, nz) in around() {
                let near = (cx + nx, cy + ny, cz + nz);
                if near == broken {
                    continue;
                }
                match look(near.0, near.1, near.2) {
                    Some(block) if is_palm_crown(block) => match owner.get(&near) {
                        Some(&other) if other != walk => {
                            held = true;
                            break 'walk;
                        }
                        Some(_) => {}
                        None => {
                            let far = (near.0 - bx).abs() > across
                                || (near.2 - bz).abs() > across
                                || (near.1 - by).abs() > upright;
                            if far || crown.len() >= MOST_CROWN {
                                held = true;
                                break 'walk;
                            }
                            owner.insert(near, walk);
                            crown.push(near);
                            frontier.push(near);
                        }
                    },
                    None => {
                        held = true;
                        break 'walk;
                    }
                    _ => {}
                }
            }
        }
        if !held {
            falling.extend(crown);
        }
    }
    falling.sort_unstable();
    falling
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{block_axis, BLOCK_AIR, BLOCK_STONE};

    /// **A hive is part of the tree it grew on, and a felled tree takes it.**
    ///
    /// Nothing holds a hive up (`blocks`, `propped: false`), so felling the
    /// oak under one left the comb hanging in the air over an empty stump --
    /// still filling, still stinging, attached to a trunk that was on the
    /// ground beside it. `take_the_hives` is what brings it down, and what
    /// the plan hands back is the honey that was in it so the caller can
    /// drop it and charge the bees for it.
    #[test]
    fn a_felled_tree_takes_the_hive_off_its_own_trunk_and_leaves_the_neighbours_alone() {
        use primitive_shared::bees::{hive_holding, honey_in, HIVE_FULL};
        use primitive_shared::types::{hive_against, Facing};

        // Two trees a few cells apart, each with a hive on the side facing
        // the other, and the near one is cut at the foot.
        let g = 20;
        let world = Wood::new(g)
            .tree(0, 0, 6)
            .tree(4, 0, 6)
            // On the east wall of the near trunk: the comb is in the cell at
            // +x, and its side points back west at the bark.
            .put((1, g + 5, 0), hive_against(hive_holding(HIVE_FULL), Facing::West))
            // ...and on the west wall of the far one, in the cell at 3.
            .put((3, g + 5, 0), hive_against(hive_holding(1), Facing::East))
            .put((0, g + 1, 0), BLOCK_AIR);
        let plan = fell((0, g + 1, 0), world.look(), None);

        assert!(
            plan.cleared.contains(&(1, g + 5, 0)),
            "the hive on the felled trunk was left hanging in the air: {:?}",
            plan.hives
        );
        assert_eq!(plan.hives.len(), 1, "a fall took a hive that was not on its trunk: {:?}", plan.hives);
        assert_eq!(plan.hives[0].0, (1, g + 5, 0));
        assert_eq!(honey_in(plan.hives[0].1), HIVE_FULL, "the fall forgot what was in the comb");
        assert!(
            !plan.cleared.contains(&(3, g + 5, 0)),
            "felling one tree robbed the hive on the tree beside it"
        );
    }

    /// A tree with nothing on it still reports no hives, and the comb's cell
    /// is never counted as crown: the lane is chosen before the hives are
    /// taken, so a hive cannot tip a tree over (see `fell`).
    #[test]
    fn a_hive_on_a_trunk_does_not_change_which_way_the_tree_goes() {
        use primitive_shared::bees::{hive_holding, HIVE_FULL};
        use primitive_shared::types::{hive_against, Facing};

        let g = 20;
        let bare = Wood::new(g).tree(0, 0, 6).put((0, g + 1, 0), BLOCK_AIR);
        let plain = fell((0, g + 1, 0), bare.look(), None);
        assert!(plain.hives.is_empty(), "a tree with no hive on it reported one");

        let hived = Wood::new(g)
            .tree(0, 0, 6)
            .put((1, g + 5, 0), hive_against(hive_holding(HIVE_FULL), Facing::West))
            .put((0, g + 1, 0), BLOCK_AIR);
        let with = fell((0, g + 1, 0), hived.look(), None);
        assert_eq!(
            direction_of(&with, (0, 0)),
            direction_of(&plain, (0, 0)),
            "the comb on the trunk changed the way the tree fell"
        );
        assert_eq!(with.laid, plain.laid, "the comb on the trunk moved the logs");
    }

    #[test]
    fn a_dead_tree_with_no_crown_falls_and_lays_every_log_it_was() {
        // A bare trunk exactly as `Biome::DeadForest` plants one: no leaf
        // anywhere over it. It used to be left standing on air over the cut
        // ("в мёртвом лесу деревья вообще не падают"), and before that it
        // fell and kept a third of itself ("пропадают"). Both halves: it
        // comes down, and nothing of it goes missing.
        let mut world = Wood::new(20);
        for n in 1..=7 {
            world = world.put((0, 20 + n, 0), BLOCK_LOG);
        }
        world = world.put((0, 21, 0), BLOCK_AIR); // the log a player just cut
        let plan = cut(&world, 0, 0);
        for n in 2..=7 {
            assert!(plan.cleared.contains(&(0, 20 + n, 0)), "the dead trunk's log at y={} was left in the air", 20 + n);
        }
        assert_eq!(plan.leaves, 0, "a bare trunk brought down leaves it never had");
        assert_eq!(plan.laid.len() as u32 + plan.spare, 6, "six logs stood over the cut and the fall gave back another number");
        assert!(plan.laid.iter().all(|&((_, y, _), b)| y == 21 && block_axis(b) != Axis::Y), "the dead trunk did not lie down");
    }

    /// The same claim against the real generator: a `Biome::DeadForest`
    /// chunk, a real standing trunk's foot found in it, cut, and every log
    /// over the cut accounted for as laid or spare.
    #[test]
    fn a_real_dead_forest_tree_comes_down_whole_from_its_foot() {
        use primitive_shared::types::{ChunkPos, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};
        use primitive_shared::worldgen::{Biome, WorldGen};

        let gen = WorldGen::new(12345);
        let mut checked = 0;
        // **Wide and sparse, and stopping once there is enough**: a dead
        // wood is a rare burnt patch now (`worldgen::WorldGen::burnt`), a
        // clearing every several kilometres rather than a sixth of the land,
        // and a square of two kilometres round the origin may hold none.
        'search: for cx in (-360..360).step_by(3) {
            for cz in (-360..360).step_by(3) {
                if checked > 20 {
                    break 'search;
                }
                let (gx, gz) = (cx * CHUNK_SIZE_X as i32 + 8, cz * CHUNK_SIZE_Z as i32 + 8);
                if gen.biome_at(gx, gz) != Biome::DeadForest {
                    continue;
                }
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for lx in 3..CHUNK_SIZE_X - 3 {
                    for lz in 3..CHUNK_SIZE_Z - 3 {
                        let col_gx = cx * CHUNK_SIZE_X as i32 + lx as i32;
                        let col_gz = cz * CHUNK_SIZE_Z as i32 + lz as i32;
                        if gen.biome_at(col_gx, col_gz) != Biome::DeadForest {
                            continue;
                        }
                        for ly in 1..(CHUNK_SIZE_Y - 2) {
                            let (here, above) = (chunk.get(lx, ly, lz), chunk.get(lx, ly + 1, lz));
                            if !is_standing_trunk(here) || !is_standing_trunk(above) || is_standing_trunk(chunk.get(lx, ly - 1, lz)) {
                                continue;
                            }
                            let stump = (lx as i32, ly as i32, lz as i32);
                            let look = |x: i32, y: i32, z: i32| {
                                if x < 0 || z < 0 || x >= CHUNK_SIZE_X as i32 || z >= CHUNK_SIZE_Z as i32 || y < 0 || y >= CHUNK_SIZE_Y as i32 {
                                    return None;
                                }
                                if (x, y, z) == stump {
                                    return Some(BLOCK_AIR); // already cut
                                }
                                Some(chunk.get(x as usize, y as usize, z as usize))
                            };
                            let over: Vec<(i32, i32, i32)> = (ly + 1..CHUNK_SIZE_Y)
                                .take_while(|&y| is_standing_trunk(chunk.get(lx, y, lz)))
                                .map(|y| (lx as i32, y as i32, lz as i32))
                                .collect();
                            let plan = fell(stump, look, None);
                            for log in &over {
                                assert!(
                                    plan.cleared.contains(log),
                                    "a real dead-forest trunk at {stump:?} in chunk {:?} left its log at {log:?} standing on air",
                                    (cx, cz)
                                );
                            }
                            assert_eq!(
                                plan.laid.len() + plan.spare as usize,
                                over.len(),
                                "a real dead-forest trunk at {stump:?} of {} logs over the cut gave back another number",
                                over.len()
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 5, "found too few dead-forest trunks to trust the result ({checked})");
    }

    // ---- savanna ----

    #[test]
    fn a_real_acacia_comes_down_whole_and_leaves_nothing_in_the_air() {
        // The crooked stem from the generator rather than typed in: the
        // shape standing in the world today is a fact about the generator,
        // and a hand-built acacia would go on passing after the real one
        // changed. It is `branches::place_branch_acacia` now -- the ordinary
        // world grows its acacias out of pieces -- so a root is a log or a
        // piece of branch, and what has to come down is either.
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_DIRT, BLOCK_GRASS, CHUNK_SIZE_X};
        use primitive_shared::worldgen::{Biome, Preset, WorldGen, Zone};

        // **In the tropics**, where open savanna is the country: at real
        // scale a temperate world has none within a day's walk
        // (`worldgen::Zone`), and the dry belt's is broken up by desert, so
        // lone acacias there are too few to trust a count.
        let gen = WorldGen::with_zone(12345, Preset::Normal, Zone::Tropics);
        let side = CHUNK_SIZE_X as i32;
        let reach = ACACIA_REACH;
        let (mut places, mut checked) = (0, 0);
        // **Along the transect, as the generator's own tests search.**
        // Climate lies by latitude, and there is no savanna within a
        // kilometre of spawn: the first version of this looked in a
        // square round the origin, found no savanna at all, and said so
        // as "too few lone acacias (0)" -- a sentence about felling,
        // failing about a map. See `worldgen::tests::chunks_in`.
        'search: for cz in (-360..360).step_by(4) {
            for cx in (-160..160).step_by(5) {
                if places == 40 {
                    break 'search;
                }
                if gen.biome_at(cx * side + 8, cz * side + 8) != Biome::Savanna {
                    continue;
                }
                places += 1;
                // Three chunks by three, so a tree near the edge of the
                // middle one is all there -- and so is any neighbour close
                // enough to have leaves in its box.
                let mut near: Vec<Chunk> = Vec::new();
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        near.push(gen.generate_chunk(ChunkPos::new(cx + dx, cz + dz)));
                    }
                }
                let at = |x: i32, y: i32, z: i32| -> Option<BlockId> {
                    if !(0..3 * side).contains(&x)
                        || !(0..3 * side).contains(&z)
                        || !(0..CHUNK_SIZE_Y as i32).contains(&y)
                    {
                        return None;
                    }
                    let chunk = &near[((z / side) * 3 + x / side) as usize];
                    Some(chunk.get((x % side) as usize, y as usize, (z % side) as usize))
                };
                let soil = |x: i32, y: i32, z: i32| {
                    at(x, y, z).is_some_and(|b| {
                        matches!(
                            block_kind(b),
                            BLOCK_GRASS | BLOCK_DIRT | primitive_shared::types::BLOCK_SANDY_SOIL | primitive_shared::types::BLOCK_DRY_TURF
                        )
                    })
                };
                // **A trunk of pieces stands on soil, or on its own flare in
                // it.** Where the generator's slope wanted a lip under a
                // trunk it put the trunk's log in that cell instead
                // (`worldgen::lips`), so the lowest piece stands on a log and
                // the log on the ground. Asked for soil directly under the
                // wood, this missed every tree rooted in a lip -- and the
                // crowding check below then called a grove a lone acacia and
                // blamed felling for the neighbour's leaves. A log-cube trunk
                // needs no arm of its own: its flare is one more log of it,
                // and that log stands on the ground.
                let ground_under = |x: i32, y: i32, z: i32| {
                    at(x, y, z).is_some_and(|b| {
                        !is_standing_trunk(b) && !primitive_shared::types::is_branch(b) && primitive_shared::types::is_collidable(b)
                    })
                };
                let rooted = |x: i32, y: i32, z: i32| {
                    at(x, y, z).is_some_and(|b| is_standing_trunk(b) || primitive_shared::types::is_branch(b))
                        && (soil(x, y - 1, z)
                            || (at(x, y, z).is_some_and(primitive_shared::types::is_branch)
                                && at(x, y - 1, z).is_some_and(is_standing_trunk)
                                && ground_under(x, y - 2, z)))
                };
                // Roots anywhere their whole neighbourhood is inside the
                // three chunks, not only in the middle one: at the
                // savanna's spacing a lone acacia is about one tree in
                // six, and the middle chunk holds about one tree.
                for lz in 2 * reach..3 * side - 2 * reach {
                    for lx in 2 * reach..3 * side - 2 * reach {
                        let Some(ly) = (1..CHUNK_SIZE_Y as i32).find(|&ly| rooted(lx, ly, lz))
                        else {
                            continue;
                        };
                        let world = ((cx - 1) * side + lx, (cz - 1) * side + lz);
                        if gen.biome_at(world.0, world.1) != Biome::Savanna {
                            continue;
                        }
                        // **Alone**: no other root within two reaches, so
                        // every leaf in this tree's box is this tree's.
                        //
                        // **At any height whose crown could reach the box,
                        // not four blocks either way.** On a hillside a
                        // neighbour rooted five up the slope hangs its
                        // plate straight into this tree's box and passed
                        // for "alone"; felling rightly left its leaves,
                        // and the test called that a leaf left in the air.
                        // It surfaced when the savanna grew into warmer,
                        // hillier country and its trees gathered into
                        // groves -- a fact about the sample, not about
                        // felling.
                        let crowded = (-2 * reach..=2 * reach).any(|dz| {
                            (-2 * reach..=2 * reach).any(|dx| {
                                (dx, dz) != (0, 0)
                                    && (ly - MAX_TRUNK..=ly + MAX_TRUNK)
                                        .any(|y| rooted(lx + dx, y, lz + dz))
                            })
                        });
                        if crowded {
                            continue;
                        }
                        let stump = (lx, ly, lz);
                        let look = |x: i32, y: i32, z: i32| {
                            if (x, y, z) == stump {
                                Some(BLOCK_AIR) // already cut
                            } else {
                                at(x, y, z)
                            }
                        };
                        let plan = fell(stump, look, None);
                        for dy in 1..=MAX_TRUNK {
                            for dz in -reach..=reach {
                                for dx in -reach..=reach {
                                    let cell = (lx + dx, ly + dy, lz + dz);
                                    let Some(block) = at(cell.0, cell.1, cell.2) else {
                                        continue;
                                    };
                                    if is_standing_trunk(block)
                                        || primitive_shared::types::is_branch(block)
                                        || block_kind(block) == BLOCK_ACACIA_LEAVES
                                    {
                                        assert!(
                                            plan.cleared.contains(&cell),
                                            "felling the acacia rooted at {world:?} left \
                                             block {} standing at {cell:?}",
                                            block_kind(block)
                                        );
                                    }
                                }
                            }
                        }
                        checked += 1;
                    }
                }
            }
        }
        assert!(places > 0, "no savanna anywhere along the transect for seed 12345");
        assert!(checked >= 3, "found too few lone acacias to trust the result ({checked})");
    }

    #[test]
    fn a_crooked_trunk_is_followed_only_where_an_acacia_crown_says_it_is_a_tree() {
        // A gable end a player stacked out of logs: a column, then two
        // steps up across corners. That is the exact shape of an acacia's
        // limb, and felling one column of a house by its bottom log must
        // not take the steps above it.
        let mut gable = Wood::new(20);
        for y in 21..=23 {
            gable = gable.put((0, y, 0), BLOCK_LOG);
        }
        gable = gable
            .put((1, 24, 0), BLOCK_LOG)
            .put((2, 25, 0), BLOCK_LOG)
            .put((0, 21, 0), BLOCK_AIR);
        let plan = cut(&gable, 0, 0);
        assert!(plan.cleared.contains(&(0, 23, 0)), "the straight column stopped coming down");
        assert!(
            !plan.cleared.contains(&(1, 24, 0)),
            "felling one column of a stepped gable took the step above it"
        );

        // ...and the same logs with an acacia's plate on the end are a
        // tree, and come down whole.
        let acacia = gable
            .put((2, 26, 0), BLOCK_ACACIA_LEAVES)
            .put((3, 26, 0), BLOCK_ACACIA_LEAVES)
            .put((5, 26, 1), BLOCK_ACACIA_LEAVES);
        let plan = cut(&acacia, 0, 0);
        for cell in [(1, 24, 0), (2, 25, 0), (2, 26, 0), (3, 26, 0), (5, 26, 1)] {
            assert!(
                plan.cleared.contains(&cell),
                "the acacia's limb or plate at {cell:?} was left in the air"
            );
        }
    }

    /// A world of air over stone at `ground`, with whatever is put into
    /// it on top.
    struct Wood {
        ground: i32,
        cells: std::collections::HashMap<(i32, i32, i32), BlockId>,
    }

    impl Wood {
        fn new(ground: i32) -> Self {
            Self {
                ground,
                cells: std::collections::HashMap::new(),
            }
        }

        /// A trunk of `height` standing on the ground at (x, z), with a
        /// canopy over it.
        fn tree(mut self, x: i32, z: i32, height: i32) -> Self {
            for n in 0..height {
                self.cells.insert((x, self.ground + 1 + n, z), BLOCK_LOG);
            }
            let top = self.ground + height;
            for ly in (top - 1)..=(top + 1) {
                for lz in -2..=2i32 {
                    for lx in -2..=2i32 {
                        if lx == 0 && lz == 0 && ly <= top {
                            continue;
                        }
                        self.cells.entry((x + lx, ly, z + lz)).or_insert(BLOCK_LEAVES);
                    }
                }
            }
            self
        }

        fn put(mut self, at: (i32, i32, i32), block: BlockId) -> Self {
            self.cells.insert(at, block);
            self
        }

        fn look(&self) -> impl Fn(i32, i32, i32) -> Option<BlockId> + '_ {
            move |x, y, z| {
                if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
                    return None;
                }
                if let Some(&block) = self.cells.get(&(x, y, z)) {
                    return Some(block);
                }
                Some(if y <= self.ground { BLOCK_STONE } else { BLOCK_AIR })
            }
        }
    }

    /// Cutting the base: the cell is already air by the time `fell` runs.
    fn cut(world: &Wood, x: i32, z: i32) -> Felled {
        fell((x, world.ground + 1, z), world.look(), None)
    }

    /// ...with somebody standing at `from`, which is the last tie-break.
    fn cut_standing_at(world: &Wood, x: i32, z: i32, from: (i32, i32)) -> Felled {
        fell((x, world.ground + 1, z), world.look(), Some(from))
    }

    /// Which way the trunk went, as a step.
    fn direction_of(plan: &Felled, stump: (i32, i32)) -> (i32, i32) {
        let ((lx, _, lz), _) = *plan
            .laid
            .iter()
            .find(|((lx, _, lz), _)| (*lx, *lz) != stump)
            .expect("a felled tree laid nothing away from its stump");
        ((lx - stump.0).signum(), (lz - stump.1).signum())
    }

    /// A small tree of branches as `worldgen::branches` grows one: boughs
    /// twelve, ten and eight wide and a twig of six on the ground at (0, 0),
    /// a limb to +X out of the third piece -- out, out and up, four, two and
    /// two -- and leaves over its tip and over the top.
    fn branch_tree(world: Wood) -> Wood {
        use primitive_shared::types::branch;
        let g = world.ground;
        world
            .put((0, g + 1, 0), branch(12))
            .put((0, g + 2, 0), branch(10))
            .put((0, g + 3, 0), branch(8))
            .put((0, g + 4, 0), branch(6))
            .put((1, g + 3, 0), branch(4))
            .put((2, g + 3, 0), branch(2))
            .put((2, g + 4, 0), branch(2))
            .put((2, g + 5, 0), BLOCK_LEAVES)
            .put((0, g + 5, 0), BLOCK_LEAVES)
            .put((-1, g + 5, 0), BLOCK_LEAVES)
    }

    #[test]
    fn a_tree_of_branches_cut_at_the_foot_comes_down_as_logs_sticks_and_leaves() {
        let world = branch_tree(Wood::new(20)).put((0, 21, 0), BLOCK_AIR);
        let plan = cut(&world, 0, 0);
        for piece in [(0, 22, 0), (0, 23, 0), (0, 24, 0), (1, 23, 0), (2, 23, 0), (2, 24, 0)] {
            assert!(plan.cleared.contains(&piece), "the piece at {piece:?} was left in the air");
        }
        for leaf in [(2, 25, 0), (0, 25, 0), (-1, 25, 0)] {
            assert!(plan.cleared.contains(&leaf), "the leaf at {leaf:?} was left in the air");
        }
        // Two boughs over the cut are timber, and a fall gives back both.
        assert_eq!(plan.laid.len() as u32 + plan.spare, felled_length(2) as u32);
        assert_eq!(plan.wood, BLOCK_LOG, "a tree of branches landed as something other than oak");
        for (_, block) in &plan.laid {
            assert_eq!(block_kind(*block), BLOCK_LOG);
            assert_ne!(block_axis(*block), Axis::Y, "a felled log landed standing");
        }
        assert_eq!(plan.twigs, 4, "the six-wide top and the limb are four twigs");
    }

    /// **A tree rooted in a hillside's lip falls from its flare and from its
    /// foot alike.** Where the generator's slope wanted a lip under a trunk
    /// of pieces, the cell is the trunk's own log instead
    /// (`worldgen::lips`, "What stands on a lip"). Cut at that flare, the
    /// tree over it comes down whole; cut at the piece over the flare, it
    /// comes down and the flare stays in the ground as the stump.
    #[test]
    fn a_tree_rooted_in_a_lip_comes_down_whether_its_flare_or_its_foot_is_cut() {
        let g = 20;
        let pieces = [(0, g + 2, 0), (0, g + 3, 0), (0, g + 4, 0), (1, g + 3, 0), (2, g + 3, 0), (2, g + 4, 0)];
        // The ground cell under the tree is the flare, and it is cut.
        let flared = branch_tree(Wood::new(g)).put((0, g, 0), BLOCK_AIR);
        let plan = fell((0, g, 0), flared.look(), None);
        for piece in [(0, g + 1, 0)].iter().chain(&pieces) {
            assert!(plan.cleared.contains(piece), "cut at the flare, the piece at {piece:?} was left in the air");
        }
        // ...and the piece over it is cut, the flare standing.
        let flared = branch_tree(Wood::new(g)).put((0, g, 0), BLOCK_LOG).put((0, g + 1, 0), BLOCK_AIR);
        let plan = cut(&flared, 0, 0);
        for piece in &pieces {
            assert!(plan.cleared.contains(piece), "cut over the flare, the piece at {piece:?} was left in the air");
        }
        assert!(!plan.cleared.contains(&(0, g, 0)), "the flare went with the tree");
    }

    /// **The bug the player reported, against every tree the world grows
    /// and every stage it grows through.**
    ///
    /// "Ты когда ломаешься маленькое дерево снизу. Ломаю 3 сверху блок.
    /// Листва остаётся. Снизу она пропадает." Break the bottom block of a
    /// small tree and part of its crown came down while the rest of it
    /// stayed in the air over the hole -- the near leaves gone and the far
    /// ones hanging, which is worse than either on its own, because a
    /// player cannot tell from it whether the game meant anything by it.
    ///
    /// Two separate causes, both of them a box that was the wrong size:
    ///
    /// * A young tree of five blocks (`branches::young_cells(5, 6, ..)`,
    ///   the stage a sapling grows into) is six sixteenths of wood at the
    ///   foot and thinner above, so every piece of it is a twig and not
    ///   one is a bough -- and the canopy box was gated on there being a
    ///   bough among what fell. Nothing outside a column of the stem came
    ///   down.
    /// * A grown tree carries its crown out to `BRANCH_REACH`, five
    ///   columns, and the box was `CANOPY_RADIUS`, three. The outer rim of
    ///   every limb's clump hung.
    ///
    /// The claim is the one a player can check by looking: **cut the foot
    /// and not one leaf of that tree is left**. Stated over each leaf the
    /// generator can dress a tree in (an oak, a birch, an apple, a maple
    /// and an acacia -- see `is_canopy_of`, which is what makes one wood
    /// wear several crowns), each of the four stages
    /// (`branches::TREE_STAGES`), and a spread of variants, because the
    /// shape is rolled: how many limbs, which way each turns, whether
    /// there is a side shoot.
    #[test]
    fn a_small_tree_of_any_wood_and_any_age_cut_at_the_foot_leaves_no_leaf_in_the_air() {
        use primitive_shared::types::{
            is_leafy, BLOCK_APPLE_LEAVES, BLOCK_MAPLE_LEAVES,
        };
        use primitive_shared::worldgen::{tree_stage_cells, TREE_STAGES};

        let crowns = [
            BLOCK_LEAVES,
            BLOCK_BIRCH_LEAVES,
            BLOCK_APPLE_LEAVES,
            BLOCK_MAPLE_LEAVES,
            BLOCK_ACACIA_LEAVES,
        ];
        let mut trees = 0;
        for &leaves in &crowns {
            for stage in 0..TREE_STAGES {
                for variant in [0u32, 1, 2, 5, 11, 23, 47, 91, 0xBEEF, 0x5A91] {
                    // Flat ground, so a limb is never refused for want of
                    // air under it and the shape is the fullest the roll
                    // can make.
                    let Some(cells) = tree_stage_cells(stage, variant, leaves, |_, _| 0) else {
                        continue;
                    };
                    let mut world = Wood::new(20);
                    for &((dx, dy, dz), id) in &cells {
                        world = world.put((dx, 20 + dy, dz), id);
                    }
                    // Read the leaves back out of the finished tree rather
                    // than off the list: wood is written after leaves and
                    // takes the cells they share (`branches::in_bark`), so
                    // the list holds leaf cells that no longer hold a leaf.
                    let standing: Vec<(i32, i32, i32)> = cells
                        .iter()
                        .map(|&(at, _)| (at.0, 20 + at.1, at.2))
                        .filter(|&at| world.look()(at.0, at.1, at.2).is_some_and(is_leafy))
                        .collect();
                    assert!(!standing.is_empty(), "stage {stage} variant {variant} grew no leaves");

                    let cut = world.put((0, 21, 0), BLOCK_AIR);
                    let plan = fell((0, 21, 0), cut.look(), None);
                    for leaf in &standing {
                        assert!(
                            plan.cleared.contains(leaf),
                            "a {} tree at stage {stage}, variant {variant}: the leaf at {leaf:?} \
                             was left hanging in the air after its foot was cut",
                            primitive_shared::types::block_name(leaves),
                        );
                    }
                    trees += 1;
                }
            }
        }
        assert!(trees >= 100, "only {trees} trees were actually built");
    }

    #[test]
    fn cutting_through_a_limb_brings_down_the_limb_and_leaves_the_tree_standing() {
        // The case a trunk walk cannot see: the cut is in the air, beside
        // the trunk, and what hangs from it is sideways.
        let world = branch_tree(Wood::new(20)).put((1, 23, 0), BLOCK_AIR);
        let plan = fell((1, 23, 0), world.look(), None);
        assert!(plan.cleared.contains(&(2, 23, 0)) && plan.cleared.contains(&(2, 24, 0)), "the limb hangs on");
        for trunk in [(0, 21, 0), (0, 22, 0), (0, 23, 0), (0, 24, 0)] {
            assert!(!plan.cleared.contains(&trunk), "cutting a limb took the trunk at {trunk:?}");
        }
        assert!(plan.laid.is_empty() && plan.spare == 0, "a limb of twigs gave timber");
        assert_eq!(plan.twigs, 2);
    }

    #[test]
    fn a_sapling_pulled_up_at_the_foot_falls_as_sticks() {
        use primitive_shared::types::branch;
        let world = Wood::new(20)
            .put((0, 22, 0), branch(4))
            .put((0, 23, 0), branch(2))
            .put((0, 24, 0), BLOCK_LEAVES);
        let plan = cut(&world, 0, 0);
        for cell in [(0, 22, 0), (0, 23, 0), (0, 24, 0)] {
            assert!(plan.cleared.contains(&cell), "the sapling's {cell:?} was left in the air");
        }
        assert!(plan.laid.is_empty() && plan.spare == 0, "a sapling gave a log");
        assert_eq!(sticks_from_twigs(plan.twigs), 1, "two twigs over the foot are one stick");
    }

    #[test]
    fn a_branch_that_still_stands_on_something_is_not_felled() {
        // A limb propped on a pillar is held up, and a cut beside it
        // brings nothing down -- not even through the log walk after it.
        let world = branch_tree(Wood::new(20))
            .put((2, 21, 0), BLOCK_STONE)
            .put((2, 22, 0), BLOCK_STONE)
            .put((1, 23, 0), BLOCK_AIR);
        let plan = fell((1, 23, 0), world.look(), None);
        assert!(plan.is_empty(), "a propped limb fell: {plan:?}");
    }

    #[test]
    fn cutting_the_base_brings_the_whole_trunk_down() {
        // The bug, stated: chopping the bottom block used to leave five
        // metres of trunk standing in the air with a canopy over it.
        let world = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        let plan = cut(&world, 0, 0);
        for n in 1..6 {
            assert!(
                plan.cleared.contains(&(0, 21 + n, 0)),
                "the log at y={} was left standing",
                21 + n
            );
        }
        assert!(plan.leaves > 0, "the canopy was left floating");
    }

    #[test]
    fn what_lands_is_every_log_that_stood_and_still_has_its_bark() {
        // Both halves of what a fall does to a tree. It used to take the
        // bark as well, which threw away which tree you had cut and left
        // timber softer than the deadfall the generator scatters -- so
        // cutting a tree down yourself gave you *different wood* from
        // finding one already down. And it used to keep two thirds of
        // the trunk, which made cutting a tree down worth less than
        // cutting it apart.
        let world = Wood::new(20).tree(0, 0, 9).put((0, 21, 0), BLOCK_AIR);
        let plan = cut(&world, 0, 0);
        let logs = plan.laid.len() as u32 + plan.spare;
        assert_eq!(logs, 8, "eight logs stood over the cut of a nine-block trunk and the fall gave back {logs}");
        assert_eq!(plan.wood, BLOCK_LOG, "the plan forgot what wood it was");
        for (_, block) in &plan.laid {
            assert_eq!(block_kind(*block), BLOCK_LOG, "the fall stripped the bark");
            assert_ne!(block_axis(*block), Axis::Y, "a felled log landed standing");
        }
    }

    #[test]
    fn a_birch_lands_as_birch_and_an_oak_as_oak() {
        // The point of keeping the bark: the two woods are still two
        // woods on the ground, and the drops match what was standing.
        let oak = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        let mut birch = Wood::new(20);
        for n in 1..6 {
            birch = birch.put((0, 21 + n, 0), BLOCK_BIRCH_LOG);
        }
        birch = birch.put((1, 26, 0), BLOCK_BIRCH_LEAVES);
        let felled_oak = cut(&oak, 0, 0);
        let felled_birch = cut(&birch, 0, 0);
        let kinds = |p: &Felled| -> Vec<BlockId> {
            p.laid.iter().map(|(_, b)| block_kind(*b)).collect()
        };
        assert!(kinds(&felled_oak).iter().all(|&k| k == BLOCK_LOG));
        assert!(kinds(&felled_birch).iter().all(|&k| k == BLOCK_BIRCH_LOG));
        assert_eq!(felled_oak.wood, BLOCK_LOG);
        assert_eq!(felled_birch.wood, BLOCK_BIRCH_LOG);
        assert_eq!(kinds(&felled_oak).len(), kinds(&felled_birch).len());
    }

    #[test]
    fn a_tree_that_cannot_fall_still_gives_its_wood() {
        // Walled in on all four sides. Nothing can be laid down, so
        // everything comes back as loose timber -- a tree that fell into
        // a wall and gave you nothing would be a tree nobody cuts twice.
        let mut world = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        for &(dx, dz) in &DIRECTIONS {
            world = world.put((dx, 21, dz), BLOCK_STONE);
        }
        let plan = cut(&world, 0, 0);
        // One log lands: the butt, straight down into the hole the cut
        // left. A tree hemmed in on every side drops where it stood,
        // which is both what happens and what keeps the stump reading as
        // the place the tree came from.
        assert_eq!(
            plan.laid,
            // Along Z: with nowhere to go, every direction scores the
            // same and the fixed order decides, which is north.
            vec![((0, 21, 0), oriented(BLOCK_LOG, Axis::Z))],
            "it laid a log through a wall"
        );
        // ...and everything else comes back as loose timber. A tree that
        // fell into a wall and gave you nothing would be a tree nobody
        // cuts twice.
        assert!(plan.spare > 0, "a walled-in tree gave nothing back");
        assert_eq!(plan.spare + 1, felled_length(5) as u32);
    }

    #[test]
    fn the_same_tree_in_the_same_place_falls_the_same_way() {
        // Everything that decides is a fact about the world, so felling
        // is something a player can aim rather than something that
        // happens to them.
        let world = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        let a = cut(&world, 0, 0);
        let b = cut(&world, 0, 0);
        assert_eq!(a.laid, b.laid);
        assert!(!a.laid.is_empty());
        // All in a line, all at the stump's own height.
        for ((_, ly, _), _) in &a.laid {
            assert_eq!(*ly, 21, "a felled log floated or sank");
        }
    }

    #[test]
    fn a_tree_falls_the_way_it_leans() {
        // The rule that makes it readable from the ground: the crown is
        // the part a player can see, so a tree whose crown hangs east
        // goes east. Every direction here has all the room and all the
        // ground it could want, so the lean is what is left to decide.
        let mut world = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        for ly in 25..=27 {
            for lz in -1..=1 {
                world = world.put((3, ly, lz), BLOCK_LEAVES);
                world = world.put((2, ly, lz), BLOCK_LEAVES);
            }
        }
        assert_eq!(direction_of(&cut(&world, 0, 0), (0, 0)), (1, 0), "it fell the wrong way");

        // ...and the mirror of it, so the test is about the lean rather
        // than about east.
        let mut west = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        for ly in 25..=27 {
            for lz in -1..=1 {
                west = west.put((-3, ly, lz), BLOCK_LEAVES);
                west = west.put((-2, ly, lz), BLOCK_LEAVES);
            }
        }
        assert_eq!(direction_of(&cut(&west, 0, 0), (0, 0)), (-1, 0));
    }

    #[test]
    fn a_tree_would_rather_not_lie_out_over_a_hole() {
        // Room is not everything: a trunk laid across a pit is a trunk
        // hanging in the air. North has the ground dug out from under
        // it, so the tree goes east instead -- with no lean either way
        // to argue about.
        let mut world = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        for step in 1..=4 {
            world = world.put((0, 20, -step), BLOCK_AIR);
        }
        assert_eq!(direction_of(&cut(&world, 0, 0), (0, 0)), (1, 0));
    }

    #[test]
    fn a_tree_with_a_choice_does_not_pick_the_cell_you_are_standing_in() {
        // The last tie-break, and the only one that knows a player
        // exists. It is last on purpose: a tree that always fell away
        // from you could never hurt you, and one that fell on you for no
        // reason you could see would be a trap.
        let world = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        // With nobody there it takes the first direction: north.
        assert_eq!(direction_of(&cut(&world, 0, 0), (0, 0)), (0, -1));
        // Stand in that cell and it takes the next one that is just as
        // good.
        let moved = cut_standing_at(&world, 0, 0, (0, -1));
        assert_ne!(direction_of(&moved, (0, 0)), (0, -1), "it fell on the feller");

        // ...but a tree with only one way to go still takes it, standing
        // there or not. This is the death in `lib::crush_anyone_under`.
        let mut boxed = Wood::new(20).tree(0, 0, 6).put((0, 21, 0), BLOCK_AIR);
        for &(dx, dz) in &[(0, -1), (1, 0), (-1, 0)] {
            boxed = boxed.put((dx, 21, dz), BLOCK_STONE);
        }
        let nowhere_else = cut_standing_at(&boxed, 0, 0, (0, 1));
        assert_eq!(
            direction_of(&nowhere_else, (0, 0)),
            (0, 1),
            "a tree with one way out refused to take it"
        );
    }

    #[test]
    fn a_single_log_is_not_a_tree() {
        // One block somebody placed. Felling it would turn a block into
        // a different block for no reason anybody could explain.
        let world = Wood::new(20).put((0, 22, 0), BLOCK_STONE);
        assert!(cut(&world, 0, 0).is_empty());
        let bare = Wood::new(20);
        assert!(cut(&bare, 0, 0).is_empty());
    }

    #[test]
    fn a_log_already_lying_down_is_not_felled_again() {
        // Deadfall is already fallen. Felling it would be a way to turn
        // one tree into two.
        let world = Wood::new(20).put((0, 22, 0), oriented(BLOCK_LOG, Axis::X));
        assert!(cut(&world, 0, 0).is_empty());
    }

    #[test]
    fn felling_stops_at_the_edge_of_what_is_loaded() {
        // `None` is "not loaded", and writing into a chunk nobody has is
        // writing into a chunk the generator will paint over.
        let plan = fell((0, 21, 0), |_x, y, _z| (y < 24).then_some(BLOCK_LOG), None);
        assert!(
            plan.cleared.iter().all(|&(_, y, _)| y < 24),
            "it felled through the edge of the loaded world"
        );
    }

    #[test]
    fn a_neighbouring_tree_keeps_its_leaves() {
        // Only this wood's canopy, and only within the radius the
        // generator plants one in. A wood is not one tree.
        let world = Wood::new(20).tree(0, 0, 6).tree(9, 0, 6);
        let plan = cut(&world, 0, 0);
        assert!(
            plan.cleared.iter().all(|&(cx, _, _)| cx < 9 - CANOPY_RADIUS),
            "felling one tree stripped its neighbour"
        );
    }

    #[test]
    fn a_trunk_taller_than_anything_is_still_bounded() {
        // A column of logs somebody stacked to the roof of the world.
        let plan = fell(
            (0, 1, 0),
            |_x, y, _z| (0..CHUNK_SIZE_Y as i32).contains(&y).then_some(BLOCK_LOG),
            None,
        );
        assert!(
            plan.cleared.len() < 4096,
            "felling walked {} cells",
            plan.cleared.len()
        );
    }

    // ---- palms ----

    /// The generator's palm `variant` leaning `lean`, rooted at (0, 0) on a
    /// beach that falls away under its lean the way `worldgen::palm_lean`
    /// chooses one: a column lower for each of the first three columns out
    /// that way, then flat -- with the root's cell already cut.
    fn a_palm_over_a_beach(variant: u32, (sx, sz): (i32, i32)) -> Wood {
        let mut beach = Wood::new(20);
        for cz in -12..=12 {
            for cx in -12..=12 {
                let out = (cx * sx + cz * sz).clamp(0, 3);
                for y in 21 - out..=20 {
                    beach = beach.put((cx, y, cz), BLOCK_AIR);
                }
            }
        }
        for ((dx, dy, dz), id) in primitive_shared::worldgen::palm_cells(variant, (sx, sz)) {
            beach = beach.put((dx, 20 + dy, dz), id);
        }
        beach.put((0, 21, 0), BLOCK_AIR)
    }

    #[test]
    fn a_leaning_palm_falls_the_way_it_leans_wherever_the_feller_stands() {
        // "падение пальм не зависит от их наклона". A palm leans over the fall
        // of its beach, so the ground rule sent every one along the shore or
        // inland. Every height and both bends, leaning each way, felled from
        // nowhere and from each side of the stump -- the lean side included,
        // where it falls on the feller: the lean is the thing a player can
        // see from the ground and stand clear of.
        let sides = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        for variant in [0u32, 0x11, 0x2, 0x3, 0x13] {
            for lean in sides {
                let beach = a_palm_over_a_beach(variant, lean);
                for feller in std::iter::once(None).chain(sides.into_iter().map(Some)) {
                    let plan = fell((0, 21, 0), beach.look(), feller);
                    let name = format!("palm {variant:#x} leaning {lean:?} felled from {feller:?}");
                    assert!(!plan.laid.is_empty(), "{name} laid nothing");
                    assert_eq!(direction_of(&plan, (0, 0)), lean, "{name} fell the wrong way");
                    for &((lx, ly, lz), _) in &plan.laid {
                        assert!(lx * lean.0 + lz * lean.1 >= 0, "{name} laid a log behind its stump at {:?}", (lx, ly, lz));
                        // **On the sand, not over it**: the lane of a leaning
                        // palm is the beach falling away, and a trunk laid at
                        // the stump's height there hangs a block in the air.
                        let under = beach.look()(lx, ly - 1, lz);
                        let laid_under = plan.laid.iter().any(|&(c, _)| c == (lx, ly - 1, lz));
                        assert!(
                            laid_under || under.is_some_and(|b| !is_air(b)),
                            "{name} left the log at {:?} hanging over the beach",
                            (lx, ly, lz)
                        );
                    }
                    assert_eq!(
                        plan.laid.len() as u32 + plan.spare,
                        felled_length(plan.cleared.iter().filter(|&&c| beach.look()(c.0, c.1, c.2).is_some_and(is_branch)).count() as i32) as u32,
                        "{name} lost timber following the slope"
                    );
                }
            }
        }
    }

    #[test]
    fn a_straight_palm_still_falls_by_room_and_ground() {
        // The other half: a palm with nothing to lean on is an oak to the four
        // rules, and on the same beach it does not lie out over the fall of it.
        let mut beach = Wood::new(20);
        for cz in -12..=12 {
            for cx in 1..=12 {
                for y in 21 - cx.min(3)..=20 {
                    beach = beach.put((cx, y, cz), BLOCK_AIR);
                }
            }
        }
        let trunk = primitive_shared::worldgen::palm_cells(0, (1, 0))[0].1;
        for y in 21..=28 {
            beach = beach.put((0, y, 0), trunk);
        }
        beach = beach.put((0, 29, 0), primitive_shared::types::BLOCK_PALM_FRONDS).put((0, 21, 0), BLOCK_AIR);
        let plan = fell((0, 21, 0), beach.look(), None);
        assert_ne!(direction_of(&plan, (0, 0)), (1, 0), "a straight palm lay out over the fall of the beach");
        assert!(plan.laid.iter().all(|&((_, ly, _), _)| ly == 21), "a straight palm's logs left the stump's height");
    }

    // ---- what a real tree gives ----

    /// One tree the generator grew, felled from its foot: how many cells of
    /// timber it stood in, and how many logs the fall handed back.
    #[derive(Debug, Clone, Copy)]
    struct Yield {
        timber: u32,
        logs: u32,
    }

    /// Is this a cell a player would call a log of a tree: a log, standing
    /// or lying (an old tree's limbs lie), or a piece of branch that gives a
    /// log when cut?
    fn is_timber(block: BlockId) -> bool {
        use primitive_shared::types::{block_drop, is_branch, BLOCK_STICK};
        primitive_shared::wood::is_log(block)
            || (is_branch(block) && block_drop(block).is_some_and(|drop| drop != BLOCK_STICK))
    }

    /// Fells up to `want` trees of `biome` in a world of `zone`, found along
    /// the generator's own transect, and says what each gave.
    ///
    /// **The tree is the timber joined to its root**, walked through every
    /// cell touching another (an acacia's limb steps across a corner), and
    /// stopped at timber lying on the ground more than a column from the
    /// root: that is another tree's foot, or deadfall. A wood's crowns touch,
    /// so asking for lone trees found almost none -- one in a forest.
    fn fell_real_trees(
        zone: primitive_shared::worldgen::Zone,
        biome: primitive_shared::worldgen::Biome,
        want: usize,
    ) -> Vec<Yield> {
        use primitive_shared::types::{
            is_branch, Chunk, ChunkPos, BLOCK_ASH, BLOCK_DIRT, BLOCK_GRASS, BLOCK_SAND, BLOCK_SNOW, CHUNK_SIZE_X,
        };
        use primitive_shared::worldgen::{Preset, WorldGen, BRANCH_REACH};

        let gen = WorldGen::with_zone(12345, Preset::Normal, zone);
        let side = CHUNK_SIZE_X as i32;
        let reach = BRANCH_REACH.max(ACACIA_REACH) + 1;
        let mut out = Vec::new();
        let mut places = 0;
        'search: for cz in (-360..360).step_by(4) {
            for cx in (-160..160).step_by(5) {
                if out.len() >= want || places >= 24 {
                    break 'search;
                }
                if gen.biome_at(cx * side + 8, cz * side + 8) != biome {
                    continue;
                }
                places += 1;
                let mut near: Vec<Chunk> = Vec::new();
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        near.push(gen.generate_chunk(ChunkPos::new(cx + dx, cz + dz)));
                    }
                }
                let at = |x: i32, y: i32, z: i32| -> Option<BlockId> {
                    if !(0..3 * side).contains(&x) || !(0..3 * side).contains(&z) || !(0..CHUNK_SIZE_Y as i32).contains(&y) {
                        return None;
                    }
                    let chunk = &near[((z / side) * 3 + x / side) as usize];
                    Some(chunk.get((x % side) as usize, y as usize, (z % side) as usize))
                };
                let on_ground = |x: i32, y: i32, z: i32| {
                    at(x, y - 1, z).is_some_and(|b| {
                        // As what the ground stands in for: a dead forest's
                        // andosol is earth (`ground::as_common`).
                        matches!(
                            primitive_shared::ground::as_common(b),
                            BLOCK_GRASS
                                | BLOCK_DIRT
                                | BLOCK_ASH
                                | BLOCK_SAND
                                | BLOCK_SNOW
                                | BLOCK_STONE
                                | primitive_shared::types::BLOCK_SANDY_SOIL
                                | primitive_shared::types::BLOCK_DRY_TURF
                        )
                    })
                };
                // A sapling's stem is twigs, sticks to a hand: not a tree to fell.
                let rooted = |x: i32, y: i32, z: i32| {
                    at(x, y, z).is_some_and(|b| (is_standing_trunk(b) || is_branch(b)) && is_timber(b)) && on_ground(x, y, z)
                };
                // One tree in a place in three chunks' middle, so every tree
                // is whole inside the three.
                for lz in (side + 1)..(2 * side - 1) {
                    for lx in (side + 1)..(2 * side - 1) {
                        if out.len() >= want {
                            break 'search;
                        }
                        let Some(ly) = (1..CHUNK_SIZE_Y as i32).find(|&ly| rooted(lx, ly, lz)) else {
                            continue;
                        };
                        if gen.biome_at((cx - 1) * side + lx, (cz - 1) * side + lz) != biome {
                            continue;
                        }
                        // Not the second column of an old tree's bole.
                        if rooted(lx - 1, ly, lz) || rooted(lx, ly, lz - 1) || rooted(lx - 1, ly, lz - 1) {
                            continue;
                        }
                        let mut seen = std::collections::HashSet::new();
                        let mut frontier = vec![(lx, ly, lz)];
                        seen.insert((lx, ly, lz));
                        while let Some((x, y, z)) = frontier.pop() {
                            for dy in -1..=1 {
                                for dz in -1..=1 {
                                    for dx in -1..=1 {
                                        let n = (x + dx, y + dy, z + dz);
                                        if (n.0 - lx).abs() > reach || (n.2 - lz).abs() > reach || n.1 < ly || seen.contains(&n) {
                                            continue;
                                        }
                                        if !at(n.0, n.1, n.2).is_some_and(is_timber) {
                                            continue;
                                        }
                                        let foreign = on_ground(n.0, n.1, n.2) && ((n.0 - lx).abs() > 1 || (n.2 - lz).abs() > 1);
                                        if foreign {
                                            continue;
                                        }
                                        seen.insert(n);
                                        frontier.push(n);
                                    }
                                }
                            }
                        }
                        // **Chopped as a player chops**: the lowest piece of
                        // the tree still standing, over and over, each cut
                        // worth what that piece drops by hand and each fall
                        // worth what it lays and spares, until none of the
                        // tree is left standing. A tree on two feet falls
                        // from the second cut, not the first, and a count
                        // that cut only one foot would say it gave nothing.
                        let mut edits: HashMap<(i32, i32, i32), BlockId> = HashMap::new();
                        let mut logs = 0u32;
                        for _ in 0..seen.len() {
                            let now = |x: i32, y: i32, z: i32| edits.get(&(x, y, z)).copied().or_else(|| at(x, y, z));
                            let Some(&cut_at) = seen
                                .iter()
                                .filter(|c| now(c.0, c.1, c.2).is_some_and(is_timber))
                                .min_by_key(|c| (c.1, c.0, c.2))
                            else {
                                break;
                            };
                            let piece = now(cut_at.0, cut_at.1, cut_at.2).unwrap_or(BLOCK_AIR);
                            if primitive_shared::types::block_drop(piece).is_some_and(|d| d != primitive_shared::types::BLOCK_STICK) {
                                logs += 1;
                            }
                            edits.insert(cut_at, BLOCK_AIR);
                            let plan = fell(cut_at, |x, y, z| edits.get(&(x, y, z)).copied().or_else(|| at(x, y, z)), None);
                            logs += plan.laid.len() as u32 + plan.spare;
                            for cell in plan.cleared {
                                edits.insert(cell, BLOCK_AIR);
                            }
                            // What was laid is on the ground as timber a
                            // player carries off, counted above -- not more
                            // tree to cut.
                            for (cell, _) in plan.laid {
                                edits.insert(cell, BLOCK_AIR);
                            }
                        }
                        out.push(Yield { timber: seen.len() as u32, logs });
                    }
                }
            }
        }
        out
    }

    /// What felling gives, tree by tree, for every wood the generator grows.
    ///
    /// ```text
    /// cargo test -p primitive_server --lib -- --ignored --nocapture what_felling_gives
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn what_felling_gives() {
        use primitive_shared::worldgen::{Biome, Zone};
        for (zone, biome) in [
            (Zone::Temperate, Biome::Forest),
            (Zone::Temperate, Biome::BirchForest),
            (Zone::Temperate, Biome::Plains),
            (Zone::Temperate, Biome::Swamp),
            (Zone::Temperate, Biome::DeadForest),
            (Zone::North, Biome::Taiga),
            (Zone::North, Biome::Tundra),
            (Zone::Tropics, Biome::Savanna),
            (Zone::DryBelt, Biome::Desert),
        ] {
            let trees = fell_real_trees(zone, biome, 12);
            let timber: u32 = trees.iter().map(|t| t.timber).sum();
            let logs: u32 = trees.iter().map(|t| t.logs).sum();
            let each: Vec<String> = trees.iter().map(|t| format!("{}/{}", t.logs, t.timber)).collect();
            println!(
                "{biome:?} ({zone:?}): {} trees, {timber} cells of timber, {logs} logs -- logs/timber each: {}",
                trees.len(),
                each.join(" ")
            );
        }
    }

    #[test]
    fn a_fall_gives_back_every_log_it_was() {
        assert_eq!(felled_length(0), 0);
        for pieces in 1..=4 * MAX_TRUNK {
            assert_eq!(felled_length(pieces), pieces, "a fall of {pieces} logs lost some of them");
        }
    }

    /// **The report, against the trees the world grows.** "С огромного
    /// дерева падает 3-4 бревна": every wood in every zone that grows one,
    /// chopped down the way a player chops (see `fell_real_trees`), gives at
    /// least a log for every cell of timber it stood in. Measured before the
    /// fall stopped keeping two thirds, the same trees gave 170 logs for 254
    /// cells in an oak wood, 213 for 317 in the taiga, 233 for 323 on the
    /// savanna -- see `what_felling_gives`.
    #[test]
    fn every_tree_the_world_grows_gives_a_log_for_every_log_it_stood_in() {
        use primitive_shared::worldgen::{Biome, Zone};
        for (zone, biome) in [
            (Zone::Temperate, Biome::Forest),
            (Zone::Temperate, Biome::BirchForest),
            (Zone::Temperate, Biome::Swamp),
            (Zone::Temperate, Biome::DeadForest),
            (Zone::North, Biome::Taiga),
            (Zone::Tropics, Biome::Savanna),
            (Zone::DryBelt, Biome::Desert),
        ] {
            let trees = fell_real_trees(zone, biome, 6);
            assert!(trees.len() >= 3, "only {} trees of {biome:?} found to fell", trees.len());
            for tree in trees {
                assert!(
                    tree.logs >= tree.timber,
                    "a tree of {biome:?} standing in {} cells of timber gave {} logs",
                    tree.timber,
                    tree.logs
                );
            }
        }
    }

    /// An old tree's bole, four columns of `height` on the ground at (0, 0)
    /// to (1, 1), a limb of three lying logs out of it to +X five up, and a
    /// crown over the top as wide as an old tree's.
    fn old_tree(height: i32) -> Wood {
        let mut world = Wood::new(20);
        let top = 20 + height;
        for ly in top - 2..=top + 1 {
            for lz in -3..=4 {
                for lx in -3..=4 {
                    world = world.put((lx, ly, lz), BLOCK_LEAVES);
                }
            }
        }
        for (bx, bz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            for n in 1..=height {
                world = world.put((bx, 20 + n, bz), BLOCK_LOG);
            }
        }
        for n in 2..=4 {
            world = world.put((n, 26, 0), oriented(BLOCK_LOG, Axis::X));
        }
        world
    }

    #[test]
    fn an_old_tree_stands_until_the_last_foot_of_its_bole_is_cut_and_then_comes_down_whole() {
        let mut world = old_tree(10);
        let feet = [(0, 0), (1, 0), (0, 1)];
        for (n, &(fx, fz)) in feet.iter().enumerate() {
            world = world.put((fx, 21, fz), BLOCK_AIR);
            let plan = cut(&world, fx, fz);
            assert!(plan.is_empty(), "the bole fell with {} of its four feet cut: {plan:?}", n + 1);
        }
        world = world.put((1, 21, 1), BLOCK_AIR);
        let plan = cut(&world, 1, 1);
        for (bx, bz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            for y in 22..=30 {
                assert!(plan.cleared.contains(&(bx, y, bz)), "the bole's log at {:?} was left in the air", (bx, y, bz));
            }
        }
        for n in 2..=4 {
            assert!(plan.cleared.contains(&(n, 26, 0)), "the limb's log at {:?} was left hanging", (n, 26, 0));
        }
        for leaf in [(-3, 29, -3), (4, 31, 4)] {
            assert!(plan.cleared.contains(&leaf), "the crown's leaf at {leaf:?} was left in the air");
        }
        assert_eq!(
            plan.laid.len() as u32 + plan.spare,
            4 * 9 + 3,
            "a bole of four nine-log columns and a limb of three gave back another number"
        );
        assert!(plan.laid.len() <= 9, "the bole was laid out longer than it stood");
    }

    #[test]
    fn two_trees_rooted_side_by_side_are_not_a_bole() {
        // A wood's spacing puts two trunks on neighbouring columns now and
        // then. Only a square of four is an old tree, so cutting one of a
        // pair still fells it.
        let world = Wood::new(20).tree(0, 0, 6).tree(1, 0, 6).put((0, 21, 0), BLOCK_AIR);
        let plan = cut(&world, 0, 0);
        assert!(plan.cleared.contains(&(0, 26, 0)), "a tree beside another did not fall");
        assert!(!plan.cleared.contains(&(1, 26, 0)), "felling one tree of a pair took the other");
    }
}
