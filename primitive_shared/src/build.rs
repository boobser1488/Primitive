//! Building as a set of crafts rather than a palette of cubes: handfuls of
//! loose ground, bricks laid course by course in mortar or without it, a dry
//! stone wall of field stones, wattle daubed with clay, and cob raised in
//! lifts that have to dry.
//!
//! ## What the player asked for
//!
//! "Постепенная добыча" took a block off a quarter at a time, and then handed
//! the whole block over at the end -- so the slices were a picture of a dig
//! and not a dig. And a wall was a crafted cube put down with a click, which
//! is the one way nobody has ever built anything. Vintage Story and
//! TerraFirmaCraft both answer it the same way, and it is the answer here: a
//! wall is *laid*, in the place it stands, out of things a person can carry,
//! and how it was laid decides what it is.
//!
//! ## Handfuls
//!
//! Every quarter a spade takes off a soil, a heap of sand or gravel, a clay
//! bank or a heap of cobble is **a handful of it** (`slice_handful`), and the
//! last quarter is the fourth. A handful weighs a quarter of its block, so a
//! pack of handfuls is the same load as the blocks they came out of.
//!
//! Four handfuls are a block again **two ways, and the two are a decision**:
//!
//! - **Heaped in place** (`dig::heaped`): a handful set down is a quarter of a cell
//!   of that material lying on the floor of it, and each one more raises it
//!   a quarter, until the fourth makes the block. What is heaped keeps its
//!   rock: granite gravel heaped is granite gravel. It is the slow way -- four
//!   clicks a cell -- and the exact one.
//! - **Packed by hand** (the "pack" rows at the end of `crafting::RECIPES`):
//!   four handfuls to a block in the pack, and **the block is the common
//!   one** -- dirt, sand, gravel, cobble. A recipe can only name one output,
//!   and fifteen rows per material is the chore `ground` refused for its
//!   rubble. What a player gives up for the speed is the rock.
//!
//! Rejected: **the block at the end, as it was, and nothing on the way.** It
//! is the thing the player called a picture rather than a dig. Rejected too:
//! **the handful as a flag on the block's own id** -- a granite-gravel id
//! with "a quarter of me" set. It is one new id for every material with no
//! table, and every recipe, chest and drop that reads a gravel would then
//! have to ask whether it was holding a gravel or a quarter of one. Five
//! kinds of handful with the material in their spare bits (`handful_of`) is
//! five things a recipe can name.
//!
//! **Only loose ground comes in handfuls**, and which ground is decided by
//! what it drops, not by a list of its own: a block whose drop is a soil, a
//! sand, a gravel, a clay or a cobble gives handfuls of that. Bedrock that
//! comes out as a dressed block of itself -- granite, limestone -- still does,
//! whole, at the last swing: a quarried stone is the one thing that *is* a
//! block. Ore comes out whole for the same reason. Peat is cut, not scooped:
//! a sod is a sod.
//!
//! ## Walls laid in place
//!
//! Four, each a real way of building and each a different trade:
//!
//! | wall | laid from | what it is for | what it costs |
//! |---|---|---|---|
//! | **brick in mortar** | a brick and a trowel of mortar a course | the strongest wall there is | a kiln, and lime or clay for the mortar |
//! | **dry brick** | a brick a course | quick, and it comes down as quickly | nothing holds it: a third of the time to pull down |
//! | **dry stone** | field stones, two a course, on a footing stone | a field wall a sheep will not climb | eight stones a cell, and it lets the wind through |
//! | **wattle and daub** | a stake, woven with rods, daubed | cheap and warm: a wall from a riverbank and a hedge | the daub is mud until it dries, and rain takes it off |
//! | **cob** | lumps of earth, clay and straw, a lift at a time | the warmest, heaviest wall | each lift has to dry before the next |
//!
//! **The stage is in the id**, as a bite is (`dig`): two bits of how far up
//! (`STAGE_SHIFT`, the bits a wood uses on furniture and a wall never has),
//! and one of the variant field for what else the stage needs -- which way a
//! wattle panel runs, whether the top lift of a cob wall is wet. So a wall
//! half built is a block the save, the mesher, the collider and the network
//! already carry, and **a build left half way stays half way**: there is
//! nothing to lose when the chunk unloads.
//!
//! **A course is a quarter of the cell**, bottom up, and the box of what is
//! laid is what is collided and drawn (`stage_box`, through `dig::part_box`).
//! A wattle panel is the exception, and is a panel: two sixteenths of rods
//! across the middle of its cell, four once it is daubed.
//!
//! ### Mortar, and why a wall without it is weaker
//!
//! History first, because the game follows it: the first mortar is **mud** --
//! clay and sand, the Mesopotamian brick's -- and the good one is **lime**:
//! limestone or chalk burnt in a kiln to quicklime, slaked with water and
//! beaten into sand. Both are rows; lime makes four trowels out of what clay
//! makes one, which is what makes the kiln and the walk to white ground worth
//! it once there is a lot of wall to lay.
//!
//! Bricks laid dry are the same bricks with nothing between them, and that
//! is felt the simplest way there is: **they come apart in a third of the
//! time** (the rows' hardness), and they give every brick back where a
//! mortared wall is broken whole into the block of brickwork it is. A dry wall
//! is a wall that is easy to *un*build -- a pen for the night, a wall you
//! will move -- and a mortared one is a house.
//!
//! **The mortar decides at the first course.** A wall begun with mortar in
//! the pack is laid in mortar, and every course after it needs a trowel; a
//! wall begun without is dry to the top. Rejected: mixing courses. A wall that
//! is two courses mortared and two dry has no honest strength, and the id has
//! no room to say which course was which.
//!
//! **A crafted cube of brickwork is no longer put down.** The "brickwork" row
//! stays -- its place in the table is its name on the wire, and the bloomery
//! is built out of the blocks it makes -- but `BLOCK_BRICKS` is not placeable:
//! a wall of bricks in the world is laid, and the fourth mortared course is
//! what writes it.
//!
//! ### The dry stone wall holds a flock at one cell high
//!
//! A pen holds animals by being two cells high: an animal climbs a single
//! step (`logic::animals`' `STEP_HEIGHT`), and a wall one cell high is a
//! step. A finished dry stone wall is the exception, and the reason is its
//! top: a field wall is capped with **cope stones set on edge**, and a sheep
//! will not put its feet on a row of blades. So a finished one bars animals
//! (`bars_animals`) and a player climbs over it -- which is what a field wall
//! is for -- and the decision is stone against height: eight field stones for
//! a cell that holds a flock, or two cells of anything for the same.
//!
//! ### Wet walls
//!
//! Daub and cob are mud until they dry, and they dry the way a cut sod of
//! peat does, by the same rules (`logic::peat::rate` on the server): sun,
//! warmth and wind; slowly under a roof; not at all in frost. **Rain undoes a
//! wet one**: the wet daub washes off the rods, and the wet top lift of a cob
//! wall slumps off it (`washed`). A dry one is finished and stays finished,
//! the peat brick's rule. And **a cob lift cannot go onto a wet one**: that is
//! how cob has always been built, and what makes a cob house a thing started
//! in a dry spell.

