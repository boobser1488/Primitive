//! Falling blocks (sand).
//!
//! Runs on the server, because the world is the server's: a client
//! simulating its own sand would disagree with everyone else's within a
//! second, and any client could then claim sand had landed wherever it
//! liked.
//!
//! ## Shape of the simulation
//!
//! A work queue of coordinates to re-examine, and a short list of
//! blocks currently in the air. A block edit pushes the cells that
//! could newly be unsupported; each tick pops a bounded number of them,
//! and any that turn out to be floating sand leave the grid. Leaving
//! pushes the cell above (whatever was resting on it is now unsupported
//! too), so a whole column peels away over several ticks -- one block
//! per tick per column -- rather than all at once.
//!
//! Two properties this shape buys:
//!
//! * **Bounded work per tick.** A player hollowing out a desert can't
//!   stall the tick loop; the queue just drains over more ticks.
//! * **Nothing is lost.** A block is in the grid or it is in the air,
//!   never neither. A chunk evicted mid-fall leaves the block waiting
//!   in the air until the chunk comes back (see `land`), and a save
//!   finishes every fall before it writes (see `ground_all`) -- both
//!   because the alternative, taken twice by earlier versions of this
//!   file, is a block that simply stops existing.
//!
//! ## Falling blocks are entities
//!
//! A block that starts falling is removed from the world and becomes a
//! **falling-block entity**: a position and a velocity, integrated every
//! tick and replicated to nearby clients, which draw it smoothly between
//! ticks. When it lands it turns back into a block.
//!
//! The earlier version teleported the block down one cell every few
//! ticks. That reads as stuttering, and it also meant the block existed
//! in the world grid the whole way down, so it briefly blocked whatever
//! was below it. An entity has neither problem: the grid cell is empty
//! while it's in the air, and the client interpolates the motion.
//!
//! ## Rock that has been let go of
//!
//! Sand falls because the table says it falls. Stone does not, and must
//! not: a cave roof the generator made has to stay up forever with
//! nobody holding it. And yet a roof somebody has *dug out from under*
//! should come down, because the whole reason to carry logs into a mine
//! is that it might. The two are reconciled by asking the question only
//! at the moment a block is removed. `on_block_changed` records the
//! changed cell as a possible **hole**; if it is air when it is examined,
//! the cell above it is judged by `is_unsupported`, and a cell that fails
//! is let go exactly the way sand is -- it leaves the grid and becomes
//! the same kind of entity. Nothing else is touched: the stone next to
//! it that nobody dug under is never asked, which is the whole of why an
//! untouched cavern is safe to walk into.
//!
//! What "unsupported" means is `Looseness`. How far a collapse may
//! spread from the cell that was dug is `MAX_COLLAPSE_REACH`. What a
//! block does to whoever is standing under it is `strikes`. And what a
//! player carries into a mine to stop all of it is a pit prop, whose
//! reach up the column it stands in is `PROP_REACH`.

use std::collections::VecDeque;

use primitive_shared::protocol::{
    entity_id, BlockChange, EntityId, EntityKind, EntitySource, EntityState,
};
use primitive_shared::types::{
    block_kind, block_weight, can_be_displaced_by_falling, is_affected_by_gravity, is_air,
    is_collidable, is_partial, merge_layers, with_layers, BlockId, BLOCK_AIR, BLOCK_CLAY,
    BLOCK_COAL_ORE, BLOCK_COBBLESTONE, BLOCK_COPPER_ORE, BLOCK_DIRT, BLOCK_GRANITE, BLOCK_GRASS,
    BLOCK_IRON_ORE, BLOCK_LIMESTONE, BLOCK_SANDSTONE, BLOCK_STONE, BLOCK_TIN_ORE, CHUNK_SIZE_Y,
};

/// Cells examined per pass. Generous -- the check is a couple of cached
/// block lookups -- but finite.
pub const MAX_CHECKS_PER_PASS: usize = 512;
/// Gravity for falling entities, blocks per second squared.
const GRAVITY: f32 = -24.0;
/// Terminal speed, so a block dropped from the sky doesn't tunnel
/// through the ground between ticks.
const MAX_FALL_SPEED: f32 = -18.0;
/// A block never falls further than this in one tick, whatever the
/// timestep -- the guarantee that the landing check can't be skipped.
const MAX_STEP: f32 = 0.9;
/// Hard cap on the queue. A pathological edit pattern should degrade
/// into "some sand doesn't fall" rather than into unbounded memory.
const MAX_QUEUE: usize = 64 * 1024;

/// How many cells *sideways* a collapse may spread from the cell that
/// was dug. Zero would drop only the cell directly over the hole; this
/// lets the roof around it follow, one ring per tick, and then stop.
///
/// The count is of horizontal hops only. A vacated cell asking about
/// the cell above it inherits the hop count rather than spending one,
/// so a free-standing pillar whose base is knocked out comes all the
/// way down instead of losing two blocks and hanging there -- a stone
/// column floating in a cave being the kind of thing a player
/// photographs and posts. What bounds the *upward* chain is not this
/// number but the order of work: the cell above a vacated one is judged
/// while the ring around it is still in place, so it sees a span of one
/// and holds. Only a genuine stub -- fewer neighbours than
/// `Looseness::min_neighbours` -- keeps going up, and a stub is finite.
///
/// Two is the figure because it is the smallest that reads as a
/// collapse rather than a single block dropping out of the roof, and
/// with it the most a single dig can bring down is the thirteen roof
/// cells within two hops (a Manhattan ball of radius two). A player
/// who keeps digging under a wide roof restarts the count with every
/// block, which is exactly the decision the mechanic is there to put in
/// front of them; a mountain does not get to make it on its own.
pub const MAX_COLLAPSE_REACH: u8 = 2;

/// Health taken per unit of block weight per block fallen (see
/// `crush_damage`). Health runs to twenty. Stone weighs 2.4, so a
/// roof cell dropping onto a player's head from directly above -- the
/// commonest case, and the one that counts as a fall of one -- takes
/// about a sixth of their health, and the same stone from three cells
/// up takes half. A block of dirt from one cell is under two, which is
/// a sting and a warning rather than an injury.
const CRUSH_DAMAGE_PER_WEIGHT_PER_BLOCK: f32 = 1.4;
/// Every blow counts as having fallen at least this far. A roof cell
/// let go from directly above a player's head has travelled a fifth of
/// a block by the time it touches them, and a stone that weighs 2.4 and
/// hurts for nought point three is not a stone landing on a head.
const MIN_CRUSH_FALL: f32 = 1.0;
/// The most one block can take. Two short of full health, so a single
/// block from the roof of a tall cavern leaves a healthy player alive
/// and a second one does not: a collapse kills, one rock does not, and
/// the player who was hurt gets to decide whether to stay under it.
const MAX_CRUSH_DAMAGE: f32 = 18.0;

/// How readily a kind of rock lets go once something has been dug out
/// from under it.
///
/// Two grades rather than a number per block, because the question a
/// player asks at a face is "is this dirt or is this stone" and the
/// answer has to be the same one the rule gives. The figures are the
/// mechanic: **a two-wide tunnel holds in anything, a three-wide one
/// falls in dirt, a five-wide one falls in stone**, and a log standing
/// floor to ceiling anywhere along the width resets the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Looseness {
    /// Boards and brick: what a player put up themselves.
    ///
    /// **The grade that was deliberately missing, and now is not.** The
    /// rule used to end at rock, on the argument that a plank roof is a
    /// thing a player built to *stop* a collapse -- which made a
    /// floating floor thirty boards wide, held up by nothing, the
    /// cheapest structure in the game. Timber holds far better than
    /// stone and it does not hold forever: six clear cells of board
    /// span themselves, and the seventh wants a post under it or a peg
    /// through it.
    ///
    /// A pegged board is not on this list at all (see
    /// `types::is_pegged`): it is fastened, so it holds whatever the
    /// span, which is the whole of what a peg buys.
    Built,
    /// Stone, ore, cobble and the sedimentary stones.
    Firm,
    /// Dirt and clay.
    Loose,
}

impl Looseness {
    /// A roof cell with fewer solid neighbours than this at its own
    /// height is a stub sticking out over the hole, and drops however
    /// short the span. Two for stone means a corner holds and a lone
    /// tongue does not; three for dirt means the corner of a dirt cliff
    /// slides off when you dig under it, which is what dirt does.
    fn min_neighbours(self) -> usize {
        match self {
            // One for timber, and that is the difference between a
            // board and a rock: a plank nailed to one neighbour is a
            // cantilever and stays up, which is why a porch roof can
            // stick out over a doorway at all. A board with *nothing*
            // beside it is a board lying in the air.
            Looseness::Built => 1,
            Looseness::Firm => 2,
            Looseness::Loose => 3,
        }
    }

    /// The widest open span, measured the *short* way across, whose
    /// roof holds itself up. One more and it comes down.
    ///
    /// The short way, because that is how a roof works: a corridor one
    /// wide and forty long is held by the two walls a metre apart, and
    /// measuring it the long way would drop the roof of every tunnel
    /// anyone dug for more than five blocks -- which was the first
    /// version, and it made mining impossible rather than dangerous.
    fn max_span(self) -> i32 {
        match self {
            // Six: a room six across can be roofed in boards with
            // nothing in the middle of it, which is a hut, and the
            // seventh cell is where a post or a peg becomes the
            // decision. Deliberately wider than stone -- a beam spans
            // and a rock arch does not -- and deliberately finite, so
            // that a hall is something a player has to *engineer*.
            Looseness::Built => 6,
            Looseness::Firm => 4,
            Looseness::Loose => 2,
        }
    }

    /// How well this grade carries *other* material laid under it, as a
    /// number that only ever gets compared. Timber over rock over
    /// earth, which is the order of `max_span` and not an accident: a
    /// thing that spans further is a thing that bears better. See
    /// `bears_over`, the only caller.
    fn strength(self) -> u8 {
        match self {
            Looseness::Built => 3,
            Looseness::Firm => 2,
            Looseness::Loose => 1,
        }
    }
}

/// The materials that can be let go of, and how loose each is.
///
/// The one list, so that "what collapses" is answered in one place.
/// **Sand and gravel are not in it and do not need to be**: the block
/// table already has them falling the moment their floor goes, which is
/// a stricter rule than any here. Leaves, snow and the rest are not
/// structure at all. Anything absent is never let go, which is the safe
/// direction -- a material that should collapse and does not is a
/// missing feature; one that collapses and should not is a lost house.
///
/// **Boards and brick were added in 1.5** and are the reason this is no
/// longer called `rock_looseness`. Until then the only thing that fell
/// was the world, and everything a player put up stood on nothing
/// forever; a floor could be run out over a canyon a board at a time.
/// Timber is generous (see `Looseness::Built`) and it is not free, and
/// what makes it free is a peg or a post -- which is a decision at
/// every joint rather than a number going up.
///
/// **A pegged board is deliberately not on the list.** Neither is a
/// log, which is the post itself, nor the workbench, chest, kiln and
/// the rest of the built furniture: a thing you put in a room is not
/// the room.
pub fn material_looseness(block: BlockId) -> Option<Looseness> {
    // **A rotten board holds like earth**, pegged or not: a peg in punk
    // holds nothing, and a span of it wants something under it every other
    // cell. Asked first, because a pegged board is otherwise not on this
    // list at all. See `primitive_shared::weathering` -- the three stages
    // before this are the warning that it is coming.
    if primitive_shared::weathering::is_rotten(block) {
        return Some(Looseness::Loose);
    }
    // **The ground's rocks, rubble and soils hold as what they stand in for**
    // (`ground::as_common`): a new rock is stone, a rock's cobble is cobble,
    // a soil is dirt -- except the soils that hold a wall (laterite, frozen
    // earth), which are firm, as their rows say by not falling.
    let kind = block_kind(block);
    if kind >= 512 {
        if primitive_shared::ground::is_soil(kind) && !primitive_shared::blocks::definition(kind).falls {
            return Some(Looseness::Firm);
        }
        let common = primitive_shared::ground::as_common(kind);
        if common != kind {
            return material_looseness(common);
        }
        if primitive_shared::ground::rock_of(kind).is_some() {
            return Some(Looseness::Firm);
        }
        if primitive_shared::wood::is_planks(kind) {
            return Some(Looseness::Built);
        }
    }
    match block_kind(block) {
        BLOCK_STONE | BLOCK_COBBLESTONE | BLOCK_SANDSTONE | BLOCK_LIMESTONE | BLOCK_GRANITE
        | BLOCK_COAL_ORE | BLOCK_COPPER_ORE | BLOCK_TIN_ORE | BLOCK_IRON_ORE => {
            Some(Looseness::Firm)
        }
        BLOCK_DIRT | BLOCK_GRASS | BLOCK_CLAY => Some(Looseness::Loose),
        // Dry turf is turf: loose earth under a dry skin.
        primitive_shared::types::BLOCK_DRY_TURF => Some(Looseness::Loose),
        primitive_shared::types::BLOCK_PLANKS
        | primitive_shared::types::BLOCK_BIRCH_PLANKS
        | primitive_shared::types::BLOCK_FIR_PLANKS
        | primitive_shared::types::BLOCK_SAXAUL_PLANKS
        | primitive_shared::types::BLOCK_BRICKS => Some(Looseness::Built),
        _ => None,
    }
}

