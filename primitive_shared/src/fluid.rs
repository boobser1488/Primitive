//! Water, as a thing with a *depth* rather than a thing that is either
//! there or not.
//!
//! ## Why the rules live here and not with the simulation
//!
//! Water flows: `primitive_server::logic::water` runs it, a cell at a
//! time, on the server's tick. What that code owns is the *world* --
//! looking cells up, writing them back, telling clients what changed.
//! What it does not own is how deep a cell ends up, and that is this
//! module.
//!
//! The split is not tidiness. Flowing water goes wrong in one specific
//! way: the client draws a surface at one height, the server thinks the
//! depth is another, and the player swims through a wall of water that
//! is not where it is drawn. That is exactly the class of disagreement
//! `primitive_shared` exists to prevent -- so the mesher, the collider,
//! the drowning check and the simulation all read their answers from the
//! same functions here.
//!
//! Everything below is a pure function of a few small numbers, which is
//! what lets the rules be tested exhaustively -- every depth against
//! every depth, with no world, no tick loop and no socket.
//!
//! ## The model
//!
//! **Water is an amount, and the amount is conserved.** A cell of water
//! holds one to [`SOURCE_DEPTH`] eighths of a block; nothing creates any
//! and nothing destroys any. Every change is a *transfer* between two
//! cells, one losing exactly what the other gains.
//!
//! There are two things water does, and they are the two functions
//! below:
//!
//! * it **falls**, into the cell underneath, as much as fits, and it
//!   does that before it does anything else -- see [`fall_transfer`];
//! * it **levels**, sideways, by half of any difference of two or more
//!   -- see [`level_transfer`].
//!
//! Everything a player recognises comes out of applying those two: a
//! spill, a waterfall, a channel that drains a pond, a hole in a lake
//! bed that fills from the lake. None of it is written down separately,
//! so none of it can disagree with the rest.
//!
//! There is a third movement, and it is *not* here on purpose: a cell
//! that has just given water away draws a share of whatever is standing
//! within reach along the same body of water, which is what makes a cut
//! in a lake bed drain the lake rather than the ring of cells around it.
//! It needs to know what is connected to what, so it belongs to the
//! simulation and lives in `primitive_server::logic::water`. What it
//! moves, it moves with [`level_transfer`] -- the same half of the same
//! difference, between two cells that are further apart.
//!
//! ## The model this replaced, and why it went
//!
//! **Minecraft's**, near enough exactly. A cell's depth was not an
//! amount but a *distance from the nearest source*: a cell beside a
//! source was seven, the next one six, and eight blocks out there was
//! nothing. A source was full by definition, and the whole spill was a
//! pure function of where the sources were.
//!
//! It has real virtues and they should be said plainly, because they are
//! what was given up. It terminated by construction -- the "what feeds
//! what" graph was acyclic, so there was nothing for two cells to trade
//! and no oscillation to prove absent. It settled in a couple of passes.
//! It cost a tenth of what this costs to run (the numbers are in
//! `primitive_server::logic::water`, in the test that measures them).
//!
//! What it could not do was let go of water. A source was endless, so a
//! pond could not be emptied, a lake could not be drained, and a channel
//! cut from the sea ran for ever out of nothing. That is not a corner
//! case; it is most of what a player wants to do *with* water once they
//! have found some, and no amount of cheapness elsewhere pays for it.
//!
//! ## What conservation costs, said plainly
//!
//! **It is slower**, by about ten times on the reference spill, because
//! the water genuinely travels instead of being recomputed in place.
//!
//! **It has to be proved to stop.** Two cells trading the same eighth
//! back and forth for ever is the classic failure of this kind of
//! simulation, and it is nearly invisible -- an eighth shimmering back
//! and forth -- while writing the world's edit overlay on every hand-over and making
//! the autosave rewrite `edits.bin` over a map that is not changing.
//! Both halves of [`level_transfer`] exist to make that impossible, and
//! there are tests here and in the simulation that say so.
//!
//! **It does not level perfectly.** Whole eighths cannot split a
//! difference of one, so a surface with somewhere to drain settles as a
//! wedge -- one eighth per block, sloping to the drain -- rather than
//! flat.
//!
//! **The wedge is drawn as a wedge now.** For a long time every depth
//! was drawn at one height, and this header argued that the wedge was
//! therefore invisible -- wrong even then, because what a player sees is
//! the *reach*: a channel dug from a pond runs eight blocks of the
//! thirteen it has water for and the last five stay dry, and a pond
//! drained through a hole in its bed keeps an eighth in every cell.
//! Drawn at full height that last eighth looked exactly like the pond
//! that was there before, which is the picture that finally lost the
//! argument: the player asked for water to be *seen* to fall as it
//! spreads. So [`surface_height`] follows the depth, and the mesher
//! slopes each cell's corners to its neighbours so the eighths read as
//! a slope rather than a staircase. Whether to move the last eighth
//! anyway, and what the two ways of doing it were measured to cost, is
//! argued out in `primitive_server::logic::water`, in
//! `a_pond_drains_down_to_a_wedge_and_no_further`.
//!
//! ## Why the depth is written the opposite way round from Minecraft's
//!
//! Minecraft's fluid level counts *up* as the water gets thinner: 0 is a
//! source, 7 is the last cell of a spill. Here the number counts down,
//! 8 to 1, because it rides in the same variant field a layer count does
//! (see `types`) and that field already means "how much of the cell is
//! full" for everything else that uses it. Flipping it would have made
//! `block_layers` lie about water alone.
//!
//! The encoding falls out of that, and it is worth stating because it is
//! the reason **no save has to be migrated and no other file had to
//! change** when the model underneath it did. An amount uses the same
//! field, over the same range, as the old distance-from-a-source did, so
//! [`depth`], [`surface_height`], [`covers`], the vertex byte the mesher
//! writes, the collider, the drowning check, the fog and the mod API all
//! go on meaning what they meant. A cell written before any of this
//! reads back as full, and an ocean generated as plain `BLOCK_WATER` is
//! an ocean of full cells, which is exactly what it should be.
//!
//! The model changed and the number did not, which is why this was a
//! day's work rather than a month's.