use crate::types::{
    block_kind, is_air, BlockId, BLOCK_AIR, BLOCK_BRICK, BLOCK_BRICKS, BLOCK_BRICK_COURSES,
    BLOCK_CLAY, BLOCK_COB, BLOCK_COB_WALL, BLOCK_DAUB, BLOCK_DIRT, BLOCK_DRY_BRICKS, BLOCK_DRY_GRASS,
    BLOCK_DRY_STONE_WALL, BLOCK_HANDFUL_CLAY, BLOCK_HANDFUL_EARTH, BLOCK_HANDFUL_GRAVEL, BLOCK_HANDFUL_SAND,
    BLOCK_MORTAR, BLOCK_MUD, BLOCK_PEBBLE, BLOCK_SANDY_SOIL, BLOCK_STAKE, BLOCK_STICK, BLOCK_STONE_CHIPS,
    BLOCK_TALL_GRASS, BLOCK_WATTLE, VARIANT_MASK, VARIANT_SHIFT, WOOD_HIGH_BIT, WOOD_LOW_SHIFT,
};

// ---- handfuls ----

/// The soils a handful of earth can be of, **dirt first**: a handful's
/// number is its place in this list, and nought is what the pack row makes.
const EARTHS: [BlockId; 12] = {
    let mut all = [BLOCK_DIRT; 12];
    all[1] = BLOCK_SANDY_SOIL;
    let mut i = 0;
    while i < crate::ground::SOILS.len() {
        all[i + 2] = crate::ground::SOILS[i];
        i += 1;
    }
    all
};

/// A form of every rock, in `ground::ROCKS` order -- the common stone's first.
const fn rock_forms(form: crate::ground::Form) -> [BlockId; 15] {
    let mut all = [0; 15];
    let mut i = 0;
    while i < 15 {
        all[i] = crate::ground::ROCKS[i].form(form);
        i += 1;
    }
    all
}
const SANDS: [BlockId; 15] = rock_forms(crate::ground::Form::Sand);
const GRAVELS: [BlockId; 15] = rock_forms(crate::ground::Form::Gravel);
const COBBLES: [BlockId; 15] = rock_forms(crate::ground::Form::Cobble);

/// Every kind of handful.
pub const HANDFULS: [BlockId; 5] =
    [BLOCK_HANDFUL_EARTH, BLOCK_HANDFUL_SAND, BLOCK_HANDFUL_GRAVEL, BLOCK_HANDFUL_CLAY, BLOCK_STONE_CHIPS];

/// What a kind of handful can be a handful of, by its number.
fn sources(handful: BlockId) -> &'static [BlockId] {
    match block_kind(handful) {
        BLOCK_HANDFUL_EARTH => &EARTHS,
        BLOCK_HANDFUL_SAND => &SANDS,
        BLOCK_HANDFUL_GRAVEL => &GRAVELS,
        BLOCK_HANDFUL_CLAY => &[BLOCK_CLAY],
        BLOCK_STONE_CHIPS => &COBBLES,
        _ => &[],
    }
}