/// Does a block laid *in* a roof carry the run of `grade` under it?
///
/// **Something that cannot be let go at all does**: a log, a pegged
/// board, a chest, a pillar of anything the collapse rule has never
/// heard of. That is the beam a player lays across a gallery, and it is
/// the older half of this rule.
///
/// **So does anything stronger than what is being measured**, which is
/// the half that arrived when boards became structural. A plank beam in
/// a rock roof still ends a rock run -- timber spans further than stone
/// and always did -- but a rock lintel does not end a *plank* run, and
/// a seam of dirt in a stone roof never ended one either. Without the
/// comparison a plank in a stone roof stopped supporting it the moment
/// planks could fall, which is a mining mechanic quietly deleted by a
/// building one.
fn bears_over(block: BlockId, grade: Looseness) -> bool {
    match material_looseness(block) {
        None => true,
        Some(other) => other.strength() > grade.strength(),
    }
}

/// What counts as holding a roof up, whether standing under the open
/// span or sitting beside a roof cell.
///
/// Any full solid cube, not only wood. A log is what a player *carries*
/// into a mine, and props are the decision this mechanic exists to
/// create -- but a pillar of stone left standing when the room was dug
/// holds the roof just as well, and a rule that said otherwise would
/// have the roof fall on a player who had been careful. Water, a torch,
/// a tuft of grass and a dropped stack hold nothing, and neither does a
/// layer of snow, which is not a cube.
fn holds_a_roof(block: BlockId) -> bool {
    is_collidable(block)
}

/// A cell that changed and may now be air with rock over it.
#[derive(Debug, Clone, Copy)]
struct Hole {
    x: i32,
    y: i32,
    z: i32,
    /// Horizontal hops from the cell that was dug. See
    /// `MAX_COLLAPSE_REACH`.
    hops: u8,
}

/// Which table a struck body's id belongs to.
///
/// Players and animals are numbered by different authorities and a
/// player's id can equal an animal's, so an id alone is not an identity
/// and a block that hit player three must not think it has already hit
/// animal three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Player,
    Animal,
}

/// Something a falling block can land on. The caller says who is where;
/// the simulation says what fell through them.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub kind: BodyKind,
    pub id: u64,
    /// The bottom centre of the box, as a player's position is.
    pub feet: (f32, f32, f32),
    pub half_width: f32,
    pub height: f32,
}

/// A block hit somebody. Data rather than an effect, for the same reason
/// block changes are: what a blow *does* -- the armour, the death
/// screen, the carcass -- belongs to the tick loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blow {
    pub kind: BodyKind,
    pub id: u64,
    pub block: BlockId,
    /// Blocks fallen before it touched them, after the floor of
    /// `MIN_CRUSH_FALL`.
    pub fallen: f32,
    pub damage: f32,
}

/// How much a block hurts, from what it weighs and how far it fell.
///
/// Weight times distance, because those are the two things a player
/// can see: a big rock from high up is the one to be afraid of. The
/// weight is the table's, which is the same figure the fall-damage maths
/// charges a player for carrying it -- so a block hurts the same whether
/// it lands on you or you land with it. The cap and the floor are
/// `MAX_CRUSH_DAMAGE` and `MIN_CRUSH_FALL`.
pub fn crush_damage(block: BlockId, fallen: f32) -> f32 {
    (block_weight(block) * fallen.max(MIN_CRUSH_FALL) * CRUSH_DAMAGE_PER_WEIGHT_PER_BLOCK)
        .min(MAX_CRUSH_DAMAGE)
}

/// What the death screen says. Rock crushes, boards come in, and
/// everything else that falls -- sand, gravel, dirt, clay -- buries.
pub fn crush_cause(block: BlockId) -> &'static str {
    match material_looseness(block) {
        Some(Looseness::Firm) => "was crushed by falling rock",
        // A roof of boards landing on someone is neither rock nor
        // earth, and the death screen saying so is most of what tells
        // a player that their own house did it.
        Some(Looseness::Built) => "was crushed by a collapsing roof",
        _ => "was buried under falling earth",
    }
}

/// The world operations the simulation needs. A trait so the logic can
/// be tested against a plain HashMap instead of a live sharded world.
pub trait BlockWorld {
    /// `None` means "not loaded" -- the simulation then leaves the cell
    /// alone rather than generating terrain to answer.
    fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId>;
    fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId);

    /// Which biome a column is, if this world can say.
    ///
    /// **Only the animals ask**, and they ask once per spawn attempt: a
    /// savanna is not on the blocks -- its turf is a meadow's turf and its
    /// trees are two hundred and forty columns apart -- so the spawner has
    /// to ask the generator which country a spot is in (see
    /// `Species::lives_in`). The server's `World` answers from its
    /// generator; everything else keeps the default and is taken for a
    /// meadow (`logic::animals::UNKNOWN_COUNTRY`).
    ///
    /// **A default method on the trait the animals already hold**, and the
    /// two alternatives were worse. A closure passed to `Animals::step` is a
    /// fourth argument threaded through a hundred test calls and every host
    /// a mod builds, for one question asked every few seconds. Reading the
    /// country off the blocks -- the way `wooded` reads a forest -- cannot
    /// be done at all, for the reason above.
    fn biome(&self, _gx: i32, _gz: i32) -> Option<primitive_shared::worldgen::Biome> {
        None
    }
}

/// A block in mid-air.
#[derive(Debug, Clone)]
pub struct FallingEntity {
    pub id: EntityId,
    pub block: BlockId,
    /// Position of the block's minimum corner.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub velocity_y: f32,
    /// Where it started falling from, for how hard it hits.
    pub start_y: f32,
    /// Where it was at the end of the previous tick. The sweep between
    /// that and `y` is what `strikes` tests, because a block moving
    /// nearly a whole cell a tick can cross a crouching player between
    /// two positions without being *at* either of them.
    prev_y: f32,
    /// Who it has already hit. A block takes several ticks to fall
    /// through a body and hits it once.
    struck: Vec<(BodyKind, u64)>,
}

impl FallingEntity {
    /// A block that has just left the cell `(gx, gy, gz)`.
    pub fn new(id: EntityId, block: BlockId, gx: i32, gy: i32, gz: i32) -> Self {
        Self {
            id,
            block,
            x: gx as f32,
            y: gy as f32,
            z: gz as f32,
            velocity_y: 0.0,
            start_y: gy as f32,
            prev_y: gy as f32,
            struck: Vec::new(),
        }
    }

    pub fn state(&self) -> EntityState {
        EntityState {
            id: self.id,
            kind: EntityKind::FallingBlock { block: self.block },
            x: f64::from(self.x),
            y: f64::from(self.y),
            z: f64::from(self.z),
        }
    }

    /// Everybody this block's movement since the last tick passed
    /// through, hit once each.
    fn strike(&mut self, bodies: &[Body], blows: &mut Vec<Blow>) {
        let (gx, gz) = (self.x.floor(), self.z.floor());
        let low = self.y.min(self.prev_y);
        let high = self.y.max(self.prev_y) + 1.0;
        for body in bodies {
            if self.struck.contains(&(body.kind, body.id)) {
                continue;
            }
            let (bx, by, bz) = body.feet;
            // Strict on every side, the same test `crush_anyone_under`
            // uses for a felled trunk: a player standing on the block
            // that left from under their feet is *touching* it and is
            // not under it.
            let hit = bx + body.half_width > gx
                && bx - body.half_width < gx + 1.0
                && bz + body.half_width > gz
                && bz - body.half_width < gz + 1.0
                && high > by
                && low < by + body.height;
            if !hit {
                continue;
            }
            self.struck.push((body.kind, body.id));
            let fallen = (self.start_y - self.y).max(MIN_CRUSH_FALL);
            blows.push(Blow {
                kind: body.kind,
                id: body.id,
                block: self.block,
                fallen,
                damage: crush_damage(self.block, fallen),
            });
        }
    }
}

#[derive(Default)]
pub struct FallingBlocks {
    queue: VecDeque<(i32, i32, i32)>,
    /// Cells that changed and may be holes with rock over them. Kept
    /// apart from `queue` because the two ask different questions of a
    /// cell: `queue` asks "are *you* sand with nothing under you", this
    /// asks "is the cell *above* you rock with nothing under it" -- and
    /// only of a cell that is air, which is how a placed block and a
    /// removed one are told apart without `on_block_changed` having to
    /// be told which it was.
    holes: VecDeque<Hole>,
    entities: Vec<FallingEntity>,
    /// Blocks that came to rest during the last `advance_entities`,
    /// kept until `strikes` has looked at them. A block that lands in
    /// the same tick it passes through a player would otherwise be gone
    /// before anyone asked whether it hit them -- and the roof cell
    /// directly over a player's head lands within a tick or two.
    landed: Vec<FallingEntity>,
    /// This simulation's own count of the blocks it has put in the air.
    /// **Not the id it replicates them under** -- see
    /// `protocol::EntitySource` for the collision that distinction
    /// exists to prevent.
    next_ordinal: u64,
    dropped: u64,
}