use crate::types::{
    block_layers, is_air, is_liquid, with_layers, BlockId, BLOCK_AIR, BLOCK_WATER, LAYERS_PER_BLOCK,
};

/// How far below the top of its cell a full cell of water is drawn.
///
/// A full cube of water and a full cube of stone read as the same solid
/// thing at foot level, and most of any shoreline is one cell deep -- so
/// standing *in* the shallows looked exactly like standing *on* them.
/// Dropping the surface a little is what makes the waterline visible.
///
/// Here rather than in the mesher because the flow simulation wants the
/// same number: a cell that is nearly full has to be drawn nearly full,
/// and "nearly full" has to mean the same thing on both sides.
pub const SURFACE_DROP: f32 = 0.12;

/// A full cell: eight eighths, and the most any cell can hold.
///
/// Not "a source" in the old sense -- nothing here is endless. It is a
/// ceiling, and it is what [`fall_transfer`] measures room against.
pub const SOURCE_DEPTH: u8 = LAYERS_PER_BLOCK;


/// How full a cell of water is, in eighths. Zero for anything that is
/// not water at all.
#[inline]
pub fn depth(block: BlockId) -> u8 {
    if !is_liquid(block) {
        return 0;
    }
    block_layers(block)
}

/// Is this cell full?
///
/// **The name is older than the model and is kept for the mod API**,
/// which has exposed it since before water was conserved. It no longer
/// means "a spring that never runs out" -- there are none of those any
/// more -- only "there is no room in here for another eighth", which is
/// the question the flow rules actually ask.
#[inline]
pub fn is_source(block: BlockId) -> bool {
    is_liquid(block) && block_layers(block) >= SOURCE_DEPTH
}

/// Is this water that is running rather than standing?
#[inline]
pub fn is_flowing(block: BlockId) -> bool {
    is_liquid(block) && block_layers(block) < SOURCE_DEPTH
}

/// The block a cell of this depth holds. Nothing at all is air, not a
/// cell of water with nothing in it.
///
/// Here rather than in the simulation because it is the other half of
/// [`depth`], and a round trip that is right in one direction and wrong
/// in the other is exactly the bug this module exists to make
/// impossible.
#[inline]
pub fn with_depth(depth: u8) -> BlockId {
    if depth == 0 {
        BLOCK_AIR
    } else {
        with_layers(BLOCK_WATER, depth.min(SOURCE_DEPTH))
    }
}

/// How high the surface of this cell sits above its floor, in blocks:
/// its depth in eighths, scaled so that a full cell sits at
/// `1 - SURFACE_DROP` and one eighth is a film an ankle deep.
///
/// **This has been each of the two possible answers, and the history
/// is the argument.** It began as the depth, then became one height for
/// every cell, for a reason that had nothing to do with physics: break
/// one block under a lake and the cell that fills is, for a while, at a
/// different height from the water around it -- a step, with a wall
/// drawn down it, in the middle of a flat sea. One cell out of place is
/// the whole of what makes a sea look broken. So the depth was made
/// invisible, and the thin end of a spill was drawn as deep as its
/// source.
///
/// It is the depth again because that last part was the thing a player
/// noticed: water poured across a floor spread as a slab of full height
/// and stopped dead, and a pond drained to its last eighth still looked
/// like a pond. The player asked for spreading water to be *seen* to
/// fall. What answers the step-in-the-sea objection is not this function
/// but the mesher: a liquid cell's top corners are averaged with the
/// liquid cells around each corner, so a lone lower cell is a dimple
/// that slopes to its neighbours, not a wall. A full cell is exactly
/// where it always was -- the sea, which is nothing but full cells, has
/// not moved by a texel -- and every cell under another cell of water
/// still reaches the top of its cell (`surface_height_with_above`).
///
/// The collider, the drowning check and the fog all read this too, so a
/// spill an eighth deep is waded through rather than swum in, which is
/// the other half of what "seen to fall" has to mean.
#[inline]
pub fn surface_height(block: BlockId) -> f32 {
    if is_liquid(block) {
        (depth(block) as f32 / SOURCE_DEPTH as f32) * (1.0 - SURFACE_DROP)
    } else {
        0.0
    }
}

/// How high the mesher draws the top of this cell, given what is above
/// it.
///
/// A cell with water above it is one slice of a column, and a column is
/// drawn as one unbroken box: the cell above hangs its floor down by
/// `SURFACE_DROP` (`underhang`), so the cell below has to reach exactly
/// `1 - SURFACE_DROP` whatever its own depth says -- and its own depth
/// is a passing thing anyway, because water falls before it does
/// anything else and a cell under water is full a step later. A cell
/// with air above it is a surface, and a surface is drawn at its depth
/// (`surface_height`). The mesher and the test that holds the column
/// together both read this, so they cannot disagree.
///
/// **And a cell with air under it is falling, and is drawn whole** (see
/// [`is_falling`]). That is the front of a fall -- the lowest cell of a
/// stream still on its way down, a cupful tipped off a ledge -- and drawn
/// at its depth it was a plate an eighth thick hanging in the air, the
/// same picture as the ladder [`fall_keeping`] took out of the middle of
/// a waterfall, left at its foot. A falling cell has no surface to draw:
/// it will not be where it is for longer than one flow step, and what it
/// looks like while there is a piece of the column arriving. Its top
/// corners are not sloped to the water beside it either (the mesher asks
/// `is_falling` for that), because a falling cell does not *belong* to
/// the surface it is passing.
///
/// What this does not change is where the water is: the collider and the
/// fog still ask [`covers_with_above`], and for the one flow step a
/// front spends in a cell they see its depth. A player cannot stand in
/// the front of a fall long enough to tell.
#[inline]
pub fn drawn_top(block: BlockId, above: BlockId, below: BlockId) -> f32 {
    if is_liquid(block) && is_lid(above) {
        // Under ice the water reaches the ice. See `is_lid`.
        1.0
    } else if is_liquid(block) && (is_liquid(above) || is_falling(block, below)) {
        1.0 - SURFACE_DROP
    } else {
        surface_height(block)
    }
}