/// **Which material rides in the handful's id: four bits**, the variant
/// field and the sixteenth bit. Sixteen numbers for fifteen rocks and twelve
/// soils. The two bits between them (`WOOD_LOW_SHIFT`) stay clear, so a
/// handful is never mistaken for anything that reads them.
const NUMBER_HIGH: BlockId = WOOD_HIGH_BIT;

fn numbered(kind: BlockId, number: usize) -> BlockId {
    let number = number as BlockId;
    kind | ((number & 0b111) << VARIANT_SHIFT) | if number & 0b1000 != 0 { NUMBER_HIGH } else { 0 }
}

fn number_of(id: BlockId) -> usize {
    usize::from(((id & VARIANT_MASK) >> VARIANT_SHIFT) | if id & NUMBER_HIGH != 0 { 0b1000 } else { 0 })
}

/// Is this a handful of anything?
#[inline]
pub fn is_handful(id: BlockId) -> bool {
    HANDFULS.contains(&block_kind(id))
}

/// A handful of `block`, or `None` for a block nobody scoops.
pub fn handful_of(block: BlockId) -> Option<BlockId> {
    let kind = block_kind(block);
    HANDFULS
        .iter()
        .find_map(|&handful| sources(handful).iter().position(|&b| b == kind).map(|n| numbered(handful, n)))
}

/// The block four of these heap into, in its own rock or soil.
pub fn block_of_handful(handful: BlockId) -> Option<BlockId> {
    if !is_handful(handful) || handful & (0b11 << WOOD_LOW_SHIFT) != 0 {
        return None;
    }
    sources(handful).get(number_of(handful)).copied()
}

/// **What one swing at `block` puts in the world beside it**: a handful of
/// what it drops, or `None` for a block that comes away whole at the end
/// (see the module doc for which do).
///
/// Asked of the cell *before* the swing, bitten or whole: the swing takes a
/// quarter of the same material either way.
pub fn slice_handful(block: BlockId) -> Option<BlockId> {
    if !crate::dig::digs_in_slices(block) {
        return None;
    }
    handful_of(crate::types::block_drop(crate::dig::whole(block))?)
}

/// What is left in a bitten cell of a handful material when it is broken:
/// the handful, and how many -- the quarters still standing. `None` for a
/// whole block, which drops as it always did, and for anything not scooped.
pub fn handfuls_left(broken: BlockId) -> Option<(BlockId, u32)> {
    if !crate::dig::is_dug(broken) {
        return None;
    }
    let handful = slice_handful(broken)?;
    let quarters = (crate::dig::left(broken) * f32::from(crate::dig::SLICES)).round() as u32;
    Some((handful, quarters.max(1)))
}

/// A cell a new heap or a new wall may be started in: air, or a tuft of
/// grass the work goes over.
fn open(existing: BlockId) -> bool {
    is_air(existing)
        || crate::ground::is_grass(existing)
        || matches!(block_kind(existing), BLOCK_TALL_GRASS | BLOCK_DRY_GRASS)
}

/// **A handful put down on `existing`**: a quarter of its block on the floor
/// of an open cell, or one more quarter on a heap -- or a bite -- of the same
/// block. `None` for anything else, a handful on a different material
/// included: two materials in a cell is a cell the id cannot describe.
fn heap(existing: BlockId, handful: BlockId) -> Option<BlockId> {
    let block = block_of_handful(handful)?;
    if open(existing) {
        return Some(crate::dig::heaped(block));
    }
    if crate::dig::is_dug(existing) && block_kind(existing) == block_kind(block) {
        return crate::dig::one_back(existing);
    }
    None
}

// ---- walls in stages ----

/// Where a wall keeps how far up it is: the two bits a wood takes on
/// furniture, which no wall is.
const STAGE_SHIFT: u32 = WOOD_LOW_SHIFT;
const STAGE_MASK: BlockId = 0b11 << STAGE_SHIFT;

/// The one variant bit a stage uses: a wattle panel's run, a cob wall's wet
/// top lift. What it means is the kind's, as a bite's bits are the ground's.
const STAGE_FLAG: BlockId = 1 << VARIANT_SHIFT;

/// A wattle panel that runs along x -- thin across z. Clear, it runs along z.
pub const ALONG_X: BlockId = STAGE_FLAG;
/// A cob wall whose top lift has not dried.
pub const WET: BlockId = STAGE_FLAG;

/// The wattle's stages, in the stage bits.
const WOVEN: u8 = 0;
const DAUBED_WET: u8 = 1;
const DAUBED: u8 = 2;

/// Is this a wall part of the way through being laid?
#[inline]
pub fn is_staged(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_BRICK_COURSES | BLOCK_DRY_BRICKS | BLOCK_DRY_STONE_WALL | BLOCK_WATTLE | BLOCK_COB_WALL
    )
}