impl FallingBlocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending(&self) -> usize {
        self.queue.len() + self.holes.len()
    }

    /// Whether `strikes` could have anything to say: something is in
    /// the air, or landed since the last step. Lets the tick loop skip
    /// gathering every player's position on the ticks -- nearly all of
    /// them -- when nothing is falling.
    pub fn may_strike(&self) -> bool {
        !self.entities.is_empty() || !self.landed.is_empty()
    }

    /// Total blocks that have finished falling, for `/stats`.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn entities(&self) -> &[FallingEntity] {
        &self.entities
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Entity snapshots within `radius` blocks of a point -- the same
    /// interest filtering players get, so a distant sandslide costs a
    /// player nothing.
    pub fn nearby_states(&self, centre: (f32, f32, f32), radius: f32) -> Vec<EntityState> {
        let radius_squared = radius * radius;
        self.entities
            .iter()
            .filter(|e| {
                let (dx, dy, dz) = (e.x - centre.0, e.y - centre.1, e.z - centre.2);
                dx * dx + dy * dy + dz * dz <= radius_squared
            })
            .map(|e| e.state())
            .collect()
    }

    /// Call after any block change. Queues the cell itself (it may be
    /// sand that can now fall) and the cell above it (whatever was
    /// resting on the old block may now be unsupported) -- and the cell
    /// itself once more as a possible hole, in case it is now air with
    /// rock over it.
    ///
    /// Every change, not only removals, because the caller does not say
    /// which it was and nothing here can tell from a coordinate. The
    /// hole check settles it later by looking: a cell that is not air
    /// when examined was not dug out, and is dropped without its roof
    /// ever being asked about. That is what keeps a torch placed under a
    /// natural cave roof from bringing the roof down -- the changed
    /// cell holds a torch, so it is no hole.
    pub fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.push(gx, gy, gz);
        // **Every cell a prop in this one could have been holding**, not
        // only the one directly over it (`PROP_REACH`). A prop broken out
        // of a gallery has to drop the sand two cells over its head in the
        // same breath, and the cell that sand is in is not adjacent to
        // anything that changed. Two extra cells on the queue per edit,
        // each answered by one lookup and `is_affected_by_gravity` saying
        // no -- which is the whole of what it costs.
        //
        // **Cells and not holes.** A hole pushed up the column would ask
        // the *rock* roof three cells over any dug cell whether it is still
        // supported, and a natural chamber's roof is wider than anything
        // holds itself up over: digging the floor of a cave you had walked
        // through would bring its ceiling down on you. Only a block that
        // falls of its own accord -- sand, gravel, a spadeful of earth --
        // is let go this way, and that is a block that was already
        // unsupported the moment the post went.
        for step in 1..=PROP_REACH {
            self.push(gx, gy + step, gz);
        }
        self.push_hole(gx, gy, gz, 0);
    }

    fn push(&mut self, gx: i32, gy: i32, gz: i32) {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 || self.pending() >= MAX_QUEUE {
            return;
        }
        self.queue.push_back((gx, gy, gz));
    }

    fn push_hole(&mut self, x: i32, y: i32, z: i32, hops: u8) {
        // The roof is the cell above, so a hole in the top layer of the
        // world has none. The cap is `MAX_QUEUE` across *both* queues
        // -- `pending` is their sum -- so that what the simulation can
        // be made to hold did not double the day holes were added.
        if y < 0 || y + 1 >= CHUNK_SIZE_Y as i32 || self.pending() >= MAX_QUEUE {
            return;
        }
        self.holes.push_back(Hole { x, y, z, hops });
    }

    /// Advances the simulation by `dt` seconds.
    ///
    /// Two halves: cells to re-examine (does anything start falling?)
    /// and entities already in the air (do they land?).
    pub fn step<W: BlockWorld + ?Sized>(&mut self, world: &W, dt: f32) -> Vec<BlockChange> {
        self.step_budgeted(world, dt, MAX_CHECKS_PER_PASS)
    }

    /// The same, with the number of cells to examine handed in.
    ///
    /// The budget is the whole of what makes this safe to run on a tick
    /// loop, so it belongs to the caller: when several mechanics share
    /// a tick they have to share the work as well (see
    /// `logic::simulation`). `step` keeps the figure this simulation
    /// ran at on its own for its whole life.
    pub fn step_budgeted<W: BlockWorld + ?Sized>(
        &mut self,
        world: &W,
        dt: f32,
        budget: usize,
    ) -> Vec<BlockChange> {
        // A quarter of the budget to holes. A hole costs more to judge
        // than a cell of sand -- up to two runs of ten lookups each --
        // and there are far fewer of them: one per edit against the
        // sand queue's two, and most are answered by the first lookup
        // (not air, or nothing above that can fall). Sharing the budget
        // rather than adding to it keeps the tick's ceiling where it was.
        let hole_budget = (budget / 4).max(1);
        let mut changes = self.spawn_new_falls(world, budget.saturating_sub(hole_budget).max(1));
        changes.extend(self.let_go_roofs(world, hole_budget));
        changes.extend(self.advance_entities(world, dt));
        changes
    }

    /// Everything that fell through somebody since the last step, each
    /// body hit once per block. Call after `step`, with where everyone
    /// is; landings from that step are looked at too and then forgotten.
    pub fn strikes(&mut self, bodies: &[Body]) -> Vec<Blow> {
        let mut blows = Vec::new();
        let mut landed = std::mem::take(&mut self.landed);
        if bodies.is_empty() {
            return blows;
        }
        for entity in self.entities.iter_mut().chain(landed.iter_mut()) {
            entity.strike(bodies, &mut blows);
        }
        blows
    }

    /// Finishes every fall that is still in progress, right now.
    ///
    /// **The world is saved as blocks, and a block in mid-air is not
    /// one.** A save taken while a dune was collapsing wrote the cell
    /// the sand had left -- that edit is in the overlay, it was
    /// broadcast, it is real -- and nothing at all about the sand,
    /// which existed only as an entity in memory. Load the world again
    /// and that block is simply not there. `/save`, an operator
    /// stopping the server and a player leaving a singleplayer world
    /// all go through the same routine, so all three lost it.
    ///
    /// The alternative was to teach the save format about things in
    /// mid-air. That is a format change, and worse, it is a second
    /// place a falling block can be wrong -- one that only shows up
    /// after a restart, which is the hardest kind of bug to be handed.
    /// Running the fall out instead costs a few dozen passes over a
    /// handful of entities and leaves the sand exactly where it was
    /// going.
    ///
    /// Blocks whose column is not in the cache stay in the air, because
    /// there is no cell to write them into: `World::set` on an
    /// uncached chunk does nothing. That is the one case this cannot
    /// rescue, and it is the same one `land` cannot.
    pub fn ground_all<W: BlockWorld + ?Sized>(&mut self, world: &W) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        // A block moves at most `MAX_STEP` per pass however large the
        // timestep, so the height of the world divided by that is the
        // most passes any fall can need. Bounded by the clamp rather
        // than by an optimistic guess.
        let passes = (CHUNK_SIZE_Y as f32 / MAX_STEP) as usize + 2;
        for _ in 0..passes {
            if self.entities.is_empty() {
                break;
            }
            changes.extend(self.advance_entities(world, 1.0));
        }
        changes
    }

    /// Looks at queued cells and turns unsupported blocks into entities.
    fn spawn_new_falls<W: BlockWorld + ?Sized>(
        &mut self,
        world: &W,
        budget: usize,
    ) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        let checks = self.queue.len().min(budget);

        for _ in 0..checks {
            let Some((gx, gy, gz)) = self.queue.pop_front() else {
                break;
            };

            let Some(block) = world.block(gx, gy, gz) else {
                continue; // chunk not loaded; forget it rather than guess
            };
            // **A bite does not survive the fall.** A block somebody has
            // been quarrying carries which face they were working and how
            // far in they had got (`dig`), and neither means anything once
            // the cell it was cut in is three cells above: the shape would
            // land bitten out of a face that is now somewhere else, and
            // sitting in the air beside it. A shovelful of half-cut earth
            // that loses its footing collapses into a shovelful of earth.
            //
            // Taken here rather than where it lands, so that everything
            // downstream -- the entity, the block it writes back, the drop
            // if it cannot land -- sees one whole block.
            // ...and **what lands is what there was** (`dig::settled`): a
            // bite that loses its footing comes to rest as a heap of the
            // quarters it still had. It used to fall whole, and once a
            // handful could be heaped a handful over a hole fell as a block
            // and dug out as four.
            let block = primitive_shared::dig::settled(block);
            if !is_affected_by_gravity(block) {
                continue;
            }
            if gy == 0 {
                continue; // bedrock floor
            }

            let Some(below) = world.block(gx, gy - 1, gz) else {
                continue;
            };
            if !can_be_displaced_by_falling(below) {
                continue; // supported
            }
            // **A pit prop holds what is over it**, and it holds it from
            // the floor of the gallery rather than from the cell directly
            // under the roof (`propped_from_below`, `PROP_REACH`).
            //
            // This used to read `is_prop(below)` and it never once fired: a
            // prop is a whole cube by its row (`blocks`), so
            // `can_be_displaced_by_falling` had already answered "supported"
            // a line above and the roof of a one-cell gallery was held by
            // the ordinary floor rule. What was actually broken is the
            // gallery a player can *walk down*: two cells high, the prop on
            // the floor, the sand two cells over its head -- and nothing
            // between them, so the sand came down on the post as if it were
            // not there. A pit prop is a post cut to the height of the
            // gallery; here it is one cell of block and its reach is what
            // says how tall a gallery it was cut for.
            if propped_from_below(world, gx, gy, gz) {
                continue;
            }

            // Leave the grid and become an entity.
            world.set(gx, gy, gz, BLOCK_AIR);
            changes.push(BlockChange {
                global_x: gx,
                global_y: gy,
                global_z: gz,
                block_id: BLOCK_AIR,
            });

            self.next_ordinal += 1;
            self.entities.push(FallingEntity::new(
                entity_id(EntitySource::FallingBlock, self.next_ordinal),
                block,
                gx,
                gy,
                gz,
            ));

            // Whatever was resting on it is now unsupported -- and the
            // cell it left is a hole under whatever *rock* was resting
            // on it: sand under a stone shelf sliding away is a dig
            // nobody made with a pick, and the shelf is asked the same
            // question.
            self.push(gx, gy + 1, gz);
            self.push_hole(gx, gy, gz, 0);
        }

        changes
    }

    /// Looks at queued holes and lets go of any roof cell over one that
    /// has nothing left holding it.
    fn let_go_roofs<W: BlockWorld + ?Sized>(
        &mut self,
        world: &W,
        budget: usize,
    ) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        let checks = self.holes.len().min(budget);

        for _ in 0..checks {
            let Some(hole) = self.holes.pop_front() else {
                break;
            };
            // Air, and only air. Not "anything a block can fall through":
            // a torch under the roof is a change to that cell and not a
            // hole in it, and water that has run into a real hole is
            // examined a tick later than it would have been, once, in
            // the direction that keeps roofs up.
            if !world.block(hole.x, hole.y, hole.z).is_some_and(is_air) {
                continue;
            }
            let roof_y = hole.y + 1;
            let Some(roof) = world.block(hole.x, roof_y, hole.z) else {
                continue;
            };
            let Some(looseness) = material_looseness(roof) else {
                continue;
            };
            if !is_unsupported(world, hole.x, roof_y, hole.z, looseness) {
                continue;
            }
            self.let_go(world, hole.x, roof_y, hole.z, roof, hole.hops, &mut changes);
        }

        changes
    }

    /// Takes a roof cell out of the grid and puts it in the air, then
    /// queues what its going may have undone.
    #[allow(clippy::too_many_arguments)]
    fn let_go<W: BlockWorld + ?Sized>(
        &mut self,
        world: &W,
        gx: i32,
        gy: i32,
        gz: i32,
        block: BlockId,
        hops: u8,
        changes: &mut Vec<BlockChange>,
    ) {
        world.set(gx, gy, gz, BLOCK_AIR);
        changes.push(BlockChange {
            global_x: gx,
            global_y: gy,
            global_z: gz,
            block_id: BLOCK_AIR,
        });
        self.next_ordinal += 1;
        self.entities.push(FallingEntity::new(
            entity_id(EntitySource::FallingBlock, self.next_ordinal),
            block,
            gx,
            gy,
            gz,
        ));

        // Sand that was resting on it, by the ordinary rule.
        self.push(gx, gy + 1, gz);
        // The cell it left is a hole under whatever is above *that*.
        // Same hop count: going up is free (see `MAX_COLLAPSE_REACH`).
        self.push_hole(gx, gy, gz, hops);
        // And the four roof cells beside it now edge a wider opening.
        // Pushed *after* the cell above, so that when the cell above is
        // judged the ring is still in place and it sees a span of one:
        // the order is what stops a collapse climbing.
        if hops < MAX_COLLAPSE_REACH {
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                self.push_hole(gx + dx, gy - 1, gz + dz, hops + 1);
            }
        }
    }

    /// Integrates entities and lands the ones that hit something.
    fn advance_entities<W: BlockWorld + ?Sized>(&mut self, world: &W, dt: f32) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        // (index, the cell that stopped it). Keeping the blocking cell
        // rather than re-deriving it later is what makes the resting
        // position exact -- see `land`.
        let mut landed: Vec<(usize, i32)> = Vec::new();
        // Last tick's landings have had their chance to be asked about
        // (see `strikes`); anything still here is from a caller that
        // never asks, and must not accumulate.
        self.landed.clear();

        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.prev_y = entity.y;
            entity.velocity_y = (entity.velocity_y + GRAVITY * dt).max(MAX_FALL_SPEED);
            // Clamping the step is what makes the landing check sound:
            // without it a fast block could pass through a floor between
            // two ticks and keep going.
            let step = (entity.velocity_y * dt).max(-MAX_STEP);
            let next_y = entity.y + step;

            let cell_below = next_y.floor() as i32;
            let blocked = cell_below < 0
                || world
                    .block(entity.x.floor() as i32, cell_below, entity.z.floor() as i32)
                    .map(|id| !can_be_displaced_by_falling(id))
                    .unwrap_or(true); // unloaded: stop rather than fall into the unknown

            if blocked {
                landed.push((index, cell_below));
            } else {
                entity.y = next_y;
            }
        }

        // Take them out back-to-front so the indices stay valid, then
        // settle them lowest first.
        //
        // The order matters. A collapsing column becomes several
        // entities, and they can be blocked on the same tick; if the
        // upper one is placed first it takes the cell the lower one was
        // going to occupy, and the lower one then has to search *up*
        // past it. Settling from the bottom means each block lands where
        // it actually fell to, and the stack keeps its original order.
        let mut settling: Vec<(FallingEntity, i32)> = landed
            .into_iter()
            .rev()
            .map(|(index, cell_below)| (self.entities.swap_remove(index), cell_below))
            .collect();
        settling.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));

        for (mut entity, cell_below) in settling {
            if let Some(rest_y) = self.land(world, &mut entity, cell_below, &mut changes) {
                // Kept for `strikes`, with its position brought to the
                // cell it was written into so the sweep it is judged by
                // ends where the block actually is.
                entity.y = rest_y as f32;
                self.landed.push(entity);
                continue;
            }
            // **It could not be put down, so it stays in the air.**
            // The only two reasons `land` refuses are a column that is
            // not in the cache and a column with no room in it, and
            // neither is a reason to destroy a block: the entity asks
            // again next tick, and lands the moment the chunk comes
            // back or somebody digs the cell out. Before this it was
            // simply dropped, which is the "the sand just vanished"
            // half of the bug report.
            //
            // The speed goes with it. It has stopped -- against a wall
            // of stone or against a chunk that has not arrived -- and
            // resuming a fall at terminal velocity because it hovered
            // for a minute would put it through the first floor it met.
            entity.velocity_y = 0.0;
            self.entities.push(entity);
        }

        changes
    }

    /// Turns a stopped entity back into a block.
    ///
    /// The resting cell is the one *above* whatever stopped it, which is
    /// not always the cell the entity's own position is in: between the
    /// two, `cell_below + 1` is the authority, because a block may have
    /// been placed under the entity (or another falling block may have
    /// landed there) since its position was last validated.
    ///
    /// If that cell is occupied the search continues upward. That case is
    /// the whole reason this isn't a one-liner: without it, a column of
    /// sand collapsing loses every block but the first, because each one
    /// lands into the cell the one below it just filled and is silently
    /// discarded.
    /// Loose material lands *into* a shallower drift of the same
    /// material rather than on top of it, which is the one thing that
    /// makes layers behave like material rather than like small blocks.
    /// Sand poured onto a half-filled cell fills it; whatever does not
    /// fit carries on up to the next one, exactly as the column search
    /// already did for whole blocks.
    ///
    /// ## It either lands or it waits; it is never destroyed
    ///
    /// `Some(y)` is the cell it was written into. `None` means "not
    /// yet" -- the caller keeps the entity in the air and asks again
    /// next tick. Two things say it:
    ///
    /// * **A cell that is not loaded.** This used to step over such a
    ///   cell and carry on upward, which is wrong twice: it can put the
    ///   block *above* a cell it was entitled to, and when the whole
    ///   column is out of the cache -- a chunk evicted while the block
    ///   was in the air, which is exactly what happens when a player
    ///   mines under a dune and walks away -- every cell answered
    ///   "unknown" and the block was quietly deleted. `set` on an
    ///   uncached chunk does nothing anyway, so there was never a cell
    ///   to land in; waiting is the only answer that keeps the sand.
    /// * **A column with no room in it**, all the way to the roof of
    ///   the world. Then the block hangs where it stopped, which is
    ///   ugly and honest, and it settles the moment anything opens up.
    ///
    /// ## Why the search is no longer bounded
    ///
    /// It used to give up after eight cells and drop the block on the
    /// floor, on the argument that searching further is O(height²) when
    /// a tall tower lands, and that a block which cannot find a home
    /// within a few cells is in a pocket somebody sealed on purpose.
    ///
    /// The first half of that is wrong: the search stops at the *first*
    /// opening, and for a tower landing on its own stack that is one
    /// cell up. The whole column is only walked when the whole column
    /// is solid, which is the case that used to lose the block. The
    /// second half is a preference, and it was paid for in blocks that
    /// ceased to exist -- a price the rest of this file (see
    /// `nothing_is_created_or_destroyed_by_falling`) refuses to pay.
    /// Resting on top of a pillar you built is surprising; a desert
    /// that evaporates when disturbed is a bug.
    fn land<W: BlockWorld + ?Sized>(
        &mut self,
        world: &W,
        entity: &mut FallingEntity,
        cell_below: i32,
        changes: &mut Vec<BlockChange>,
    ) -> Option<i32> {
        let gx = entity.x.floor() as i32;
        let gz = entity.z.floor() as i32;
        // The search starts at the cell that *stopped* it, not above
        // it: loose material lands into a shallower drift of the same
        // material, and that drift is exactly the thing that stopped
        // it. Anything it cannot merge with is stepped over by the loop
        // below, so a block landing on stone still comes to rest on top
        // of the stone as it always did.
        let start = cell_below.max(0);
        let mut carried = entity.block;

        for gy in start..CHUNK_SIZE_Y as i32 {
            let Some(occupant) = world.block(gx, gy, gz) else {
                // Not loaded, so not a cell anything can be written
                // into -- and not a cell that may be stepped over
                // either, because the answer might have been "yes".
                // Wait for the chunk instead, carrying whatever is
                // left after any partial fill on the way up.
                entity.block = carried;
                return None;
            };

            let landed = if can_be_displaced_by_falling(occupant) {
                carried
            } else if let Some((merged, spilled)) =
                is_partial(occupant).then(|| merge_layers(occupant, carried)).flatten()
            {
                if spilled > 0 {
                    // Fill this cell and take the rest upward.
                    world.set(gx, gy, gz, merged);
                    changes.push(BlockChange {
                        global_x: gx,
                        global_y: gy,
                        global_z: gz,
                        block_id: merged,
                    });
                    self.push(gx, gy, gz);
                    carried = with_layers(carried, spilled);
                    continue;
                }
                merged
            } else {
                continue; // something solid and unrelated; keep looking up
            };

            world.set(gx, gy, gz, landed);
            self.dropped += 1;
            // It may itself be unsupported (a block was mined out from
            // under it mid-fall), and whatever is above it now has
            // something to rest on.
            self.push(gx, gy, gz);
            self.push(gx, gy + 1, gz);
            changes.push(BlockChange {
                global_x: gx,
                global_y: gy,
                global_z: gz,
                block_id: landed,
            });
            return Some(gy);
        }

        // Solid from here to the roof of the world. Nothing to do but
        // hold on to it -- see this function's doc comment. What it
        // holds is what is left: a landing that filled two cells on its
        // way up and ran out of column would otherwise put the whole
        // original block back in the air and quietly mint material.
        entity.block = carried;
        None
    }
}