/// Does this block, lying on water, close the water over rather than
/// stand on its surface?
///
/// **Ice, and only ice.** `water::Frost` freezes the top cell of a full
/// column where it stands, so the cell under the ice is water that was
/// never a surface and is not one now: it is full to the top of its cell
/// and the ice's underside is its ceiling. The water under it used to be
/// drawn the way water under *air* is, `SURFACE_DROP` short of the top --
/// so from below there was a surface a twelfth of a block under the ice,
/// hiding it, and at the edge of the lid a slot of nothing between the
/// two that the pond's own side faces did not close. A player diving
/// under a frozen bay called its underside broken.
///
/// The same answer goes to [`surface_height_with_above`], and so to the
/// collider, the drowning check and the fog: an eye in that twelfth is
/// under water, not in a pocket of air under the ice.
///
/// Not any solid block: a block set into a lake *is* stood on the surface
/// round it, and the slot under it is closed from the other side (see
/// `mesh::face_visible`). Ice is different because it is the water's own
/// top cell turned over, flush with every open cell beside it.
#[inline]
pub fn is_lid(above: BlockId) -> bool {
    crate::types::block_kind(above) == crate::types::BLOCK_ICE
}

/// Is this cell water in the air -- nothing under it but air?
///
/// Plain air only, not everything water can fall into. A reed or a tuft
/// under a cell of water is in the same cell as the water that stands
/// round it more often than not -- a shore generated with its plants
/// under the surface -- and calling that cell "falling" would draw a
/// full-height block standing up out of a lake. Water over air, on the
/// other hand, is never at rest: the flow rule moves it down on its next
/// turn, so a cell answering yes here is always on its way somewhere.
#[inline]
pub fn is_falling(block: BlockId, below: BlockId) -> bool {
    is_liquid(block) && is_air(below)
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
/// this -- a column of water is one unbroken box or a deep lake shows a
/// seam across every layer of it -- but it knew it privately, in a
/// two-line `if` of its own, and nothing else did.
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
/// bargain of this module: the collider, the anti-cheat and the drowning
/// check must not each have their own idea of where the top of the water
/// is.
///
/// **The mesher does not read this**, and the reason is worth stating
/// where it will be seen. This says a submerged cell is full *to the top
/// of its own cell*, which is the honest answer to "is this point under
/// water" and the wrong place to put a face: drawn that way, the cell a
/// waterfall lands in stands [`SURFACE_DROP`] proud of the pool around
/// it, and the band it stands proud by is drawn by nobody. So the mesher
/// closes the same seam from the other end -- see [`underhang`] -- and
/// the two agree on the water's *volume*, which is the only thing they
/// have to agree on.
#[inline]
pub fn surface_height_with_above(block: BlockId, above: BlockId) -> f32 {
    if is_liquid(block) && (is_liquid(above) || is_lid(above)) {
        1.0
    } else {
        surface_height(block)
    }
}

/// How far a cell of water is drawn *below its own floor*, given what
/// is under it.
///
/// **The other half of the same seam [`surface_height_with_above`]
/// closes, and the half the mesher draws with.** A column of water has
/// to be one unbroken box from the bed to the surface, and there are
/// only two ways to get that: the lower cell reaches up to the top of
/// its cell, or the upper cell hangs down to meet it. They cost the
/// same and they do not look the same.
///
/// Reaching *up* was what the mesher did, and it put a step in the
/// water. The cell a waterfall lands in stood [`SURFACE_DROP`] proud of
/// the pool around it; the faces it shared with that pool were culled,
/// because water against water is always culled; and its own top face
/// was culled too, because the falling cell above covered it. So
/// nothing at all was drawn in the band between the two surfaces, the
/// pool had a cell-sized hole in it with the bed visible through it,
/// and the falling column hung over the hole with nothing joining them.
/// A player photographed that and called it a cube of water floating in
/// the air.
///
/// Hanging *down* has no such case: the face the two cells share is
/// culled either way, so the only thing that moves is the bottom edge
/// of a submerged cell's sides -- and every water surface in the world
/// stays at one height, which is the whole argument of
/// [`surface_height`].
///
/// What it does **not** change is where the water *is*: the collider,
/// the drowning check and the fog ask [`covers_with_above`], and the
/// union of the drawn boxes down a column is exactly the range that
/// answers yes. Only the partition between one cell and the next moves,
/// and nothing outside the mesher can see a partition.
#[inline]
pub fn underhang(block: BlockId, below: BlockId) -> f32 {
    if is_liquid(block) && is_liquid(below) {
        SURFACE_DROP
    } else {
        0.0
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

// ---------------------------------------------------------------------
// The flow rule. Three small functions, and between them they are the
// whole of what water does.
// ---------------------------------------------------------------------




// # Water as an amount rather than a level
//
// **This is the second model this file has had, and the difference is
// conservation.** The first one asked "how far is this cell from a
// source", and answered with a number that got smaller as it went --
// which is Minecraft's rule, and it works, and it has one consequence
// that turned out to be the whole complaint: a source is full *by
// definition*, for ever. Nothing takes water out of one. So a pond
// could not be emptied, a lake could not be drained, and "осушение
// воды" was not a thing the model could express at all.
//
// Here water is a quantity. A cell holds `1..=SOURCE_DEPTH` of it,
// nothing creates any and nothing destroys any: every change is a
// *transfer* between two cells, one losing exactly what the other
// gains. Cut a channel out of a pond and the pond leaves through it.
//
// ## What this deliberately keeps
//
// The **encoding**. An amount is carried in the same bits, over the
// same range, as the old depth, so `depth`, `surface_height`,
// `covers`, the vertex byte the renderer reads, the collider, the fog
// and the mod API all go on meaning what they meant. A save written
// before this change opens after it. The model changed; the number did
// not, and that is why this was a day's work rather than a month's.
//
// A full cell is still what `is_source` calls a source -- it is just
// no longer eternal. An ocean is full cells surrounded by full cells,
// so nothing moves and it costs nothing, which is the same early exit
// the old model had for the same reason.
//
// ## Why these two functions and no more
//
// Because they are the two things water does: it falls, and it levels.
// Everything else -- fronts, spreading, the seven-block reach of a
// spill -- comes out of applying them, rather than being written down
// separately and then having to agree with them.

/// What moves from a cell into the one below it.
///
/// **Falling before levelling, and all of it at once.** Water does not
/// half-fall: given somewhere to go down, it goes, and only what is
/// left over spreads. Doing it the other way makes a waterfall that
/// puddles at the top, which is the single most obviously wrong thing
/// a fluid can do.
///
/// Returns the amount to take from `here` and give to `below`.
#[inline]
pub fn fall_transfer(here: u8, below: u8) -> u8 {
    let room = SOURCE_DEPTH.saturating_sub(below);
    here.min(room)
}

/// What moves from a cell into the one below it, when the cell is **fed**
/// -- something above it or beside it will hand it more water.
///
/// [`fall_transfer`], except that a fed cell keeps its last eighth. A cell
/// with nothing feeding it falls exactly as before, all of it.
///
/// **Why it exists: a waterfall was a ladder of slabs.** A lake cut
/// through its lip fell as drops in every other cell -- `w3, air, w3,
/// air` -- and nothing was wrong with any single transfer. The lip gave
/// all it had to the cell below; that cell gave all of it on to the next
/// one on the following step; and the lip heard that it had room again
/// only when *that* change woke it, a step after the room appeared. So
/// the lip poured every second step, each pour travelled down as a packet
/// with an empty cell above it, and since every depth is drawn at its own
/// height the column was thin plates hanging in air with nothing between
/// them. The player called the water broken.
///
/// **With this, a cell that is being fed is never emptied by falling**, so
/// a column that is running cannot have a gap in it whatever order the
/// cells are visited in: the lip keeps an eighth while the lake behind it
/// can still pour, the cell under the lip keeps one while the lip holds
/// water, and so on down to the foot. What is kept is one eighth and no
/// more, so the stream still carries everything else down at a cell a
/// step, and a column of mostly-single eighths is still drawn whole --
/// a cell with water above it reaches the top of its cell ([`drawn_top`]).
///
/// **"Fed" is read strictly, and that is what lets the stream stop.** The
/// caller answers it (it is a question about neighbours, not about two
/// numbers): water *above*, which always falls into a cell with room, or
/// a side that would level into this cell once it holds one eighth --
/// `level_transfer(side, 1) > 0`, three eighths or more. Not "any water
/// beside it": a lake drained down to its wedge leaves an eighth or two
/// at the lip, which can pour no more, and a rule that kept a film for
/// *that* would leave the whole fall standing in the air for ever, drawn
/// as a column, over a lake that has stopped. Read strictly, every kept
/// eighth has a neighbour that is about to move, so no state with a kept
/// eighth is a rest state, and the top of a column that is no longer fed
/// empties first and takes the rest down with it.
///
/// Conservation is untouched -- this only ever moves *less* than
/// [`fall_transfer`] -- and so is termination: every fall still lowers
/// the water, and nothing here can move water back up.
///
/// ## What was weighed against it
///
/// * **A "falling" flag in the variant, drawn as a full column.** No room:
///   the three variant bits already count the eighths, all eight values
///   of them, and a second water block id is a protocol, save and mod-API
///   change. And it would draw the drops, not the gaps -- the air between
///   two packets holds no water to flag.
/// * **The mesher filling the air between drops.** The rendering answer,
///   and the wrong one: it draws water where the server, the collider and
///   the fog all say there is none, and a player who stands in the gap
///   breathes in a waterfall.
/// * **Moving the column in lockstep** -- when a cell falls, let the water
///   above it fall into the room at once, up the column, in the same step.
///   The most physical of the three (a stream's depth per cell becomes
///   exactly the lip's flow), and it needs a per-step record of which
///   cells have already moved, or a cell refilled from above falls a
///   second time in the step and the packets merge back into a ladder.
///   It also makes one cell of budget cost a whole column's reads. Keeping
///   an eighth needs neither, and reaches the same picture.
#[inline]
pub fn fall_keeping(here: u8, below: u8, fed: bool) -> u8 {
    let moved = fall_transfer(here, below);
    if fed && moved == here {
        moved.saturating_sub(1)
    } else {
        moved
    }
}

/// What moves sideways between two cells at the same height.
///
/// **Half the difference, and only when the difference is at least
/// two.** Both halves of that are what make the simulation *stop*.
///
/// Half, because moving the whole difference swaps the two cells and
/// the pair oscillates for ever -- the classic way a levelling
/// automaton fails to settle. Half strictly reduces the spread between
/// them every time.
///
/// At least two, because water is in whole units here: a difference of
/// one cannot be split, and a rule that moved it anyway would hand the
/// unit back and forth between two cells at the tick rate, which is
/// both a visible shimmer and an unbounded amount of work for a puddle
/// that has already settled.
///
/// Returns the amount to take from `here` and give to `there`; zero
/// when `there` is the same or higher, so the caller can ask about all
/// four sides without deciding anything itself.
#[inline]
pub fn level_transfer(here: u8, there: u8) -> u8 {
    if here <= there + 1 {
        return 0;
    }
    (here - there) / 2
}

/// What a shower of rain adds to a cell that it lands in.
///
/// **The only thing in this file that makes water**, and every word of
/// the rule is there to bound it. Nothing else creates an eighth; the
/// model exists because the endless source had to go, and a sky that
/// poured for ever would be the endless source with a different name --
/// a pond that cannot be emptied because it fills faster than it drains
/// is exactly the complaint this model was written to answer.
///
/// So: **one eighth, never past full, and never into a dry cell.**
///
/// * *One eighth*, so that what the sky does is slow next to what a
///   channel does. A drain a player has dug has to win.
/// * *Never past full*, so a cell has a ceiling that does not depend on
///   how long it has rained. Rain over an ocean does nothing at all,
///   because an ocean is already full.
/// * *Never into a dry cell*, and this is the one that is not obvious.
///   The sky reaches every open cell in the world, so a rule that wet
///   dry ground would turn the whole landscape into water in a
///   downpour -- a film an eighth deep over every field, which is still
///   water to the collider and the fog. Rain deepens water that is
///   already there. It does not put water where there was none, which
///   means the shape of the world's water is still made by the world
///   and not by the weather.
///
/// The caller decides *where* -- a cell open to the sky, with nothing it
/// can pour into -- because that is a question about the world rather
/// than about a number. See `primitive_server::logic::water::Rainfall`.
#[inline]
pub fn rain_transfer(here: u8) -> u8 {
    if here == 0 || here >= SOURCE_DEPTH {
        0
    } else {
        1
    }
}

/// How fast the open sea carries what floats on it, in blocks a second.
///
/// A third of a block a second is a fifth of a walking pace: a raft left
/// alone crosses twenty blocks in a minute, which is a thing a player
/// notices on a voyage and never a thing that takes a boat away from
/// under them while they look at their pack. A swimmer makes about four
/// blocks a second, so this is a lean, not a wall.
pub const CURRENT_SPEED: f32 = 0.35;

/// How wide one turn of the circulation is, in blocks.
///
/// Two kilometres, so crossing an ocean means crossing several and a
/// coastal trip means feeling one push. Smaller and the sea reads as a
/// washing machine; larger and a whole voyage is one shove in one
/// direction, which is a wind, not a current.
const GYRE: f32 = 2_000.0;

/// How deep the water has to be before it carries anything, and where it
/// carries at full strength, in blocks.
///
/// Nothing in the shallows: a current in the surf would drag a player off
/// a beach they were walking along and would push a raft off a mooring.
/// The shelf is the sea a player builds on, and this leaves it alone. It
/// starts past the shelf's ten blocks and reaches full over the drop that
/// `worldgen::scale::ABYSS_DROP` opened.
const CURRENT_FROM: f32 = 12.0;
const CURRENT_TO: f32 = 28.0;

/// How many days one turn of the pattern takes.
const CURRENT_DRIFT_DAYS: f32 = 6.0;

/// **Which way the open sea is moving here**, in blocks a second, as
/// (x, z), and nothing at all in shallow water.
///
/// **A circulation rather than a direction**, and that is the decision
/// worth writing down. The obvious sea current is "everything drifts
/// south-west", and it is wrong in a way a player finds in an afternoon:
/// water that all moves one way has to come from somewhere and go
/// somewhere, so a raft moored on one coast is gone and a raft on the far
/// coast is pinned against the sand for ever.
///
/// So this is the perpendicular of the gradient of a stream function --
/// the standard way to write a flow that cannot pile water up anywhere,
/// because the field it makes has no sources and no sinks. What comes out
/// is gyres: a raft let go drifts a long curve and comes round, and the
/// sea has *places* -- a belt that carries you west, a belt that carries
/// you back, and the slack water between them where nothing happens.
///
/// **Worked out from the clock and the place, like the wind**
/// (`raft::wind`), so the server that moves a raft and the client that
/// predicts it get the same answer with nothing on the wire. The pattern
/// turns slowly, over [`CURRENT_DRIFT_DAYS`], so the belt a sailor learned
/// last week has moved by this one -- but never fast enough that a crossing
/// changes its mind in the middle.
///
/// Takes the depth rather than looking at the world: this module has no
/// world, and the caller is already holding the column it is asking about.
pub fn current_at(x: f32, z: f32, depth: f32, world_days: f32) -> (f32, f32) {
    let strength = ((depth - CURRENT_FROM) / (CURRENT_TO - CURRENT_FROM)).clamp(0.0, 1.0);
    if strength <= 0.0 {
        return (0.0, 0.0);
    }
    use core::f32::consts::TAU;
    let days = if world_days.is_finite() { world_days } else { 0.0 };
    let turn = days / CURRENT_DRIFT_DAYS * TAU;
    // The stream function, whose level lines are the streamlines: two
    // waves crossed, drifting with the clock.
    //
    //   psi(x, z) = sin(x / GYRE + turn) * cos(z / GYRE - turn)
    //
    // The flow is (d psi / d z, -d psi / d x), which is what makes it
    // divergence-free -- see the note above, and
    // `the_sea_circulates_rather_than_draining_into_a_corner`.
    let (sx, cx) = (x / GYRE + turn).sin_cos();
    let (sz, cz) = (z / GYRE - turn).sin_cos();
    let along_x = -sx * sz;
    let along_z = -cx * cz;
    (along_x * CURRENT_SPEED * strength, along_z * CURRENT_SPEED * strength)
}

#[cfg(test)]
mod tests {
    use super::{current_at, CURRENT_SPEED};

    /// **The sea has to come back.** A flow that all went one way would
    /// take every moored raft off one coast and pin the rest against the
    /// other; this is the property that says it circulates instead. Flow
    /// measured out of a square, which is what "nothing piles up" means:
    /// what goes in comes out.
    #[test]
    fn the_sea_circulates_rather_than_draining_into_a_corner() {
        let deep = 40.0;
        for (cx, cz) in [(0.0, 0.0), (3_000.0, -1_200.0), (-800.0, 5_000.0)] {
            let side = 40.0;
            let mut out_of_it = 0.0;
            let steps = 64;
            for i in 0..steps {
                let t = (i as f32 + 0.5) / steps as f32 * side - side / 2.0;
                // ...through the four walls, outward positive.
                out_of_it += current_at(cx + side / 2.0, cz + t, deep, 1.0).0;
                out_of_it -= current_at(cx - side / 2.0, cz + t, deep, 1.0).0;
                out_of_it += current_at(cx + t, cz + side / 2.0, deep, 1.0).1;
                out_of_it -= current_at(cx + t, cz - side / 2.0, deep, 1.0).1;
            }
            let per_wall = out_of_it / (4.0 * steps as f32);
            assert!(
                per_wall.abs() < CURRENT_SPEED / 100.0,
                "water is piling up at ({cx}, {cz}): {per_wall} a sample out of the square"
            );
        }
    }

    #[test]
    fn the_shallows_carry_nothing_and_the_deep_carries_at_most_its_own_speed() {
        // A current in the surf would drag a player off the beach they
        // were walking along and shift a raft tied up in a bay.
        for depth in [0.0, 4.0, 11.9] {
            assert_eq!(current_at(120.0, -340.0, depth, 2.0), (0.0, 0.0), "the shallows moved at {depth} deep");
        }
        let mut fastest: f32 = 0.0;
        for x in (-4_000..4_000).step_by(97) {
            for z in (-4_000..4_000).step_by(103) {
                let (dx, dz) = current_at(x as f32, z as f32, 40.0, 3.5);
                fastest = fastest.max(dx.hypot(dz));
            }
        }
        assert!(fastest <= CURRENT_SPEED + 1e-4, "the sea ran at {fastest} blocks a second");
        assert!(fastest > CURRENT_SPEED * 0.9, "the sea never reaches its own speed: {fastest}");
    }

    #[test]
    fn the_belts_move_over_the_weeks_but_not_inside_one_crossing() {
        // The pattern turns, or a sailor learns the sea once and it is
        // never news again -- and it must not turn so fast that a
        // crossing begun with the current ends against it.
        let (here, deep) = ((1_500.0, 900.0), 40.0);
        let now = current_at(here.0, here.1, deep, 0.0);
        let in_an_hour = current_at(here.0, here.1, deep, 1.0 / 24.0);
        let in_a_fortnight = current_at(here.0, here.1, deep, 14.0);
        let moved = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1);
        assert!(moved(now, in_an_hour) < CURRENT_SPEED * 0.2, "the sea changed its mind inside an hour");
        assert!(moved(now, in_a_fortnight) > CURRENT_SPEED * 0.2, "the sea is the same sea a fortnight later");
    }

    /// Both sides of the wire work it out; neither sends it.
    #[test]
    fn the_same_place_at_the_same_hour_is_the_same_current() {
        assert_eq!(current_at(210.0, -77.0, 33.0, 4.25), current_at(210.0, -77.0, 33.0, 4.25));
        // ...and a clock that has gone wrong does not put NaN in a raft.
        let (dx, dz) = current_at(0.0, 0.0, 40.0, f32::NAN);
        assert!(dx.is_finite() && dz.is_finite(), "a broken clock made the sea {dx}, {dz}");
    }

    use super::*;
    use crate::types::{BLOCK_STONE, BLOCK_WATER};

    /// Nothing is created and nothing is lost.
    ///
    /// **The property the whole model exists for.** The old rule
    /// computed a level from the neighbours, so a source made water out
    /// of nothing for ever and a pond could not be drained. Here every
    /// change is a transfer: what one cell loses, another gains,
    /// exactly.
    #[test]
    fn moving_water_between_two_cells_neither_makes_nor_loses_any() {
        for here in 0..=SOURCE_DEPTH {
            for there in 0..=SOURCE_DEPTH {
                let sideways = level_transfer(here, there);
                assert!(sideways <= here, "a cell gave away {sideways} of {here}");
                assert!(
                    there + sideways <= SOURCE_DEPTH,
                    "{there} took {sideways} and overflowed",
                );

                let down = fall_transfer(here, there);
                assert!(down <= here, "a cell dropped {down} of {here}");
                assert!(
                    there + down <= SOURCE_DEPTH,
                    "{there} took {down} from above and overflowed",
                );
            }
        }
    }

    /// Levelling settles instead of sloshing.
    ///
    /// **Both halves of `level_transfer` are here to make this true**,
    /// and a levelling automaton that fails it is the classic way this
    /// kind of simulation goes wrong: two cells swapping the same unit
    /// at the tick rate, for ever, which is a shimmer on screen and an
    /// unbounded amount of work under it.
    ///
    /// Run to a fixed point rather than for a fixed count, so the test
    /// says "it stops" rather than "it had not obviously failed after
    /// n steps".
    #[test]
    fn a_row_of_puddles_levels_out_and_then_stands_still() {
        // The worst case for a rule that moves half: everything at one
        // end, nothing at the other.
        let mut row = [SOURCE_DEPTH, 0, 0, 0, 0, 0, 0, 0];
        let total: u32 = row.iter().map(|v| *v as u32).sum();

        let mut steps = 0;
        loop {
            let mut moved = false;
            for i in 0..row.len() - 1 {
                let (left, right) = (row[i], row[i + 1]);
                let there = level_transfer(left, right);
                if there > 0 {
                    row[i] -= there;
                    row[i + 1] += there;
                    moved = true;
                }
                let back = level_transfer(row[i + 1], row[i]);
                if back > 0 {
                    row[i + 1] -= back;
                    row[i] += back;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
            steps += 1;
            assert!(steps < 1000, "the row never settled: {row:?}");
        }

        let after: u32 = row.iter().map(|v| *v as u32).sum();
        assert_eq!(total, after, "levelling changed how much water there was");
        // Settled means no two neighbours differ by more than one --
        // which is as level as whole units can be.
        for pair in row.windows(2) {
            assert!(
                pair[0].abs_diff(pair[1]) <= 1,
                "it stopped without levelling: {row:?}",
            );
        }
    }

    /// An ocean costs nothing, because nothing about it is unequal.
    ///
    /// The old model earned this with an early return on sources. This
    /// one earns it from the rule itself, which is better: there is no
    /// case to remember to keep.
    #[test]
    fn water_beside_water_of_the_same_depth_does_not_move() {
        for depth in 1..=SOURCE_DEPTH {
            assert_eq!(level_transfer(depth, depth), 0);
        }
        // ...and a full cell cannot pour into another full one, which
        // is the same statement for the cell below.
        assert_eq!(fall_transfer(SOURCE_DEPTH, SOURCE_DEPTH), 0);
    }

    /// Water falls before it spreads, and falls all at once.
    #[test]
    fn a_cell_with_somewhere_to_fall_empties_into_it_first() {
        // Into nothing: all of it goes.
        assert_eq!(fall_transfer(5, 0), 5);
        // Into something part full: as much as fits.
        assert_eq!(fall_transfer(5, SOURCE_DEPTH - 2), 2);
        // A waterfall does not puddle at the top: whatever is left
        // after the fall is what spreads, and here nothing is left.
        let here = 6;
        let dropped = fall_transfer(here, 0);
        assert_eq!(here - dropped, 0, "water stayed up when it could have fallen");
    }

    /// A fed cell keeps one eighth of a fall and nothing else changes.
    #[test]
    fn a_fed_falling_cell_keeps_one_eighth_and_an_unfed_one_keeps_nothing() {
        for here in 0..=SOURCE_DEPTH {
            for below in 0..=SOURCE_DEPTH {
                let plain = fall_transfer(here, below);
                // Unfed is the old rule exactly: a drop, or the last of a
                // stream, still leaves nothing behind in the air.
                assert_eq!(fall_keeping(here, below, false), plain);
                let fed = fall_keeping(here, below, true);
                // Never more than the old rule, so no water is made...
                assert!(fed <= plain, "{here} over {below} fell {fed}, more than {plain}");
                // ...and a fed cell with water in it is never emptied.
                if here > 0 && plain > 0 {
                    assert!(here - fed >= 1, "{here} over {below} was emptied while fed");
                }
                // Only the emptying case is touched: a cell whose fall
                // was already stopped by a nearly full cell below keeps
                // what that left it, not an eighth less.
                if plain < here {
                    assert_eq!(fed, plain, "{here} over {below} was held back needlessly");
                }
            }
        }
    }

    /// Levelling settles because it strictly flattens, and that is the
    /// whole proof -- at any distance.
    ///
    /// **Why this is stated as a sum of squares and not as "it looks
    /// settled".** The simulation now applies [`level_transfer`] between
    /// two cells that are not neighbours -- a cell that has just given
    /// water away draws on whatever is standing within reach of it, see
    /// `primitive_server::logic::water` -- and the obvious worry about a
    /// rule that reaches is that it can find a partner four cells away
    /// and trade with it for ever. It cannot, and the reason has nothing
    /// to do with how far apart they are: every transfer strictly
    /// reduces the sum of the squares of the depths, which is a
    /// non-negative integer. A quantity that goes down by at least one
    /// on every write and cannot go below zero is a simulation that
    /// stops.
    ///
    /// Falling is the other half and it is bounded the same way, by a
    /// quantity that counts how high the water is: an eighth that falls
    /// drops a whole storey, and the most the squares can go *up* by
    /// when it lands is 256. So `256 * (how high everything is) + (the
    /// sum of the squares)` falls on every transfer of either kind.
    #[test]
    fn every_transfer_flattens_the_water_and_so_the_simulation_stops() {
        for here in 0..=SOURCE_DEPTH {
            for there in 0..=SOURCE_DEPTH {
                let moved = level_transfer(here, there);
                if moved == 0 {
                    continue;
                }
                let square = |a: u8, b: u8| a as u32 * a as u32 + b as u32 * b as u32;
                assert!(
                    square(here - moved, there + moved) < square(here, there),
                    "moving {moved} from {here} to {there} did not flatten anything"
                );
                // ...and it never overshoots into a swap, which would
                // flatten nothing and oscillate for ever.
                assert!(
                    here - moved >= there + moved,
                    "moving {moved} from {here} to {there} put the deeper cell underneath"
                );
            }
        }
    }

    /// The sky is the only thing here that makes water, and what bounds
    /// it is written into the rule rather than into the caller.
    #[test]
    fn rain_deepens_water_that_is_there_and_never_starts_any() {
        // Nothing from nothing: a dry cell stays dry however long it
        // rains. Every open cell in the world is rained on, and one
        // eighth draws exactly like eight, so the alternative is a
        // meadow that turns into a lake in a downpour.
        assert_eq!(rain_transfer(0), 0);
        // A ceiling that does not depend on how long it rains.
        assert_eq!(rain_transfer(SOURCE_DEPTH), 0);
        for here in 1..SOURCE_DEPTH {
            assert_eq!(rain_transfer(here), 1, "the sky poured {here} deep");
            assert!(here + rain_transfer(here) <= SOURCE_DEPTH);
        }
    }

    #[test]
    fn a_cell_that_is_not_water_has_no_depth() {
        assert_eq!(depth(BLOCK_STONE), 0);
        assert_eq!(depth(crate::types::BLOCK_AIR), 0);
        assert_eq!(surface_height(BLOCK_STONE), 0.0);
        assert!(!is_source(BLOCK_STONE));
        assert!(!is_flowing(BLOCK_STONE));
    }

    #[test]
    fn a_full_cell_is_a_source_and_is_drawn_a_little_low() {
        assert_eq!(depth(BLOCK_WATER), SOURCE_DEPTH);
        assert!(is_source(BLOCK_WATER));
        assert!(!is_flowing(BLOCK_WATER));
        assert!((surface_height(BLOCK_WATER) - (1.0 - SURFACE_DROP)).abs() < 1e-6);
    }

    #[test]
    fn a_save_written_before_any_of_this_reads_back_as_an_ocean_of_sources() {
        // **The whole reason the encoding is this way round.** The
        // variant field's "full" value is a source, and plain
        // `BLOCK_WATER` -- which is what every world ever generated is
        // made of -- carries exactly that. No migration, and an ocean
        // that behaves like an ocean the moment the new rules load.
        assert!(is_source(BLOCK_WATER));
        assert_eq!(with_depth(SOURCE_DEPTH), BLOCK_WATER);
    }

    /// Each eighth is drawn one eighth higher than the last, a full cell
    /// exactly where the sea has always been, and one eighth as a film
    /// rather than nothing.
    #[test]
    fn every_depth_survives_the_round_trip_and_is_drawn_at_its_own_height() {
        let full = surface_height(BLOCK_WATER);
        let mut below = 0.0f32;
        for n in 1..=SOURCE_DEPTH {
            let cell = with_depth(n);
            assert_eq!(depth(cell), n, "depth {n} did not survive the round trip");
            assert_eq!(is_source(cell), n == SOURCE_DEPTH);
            assert_eq!(is_flowing(cell), n < SOURCE_DEPTH);
            let height = surface_height(cell);
            assert!(height > below, "depth {n} is not drawn above depth {}", n - 1);
            assert!(height <= full, "depth {n} is drawn above a full cell");
            let step = full / SOURCE_DEPTH as f32;
            assert!(
                (height - step * n as f32).abs() < 1e-6,
                "depth {n}: {height} is not {n} eighths of a full cell"
            );
            below = height;
        }
        assert!((full - (1.0 - SURFACE_DROP)).abs() < 1e-6, "a full cell has moved");
        assert_eq!(with_depth(0), crate::types::BLOCK_AIR, "nothing is air");
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

        // The last cell of a spill is still a cell of water, and is
        // covered exactly as deeply as it is drawn -- which is an
        // eighth. That is the point: what the collider walks into is
        // what the mesher drew, ankle-deep here and waist-deep there.
        let barely = with_depth(1);
        assert!(covers(barely, 0.05));
        assert!(covers(barely, surface_height(barely) - 0.01));
        assert!(!covers(barely, surface_height(barely) + 0.01));
        assert!(!covers(barely, 0.5), "a film of water is not something you swim in");
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

        // A thin cell under a full one is covered to the top like any
        // other: the depth is invisible, and a seam through the middle
        // of a waterfall is exactly what this prevents.
        let barely = with_depth(1);
        assert!(covers_with_above(barely, BLOCK_WATER, deep));
        assert_eq!(surface_height_with_above(barely, BLOCK_WATER), 1.0);
    }

    #[test]
    fn a_column_of_water_is_drawn_as_one_unbroken_box() {
        // **The bug a player photographed as "a cube of water floating
        // in the air".** A column of water -- a waterfall, the depth of
        // a lake, anything with water stacked on water -- is drawn cell
        // by cell, and the boxes have to meet exactly: a gap is a slot
        // you can see the bed through, an overlap is a doubled pane of
        // blending.
        //
        // The mesher draws a cell from `-underhang` to `drawn_top`,
        // both measured from the cell's own floor. So for any two cells
        // one above the other, the top of the lower has to be the bottom
        // of the upper -- whatever the lower's own depth happens to be
        // while the column is still settling.
        let air = crate::types::BLOCK_AIR;
        for lower in [BLOCK_WATER, with_depth(1), with_depth(5)] {
            for upper in [BLOCK_WATER, with_depth(1), with_depth(7)] {
                let lower_top = drawn_top(lower, upper, BLOCK_STONE);
                // The upper cell's floor is one block up, hung down by
                // its underhang.
                let upper_bottom = 1.0 - underhang(upper, lower);
                assert!(
                    (lower_top - upper_bottom).abs() < 1e-6,
                    "a cell of {upper} over a cell of {lower} is drawn from {upper_bottom}                      while the one under it stops at {lower_top}"
                );
            }
            // ...and with air over it, the cell keeps its surface: the
            // drop is what makes a waterline visible, and hanging the
            // *lower* cell up to meet the upper one -- which is what
            // this used to do -- put a step in the middle of a pool.
            assert_eq!(underhang(air, lower), 0.0, "air is not drawn at all");
            assert!(
                (drawn_top(lower, air, BLOCK_STONE) - surface_height(lower)).abs() < 1e-6,
                "a surface cell is drawn at its own depth"
            );
        }
        // Nothing hangs below dry ground, or a pool would be drawn
        // through its own bed.
        assert_eq!(underhang(BLOCK_WATER, BLOCK_STONE), 0.0);
        assert_eq!(underhang(BLOCK_WATER, air), 0.0);
        assert_eq!(underhang(BLOCK_STONE, BLOCK_WATER), 0.0);
    }

    #[test]
    fn water_under_ice_is_full_to_the_ice_for_the_eye_and_for_the_body() {
        let ice = crate::types::BLOCK_ICE;
        for depth in 1..=SOURCE_DEPTH {
            let water = with_depth(depth);
            assert_eq!(drawn_top(water, ice, BLOCK_STONE), 1.0, "{depth} eighths under ice drawn short");
            assert_eq!(surface_height_with_above(water, ice), 1.0);
            assert!(covers_with_above(water, ice, 0.99), "an eye just under the ice is out of the water");
        }
        // A stone set into a lake is still stood on the surface round it.
        assert!(!is_lid(BLOCK_STONE));
        assert_eq!(surface_height_with_above(BLOCK_WATER, BLOCK_STONE), surface_height(BLOCK_WATER));
    }

    #[test]
    fn the_front_of_a_fall_is_drawn_as_a_whole_cell_and_not_as_a_plate() {
        // Water with air under it is on its way down, and an eighth of it
        // drawn at its depth was a plate hanging in the air.
        let air = crate::types::BLOCK_AIR;
        for depth in 1..=SOURCE_DEPTH {
            let falling = with_depth(depth);
            assert_eq!(drawn_top(falling, air, air), 1.0 - SURFACE_DROP, "{depth} eighths in the air");
            assert!(is_falling(falling, air));
            // Standing on anything, it is a surface again.
            for floor in [BLOCK_STONE, BLOCK_WATER, crate::types::BLOCK_TALL_GRASS] {
                assert!(!is_falling(falling, floor));
                assert!((drawn_top(falling, air, floor) - surface_height(falling)).abs() < 1e-6);
            }
        }
        // Air over air is not water falling.
        assert!(!is_falling(air, air));
        assert_eq!(drawn_top(air, air, air), 0.0);
    }

    #[test]
    fn what_is_drawn_and_what_is_swum_in_cover_the_same_water() {
        // The two halves of the same volume, asked from opposite ends.
        // `covers_with_above` is what the collider, the drowning check
        // and the fog read; the mesher reads `underhang` and
        // `surface_height`. They partition a column differently -- the
        // drawn boxes are offset by the drop from the cells they belong
        // to -- and the *union* has to be the same, or a player swims
        // through water that is not where it is drawn.
        //
        // A column of three cells of water standing on stone, sampled
        // finely from the bed to well above the surface.
        let cell_at = |y: i32| if (0..3).contains(&y) { BLOCK_WATER } else { crate::types::BLOCK_AIR };
        for step in -10..=45 {
            let height = step as f32 / 10.0;
            let cell = height.floor() as i32;
            let within = height - cell as f32;
            let swum = covers_with_above(cell_at(cell), cell_at(cell + 1), within);
            // Drawn: some cell's box contains this height.
            let drawn = (-1..=4).any(|y| {
                let block = cell_at(y);
                if !is_liquid(block) {
                    return false;
                }
                let bottom = y as f32 - underhang(block, cell_at(y - 1));
                let top = y as f32 + surface_height(block);
                height >= bottom && height < top
            });
            assert_eq!(
                swum, drawn,
                "at {height} the water is {} but drawn {}",
                if swum { "swum in" } else { "not swum in" },
                if drawn { "there" } else { "nowhere" }
            );
        }
    }







}