/// How many courses are laid, 1..=4 -- or, for wattle, which stage it is.
#[inline]
pub fn courses(id: BlockId) -> u8 {
    ((id & STAGE_MASK) >> STAGE_SHIFT) as u8 + 1
}

/// A wattle panel's stage, from nought: `WOVEN`, `DAUBED_WET`, `DAUBED`.
#[inline]
fn stage(id: BlockId) -> u8 {
    courses(id) - 1
}

fn with_courses(kind: BlockId, courses: u8, flag: BlockId) -> BlockId {
    block_kind(kind) | (BlockId::from(courses - 1) << STAGE_SHIFT) | flag
}

/// Is this an id a wall could be? Asked by `types::is_known_block`: the bits
/// are on the wire, and a mortared wall four courses up is `BLOCK_BRICKS`,
/// never itself.
pub fn is_valid(id: BlockId) -> bool {
    let kind = block_kind(id);
    let flags = id & !(crate::types::KIND_MASK | STAGE_MASK);
    let flag_allowed = matches!(kind, BLOCK_WATTLE | BLOCK_COB_WALL);
    if flags != 0 && !(flag_allowed && flags == STAGE_FLAG) {
        return false;
    }
    match kind {
        BLOCK_BRICK_COURSES => courses(id) <= 3,
        BLOCK_WATTLE => stage(id) <= DAUBED,
        _ => is_staged(id),
    }
}

/// Is the top lift of this cob wall, or the daub on this wattle, still wet?
#[inline]
pub fn is_wet(id: BlockId) -> bool {
    match block_kind(id) {
        BLOCK_COB_WALL => id & WET != 0,
        BLOCK_WATTLE => stage(id) == DAUBED_WET,
        _ => false,
    }
}

/// The same wall, dried. Anything not wet comes back as it was.
pub fn dried(id: BlockId) -> BlockId {
    match block_kind(id) {
        BLOCK_COB_WALL => id & !WET,
        BLOCK_WATTLE if is_wet(id) => with_courses(id, DAUBED + 1, id & ALONG_X),
        _ => id,
    }
}

/// **What rain leaves of a wet wall**: the rods, with the daub washed off
/// them; a cob wall one lift lower, and nothing at all where the wet lift
/// was the only one. Anything not wet comes back as it was.
pub fn washed(id: BlockId) -> BlockId {
    match block_kind(id) {
        BLOCK_COB_WALL if is_wet(id) => match courses(id) {
            1 => BLOCK_AIR,
            n => with_courses(id, n - 1, 0),
        },
        BLOCK_WATTLE if is_wet(id) => with_courses(id, WOVEN + 1, id & ALONG_X),
        _ => id,
    }
}

/// How many seconds of the best drying weather a wet wall takes to dry --
/// the peat's clock (`logic::peat::DRY_SECONDS` is fifteen minutes for a
/// sod). Daub is a skin of mud a thumb thick and dries in a third of a sod's
/// time; a lift of cob is a sod the size of a quarter of a room's wall and
/// takes a sod's.
pub fn dry_seconds(id: BlockId) -> f32 {
    match block_kind(id) {
        BLOCK_WATTLE => 300.0,
        _ => 900.0,
    }
}

/// **The box of what has been laid**, in cell units, or `None` for a wall
/// that fills its cell and is drawn and collided as the cube its row is.
///
/// Courses are quarters from the floor up. A wattle panel is across the
/// middle of its cell: two sixteenths of rods, four once it is daubed. A cob
/// wall four lifts up with the top one wet is still a box, so that it wears
/// the wet picture (`drawn_as`) until it has dried into the cube.
pub fn stage_box(id: BlockId) -> Option<([f32; 3], [f32; 3])> {
    if !is_staged(id) {
        return None;
    }
    let n = courses(id);
    match block_kind(id) {
        BLOCK_WATTLE => {
            let half = if stage(id) >= DAUBED_WET { 2.0 / 16.0 } else { 1.0 / 16.0 };
            let (lo, hi) = (0.5 - half, 0.5 + half);
            Some(if id & ALONG_X != 0 { ([0.0, 0.0, lo], [1.0, 1.0, hi]) } else { ([lo, 0.0, 0.0], [hi, 1.0, 1.0]) })
        }
        BLOCK_COB_WALL if n == 4 && is_wet(id) => Some(([0.0; 3], [1.0; 3])),
        _ if n >= 4 => None,
        _ => Some(([0.0; 3], [1.0, f32::from(n) / 4.0, 1.0])),
    }
}

/// **Whose picture a stage wears**: the kind's own, except where the stage
/// is visibly something else -- a mortared wall part way up is brickwork,
/// wet cob and wet daub are mud.
pub fn drawn_as(id: BlockId) -> BlockId {
    match block_kind(id) {
        BLOCK_BRICK_COURSES => BLOCK_BRICKS,
        BLOCK_COB_WALL if is_wet(id) => BLOCK_MUD,
        BLOCK_WATTLE => match stage(id) {
            WOVEN => BLOCK_WATTLE,
            DAUBED_WET => BLOCK_MUD,
            _ => BLOCK_DAUB,
        },
        _ => id,
    }
}