/// How far over a pit prop the thing it holds may be, in cells.
///
/// **Three: a gallery a player walks down with a pack on, and its roof.**
/// A prop is one cell of block and a real pit prop is a post cut to the
/// height of the working, so the reach is what says how tall a working one
/// post was cut for. Two would hold nothing but a crawl; four and a
/// chamber a player can jump in is held up by a stick on the floor, which
/// is the point at which a post stops being a decision. Past three the
/// answer is a pillar -- props stack, and a stack of them is a pillar
/// (`types::BLOCK_PROP`).
pub const PROP_REACH: i32 = 3;

/// Is `(gx, gy, gz)` standing over a pit prop, with nothing but air
/// between them?
///
/// Air and only air: anything else in the column is either holding the
/// cell up by itself or is a roof of its own, and in both cases the post
/// under it is not what the question is about. Unloaded stops the walk for
/// the reason everything in this file stops at unloaded -- the other guess
/// drops rock on the strength of a chunk nobody can see.
fn propped_from_below<W: BlockWorld + ?Sized>(world: &W, gx: i32, gy: i32, gz: i32) -> bool {
    for step in 1..=PROP_REACH {
        match world.block(gx, gy - step, gz) {
            Some(block) if primitive_shared::types::is_prop(block) => return true,
            Some(block) if is_air(block) => {}
            _ => return false,
        }
    }
    false
}

/// Whether a roof cell -- rock at `(gx, gy, gz)` with a hole under it
/// -- has lost what was holding it up.
///
/// Two ways to fail, either of which is enough. **Too few neighbours**:
/// fewer than `Looseness::min_neighbours` solid cells beside it at its
/// own height, so it is a stub hanging out over the hole. **Too wide a
/// span**: the open run under the roof through the hole, measured the
/// short way across, is wider than `Looseness::max_span`.
///
/// Anything not loaded counts as support. The other guess drops rock
/// on the strength of a chunk nobody can see, and a chunk that arrives
/// later and turns out to have been solid cannot put it back.
fn is_unsupported<W: BlockWorld + ?Sized>(
    world: &W,
    gx: i32,
    gy: i32,
    gz: i32,
    looseness: Looseness,
) -> bool {
    // **A post under it holds it, whatever its neighbours are doing.** The
    // span test below would agree -- a propped run is one cell wide -- but
    // the neighbour test would not: a single cell of roof over a post at
    // the end of a working has no neighbours at all, and it is the one
    // cell most obviously being held up.
    if propped_from_below(world, gx, gy, gz) {
        return false;
    }
    let neighbours = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .filter(|&(dx, dz)| {
            world
                .block(gx + dx, gy, gz + dz)
                .is_none_or(holds_a_roof)
        })
        .count();
    if neighbours < looseness.min_neighbours() {
        return true;
    }
    let span = span_under(world, gx, gy, gz, 1, 0, looseness)
        .min(span_under(world, gx, gy, gz, 0, 1, looseness));
    span > looseness.max_span()
}