/// **Is this wall a wall yet** -- to the smoke, the sky and a room? A wall
/// four courses up, and a wattle panel once it is daubed. A panel of bare
/// rods lets the smoke out and the night in, which is why it is daubed.
pub fn closes(id: BlockId) -> bool {
    match block_kind(id) {
        BLOCK_WATTLE => stage(id) >= DAUBED_WET,
        BLOCK_BRICK_COURSES => false,
        _ => is_staged(id) && courses(id) >= 4,
    }
}

/// **Will an animal not climb this?** A finished dry stone wall, capped with
/// its cope stones on edge. See the module doc.
#[inline]
pub fn bars_animals(id: BlockId) -> bool {
    block_kind(id) == BLOCK_DRY_STONE_WALL && courses(id) >= 4
}

/// **What a wall gives back when it is taken down**: what was laid into it,
/// less what cannot be got back -- mortar, which sets, and daub, which
/// crumbles. `None` for anything that is not a wall in stages.
///
/// Field stones come back as common stones, and cob as earth: the wall
/// remembers how far up it is, not what rock or soil went into each course.
pub fn refund(id: BlockId) -> Option<Vec<(BlockId, u32)>> {
    if !is_staged(id) {
        return None;
    }
    let n = u32::from(courses(id));
    Some(match block_kind(id) {
        BLOCK_BRICK_COURSES | BLOCK_DRY_BRICKS => vec![(BLOCK_BRICK, n)],
        BLOCK_DRY_STONE_WALL => vec![(BLOCK_PEBBLE, 1 + STONES_FOOTING + (n - 1) * STONES_A_COURSE)],
        BLOCK_WATTLE => vec![(BLOCK_STAKE, 1), (BLOCK_STICK, RODS)],
        _ => vec![(BLOCK_HANDFUL_EARTH, n)],
    })
}

/// Field stones the first course of a dry stone wall costs **on top of the
/// footing stone lying there**: one, so the footing stone is half of the
/// first course and a cell is eight stones in all.
const STONES_FOOTING: u32 = 1;
/// ...and every course after it.
const STONES_A_COURSE: u32 = 2;
/// Rods woven between the stake's uprights.
const RODS: u32 = 3;
/// Trowels of daub a panel takes.
const DAUB_A_PANEL: u32 = 2;

/// One stage laid: what the cell becomes, how many of the held thing it
/// spent, and whether a trowel of mortar went under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Laid {
    pub result: BlockId,
    pub spends: u32,
    pub mortar: bool,
}

/// Is `held` a field stone -- any rock's pebble?
fn is_field_stone(held: BlockId) -> bool {
    matches!(crate::ground::rock_of(held), Some((_, Some(crate::ground::Form::Pebble))))
}

/// **Would laying `held` on `existing` be building at all?** The client's
/// question before it sends anything: a handful on air or on a heap of its
/// own, a brick on air or on its wall, and so on. Whether the pack can pay,
/// and whether the ground under it holds, is `lay`'s -- and the server's.
pub fn builds_on(existing: BlockId, held: BlockId) -> bool {
    // A finished wall is not built on *in its own cell*: the next course
    // goes in the cell over it, which is a new wall started on a full top.
    let kind = block_kind(existing);
    let unfinished = is_staged(existing) && courses(existing) < 4;
    match block_kind(held) {
        _ if is_handful(held) => heap(existing, held).is_some(),
        BLOCK_BRICK => open(existing) || kind == BLOCK_BRICK_COURSES || (kind == BLOCK_DRY_BRICKS && unfinished),
        _ if is_field_stone(held) => is_field_stone(existing) || (kind == BLOCK_DRY_STONE_WALL && unfinished),
        BLOCK_STICK => kind == BLOCK_STAKE,
        BLOCK_DAUB => kind == BLOCK_WATTLE && stage(existing) == WOVEN,
        BLOCK_COB => open(existing) || (kind == BLOCK_COB_WALL && unfinished),
        _ => false,
    }
}

/// **One stage of a wall, or one handful of a heap, laid on `existing`**
/// with `held` in the hand, `mortar` somewhere in the pack and `under` the
/// cell beneath: what it becomes and what it costs, or why not.
///
/// `along_x` is which way a new wattle panel runs: across the builder's
/// view, so they are looking at its face.
///
/// The reasons are what the server tells the player, in the words the
/// interface translates (`ui::lang`).
pub fn lay(existing: BlockId, held: BlockId, mortar: bool, under: BlockId, along_x: bool) -> Result<Laid, &'static str> {
    let kind = block_kind(existing);
    // **A new wall or a new heap stands on something.** Not on a heap part
    // way up, not on air: the course under it is what it is laid on.
    let footed = crate::types::has_full_top(under);
    let fresh = |result: BlockId, spends: u32, mortar: bool| {
        if footed {
            Ok(Laid { result, spends, mortar })
        } else {
            Err("that needs solid ground under it")
        }
    };
    let next = |spends: u32, mortar: bool| {
        let n = courses(existing);
        Ok(Laid { result: with_courses(existing, n + 1, 0), spends, mortar })
    };
    if is_handful(held) {
        let result = heap(existing, held).ok_or("that does not go there")?;
        return if open(existing) { fresh(result, 1, false) } else { Ok(Laid { result, spends: 1, mortar: false }) };
    }
    match block_kind(held) {
        BLOCK_BRICK => match kind {
            _ if open(existing) => {
                if mortar {
                    fresh(with_courses(BLOCK_BRICK_COURSES, 1, 0), 1, true)
                } else {
                    fresh(with_courses(BLOCK_DRY_BRICKS, 1, 0), 1, false)
                }
            }
            BLOCK_BRICK_COURSES if !mortar => Err("that wall is laid in mortar, and you have none"),
            BLOCK_BRICK_COURSES if courses(existing) >= 3 => Ok(Laid { result: BLOCK_BRICKS, spends: 1, mortar: true }),
            BLOCK_BRICK_COURSES => next(1, true),
            BLOCK_DRY_BRICKS if courses(existing) >= 4 => Err("that wall is finished"),
            BLOCK_DRY_BRICKS => next(1, false),
            _ => Err("that does not go there"),
        },
        _ if is_field_stone(held) => match kind {
            // The footing stone is the pebble already lying there: it is
            // what the first course is laid round, and it stays in the wall.
            _ if is_field_stone(existing) => fresh(with_courses(BLOCK_DRY_STONE_WALL, 1, 0), STONES_FOOTING, false),
            BLOCK_DRY_STONE_WALL if courses(existing) >= 4 => Err("that wall is finished"),
            BLOCK_DRY_STONE_WALL => next(STONES_A_COURSE, false),
            _ => Err("that does not go there"),
        },
        BLOCK_STICK if kind == BLOCK_STAKE => Ok(Laid {
            result: with_courses(BLOCK_WATTLE, WOVEN + 1, if along_x { ALONG_X } else { 0 }),
            spends: RODS,
            mortar: false,
        }),
        BLOCK_DAUB if kind == BLOCK_WATTLE => match stage(existing) {
            WOVEN => Ok(Laid { result: with_courses(existing, DAUBED_WET + 1, existing & ALONG_X), spends: DAUB_A_PANEL, mortar: false }),
            _ => Err("that wall is finished"),
        },
        BLOCK_COB => match kind {
            _ if open(existing) => fresh(with_courses(BLOCK_COB_WALL, 1, WET), 1, false),
            BLOCK_COB_WALL if is_wet(existing) => Err("the lift under it is still wet"),
            BLOCK_COB_WALL if courses(existing) >= 4 => Err("that wall is finished"),
            BLOCK_COB_WALL => Ok(Laid { result: with_courses(existing, courses(existing) + 1, WET), spends: 1, mortar: false }),
            _ => Err("that does not go there"),
        },
        _ => Err("that does not go there"),
    }
}

/// **Is this spent by being laid in a wall** rather than by a recipe? Daub
/// and cob: the two things made for nothing but a wall. (Bricks, stones and
/// sticks are laid too, and are crafted with besides.)
#[inline]
pub fn is_laid(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_DAUB | BLOCK_COB)
}