/// How many open cells in a row lie under continuous rock along one
/// axis, through the cell beneath the roof cell `(gx, gy, gz)`.
///
/// The run continues while the cell at the hole's height can be fallen
/// through and nothing over it is *bearing*. Either kind of support
/// ends it: a log (or a pillar of anything solid) standing floor to
/// ceiling stops it from below, and a plank or a log laid *in* the roof
/// stops it from above -- which is what makes a beam every few blocks
/// as good as a pillar every few blocks, and the two together the
/// mining a player learns to do.
///
/// Rock over a run cell does not end it, being the very thing in
/// question, and **neither does a gap where the roof has already
/// gone**. The first version stopped the run at a gap, reading "no
/// rock above" as "no roof to hold up", and a collapse then never
/// spread: the roof cell beside the one that had just dropped measured
/// its span up to the hole and found it short. A hole in the roof is
/// the opposite of a support.
///
/// Never looks further than one past the widest span anything holds:
/// the answer beyond that is the same, and a run measured to the end
/// of a long gallery on every edit would be the mechanic's whole cost.
fn span_under<W: BlockWorld + ?Sized>(
    world: &W,
    gx: i32,
    gy: i32,
    gz: i32,
    dx: i32,
    dz: i32,
    looseness: Looseness,
) -> i32 {
    let far_enough = Looseness::Firm.max_span() + 1;
    let mut run = 1;
    for sign in [1, -1] {
        for step in 1..=far_enough {
            let (x, z) = (gx + dx * step * sign, gz + dz * step * sign);
            // ...and a run cell with a post standing under it is not
            // open: the prop ends the span exactly as a pillar of stone
            // does, from wherever in the gallery it was set
            // (`propped_from_below`).
            let open = world.block(x, gy - 1, z).is_some_and(can_be_displaced_by_falling)
                && !propped_from_below(world, x, gy, z);
            // Unloaded counts as bearing, as everywhere in this file.
            let bearing_above = world
                .block(x, gy, z)
                .is_none_or(|b| holds_a_roof(b) && bears_over(b, looseness));
            if !open || bearing_above {
                break;
            }
            run += 1;
        }
    }
    run
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_SAND, BLOCK_STONE, BLOCK_WATER};
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct TestWorld {
        blocks: RefCell<HashMap<(i32, i32, i32), BlockId>>,
        /// Coordinates that read as "not loaded".
        missing: RefCell<Vec<(i32, i32, i32)>>,
        /// What every column answers to `BlockWorld::biome`: nothing, as a
        /// world that cannot say, until a test sets a country.
        biome: std::cell::Cell<Option<primitive_shared::worldgen::Biome>>,
    }

    impl TestWorld {
        /// Makes every column of this world the given biome -- or, with
        /// `None`, a world that cannot say -- for the spawner's sake. The
        /// blocks stay what the test put there.
        pub fn set_biome(&self, biome: Option<primitive_shared::worldgen::Biome>) {
            self.biome.set(biome);
        }
        pub fn put(&self, gx: i32, gy: i32, gz: i32, id: BlockId) {
            self.blocks.borrow_mut().insert((gx, gy, gz), id);
        }
        /// Makes a cell read as "not loaded", which every simulation has
        /// to leave alone rather than guess at.
        pub fn missing_cell(&self, gx: i32, gy: i32, gz: i32) {
            self.missing.borrow_mut().push((gx, gy, gz));
        }
        /// ...and the chunk arrives. The other half of `missing_cell`,
        /// and the only way to test what a simulation does when terrain
        /// it was waiting on finally loads.
        pub fn load_cell(&self, gx: i32, gy: i32, gz: i32) {
            self.missing.borrow_mut().retain(|c| *c != (gx, gy, gz));
        }
        pub fn get(&self, gx: i32, gy: i32, gz: i32) -> BlockId {
            self.blocks
                .borrow()
                .get(&(gx, gy, gz))
                .copied()
                .unwrap_or(BLOCK_AIR)
        }
    }

    impl BlockWorld for TestWorld {
        fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            if self.missing.borrow().contains(&(gx, gy, gz)) {
                return None;
            }
            Some(self.get(gx, gy, gz))
        }
        fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId) {
            self.put(gx, gy, gz, block);
        }
        fn biome(&self, _gx: i32, _gz: i32) -> Option<primitive_shared::worldgen::Biome> {
            self.biome.get()
        }
    }

    const TICK: f32 = 1.0 / 20.0;

    /// Runs until nothing is queued and nothing is in the air.
    fn settle(sim: &mut FallingBlocks, world: &TestWorld, limit: usize) -> Vec<BlockChange> {
        let mut all = Vec::new();
        for _ in 0..limit {
            if sim.pending() == 0 && sim.entity_count() == 0 {
                break;
            }
            all.extend(sim.step(world, TICK));
        }
        all
    }

    #[test]
    fn a_rotten_board_pegged_or_not_holds_like_earth_and_a_grey_one_still_holds_like_a_board() {
        use primitive_shared::types::{BLOCK_PEGGED_PLANKS, BLOCK_PLANKS};
        use primitive_shared::weathering::{weathered, ROTTEN};
        assert_eq!(material_looseness(weathered(BLOCK_PLANKS, ROTTEN)), Some(Looseness::Loose));
        assert_eq!(
            material_looseness(weathered(BLOCK_PEGGED_PLANKS, ROTTEN)),
            Some(Looseness::Loose),
            "a peg still held in rotten wood"
        );
        assert_eq!(material_looseness(weathered(BLOCK_PLANKS, ROTTEN - 1)), Some(Looseness::Built));
        assert_eq!(material_looseness(weathered(BLOCK_PEGGED_PLANKS, ROTTEN - 1)), None, "a grey peg let go");
    }

    #[test]
    fn unsupported_sand_falls_until_it_lands() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 10, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        settle(&mut sim, &world, 100);

        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "should rest on the stone");
        assert_eq!(world.get(0, 10, 0), BLOCK_AIR, "should have left the top");
    }

    #[test]
    fn supported_sand_stays_put() {
        let world = TestWorld::default();
        world.put(0, 5, 0, BLOCK_STONE);
        world.put(0, 6, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 6, 0);
        let changes = settle(&mut sim, &world, 20);

        assert!(changes.is_empty(), "nothing should have moved");
        assert_eq!(world.get(0, 6, 0), BLOCK_SAND);
    }

    #[test]
    fn removing_the_support_makes_the_whole_column_collapse() {
        // This is the case the "push the cell above" rule exists for: a
        // stack of sand has to come down, not just its bottom block.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 1, 0, BLOCK_STONE); // the support, about to be mined
        for y in 2..=6 {
            world.put(0, y, 0, BLOCK_SAND);
        }

        let mut sim = FallingBlocks::new();
        world.set(0, 1, 0, BLOCK_AIR); // player mines it
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 200);

        for y in 1..=5 {
            assert_eq!(world.get(0, y, 0), BLOCK_SAND, "sand missing at y={y}");
        }
        assert_eq!(world.get(0, 6, 0), BLOCK_AIR, "the column should have dropped");
    }

    /// A post on the floor of a gallery a player can walk down, with the
    /// sand two cells over its head: the case the whole mechanic exists
    /// for, and the case it used to fail. See `PROP_REACH`.
    #[test]
    fn a_post_on_the_floor_of_a_gallery_holds_the_sand_over_a_players_head() {
        let prop = primitive_shared::types::placed(primitive_shared::types::BLOCK_PROP, 0.0, (0, 1, 0));
        let world = TestWorld::default();
        world.put(0, 4, 0, BLOCK_STONE); // the floor of the working
        world.put(0, 5, 0, prop); // the post, where a player can set it
        world.put(0, 7, 0, BLOCK_SAND); // the roof, two cells over it

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 5, 0);
        settle(&mut sim, &world, 200);

        assert_eq!(world.get(0, 7, 0), BLOCK_SAND, "the sand came down on the post as if it were not there");
        assert_eq!(world.get(0, 5, 0), prop, "the post itself moved");
    }

    #[test]
    fn the_same_gallery_with_no_post_in_it_caves_in() {
        let world = TestWorld::default();
        world.put(0, 4, 0, BLOCK_STONE);
        world.put(0, 7, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 5, 0);
        settle(&mut sim, &world, 200);

        assert_eq!(world.get(0, 7, 0), BLOCK_AIR, "the sand hung in the air over an open gallery");
        assert_eq!(world.get(0, 5, 0), BLOCK_SAND, "the sand did not land on the floor");
    }

    /// **Cut the post and the roof comes down in the same breath**, and
    /// not the next time somebody digs nearby: the cells a prop could have
    /// been holding are queued by every edit (`on_block_changed`).
    #[test]
    fn breaking_the_post_drops_what_it_was_holding_at_once() {
        let prop = primitive_shared::types::placed(primitive_shared::types::BLOCK_PROP, 0.0, (0, 1, 0));
        let world = TestWorld::default();
        world.put(0, 4, 0, BLOCK_STONE);
        world.put(0, 5, 0, prop);
        world.put(0, 7, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 5, 0);
        settle(&mut sim, &world, 200);
        assert_eq!(world.get(0, 7, 0), BLOCK_SAND, "the post did not hold in the first place");

        world.set(0, 5, 0, BLOCK_AIR); // a player breaks it, or a fire takes it
        sim.on_block_changed(0, 5, 0);
        settle(&mut sim, &world, 200);

        assert_eq!(world.get(0, 7, 0), BLOCK_AIR, "the roof stayed up with nothing under it");
        assert_eq!(world.get(0, 5, 0), BLOCK_SAND, "the sand did not come down to the floor");
    }

    /// A chamber taller than the reach wants a pillar, and a stack of
    /// props is one. See `PROP_REACH` for why the reach is finite.
    #[test]
    fn a_roof_further_over_the_post_than_a_gallery_is_tall_is_not_held() {
        let prop = primitive_shared::types::placed(primitive_shared::types::BLOCK_PROP, 0.0, (0, 1, 0));
        let world = TestWorld::default();
        world.put(0, 4, 0, BLOCK_STONE);
        world.put(0, 5, 0, prop);
        world.put(0, 9, 0, BLOCK_SAND); // four cells over the post

        let mut sim = FallingBlocks::new();
        // The roof's own cell: it is further over the post than the post
        // reaches, which is exactly why the edit under it does not queue it.
        sim.on_block_changed(0, 9, 0);
        settle(&mut sim, &world, 200);
        assert_eq!(world.get(0, 9, 0), BLOCK_AIR, "a stick on the floor held up a chamber");

        // ...and a second prop on the first one does hold it.
        world.set(0, 6, 0, prop);
        world.set(0, 9, 0, BLOCK_SAND);
        sim.on_block_changed(0, 9, 0);
        settle(&mut sim, &world, 200);
        assert_eq!(world.get(0, 9, 0), BLOCK_SAND, "a pillar of props does not hold what one prop nearly did");
    }

    /// **A post ends the span of a rock roof too**, from the floor of the
    /// gallery it is set in: the other half of what propping is for, and
    /// the half that matters in a hall dug wider than rock arches over.
    #[test]
    fn a_post_ends_the_span_of_a_rock_roof_from_the_floor_it_stands_on() {
        let prop = primitive_shared::types::placed(primitive_shared::types::BLOCK_PROP, 0.0, (0, 1, 0));
        let build = |with_post: bool| {
            let world = TestWorld::default();
            // A hall eleven cells across and two high, roofed in stone.
            for x in -5..=5 {
                for z in -5..=5 {
                    world.put(x, 4, z, BLOCK_STONE);
                    world.put(x, 7, z, BLOCK_STONE);
                }
            }
            // **A row of posts every third cell across the hall**, on the
            // floor -- not a floor of them. That is what a player
            // actually does, and it is what says the span is ended from
            // where the post stands rather than that a solid storey is
            // holding the roof up. Across and not in a grid, because the
            // span is measured the short way (`Looseness::max_span`): a
            // grid of posts leaves the lanes between them as long as the
            // hall, and it is the short measure that has to come out
            // under the bound.
            if with_post {
                for x in (-5..=5).step_by(3) {
                    for z in -5..=5 {
                        world.put(x, 5, z, prop);
                    }
                }
            }
            let mut sim = FallingBlocks::new();
            sim.on_block_changed(0, 5, 0);
            sim.on_block_changed(0, 6, 0);
            settle(&mut sim, &world, 400);
            world.get(0, 7, 0)
        };
        assert_eq!(build(false), BLOCK_AIR, "a hall eleven cells across roofed in stone stood up by itself");
        assert_eq!(build(true), BLOCK_STONE, "the posts did not hold the roof of the hall");
    }

    #[test]
    fn sand_sinks_through_water() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        for y in 1..=5 {
            world.put(0, y, 0, BLOCK_WATER);
        }
        world.put(0, 6, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 6, 0);
        settle(&mut sim, &world, 100);

        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "sand should reach the bottom");
    }

    #[test]
    fn other_blocks_are_left_alone() {
        let world = TestWorld::default();
        world.put(0, 10, 0, BLOCK_STONE); // floating stone stays floating

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        let changes = settle(&mut sim, &world, 20);

        assert!(changes.is_empty());
        assert_eq!(world.get(0, 10, 0), BLOCK_STONE);
    }

    #[test]
    fn sand_does_not_fall_out_of_the_world() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 0, 0);
        settle(&mut sim, &world, 20);

        assert_eq!(world.get(0, 0, 0), BLOCK_SAND, "y=0 is the floor");
    }

    #[test]
    fn an_unloaded_chunk_is_skipped_rather_than_guessed() {
        let world = TestWorld::default();
        world.put(0, 10, 0, BLOCK_SAND);
        world.missing.borrow_mut().push((0, 9, 0));

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        let changes = settle(&mut sim, &world, 20);

        assert!(changes.is_empty(), "must not move sand into unloaded space");
        assert_eq!(world.get(0, 10, 0), BLOCK_SAND);
    }

    #[test]
    fn a_falling_block_becomes_an_entity_and_lands_as_a_block() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 10, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);

        // First step: it leaves the grid and enters the air.
        sim.step(&world, TICK);
        assert_eq!(sim.entity_count(), 1, "should be airborne");
        assert_eq!(world.get(0, 10, 0), BLOCK_AIR, "grid cell must be empty");

        settle(&mut sim, &world, 500);
        assert_eq!(sim.entity_count(), 0, "should have landed");
        assert_eq!(world.get(0, 1, 0), BLOCK_SAND);
    }

    #[test]
    fn a_falling_entity_accelerates_instead_of_moving_at_a_fixed_rate() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 40, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 40, 0);
        sim.step(&world, TICK);

        let mut heights = Vec::new();
        for _ in 0..6 {
            sim.step(&world, TICK);
            heights.push(sim.entities()[0].y);
        }
        let first_drop = heights[0] - heights[1];
        let later_drop = heights[4] - heights[5];
        assert!(
            later_drop > first_drop,
            "gravity should accelerate it ({first_drop} then {later_drop})"
        );
    }

    #[test]
    fn a_fast_block_cannot_tunnel_through_the_floor() {
        // The per-tick step is clamped precisely so the landing check
        // can't be skipped over.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 60, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 60, 0);
        // A deliberately huge timestep, the sort a stalled server
        // produces.
        for _ in 0..400 {
            sim.step(&world, 0.5);
            if sim.entity_count() == 0 {
                break;
            }
        }
        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "it fell through the floor");
    }

    #[test]
    fn entities_are_reported_only_to_players_near_them() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 30, 0, BLOCK_SAND);
        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 30, 0);
        sim.step(&world, TICK);

        assert_eq!(sim.nearby_states((0.0, 30.0, 0.0), 16.0).len(), 1);
        assert!(
            sim.nearby_states((500.0, 30.0, 500.0), 16.0).is_empty(),
            "a distant player should not be told about it"
        );
    }

    #[test]
    fn a_collapsing_column_keeps_every_block() {
        // Regression. A column becomes one entity per block, and they
        // fall independently; the lowest lands first and fills the cell
        // the next one is heading for. The landing code used to give up
        // at that point and drop the block on the floor of the
        // simulation -- so mining under a five-high sand tower left one
        // block of sand and four that had simply ceased to exist.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 1, 0, BLOCK_STONE); // support, about to be mined
        for y in 2..=6 {
            world.put(0, y, 0, BLOCK_SAND);
        }

        let mut sim = FallingBlocks::new();
        world.set(0, 1, 0, BLOCK_AIR);
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 500);

        let recovered = (1..=6).filter(|&y| world.get(0, y, 0) == BLOCK_SAND).count();
        assert_eq!(recovered, 5, "all five blocks should still exist");
        assert_eq!(sim.dropped(), 5, "every block should be reported as landed");
        for y in 1..=5 {
            assert_eq!(world.get(0, y, 0), BLOCK_SAND, "gap at y={y}");
        }
    }

    #[test]
    fn a_tall_column_lands_in_its_original_order() {
        // Ten blocks, so several are in the air at once and some are
        // blocked on the same tick. They must stack, not interleave with
        // gaps or overwrite each other.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        for y in 20..30 {
            world.put(0, y, 0, BLOCK_SAND);
        }

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 20, 0);
        settle(&mut sim, &world, 2000);

        for y in 1..=10 {
            assert_eq!(world.get(0, y, 0), BLOCK_SAND, "gap at y={y}");
        }
        assert_eq!(world.get(0, 11, 0), BLOCK_AIR, "the stack is exactly 10 high");
    }

    #[test]
    fn a_block_placed_under_a_falling_one_mid_flight_does_not_swallow_it() {
        // The cell the entity is *in* was free when it was last checked;
        // by the time it lands someone may have built there. Resting on
        // top of the new block is right; vanishing into it is not.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 12, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 12, 0);
        for _ in 0..4 {
            sim.step(&world, TICK);
        }
        assert_eq!(sim.entity_count(), 1, "should still be airborne");

        // Build a floor right under it, where it is now.
        let occupied = sim.entities()[0].y.floor() as i32;
        world.put(0, occupied, 0, BLOCK_STONE);
        settle(&mut sim, &world, 500);

        assert_eq!(
            world.get(0, occupied + 1, 0),
            BLOCK_SAND,
            "should have come to rest on top of the new block"
        );
    }

    /// How tall the stone plug in the next two tests is. Comfortably
    /// past the eight cells the landing search used to give up after,
    /// so a pass means the search really did keep going rather than
    /// happening to fit inside the old bound.
    const PLUG: i32 = 12;

    #[test]
    fn a_block_with_no_room_above_the_thing_that_stopped_it_climbs_out_instead_of_ceasing_to_exist() {
        // The old behaviour: eight cells of stone above the blocker and
        // the block was deleted, on the argument that a player who
        // sealed a pocket asked for it. What the player actually asked
        // for was a wall; the sand was theirs and it is gone. Now it
        // rides up the column and rests on top, and the wall is still
        // untouched -- which is the part that did matter.
        let world = TestWorld::default();
        for y in 0..PLUG {
            world.put(0, y, 0, BLOCK_STONE);
        }
        let mut sim = FallingBlocks::new();
        sim.entities.push(FallingEntity::new(1, BLOCK_SAND, 0, 2, 0));
        sim.step(&world, TICK);

        assert_eq!(sim.entity_count(), 0, "it should have come to rest");
        assert_eq!(world.get(0, PLUG, 0), BLOCK_SAND, "the block was destroyed");
        for y in 0..PLUG {
            assert_eq!(world.get(0, y, 0), BLOCK_STONE, "stone at y={y} was overwritten");
        }
    }

    #[test]
    fn a_block_with_nowhere_at_all_to_go_stays_in_the_air_rather_than_being_deleted() {
        // Solid to the roof of the world, which is the only case left
        // in which there is genuinely no cell to land in. Hanging there
        // is ugly; it is also the difference between a block a player
        // can still dig out and a block that never existed.
        let world = TestWorld::default();
        for y in 0..CHUNK_SIZE_Y as i32 {
            world.put(0, y, 0, BLOCK_STONE);
        }
        let mut sim = FallingBlocks::new();
        sim.entities.push(FallingEntity::new(1, BLOCK_SAND, 0, 2, 0));
        for _ in 0..10 {
            sim.step(&world, TICK);
        }

        assert_eq!(sim.entity_count(), 1, "the block was thrown away");
        for y in 0..CHUNK_SIZE_Y as i32 {
            assert_eq!(world.get(0, y, 0), BLOCK_STONE, "stone at y={y} was overwritten");
        }
    }

    #[test]
    fn a_block_whose_column_leaves_the_cache_mid_fall_waits_for_it_rather_than_vanishing() {
        // The chunk-eviction case, and the nastiest of the three: a
        // player mines under a dune and walks away, the chunk is
        // evicted while the sand is still in the air, and every cell
        // the landing search asks about answers "not loaded". It used
        // to step over all of them and then run off the end of the
        // search, which deleted the block -- and the cell it came from
        // had already been broadcast as air, so the sand was gone from
        // both ends.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 10, 0, BLOCK_SAND);
        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        sim.step(&world, TICK);
        assert_eq!(sim.entity_count(), 1, "should be airborne");

        // The whole column goes out of the cache.
        for y in 0..CHUNK_SIZE_Y as i32 {
            world.missing_cell(0, y, 0);
        }
        for _ in 0..40 {
            sim.step(&world, TICK);
        }
        assert_eq!(sim.entity_count(), 1, "the block was dropped while nobody could see");

        // ...and the chunk comes back.
        for y in 0..CHUNK_SIZE_Y as i32 {
            world.load_cell(0, y, 0);
        }
        settle(&mut sim, &world, 500);
        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "it never finished falling");
    }

    #[test]
    fn sand_landing_on_sand_stacks_rather_than_vanishing() {
        // What the three layer-depth tests here were really protecting:
        // material is conserved. They measured it in eighths of a cell,
        // which is a unit that no longer exists -- a block of sand is a
        // block of sand -- but a desert that quietly evaporates every
        // time it is disturbed is still the failure worth catching.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 1, 0, BLOCK_SAND);
        world.put(0, 9, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 9, 0);
        settle(&mut sim, &world, 300);

        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "the lower cell emptied");
        assert_eq!(world.get(0, 2, 0), BLOCK_SAND, "the faller went nowhere");
        assert_eq!(world.get(0, 9, 0), BLOCK_AIR, "it never left");
    }

    #[test]
    fn a_falling_block_lands_as_the_block_it_was() {
        // The entity carries its id down with it and puts back exactly
        // that -- no rounding, no substitution.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 10, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        sim.step(&world, TICK);
        assert_eq!(sim.entities()[0].block, BLOCK_SAND, "it changed in mid-air");
        settle(&mut sim, &world, 200);
        assert_eq!(world.get(0, 1, 0), BLOCK_SAND);
    }

    /// Every eighth of loose material in a column, by kind.
    ///
    /// The unit is the eighth rather than the block, because a block and
    /// eight layers of it are the same amount of stuff and the whole
    /// question here is whether that stays true.
    fn material_in(world: &TestWorld, gx: i32, gz: i32) -> std::collections::BTreeMap<BlockId, u32> {
        use primitive_shared::types::{block_kind, block_layers, is_loose};
        let mut totals = std::collections::BTreeMap::new();
        for y in 0..CHUNK_SIZE_Y as i32 {
            let block = world.get(gx, y, gz);
            if is_loose(block) {
                *totals.entry(block_kind(block)).or_insert(0) += block_layers(block) as u32;
            }
        }
        totals
    }

    /// ...plus whatever is still in the air.
    fn material_in_flight(sim: &FallingBlocks) -> std::collections::BTreeMap<BlockId, u32> {
        use primitive_shared::types::{block_kind, block_layers};
        let mut totals = std::collections::BTreeMap::new();
        for entity in sim.entities() {
            *totals.entry(block_kind(entity.block)).or_insert(0) += block_layers(entity.block) as u32;
        }
        totals
    }

    #[test]
    fn nothing_is_created_or_destroyed_by_falling() {
        // A property rather than a case: sand and gravel both fall now,
        // both come in layers, and a landing may merge, overflow into
        // the cell above, or find its cell taken since it was last
        // looked at. Every one of those is a place to quietly lose a
        // block -- which is what the collapsing-column bug was -- or to
        // quietly gain one.
        use primitive_shared::types::{with_layers, BLOCK_GRAVEL};
        let mut seed = 0x1234_5678u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 16
        };

        for case in 0..200 {
            let world = TestWorld::default();
            let mut sim = FallingBlocks::new();
            world.put(0, 0, 0, BLOCK_STONE);

            // A tower of loose material with gaps, ledges and water in
            // it, over a floor with a hole or two.
            let height = 4 + next() % 20;
            for y in 1..height as i32 {
                // The lowest cell is always loose, so every case has
                // something to conserve -- one tower in a hundred came
                // out as stone and water and proved nothing.
                let roll = if y == 1 { 5 + next() % 5 } else { next() % 10 };
                let block = match roll {
                    0 | 1 => BLOCK_STONE,
                    2 => BLOCK_WATER,
                    3 | 4 => BLOCK_AIR,
                    5 | 6 => with_layers(BLOCK_SAND, 1 + (next() % 8) as u8),
                    7 => with_layers(BLOCK_GRAVEL, 1 + (next() % 8) as u8),
                    8 => BLOCK_SAND,
                    _ => BLOCK_GRAVEL,
                };
                world.put(0, y, 0, block);
                sim.on_block_changed(0, y, 0);
            }

            let before = material_in(&world, 0, 0);
            assert!(
                !before.is_empty(),
                "case {case}: a tower with nothing loose in it proves nothing"
            );
            settle(&mut sim, &world, 4000);
            let after = material_in(&world, 0, 0);
            let airborne = material_in_flight(&sim);
            assert!(
                airborne.is_empty(),
                "case {case}: something never landed: {airborne:?}"
            );
            assert_eq!(
                before, after,
                "case {case}: the column started with {before:?} and ended with {after:?}"
            );

            // ...and what is left is *settled*: no loose block with a
            // hole under it, which is the other half of "it fell".
            for y in 1..CHUNK_SIZE_Y as i32 {
                let block = world.get(0, y, 0);
                if is_affected_by_gravity(block)
                    && can_be_displaced_by_falling(world.get(0, y - 1, 0))
                {
                    panic!("case {case}: {block:#x} at y={y} is still floating");
                }
            }
        }
    }

    #[test]
    fn a_world_saved_mid_fall_keeps_the_sand_that_was_in_the_air() {
        // The save writes blocks. A block in mid-air is an entity, and
        // the cell it came from is already recorded as empty -- so
        // before this, `/save` (and leaving a singleplayer world, and
        // stopping the server) turned every block that happened to be
        // falling into nothing at all.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        for y in 20..25 {
            world.put(0, y, 0, BLOCK_SAND);
        }

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 20, 0);
        // Long enough for the whole column to be off the ground and
        // none of it to have landed.
        for _ in 0..8 {
            sim.step(&world, TICK);
        }
        assert!(sim.entity_count() > 1, "the fixture has to be mid-fall");

        let changes = sim.ground_all(&world);
        assert_eq!(sim.entity_count(), 0, "something was still in the air");
        assert!(!changes.is_empty(), "the clients were never told");

        let landed = (0..CHUNK_SIZE_Y as i32)
            .filter(|&y| world.get(0, y, 0) == BLOCK_SAND)
            .count();
        assert_eq!(landed, 5, "the save would have written {landed} of five blocks");
    }

    #[test]
    fn sand_over_a_half_dug_block_stays_where_it_is() {
        // **The decision, and the reason it is this way round.** A bite is
        // not air: a cell with three quarters of a stone block in it still
        // stops a body, still holds a roof up and still carries a drift.
        // What brings sand down is a cell *becoming air*, and a player who
        // wants the drift down finishes the block.
        //
        // The alternative -- a bite that stops holding sand -- was weighed
        // and refused on what it would do: the sand above has nowhere to go
        // (the cell under it is rock, not air), so it would either hang in
        // the air with the simulation waking it every pass or bury the
        // block the player was three quarters of the way through and give
        // nothing back for it.
        use primitive_shared::dig;
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        let bitten = dig::next_bite(BLOCK_STONE, dig::Side::PosX).unwrap();
        world.put(0, 1, 0, bitten);
        world.put(0, 2, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 1, 0);
        sim.on_block_changed(0, 2, 0);
        settle(&mut sim, &world, 400);

        assert_eq!(world.get(0, 2, 0), BLOCK_SAND, "the drift came down onto a block that is still there");
        assert_eq!(world.get(0, 1, 0), bitten, "the bite was buried");
        assert_eq!(sim.entity_count(), 0, "something is still in the air over a floor");
    }

    #[test]
    fn a_bite_that_falls_lands_as_a_heap_of_what_was_left_of_it() {
        // The bite names a face of the cell it was cut in, and three cells
        // down that face is somewhere else -- so what lands lies on the
        // floor of its new cell (`dig::settled`), and it is **as much as
        // there was**: three quarters of a drift is three quarters of a
        // drift. It used to land whole, which was a quarter of sand made
        // out of nothing, and once a handful could be heaped over a hole
        // (`build`) it was four.
        use primitive_shared::dig;
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        let bitten = dig::next_bite(BLOCK_SAND, dig::Side::NegZ).unwrap();
        world.put(0, 6, 0, bitten);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 6, 0);
        settle(&mut sim, &world, 400);

        assert_eq!(world.get(0, 6, 0), BLOCK_AIR, "the bitten drift never left its cell");
        let landed = world.get(0, 1, 0);
        assert_eq!(landed, dig::settled(bitten), "what landed is {landed:#x}");
        assert_eq!(dig::left(landed), dig::left(bitten), "the fall changed how much sand there was");
        assert!(matches!(dig::bite(landed), Some((dig::Side::PosY, _))), "it did not land lying on its floor");
    }

    #[test]
    fn work_per_pass_is_bounded() {
        // A big excavation must not turn one tick into an unbounded loop.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        for i in 0..(MAX_CHECKS_PER_PASS as i32 * 3) {
            world.put(i, 20, 0, BLOCK_SAND);
            sim.on_block_changed(i, 20, 0);
        }
        let before = sim.pending();
        sim.step(&world, TICK);
        assert!(
            sim.pending() > 0,
            "a single pass should not have drained a queue this big"
        );
        assert!(sim.pending() < before + MAX_CHECKS_PER_PASS);
    }

    #[test]
    fn the_queue_cannot_grow_without_limit() {
        let mut sim = FallingBlocks::new();
        for i in 0..(MAX_QUEUE as i32 + 5000) {
            sim.on_block_changed(i, 30, 0);
        }
        assert!(sim.pending() <= MAX_QUEUE);
    }

    #[test]
    fn leaving_the_grid_and_landing_are_both_reported() {
        // Clients need the vacated cell and, later, the filled one --
        // otherwise the block appears to duplicate or vanish.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 3, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 3, 0);

        let leaving = sim.step(&world, TICK);
        assert_eq!(leaving.len(), 1);
        assert_eq!(leaving[0].block_id, BLOCK_AIR);
        assert_eq!(leaving[0].global_y, 3);

        let rest = settle(&mut sim, &world, 500);
        let landing = rest
            .iter()
            .find(|c| c.block_id == BLOCK_SAND)
            .expect("no landing reported");
        assert_eq!(landing.global_y, 1);
    }

    // ---- rock that has been let go of ----

    use primitive_shared::types::{BLOCK_DIRT, BLOCK_LOG, BLOCK_PLANKS, BLOCK_TORCH_LIT};

    /// The height the galleries below are dug at, and the roof over
    /// them. The mass runs from the floor of the world to `TOP`.
    const ROOM_Y: i32 = 5;
    const ROOF_Y: i32 = ROOM_Y + 1;
    const TOP: i32 = 9;

    /// A solid mass of `block`, `half` cells each way from the origin
    /// and `TOP` high. Everything the digging tests start from.
    fn mass(world: &TestWorld, block: BlockId, half: i32) {
        for x in -half..=half {
            for z in -half..=half {
                for y in 0..=TOP {
                    world.put(x, y, z, block);
                }
            }
        }
    }

    /// Hollows out a room `radius` cells each way, two high with its
    /// top at `ROOM_Y`, *without telling the simulation* -- the way the
    /// world generator makes a cave. `keep` is one cell of the top row
    /// left in place for a test to dig out afterwards.
    ///
    /// Two high because a mine is: a room one block high has nowhere for
    /// a fallen roof cell to go but the hole it fell into, where it
    /// stands floor to ceiling and props the roof beside it. That is
    /// correct and it is what the first version of these tests measured
    /// by mistake.
    fn carve(world: &TestWorld, radius: i32, keep: Option<(i32, i32)>) {
        for x in -radius..=radius {
            for z in -radius..=radius {
                world.put(x, ROOM_Y - 1, z, BLOCK_AIR);
                if keep == Some((x, z)) {
                    continue;
                }
                world.put(x, ROOM_Y, z, BLOCK_AIR);
            }
        }
    }

    /// A player's pick: the block goes, and the simulation is told
    /// exactly what an edit tells it.
    fn mine(world: &TestWorld, sim: &mut FallingBlocks, x: i32, y: i32, z: i32) {
        world.set(x, y, z, BLOCK_AIR);
        sim.on_block_changed(x, y, z);
    }

    /// Cells at height `y` within `half` of the origin holding `block`.
    fn count_at(world: &TestWorld, block: BlockId, half: i32, y: i32) -> usize {
        let mut n = 0;
        for x in -half..=half {
            for z in -half..=half {
                if world.get(x, y, z) == block {
                    n += 1;
                }
            }
        }
        n
    }

    /// Every cell of `block` in the mass, at any height.
    fn total(world: &TestWorld, block: BlockId, half: i32) -> usize {
        (0..CHUNK_SIZE_Y as i32)
            .map(|y| count_at(world, block, half, y))
            .sum()
    }

    #[test]
    fn mining_out_the_last_cell_of_a_wide_gallery_brings_the_roof_down() {
        // Five wide, five long, one cell of stone left in the middle of
        // the room: the fifth cell across. Taking it out is what makes
        // the span five, and five is one more than stone holds.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        mass(&world, BLOCK_STONE, 6);
        carve(&world, 2, Some((0, 0)));
        let before = total(&world, BLOCK_STONE, 6);

        mine(&world, &mut sim, 0, ROOM_Y, 0);
        settle(&mut sim, &world, 2000);

        assert_eq!(
            world.get(0, ROOF_Y, 0),
            BLOCK_AIR,
            "the roof over the dug cell stayed up"
        );
        assert_eq!(
            world.get(0, ROOM_Y - 1, 0),
            BLOCK_STONE,
            "the roof should be lying on the floor under the dug cell"
        );
        assert!(
            count_at(&world, BLOCK_AIR, 2, ROOF_Y) > 1,
            "the roof around the hole should have followed it down"
        );
        assert_eq!(sim.entity_count(), 0, "something never landed");
        assert_eq!(
            total(&world, BLOCK_STONE, 6),
            before - 1,
            "a collapse moves stone; it does not make or lose any (the one is the block that was mined)"
        );
    }

    #[test]
    fn a_narrow_tunnel_in_stone_holds() {
        // Two high, one wide, the whole width of the mass -- the
        // ordinary way anyone gets anywhere underground. The first
        // version measured the span the long way and dropped the roof
        // of this on the fifth block.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        mass(&world, BLOCK_STONE, 6);

        let mut changes = Vec::new();
        for x in -6..=6 {
            mine(&world, &mut sim, x, ROOM_Y, 0);
            mine(&world, &mut sim, x, ROOM_Y + 1, 0);
            changes.extend(settle(&mut sim, &world, 200));
        }

        assert!(changes.is_empty(), "something moved: {changes:?}");
        for x in -6..=6 {
            assert_eq!(world.get(x, ROOM_Y + 2, 0), BLOCK_STONE, "roof gone at x={x}");
        }
    }

    #[test]
    fn clay_lets_go_sooner_than_stone() {
        // The same three-wide room in each. Stone holds four; clay
        // holds two. Clay rather than dirt since dirt falls like sand
        // (`BlockDef::falls`): a dirt roof over a dug hole is not a roof
        // that lets go, it is earth pouring in, and that is
        // `dug_earth_pours_into_the_hole_like_sand`.
        for (material, should_fall) in [(BLOCK_STONE, false), (BLOCK_CLAY, true)] {
            let world = TestWorld::default();
            let mut sim = FallingBlocks::new();
            mass(&world, material, 6);
            carve(&world, 1, Some((0, 0)));

            mine(&world, &mut sim, 0, ROOM_Y, 0);
            settle(&mut sim, &world, 2000);

            let roof = world.get(0, ROOF_Y, 0);
            if should_fall {
                assert_eq!(roof, BLOCK_AIR, "a three-wide clay roof should have come down");
            } else {
                assert_eq!(roof, material, "a three-wide stone roof should have held");
            }
        }
    }

    #[test]
    fn dug_earth_pours_into_the_hole_like_sand() {
        // "Сделай землю и любые сыпучие блоки такими же как песок": a
        // mass of earth over a dug-out cell does not stand over it, it
        // runs down into it until the hole is full.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        mass(&world, BLOCK_DIRT, 6);
        carve(&world, 1, Some((0, 0)));
        mine(&world, &mut sim, 0, ROOM_Y, 0);
        settle(&mut sim, &world, 2000);
        assert_eq!(world.get(0, ROOM_Y, 0), BLOCK_DIRT, "the dug cell under a mass of earth stayed open");
    }

    #[test]
    fn a_log_standing_under_the_roof_holds_it_up() {
        // The five-wide room that falls in the first test, with one log
        // stood floor to ceiling at the edge of the span. The run of
        // open cells under the roof stops at it, and four is a span
        // stone holds.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        mass(&world, BLOCK_STONE, 6);
        carve(&world, 2, Some((0, 0)));
        world.put(2, ROOM_Y, 0, BLOCK_LOG);

        mine(&world, &mut sim, 0, ROOM_Y, 0);
        let changes = settle(&mut sim, &world, 2000);

        assert!(changes.is_empty(), "the prop was ignored: {changes:?}");
        assert_eq!(world.get(0, ROOF_Y, 0), BLOCK_STONE);
    }

    #[test]
    fn a_plank_laid_in_the_roof_holds_it_up() {
        // The other kind of prop: not under the span but in the roof.
        // A run is only counted under rock that could fall, and a plank
        // cannot, so the beam ends the span the way a pillar does.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        mass(&world, BLOCK_STONE, 6);
        carve(&world, 2, Some((0, 0)));
        world.put(2, ROOF_Y, 0, BLOCK_PLANKS);

        mine(&world, &mut sim, 0, ROOM_Y, 0);
        let changes = settle(&mut sim, &world, 2000);

        assert!(changes.is_empty(), "the beam was ignored: {changes:?}");
        assert_eq!(world.get(0, ROOF_Y, 0), BLOCK_STONE);
    }

    #[test]
    fn a_natural_cave_does_not_fall_in_until_somebody_digs() {
        // An eleven-wide cavern the generator made, three high, with
        // nobody having told the simulation anything. It has to stay
        // up: through time, through a torch being put up under the
        // roof, through a wall being notched four cells wide -- and
        // come down at the fifth, which is the dig that made a span.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        mass(&world, BLOCK_STONE, 8);
        for x in -5..=5 {
            for z in -5..=5 {
                for y in ROOM_Y - 2..=ROOM_Y {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        let roof_before = count_at(&world, BLOCK_STONE, 8, ROOF_Y);

        let mut changes = Vec::new();
        for _ in 0..100 {
            changes.extend(sim.step(&world, TICK));
        }
        assert!(changes.is_empty(), "the cavern fell in on its own: {changes:?}");

        // A torch under the roof is a change to a cell, not a hole in it.
        world.put(0, ROOM_Y, 0, BLOCK_TORCH_LIT);
        sim.on_block_changed(0, ROOM_Y, 0);
        let changes = settle(&mut sim, &world, 200);
        assert!(changes.is_empty(), "placing a torch brought the roof down: {changes:?}");

        // Notch the east wall at roof height, one cell at a time.
        for z in -2..=1 {
            mine(&world, &mut sim, 6, ROOM_Y, z);
            let changes = settle(&mut sim, &world, 200);
            assert!(changes.is_empty(), "a notch {} wide fell in: {changes:?}", z + 3);
        }
        assert_eq!(count_at(&world, BLOCK_STONE, 8, ROOF_Y), roof_before);

        mine(&world, &mut sim, 6, ROOM_Y, 2);
        settle(&mut sim, &world, 2000);
        assert_eq!(
            world.get(6, ROOF_Y, 2),
            BLOCK_AIR,
            "the fifth cell of the notch should have brought its roof down"
        );
        assert!(
            count_at(&world, BLOCK_STONE, 8, ROOF_Y) < roof_before - 1,
            "the roof beside the notch should have followed"
        );
    }

    /// **A hut stands and a hall does not.** A plank roof laid over a
    /// room six across holds itself once the post in the middle is
    /// taken out; the same roof over a room nine across comes in. This
    /// is the whole of what boards being structural means, and the two
    /// halves have to be one test, because a rule that only ever drops
    /// things is indistinguishable from a rule that is too strict.
    #[test]
    fn a_plank_roof_spans_a_hut_and_falls_in_a_hall() {
        use primitive_shared::types::BLOCK_PLANKS;
        // Five across holds (a hut), nine does not (a hall). The rule is
        // `Looseness::Built::max_span`, and the cell counted is the run
        // under the roof including itself -- so six is the last width
        // that stands on nothing.
        for (half_width, comes_down) in [(2, false), (4, true)] {
            let world = TestWorld::default();
            let mut sim = FallingBlocks::new();
            // Walls of stone, a floor of stone, a roof of boards, and
            // one post of boards holding the middle of it.
            mass(&world, BLOCK_STONE, 12);
            for x in -half_width..=half_width {
                for z in -half_width..=half_width {
                    world.put(x, ROOM_Y - 1, z, BLOCK_AIR);
                    world.put(x, ROOM_Y, z, BLOCK_AIR);
                    world.put(x, ROOF_Y, z, BLOCK_PLANKS);
                }
            }
            world.put(0, ROOM_Y, 0, BLOCK_PLANKS);

            mine(&world, &mut sim, 0, ROOM_Y, 0);
            settle(&mut sim, &world, 5000);

            let roof_gone = world.get(0, ROOF_Y, 0) == BLOCK_AIR;
            assert_eq!(
                roof_gone, comes_down,
                "a room {} across: the roof {} come down",
                half_width * 2 + 1,
                if roof_gone { "did" } else { "did not" }
            );
        }
    }

    /// **A peg is what makes the hall stand.** The same nine-wide room
    /// whose roof came in above, with the boards over the missing post
    /// fastened: nothing falls. A pegged board is not a material that
    /// can be let go (`material_looseness` does not know it), and it
    /// ends the span its neighbours measure across (`span_under` reads
    /// it as bearing), so one peg does two jobs and both are the point.
    #[test]
    fn a_peg_holds_a_span_no_plain_board_would() {
        use primitive_shared::types::{BLOCK_PEGGED_PLANKS, BLOCK_PLANKS};
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        mass(&world, BLOCK_STONE, 12);
        for x in -4..=4 {
            for z in -4..=4 {
                world.put(x, ROOM_Y - 1, z, BLOCK_AIR);
                world.put(x, ROOM_Y, z, BLOCK_AIR);
                world.put(x, ROOF_Y, z, BLOCK_PLANKS);
            }
        }
        // The joint over the post, fastened -- one cell, driven by hand.
        world.put(0, ROOF_Y, 0, BLOCK_PEGGED_PLANKS);
        world.put(0, ROOM_Y, 0, BLOCK_PLANKS);

        mine(&world, &mut sim, 0, ROOM_Y, 0);
        settle(&mut sim, &world, 5000);

        assert_eq!(
            world.get(0, ROOF_Y, 0),
            BLOCK_PEGGED_PLANKS,
            "the pegged board itself let go"
        );
        for x in -4..=4 {
            for z in -4..=4 {
                assert_ne!(
                    world.get(x, ROOF_Y, z),
                    BLOCK_AIR,
                    "({x}, {z}) fell out of a roof a peg was holding"
                );
            }
        }
    }

    #[test]
    fn a_collapse_stops_after_the_cap() {
        // A gallery far wider than any span, in stone and in dirt. The
        // roof comes down within `MAX_COLLAPSE_REACH` hops of the dug
        // cell and nowhere else -- however unsupported the rest of it
        // is -- and the layer above the roof stays where it was in
        // stone. Without the cap this test does not finish: every ring
        // finds the next one just as unsupported.
        let reach = MAX_COLLAPSE_REACH as i32;
        for material in [BLOCK_STONE, BLOCK_CLAY] {
            let world = TestWorld::default();
            let mut sim = FallingBlocks::new();
            mass(&world, material, 24);
            carve(&world, 20, Some((0, 0)));

            mine(&world, &mut sim, 0, ROOM_Y, 0);
            settle(&mut sim, &world, 5000);

            let mut fallen = 0;
            for x in -24..=24 {
                for z in -24..=24 {
                    for y in ROOF_Y..=TOP {
                        if world.get(x, y, z) == BLOCK_AIR {
                            fallen += 1;
                            assert!(
                                x.abs() + z.abs() <= reach,
                                "{material:#x}: ({x}, {y}, {z}) fell, {} hops from the dig",
                                x.abs() + z.abs()
                            );
                        }
                    }
                }
            }
            if material == BLOCK_STONE {
                // 1 + 4 + 8: the Manhattan ball of radius two, and only
                // the roof layer.
                assert_eq!(fallen, 13, "stone: a different patch came down");
                assert_eq!(count_at(&world, BLOCK_STONE, 24, ROOF_Y + 1), 49 * 49);
            } else {
                assert!(fallen >= 13, "clay: {fallen} cells fell, fewer than stone");
            }
            // The chain upward is bounded too, and the reason is the
            // order of work: the first cell of any layer to go is
            // judged with everything beside it still in place, so it
            // never takes the cell above it with it -- each layer loses
            // fewer cells than the one under it and the chimney closes.
            // Dirt, with a span of two, loses seven of the layer above
            // the roof and one of the layer above that; stone loses
            // nothing above the roof. Three layers up, both are whole.
            assert_eq!(
                count_at(&world, material, 24, ROOF_Y + 3),
                49 * 49,
                "{material:#x}: the collapse climbed"
            );
            assert_eq!(sim.entity_count(), 0, "{material:#x}: something never landed");
        }
    }

    #[test]
    fn a_tongue_of_stone_with_one_neighbour_drops_when_dug_under_but_a_corner_with_two_stays() {
        // A ledge two long sticking out of a wall, each cell resting on
        // a block of its own. Dig out from under the tip: it has one
        // neighbour and goes; the cell between it and the wall has two,
        // is still standing on something, and stays.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        for x in -3..=3 {
            world.put(x, 0, 0, BLOCK_STONE); // floor
        }
        for y in 1..=8 {
            world.put(2, y, 0, BLOCK_STONE); // wall
        }
        world.put(1, 6, 0, BLOCK_STONE);
        world.put(0, 6, 0, BLOCK_STONE); // the tip
        world.put(1, 5, 0, BLOCK_STONE);
        world.put(0, 5, 0, BLOCK_STONE); // under the tip, about to go

        mine(&world, &mut sim, 0, 5, 0);
        settle(&mut sim, &world, 500);

        assert_eq!(world.get(0, 6, 0), BLOCK_AIR, "the tip should have dropped");
        assert_eq!(world.get(0, 1, 0), BLOCK_STONE, "...onto the floor");
        assert_eq!(world.get(1, 6, 0), BLOCK_STONE, "the corner should have stayed");
        for y in 1..=8 {
            assert_eq!(world.get(2, y, 0), BLOCK_STONE, "the wall lost a block at y={y}");
        }
    }

    /// Drops one block from `from_y` onto a body, stepping and asking
    /// about blows every tick until it has landed. The floor is at
    /// y = 0, so a standing body's feet are at 1.
    fn drop_onto(block: BlockId, from_y: i32, body: Body, dt: f32) -> Vec<Blow> {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        let mut sim = FallingBlocks::new();
        sim.entities.push(FallingEntity::new(1, block, 0, from_y, 0));
        let mut blows = Vec::new();
        for _ in 0..500 {
            sim.step(&world, dt);
            blows.extend(sim.strikes(&[body]));
            if sim.entity_count() == 0 {
                break;
            }
        }
        assert_eq!(sim.entity_count(), 0, "the fixture never landed");
        blows
    }

    fn standing_player() -> Body {
        Body {
            kind: BodyKind::Player,
            id: 7,
            feet: (0.5, 1.0, 0.5),
            half_width: 0.3,
            height: 1.8,
        }
    }

    #[test]
    fn a_block_that_lands_on_a_player_hurts_them_in_proportion_to_the_fall() {
        let near = drop_onto(BLOCK_STONE, 5, standing_player(), TICK);
        let far = drop_onto(BLOCK_STONE, 8, standing_player(), TICK);
        assert_eq!(near.len(), 1, "one block is one blow, however many ticks it takes: {near:?}");
        assert_eq!(far.len(), 1, "one block is one blow, however many ticks it takes: {far:?}");
        assert_eq!((near[0].kind, near[0].id), (BodyKind::Player, 7));
        assert!(
            far[0].damage > near[0].damage,
            "further should hurt more: {} from 8 against {} from 5",
            far[0].damage,
            near[0].damage
        );
        // Stone from three cells above the head: half of twenty, give
        // or take -- badly hurt and alive.
        let three = drop_onto(BLOCK_STONE, 6, standing_player(), TICK)[0].damage;
        assert!((7.0..14.0).contains(&three), "stone from three blocks did {three}");
        assert_eq!(near[0].block, BLOCK_STONE);
        assert_eq!(crush_cause(near[0].block), "was crushed by falling rock");
    }

    #[test]
    fn a_block_of_dirt_from_directly_overhead_stings_rather_than_wounds() {
        // The floor of one block fallen is what makes this hurt at all:
        // the block has moved a fifth of a cell by the time it touches
        // the head.
        let blows = drop_onto(BLOCK_DIRT, 3, standing_player(), TICK);
        assert_eq!(blows.len(), 1, "{blows:?}");
        assert_eq!(blows[0].fallen, MIN_CRUSH_FALL);
        assert!(
            blows[0].damage > 1.0 && blows[0].damage < 2.5,
            "dirt from overhead did {}",
            blows[0].damage
        );
        assert_eq!(crush_cause(blows[0].block), "was buried under falling earth");
    }

    #[test]
    fn no_block_can_take_more_than_the_cap_in_one_blow() {
        let blows = drop_onto(BLOCK_STONE, 60, standing_player(), TICK);
        assert_eq!(blows.len(), 1);
        assert_eq!(blows[0].damage, MAX_CRUSH_DAMAGE);
    }

    #[test]
    fn a_block_falling_past_somebody_who_is_not_under_it_misses() {
        let mut beside = standing_player();
        beside.feet = (1.5, 1.0, 0.5); // the next cell over
        assert!(drop_onto(BLOCK_STONE, 8, beside, TICK).is_empty());
        // ...and the block a player is standing on, sliding out from
        // under them, is touching them and not on them.
        let mut on_top = standing_player();
        on_top.feet = (0.5, 9.0, 0.5);
        assert!(drop_onto(BLOCK_SAND, 8, on_top, TICK).is_empty());
    }

    #[test]
    fn a_block_that_lands_in_the_same_tick_it_reaches_somebody_still_hits_them() {
        // A short body and a big timestep, so the block's last move
        // both enters the body and puts it on the floor. Before landings
        // were kept for `strikes`, the block was gone from the entity
        // list before anyone asked and the hit was never counted.
        let body = Body {
            kind: BodyKind::Animal,
            id: 3,
            feet: (0.5, 1.0, 0.5),
            half_width: 0.3,
            height: 0.3,
        };
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        let mut sim = FallingBlocks::new();
        let mut entity = FallingEntity::new(1, BLOCK_STONE, 0, 2, 0);
        entity.y = 2.3;
        entity.prev_y = 2.3;
        sim.entities.push(entity);

        sim.step(&world, 0.5);
        assert!(
            sim.strikes(&[body]).is_empty(),
            "the first step stops short of the body"
        );
        sim.step(&world, 0.5);
        assert_eq!(sim.entity_count(), 0, "the fixture should have landed on the second step");
        let blows = sim.strikes(&[body]);
        assert_eq!(blows.len(), 1, "{blows:?}");
        assert_eq!((blows[0].kind, blows[0].id), (BodyKind::Animal, 3));
        assert_eq!(world.get(0, 1, 0), BLOCK_STONE);
        // Asked again, it is gone: a landing is reported once.
        assert!(sim.strikes(&[body]).is_empty());
    }
}