/// Is `id` mortar? For the pack search the server does before `lay`.
#[inline]
pub fn is_mortar(id: BlockId) -> bool {
    block_kind(id) == BLOCK_MORTAR
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BLOCK_CHALK_GRAVEL, BLOCK_COBBLESTONE, BLOCK_GRANITE_COBBLE, BLOCK_GRANITE_PEBBLE, BLOCK_GRASS,
        BLOCK_GRAVEL, BLOCK_LOAM, BLOCK_SAND, BLOCK_STONE,
    };

    fn dig_out(block: BlockId) -> Vec<BlockId> {
        // One swing at a time, the way the server takes them: a handful per
        // slice while there is a bite to take, and the rest at the break.
        let mut got = Vec::new();
        let mut cell = block;
        loop {
            match crate::dig::next_bite(cell, crate::dig::Side::PosY) {
                Some(next) => {
                    got.extend(slice_handful(cell));
                    cell = next;
                }
                None => {
                    let (handful, n) = handfuls_left(cell).expect("the last quarter is a handful");
                    got.extend(std::iter::repeat_n(handful, n as usize));
                    return got;
                }
            }
        }
    }

    #[test]
    fn a_block_of_loose_ground_digs_out_into_four_handfuls_of_itself() {
        for block in [BLOCK_DIRT, BLOCK_LOAM, BLOCK_SAND, BLOCK_GRAVEL, BLOCK_CHALK_GRAVEL, BLOCK_CLAY, BLOCK_GRANITE_COBBLE] {
            let got = dig_out(block);
            assert_eq!(got.len(), 4, "{block} gave {got:?}");
            assert!(got.iter().all(|&h| h == got[0]), "four different handfuls out of {block}");
            assert_eq!(block_of_handful(got[0]), Some(block), "a handful of {block} heaps into something else");
        }
        // Stone breaks into cobble, and a quarter of a cobble is chips.
        assert_eq!(dig_out(BLOCK_STONE)[0], handful_of(BLOCK_COBBLESTONE).unwrap());
    }

    #[test]
    fn four_handfuls_heaped_in_place_are_the_block_again_rock_and_all() {
        let handful = handful_of(BLOCK_CHALK_GRAVEL).unwrap();
        let mut cell = BLOCK_AIR;
        for quarter in 1..=4 {
            let laid = lay(cell, handful, false, BLOCK_STONE, false).expect("a handful goes down");
            assert_eq!(laid.spends, 1);
            cell = laid.result;
            if quarter < 4 {
                let (_, max) = stage_box(cell).or(crate::dig::bite_box(cell)).unwrap();
                assert!((max[1] - quarter as f32 / 4.0).abs() < 1e-6, "{quarter} handfuls are {} high", max[1]);
            }
        }
        assert_eq!(cell, BLOCK_CHALK_GRAVEL);
        // ...and a handful of something else does not go on the heap.
        let heap_of_chalk = crate::dig::heaped(BLOCK_CHALK_GRAVEL);
        assert!(!builds_on(heap_of_chalk, handful_of(BLOCK_SAND).unwrap()));
    }

    #[test]
    fn a_handful_is_not_heaped_on_air_or_on_a_heap_part_way_up() {
        let handful = handful_of(BLOCK_SAND).unwrap();
        assert!(lay(BLOCK_AIR, handful, false, BLOCK_AIR, false).is_err());
        assert!(lay(BLOCK_AIR, handful, false, crate::dig::heaped(BLOCK_SAND), false).is_err());
    }

    #[test]
    fn a_handful_weighs_a_quarter_of_the_block_it_came_out_of() {
        for block in [BLOCK_DIRT, BLOCK_SAND, BLOCK_GRANITE_COBBLE, BLOCK_CLAY] {
            let handful = handful_of(block).unwrap();
            let (whole, quarter) = (crate::types::block_weight(block), crate::types::block_weight(handful));
            assert!((quarter * 4.0 - whole).abs() < 1e-4, "{block}: {quarter} x4 is not {whole}");
        }
    }

    #[test]
    fn every_handful_id_names_its_block_and_nothing_else_is_a_handful() {
        for &handful in &HANDFULS {
            for (n, &block) in sources(handful).iter().enumerate() {
                let id = numbered(handful, n);
                assert!(crate::types::is_known_block(id), "handful {id:#06x} of {block} is refused");
                if handful_of(block) == Some(id) {
                    assert_eq!(block_of_handful(id), Some(block));
                }
            }
            let past = sources(handful).len();
            if past < 16 {
                assert!(!crate::types::is_known_block(numbered(handful, past)), "a handful past the table is accepted");
            }
        }
    }

    #[test]
    fn a_mortared_wall_is_four_courses_of_a_brick_and_a_trowel_and_ends_as_brickwork() {
        let mut cell = BLOCK_AIR;
        for course in 1..=4u32 {
            let laid = lay(cell, BLOCK_BRICK, true, BLOCK_STONE, false).expect("a course goes on");
            assert!(laid.mortar, "course {course} took no mortar");
            cell = laid.result;
            assert!(crate::types::is_known_block(cell));
        }
        assert_eq!(cell, BLOCK_BRICKS);
        // ...and without a trowel the second course is refused, not laid dry.
        let one = lay(BLOCK_AIR, BLOCK_BRICK, true, BLOCK_STONE, false).unwrap().result;
        assert!(lay(one, BLOCK_BRICK, false, BLOCK_STONE, false).is_err());
    }

    #[test]
    fn bricks_laid_dry_come_down_in_a_third_of_the_time_and_give_every_brick_back() {
        let mut cell = BLOCK_AIR;
        for _ in 0..4 {
            cell = lay(cell, BLOCK_BRICK, false, BLOCK_STONE, false).unwrap().result;
        }
        assert_eq!(block_kind(cell), BLOCK_DRY_BRICKS);
        assert_eq!(stage_box(cell), None, "a dry wall four courses up fills its cell");
        let dry = crate::types::break_seconds(cell).unwrap();
        let mortared = crate::types::break_seconds(BLOCK_BRICKS).unwrap();
        assert!(dry * 3.0 <= mortared + 1e-3, "dry {dry}s against mortared {mortared}s");
        assert_eq!(refund(cell), Some(vec![(BLOCK_BRICK, 4)]));
    }

    #[test]
    fn a_dry_stone_wall_starts_on_a_footing_stone_and_only_the_finished_one_bars_a_flock() {
        assert!(!builds_on(BLOCK_AIR, BLOCK_PEBBLE), "a wall begun on nothing");
        let mut cell = BLOCK_GRANITE_PEBBLE;
        let mut spent = 1;
        for _ in 0..4 {
            assert!(!bars_animals(cell));
            let laid = lay(cell, BLOCK_PEBBLE, false, BLOCK_STONE, false).unwrap();
            spent += laid.spends;
            cell = laid.result;
        }
        assert!(bars_animals(cell));
        assert_eq!(spent, 8, "a cell of dry stone is eight stones, the footing one among them");
        assert_eq!(refund(cell), Some(vec![(BLOCK_PEBBLE, 8)]));
    }

    #[test]
    fn wattle_is_a_stake_woven_then_daubed_and_the_rain_takes_wet_daub_off_it() {
        let woven = lay(BLOCK_STAKE, BLOCK_STICK, false, BLOCK_STONE, true).unwrap().result;
        assert!(!closes(woven), "bare rods closed a room");
        let (min, max) = stage_box(woven).unwrap();
        assert!(max[2] - min[2] < 0.2 && max[0] - min[0] == 1.0, "a panel along x is thin across z");
        let daubed = lay(woven, BLOCK_DAUB, false, BLOCK_STONE, true).unwrap().result;
        assert!(is_wet(daubed) && closes(daubed));
        assert_eq!(washed(daubed), woven);
        assert!(!is_wet(dried(daubed)));
        assert_eq!(washed(dried(daubed)), dried(daubed), "rain undid dry daub");
    }

    #[test]
    fn a_cob_lift_waits_for_the_one_under_it_to_dry() {
        let one = lay(BLOCK_AIR, BLOCK_COB, false, BLOCK_STONE, false).unwrap().result;
        assert!(is_wet(one));
        assert!(lay(one, BLOCK_COB, false, BLOCK_STONE, false).is_err(), "cob went onto a wet lift");
        let two = lay(dried(one), BLOCK_COB, false, BLOCK_STONE, false).unwrap().result;
        assert_eq!(courses(two), 2);
        assert_eq!(washed(two), dried(one), "rain took more than the wet lift");
        assert_eq!(washed(one), BLOCK_AIR);
    }

    #[test]
    fn a_wall_part_way_up_is_collided_and_drawn_as_the_courses_laid() {
        let two = lay(dried(lay(BLOCK_AIR, BLOCK_COB, false, BLOCK_STONE, false).unwrap().result), BLOCK_COB, false, BLOCK_STONE, false)
            .unwrap()
            .result;
        let two = dried(two);
        let mut boxes = Vec::new();
        crate::geometry::for_each_block_box(two, 0, 0, 0, |_, _, _| BLOCK_AIR, |a, b| boxes.push((a, b)));
        assert_eq!(boxes, vec![([0.0; 3], [1.0, 0.5, 1.0])]);
        assert!((crate::types::collision_height(two) - 0.5).abs() < 1e-6);
        assert!(!crate::types::is_opaque(two) && !crate::types::has_full_top(two));
        assert!(!crate::types::blocks_the_sky(two), "half a wall closed a room");
    }

    #[test]
    fn every_stage_of_every_wall_is_an_id_the_game_accepts_and_nothing_past_it_is() {
        for kind in [BLOCK_BRICK_COURSES, BLOCK_DRY_BRICKS, BLOCK_DRY_STONE_WALL, BLOCK_WATTLE, BLOCK_COB_WALL] {
            for n in 1..=4u8 {
                for flag in [0, STAGE_FLAG] {
                    let id = with_courses(kind, n, flag);
                    assert_eq!(crate::types::is_known_block(id), is_valid(id), "{id:#06x}");
                }
            }
        }
        assert!(!is_valid(with_courses(BLOCK_BRICK_COURSES, 4, 0)), "a mortared wall of four is brickwork");
        assert!(!is_valid(with_courses(BLOCK_DRY_BRICKS, 2, STAGE_FLAG)));
        assert!(!is_valid(with_courses(BLOCK_WATTLE, 4, 0)));
    }

    #[test]
    fn nothing_new_is_built_over_a_grass_block_or_under_a_heap() {
        // A handful on turf goes into the cell over it, not into the turf.
        assert!(!builds_on(BLOCK_GRASS, handful_of(BLOCK_DIRT).unwrap()));
        assert!(builds_on(BLOCK_TALL_GRASS, BLOCK_COB), "a tuft stopped a wall");
    }

    #[test]
    fn the_ids_still_free_under_seven_hundred_are_listed() {
        // Not a check: a list, for the next change that adds blocks. It
        // prints what `blocks::BLOCKS` has not taken, so nobody counts
        // through a run by hand and lands on an id a save already holds.
        let free: Vec<BlockId> = (1..700).filter(|&id| !crate::blocks::is_defined(id)).collect();
        println!("{} ids free under 700: {free:?}", free.len());
        for id in [BLOCK_HANDFUL_EARTH, BLOCK_COB_WALL] {
            assert!(crate::blocks::is_defined(id));
        }
    }
}
