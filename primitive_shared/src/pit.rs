//! Fires in the ground: the pit kiln, the charcoal pit and the firepit,
//! brought across from TerraFirmaCraft -- and what both sides have to agree
//! on about them.
//!
//! ## What TerraFirmaCraft does
//!
//! Read off its field guide and its source (the 1.20 branch), because the
//! guide leaves out the rules that decide whether a structure works.
//!
//! * **The pit kiln** ([guide](https://terrafirmacraft.github.io/Field-Guide/en_us/getting_started/pit_kiln.html),
//!   `PitKilnBlockEntity`, `PitKilnBlock`). Up to four items are placed
//!   into a one-by-one hole; eight straw (or two thatch) go on them, then
//!   eight logs; the top is lit with a firestarter or a thrown torch. It
//!   burns for `pitKilnTicks`, which defaults to 8000 ticks -- eight
//!   in-game hours, fifty real seconds to the hour, so under seven real
//!   minutes -- heating what is inside towards `pitKilnTemperature`,
//!   1400 °C. `isValid` wants all four horizontal neighbours sturdy and
//!   *not flammable*, a sturdy block under it and nothing but air or fire
//!   over it. Invalid mid-burn, it empties its fuel and leaves the items
//!   as they were. The flame is a vanilla fire block on top of the kiln,
//!   and vanilla rain puts that out.
//! * **The firepit** ([guide](https://terrafirmacraft.github.io/Field-Guide/en_us/getting_started/firepit.html)).
//!   One log and three sticks -- and up to five pieces of kindling, each
//!   worth ten per cent more chance -- thrown onto one block and struck
//!   with a firestarter. Four fuel slots, a temperature, cooking; a
//!   ceramic pot or a wrought-iron grill can be set on it
//!   ([pot](https://terrafirmacraft.github.io/Field-Guide/en_us/food/pot.html):
//!   water and three to five foods boil into soup;
//!   [grill](https://terrafirmacraft.github.io/Field-Guide/en_us/food/grill.html):
//!   five cooking slots instead of one).
//! * **The charcoal pit** ([guide](https://terrafirmacraft.github.io/Field-Guide/en_us/metalworking/charcoal_pit.html),
//!   `BurningLogPileBlockEntity`, `BurningLogPileBlock`). Logs are laid as
//!   log piles with shift and right click, and more are pushed into a pile
//!   with a plain right click. Every face of every pile is covered with a
//!   solid, non-flammable block -- or another pile -- and one pile is lit
//!   and covered. A burning pile lights the piles beside it. After
//!   `charcoalTicks`, 18000 by default (eighteen in-game hours), a column
//!   of piles becomes charcoal piles holding `logs × (0.25..0.5)`
//!   charcoal, random, at most eight layers. A face left open sets fire to
//!   the air beside it, and the wood goes up as ordinary fire.
//!
//! ## What the player asked for, which wins
//!
//! > «Выкапывается 1 блок земли в глубину. Кладутся в низ предметы для
//! > обжога и кладётся 8 сена а сверху 8 брёвен. После поджигается и горит
//! > 1 час реального времени.»
//!
//! That is TerraFirmaCraft's kiln, with two things changed, and both of
//! them are the player's and not up for trade here:
//!
//! * **Fibre, not straw.** The player's first words said hay, and hay was
//!   built -- fibre dried on a rack -- and then the player said fibre ("нужно
//!   для печи использовать волокно а не сено"). Fibre is what pulling up
//!   grass gives, so eight of it is a meadow's walk and not eight minutes of
//!   a rack the tanner also wants; an item made only to be burnt in one place
//!   was a step between the meadow and the kiln that decided nothing.
//! * **One real hour, not eight game hours.** [`PIT_KILN_SECONDS`]. Seven
//!   minutes is a wait; an hour is a plan -- you light it before you go to
//!   the hills and the pots are there when you come back, and whether the
//!   sky will hold for an hour is a question you have to answer before you
//!   spend sixteen armfuls of fuel on it.
//!
//! ## Where this departs from TerraFirmaCraft, and why
//!
//! * **The pottery is fired at the end, not heated on the way.** TFC walks
//!   the items up a temperature curve because its heat is a number items
//!   carry about with them. Nothing here carries heat out of a hearth, and
//!   the player's rule is a sentence with a "then" in it: it burns for an
//!   hour, and then the pottery is fired. So a kiln that goes out at
//!   fifty-nine minutes has fired nothing.
//! * **Rain for [`RAIN_PUTS_OUT_SECONDS`], not the first drop.** TFC's
//!   flame is a fire block that rain removes on a random tick, which in
//!   practice is seconds. A short grace says the same thing and tells the
//!   player why: the kiln is announced as guttering before it is gone.
//! * **Flint lights it**, because flint is what lights every fire in this
//!   world (`logic::fire::STRIKER` on the server). There is no thrown
//!   torch: a torch here is lit *from* a fire, never the other way.
//! * **Covering the pit smothers it.** TFC's rule, kept: the cell over a
//!   kiln has to be open. A roof keeps the rain off as long as it is not
//!   lying on the logs -- two cells up is a roof, one cell up is a lid.
//! * **The charcoal pit burns for an hour too** ([`CHARCOAL_SECONDS`]), not
//!   TFC's 18:8 against the kiln. At TFC's ratio a charcoal burn would take
//!   two and a quarter hours, against a campfire that turns three logs into
//!   a lump of charcoal in eight seconds -- and nobody would dig the pit.
//!   What the pit is for is *yield*, not speed: see [`charcoal_from`].
//! * **Yield is half a charcoal a log, always.** TFC rolls between a
//!   quarter and a half. A roll a player cannot see is a roll they cannot
//!   plan around, and the one decision the pit is -- wood against time --
//!   needs a number to be made against. Half, because the campfire's
//!   recipe gives a third: the pit is the patient way and pays for it.
//! * **An open pile burns to ash** after [`OPEN_PILE_SECONDS`], rather than
//!   setting fire to the air beside it. There is no spreading fire block in
//!   this world, and a pile that quietly became charcoal with a face open
//!   would make covering it a chore with no consequence. Thirty seconds is
//!   the time to strike it on the open face and put the last block over.
//! * **A firepit always catches.** TFC's kindling is a chance, and a
//!   fire that fails at random is not a decision. Three sticks and a log
//!   lying on a cell, struck with flint, is a fire.
//! * **The pot and the grill are not here.** They would be the next thing,
//!   and they need a second kind of container on a hearth; see the report
//!   in `CHANGELOG.md`.
//!
//! ## Why the stage is in the block id and the contents are not
//!
//! Every stage of a kiln -- pottery, fibre, logs, alight -- has to be *seen*
//! by everybody near it, saved with the world and meshed by a client that
//! knows nothing but the chunk. The block id already does all three for
//! free, and its variant field has three bits, which is exactly one of the
//! eight counts. So the counts are ids: [`Stage`] is four kinds with a
//! number in each.
//!
//! What does *not* fit is which pieces of pottery are in the pit -- four
//! slots of four kinds is 256 answers, and a vessel, a mould, a jug and a
//! brick drawn exactly would need a hundred ids or a side channel into the
//! mesher. Three ways were weighed:
//!
//! * **A message of placed items the client meshes with.** Honest, and it
//!   reaches through the chunk streamer, the mesh job's inputs and the
//!   chunk manager, which is three files and a new failure (a chunk meshed
//!   before its pottery arrived) for a picture a player sees for the few
//!   seconds between laying the pots and covering them.
//! * **An id per mixture.** Seventy ids for four pieces of four kinds, each
//!   with a row, a picture entry and a name.
//! * **How many and whether fired, in the id; what, on the server
//!   (chosen).** The pit draws that many small pots, grey raw and red
//!   fired; the server's map holds the real pieces, gives back exactly
//!   those, and fires exactly those.

use crate::types::{
    block_kind, is_air, is_collidable, BlockId, BLOCK_AIR, BLOCK_ASH, BLOCK_BRICK,
    BLOCK_BRICK_RAW, BLOCK_CHARCOAL_PILE, BLOCK_FIBER, BLOCK_JUG, BLOCK_JUG_RAW,
    BLOCK_LOG_PILE, BLOCK_LOG_PILE_LIT, BLOCK_MOULD, BLOCK_MOULD_RAW, BLOCK_PIT_KILN,
    BLOCK_PIT_KILN_FIBRE, BLOCK_PIT_KILN_LIT, BLOCK_PIT_KILN_LOGS, BLOCK_VESSEL, BLOCK_VESSEL_RAW,
    CHUNK_SIZE_Y, VARIANT_SHIFT,
};

/// How many pieces one pit holds. TerraFirmaCraft's four.
pub const POTTERY_MAX: u8 = 4;
/// «8 сена», and then fibre in its place -- see the module note.
pub const FIBRE_NEEDED: u8 = 8;
/// «а сверху 8 брёвен».
pub const LOGS_NEEDED: u8 = 8;

/// How long a lit pit kiln burns before its pottery is fired, in seconds.
///
/// **«горит 1 час реального времени».** The player's number, word for
/// word, and not TerraFirmaCraft's eight game hours: see the module note.
///
/// **Real time is time the server is running.** A multiplayer server
/// keeps running, so there it is the wall clock. A singleplayer server is
/// inside the game and stops when the game is closed, and the kiln stops
/// with it: the seconds left are saved (`logic::pits` on the server) and
/// counted down again from where they were. The rejected reading was the
/// wall clock across a closed game -- light the kiln, quit, come back in
/// an hour to finished pots -- which makes the hour a thing you wait out
/// *without playing*, and is the one reading of "an hour" in which the rain
/// can never reach it.
///
/// A double, because a float counting an hour down in twentieths of a
/// second loses whole seconds to rounding before it gets there.
pub const PIT_KILN_SECONDS: f64 = 3600.0;

/// How long rain has to fall on an open pit kiln before it is out.
///
/// Long enough that the first drops are news rather than a verdict -- the
/// server says the kiln is getting wet -- and short enough that there is
/// no running for planks: the roof was the decision, and it was made when
/// the kiln was lit.
pub const RAIN_PUTS_OUT_SECONDS: f32 = 20.0;

/// How long a lit log pile takes to become charcoal, in seconds.
///
/// **The kiln's hour**, and not TerraFirmaCraft's two and a quarter times
/// it. See the module note: the pit is the way to more charcoal per log,
/// not the way to charcoal soon, and at two hours against a campfire's
/// eight seconds nobody would take it.
pub const CHARCOAL_SECONDS: f64 = 3600.0;

/// How long a lit log pile may stand with a face open before it has burnt
/// to ash.
///
/// The pile has to be struck on a face that is open -- nothing reaches a
/// covered one -- so this is the time to strike it and put the last block
/// over. Thirty seconds is a spade of earth with time to spare, and not
/// long enough to burn half of it open and cover the rest.
pub const OPEN_PILE_SECONDS: f32 = 30.0;

/// The most logs one pile holds. One variant field's worth.
pub const PILE_LOGS_MAX: u8 = 8;

/// What a firepit is laid from: TerraFirmaCraft's three sticks and a log.
pub const FIREPIT_STICKS: u32 = 3;
pub const FIREPIT_LOGS: u32 = 1;

/// What firing makes of a piece of raw pottery, or `None` if it is not
/// something a pit kiln fires.
///
/// The same four the kiln fires, and the same answers: a raw vessel is a
/// vessel however it was fired, so what comes out of the ground goes into
/// every recipe the kiln's does.
pub fn fires_into(raw: BlockId) -> Option<BlockId> {
    Some(match block_kind(raw) {
        BLOCK_VESSEL_RAW => BLOCK_VESSEL,
        BLOCK_MOULD_RAW => BLOCK_MOULD,
        BLOCK_JUG_RAW => BLOCK_JUG,
        BLOCK_BRICK_RAW => BLOCK_BRICK,
        _ => return None,
    })
}

/// Is this something a pit kiln takes as pottery?
#[inline]
pub fn is_raw_pottery(block: BlockId) -> bool {
    fires_into(block).is_some()
}

/// Is this a log, for a kiln's fuel or a pile?
///
/// Any wood's log (`wood::is_log`). A stripped log is not: it is timber
/// that has been worked for something, and what the pit gives back when it
/// is broken open is what went in, so there is nothing to gain by taking it.
#[inline]
pub fn is_log(block: BlockId) -> bool {
    crate::wood::is_log(block)
}

/// Is this fibre?
#[inline]
pub fn is_fibre(block: BlockId) -> bool {
    block_kind(block) == BLOCK_FIBER
}

/// Where a pit kiln is in being built and burnt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Pottery on the floor of the pit, one to four pieces, raw or fired.
    Pottery { pieces: u8, fired: bool },
    /// That much fibre over it, one to eight.
    Fibre(u8),
    /// Eight fibre, and that many logs over it, one to eight.
    Logs(u8),
    /// Alight.
    Burning,
}

/// The variant field of an id, as a number.
#[inline]
fn variant(block: BlockId) -> u8 {
    ((block >> VARIANT_SHIFT) & 0b111) as u8
}

/// An id of this kind with this number in its variant field.
#[inline]
fn with_variant(kind: BlockId, value: u8) -> BlockId {
    kind | (BlockId::from(value & 0b111) << VARIANT_SHIFT)
}

impl Stage {
    /// What stage a block is, if it is a pit kiln at all.
    pub fn of(block: BlockId) -> Option<Stage> {
        let v = variant(block);
        Some(match block_kind(block) {
            BLOCK_PIT_KILN => Stage::Pottery {
                pieces: (v & 0b11) + 1,
                fired: v & 0b100 != 0,
            },
            BLOCK_PIT_KILN_FIBRE => Stage::Fibre(v + 1),
            BLOCK_PIT_KILN_LOGS => Stage::Logs(v + 1),
            BLOCK_PIT_KILN_LIT => Stage::Burning,
            _ => return None,
        })
    }

    /// The id that draws this stage. Counts out of range are clamped
    /// rather than wrapped: a pit asked to show five pots shows four, and
    /// never one.
    pub fn block(self) -> BlockId {
        match self {
            Stage::Pottery { pieces, fired } => with_variant(
                BLOCK_PIT_KILN,
                (pieces.clamp(1, POTTERY_MAX) - 1) | if fired { 0b100 } else { 0 },
            ),
            Stage::Fibre(fibre) => with_variant(BLOCK_PIT_KILN_FIBRE, fibre.clamp(1, FIBRE_NEEDED) - 1),
            Stage::Logs(logs) => with_variant(BLOCK_PIT_KILN_LOGS, logs.clamp(1, LOGS_NEEDED) - 1),
            Stage::Burning => BLOCK_PIT_KILN_LIT,
        }
    }

    /// How much fibre is in a pit at this stage.
    pub fn fibre(self) -> u8 {
        match self {
            Stage::Pottery { .. } => 0,
            Stage::Fibre(fibre) => fibre,
            Stage::Logs(_) | Stage::Burning => FIBRE_NEEDED,
        }
    }

    /// ...and how many logs.
    pub fn logs(self) -> u8 {
        match self {
            Stage::Pottery { .. } | Stage::Fibre(_) => 0,
            Stage::Logs(logs) => logs,
            Stage::Burning => LOGS_NEEDED,
        }
    }
}

/// Is this cell a pit kiln, at any stage?
#[inline]
pub fn is_pit_kiln(block: BlockId) -> bool {
    Stage::of(block).is_some()
}

/// A pile of this many logs, unlit.
pub fn log_pile(logs: u8) -> BlockId {
    with_variant(BLOCK_LOG_PILE, logs.clamp(1, PILE_LOGS_MAX) - 1)
}

/// ...and the same pile alight. The count stays in the id, so a burning
/// pile knows its yield whoever is looking at it.
pub fn log_pile_lit(logs: u8) -> BlockId {
    with_variant(BLOCK_LOG_PILE_LIT, logs.clamp(1, PILE_LOGS_MAX) - 1)
}

/// How many logs a pile holds, lit or not; `None` for anything else.
pub fn pile_logs(block: BlockId) -> Option<u8> {
    matches!(block_kind(block), BLOCK_LOG_PILE | BLOCK_LOG_PILE_LIT).then(|| variant(block) + 1)
}

/// Is this a pile of logs, lit or not?
#[inline]
pub fn is_log_pile(block: BlockId) -> bool {
    pile_logs(block).is_some()
}

/// How thick one log of a pile is, in cells: a third, so three lie side by
/// side across the cell and three courses stand in its height.
///
/// **A third and not five sixteenths**, which would have been a whole number
/// to write: at five, a full pile stops a sixteenth under the top of its
/// cell, and the next pile stacked on it -- the charcoal pit is piles on
/// piles -- floats that sixteenth over it with a line of sky between.
pub const PILE_LOG: f32 = 1.0 / 3.0;

/// Where each log of an unlit pile lies, by the order it went on: the
/// corner of its end nearest the cell's origin, (x, y) in cells. Every log
/// runs the whole cell along z.
///
/// **Laid as a woodpile is laid**: three on the ground, the first in the
/// middle so one log is one log lying where it was put down; two in the
/// grooves between them, as round logs settle; three across the top. Three,
/// two, three is the eight a pile holds, and the top course is whole, so a
/// full pile is flat on top -- which is what lets another pile or a spade
/// of earth go on it (`types::has_full_top`).
pub const PILE_LOGS_AT: [[f32; 2]; PILE_LOGS_MAX as usize] = [
    [PILE_LOG, 0.0],
    [0.0, 0.0],
    [2.0 * PILE_LOG, 0.0],
    [0.5 * PILE_LOG, PILE_LOG],
    [1.5 * PILE_LOG, PILE_LOG],
    [PILE_LOG, 2.0 * PILE_LOG],
    [0.0, 2.0 * PILE_LOG],
    [2.0 * PILE_LOG, 2.0 * PILE_LOG],
];

/// The box each log of an unlit pile fills, in cell units: what the mesher
/// draws each log round (`mesh::log_pile_block`) and what a body walks into
/// (`geometry::for_each_block_box`). Nothing for anything else -- a pile
/// alight is drawn as its whole cell of fire, and collides as one.
///
/// **The box follows the count**, and that was a choice between three:
///
/// * *The whole cell, whatever is in it* -- what it was. One log on the
///   ground with a metre of invisible wall round it, which a player steps
///   onto a metre up and cannot see, and aims at a metre of air.
/// * *A height by courses, the whole cell across* -- one box, the collider's
///   cheap path. Still stands a player on air beside a lone log, and the
///   brick courses leave notches no single box has.
/// * *Each log's own box* (chosen). Eight at most, like the slices of a palm;
///   a player steps up one log at a time, walks over a single one, and
///   stands on a full pile as on any block. What a square box does not say
///   is the rounded corners (`mesh::log_pile_block` bevels them a
///   sixteenth) -- no body can get into a sixteenth.
pub fn pile_log_boxes(block: BlockId) -> impl Iterator<Item = ([f32; 3], [f32; 3])> {
    let logs = if block_kind(block) == BLOCK_LOG_PILE { pile_logs(block).unwrap_or(0) } else { 0 };
    PILE_LOGS_AT.iter().take(usize::from(logs)).map(|&[x, y]| ([x, y, 0.0], [x + PILE_LOG, y + PILE_LOG, 1.0]))
}

/// The box round every log of an unlit pile, in cell units: what a ray
/// stops at and the cracks are drawn on, and the height the server's ground
/// probe reads (`types::collision_height`). `None` for anything else.
pub fn pile_extent(block: BlockId) -> Option<([f32; 3], [f32; 3])> {
    pile_log_boxes(block).reduce(|(lo, hi), (from, to)| {
        (std::array::from_fn(|k| lo[k].min(from[k])), std::array::from_fn(|k| hi[k].max(to[k])))
    })
}

/// A heap of this much charcoal.
pub fn charcoal_pile(count: u8) -> BlockId {
    with_variant(BLOCK_CHARCOAL_PILE, count.clamp(1, 8) - 1)
}

/// How much charcoal a heap holds; `None` for anything else.
pub fn charcoal_in(block: BlockId) -> Option<u8> {
    (block_kind(block) == BLOCK_CHARCOAL_PILE).then(|| variant(block) + 1)
}

/// What a pile of this many logs leaves when it has burnt through under
/// cover: half a charcoal a log, rounded down.
///
/// **Against the campfire's third.** The "charcoal" recipe turns three
/// logs into one lump in eight seconds at any fire; the pit turns two into
/// one in an hour, with nothing to watch. Wood against time, and the
/// answer depends on where the player lives -- a forest camp burns logs at
/// the fire, a steppe camp digs the pit. A single log gives nothing but
/// ash, which is what one log smothered in a hole gives.
pub fn charcoal_from(logs: u8) -> u8 {
    logs / 2
}

/// What a lit pile leaves when it has burnt through, covered or not.
pub fn burnt_pile(logs: u8, covered: bool) -> BlockId {
    let charcoal = charcoal_from(logs);
    if covered && charcoal > 0 {
        charcoal_pile(charcoal)
    } else {
        BLOCK_ASH
    }
}

/// Does this block give off smoke and sparks the way a hearth does?
///
/// For the client's particles, which look for lit hearths and would
/// otherwise draw a burning kiln as a pile of logs that happens to glow.
#[inline]
pub fn smokes(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_PIT_KILN_LIT | BLOCK_LOG_PILE_LIT)
}

/// Can this block be the wall, floor or cover of a fire in the ground?
///
/// **TerraFirmaCraft's two words, "sturdy" and "not flammable".** Sturdy
/// is a whole cell, solid, with a full top -- earth, sand, gravel, stone,
/// brick. Not flammable rules out what fire eats: wood and what is made of
/// it, leaves and plants, anything the fuel table burns, and every stage of
/// the pits themselves (a kiln is not the wall of the kiln next to it).
pub fn is_fireproof(block: BlockId) -> bool {
    use crate::blocks::{definition, Shape, Work};
    let def = definition(block);
    is_collidable(block)
        && crate::types::has_full_top(block)
        && def.shape == Shape::Cube
        // Not `def.foliage`, which is "takes the biome tint" and so is true
        // of turf: it refused every pit dug in a meadow. Leaves are ruled
        // out already, by not being collidable.
        && !def.container
        && !matches!(def.work, Work::Wood | Work::Plant)
        // Turf is `Work::Any` -- it is dug, not cut -- so a pit in a meadow
        // has grass for walls, which is right: TerraFirmaCraft's grass is
        // sturdy and does not burn. Wool is the one whole cube that burns
        // and is not wood.
        && block_kind(block) != crate::types::BLOCK_WOOL
        && !crate::hearth::is_fuel(block)
        && !is_pit_kiln(block)
        && !is_log_pile(block)
        && block_kind(block) != BLOCK_CHARCOAL_PILE
        && !crate::types::is_hearth(block)
}

/// What is wrong with a pit, when something is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breach {
    /// Nothing, or something that burns, under it.
    Floor,
    /// A side open, or made of something that burns.
    Wall,
    /// Something lying on it.
    Covered,
}

impl Breach {
    /// What the player is told.
    pub fn says(self) -> crate::notice::Notice {
        use crate::notice::Notice;
        match self {
            Breach::Floor => Notice::PitNeedsFloor,
            Breach::Wall => Notice::PitOpenAtSide,
            Breach::Covered => Notice::PitSmothered,
        }
    }
}

/// A neighbour nobody has loaded, so nothing can be said about it.
///
/// A type of its own rather than `()`, so the answer "I cannot see that
/// wall" has a name wherever it is matched -- and so it cannot be mistaken
/// for any other unit a caller happens to be carrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unseen;

/// Is the cell at `at` a pit a fire can burn in -- a floor, four walls and
/// nothing on top?
///
/// `look` answering `None` is a cell nobody has loaded, and that is
/// reported as [`Unseen`] rather than as a breach: a kiln in a chunk the
/// server has evicted must not be put out for a wall it cannot see.
pub fn breach(
    look: impl Fn(i32, i32, i32) -> Option<BlockId>,
    at: (i32, i32, i32),
) -> Result<Option<Breach>, Unseen> {
    let (x, y, z) = at;
    let floor = look(x, y - 1, z).ok_or(Unseen)?;
    if !is_fireproof(floor) {
        return Ok(Some(Breach::Floor));
    }
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        if !is_fireproof(look(x + dx, y, z + dz).ok_or(Unseen)?) {
            return Ok(Some(Breach::Wall));
        }
    }
    if is_collidable(look(x, y + 1, z).ok_or(Unseen)?) {
        return Ok(Some(Breach::Covered));
    }
    Ok(None)
}

/// Would a piece of pottery held at the block `floor` go into a new pit
/// above it?
///
/// The client asks it to decide whether a right click is a use or nothing
/// at all, and the server asks it again before it believes one.
pub fn takes_pottery_above(look: impl Fn(i32, i32, i32) -> Option<BlockId>, floor: (i32, i32, i32)) -> bool {
    let above = (floor.0, floor.1 + 1, floor.2);
    look(above.0, above.1, above.2).is_some_and(is_air) && breach(&look, above) == Ok(None)
}

/// Can the rain reach this cell?
///
/// Straight up, and anything solid is a roof: the fires' rule
/// (`logic::fire::open_to_the_sky` on the server), so a shelter that keeps
/// a campfire alight keeps a kiln alight. Unloaded cells count as open,
/// for the fires' reason.
pub fn open_to_the_sky(look: impl Fn(i32, i32, i32) -> Option<BlockId>, at: (i32, i32, i32)) -> bool {
    let (x, y, z) = at;
    ((y + 1)..CHUNK_SIZE_Y as i32).all(|above| !look(x, above, z).is_some_and(is_collidable))
}

/// Is every face of a pile of logs covered -- by something fireproof, or by
/// another pile?
///
/// [`Unseen`] for a neighbour nobody has loaded, for `breach`'s reason.
pub fn pile_covered(look: impl Fn(i32, i32, i32) -> Option<BlockId>, at: (i32, i32, i32)) -> Result<bool, Unseen> {
    let (x, y, z) = at;
    for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
        let next = look(x + dx, y + dy, z + dz).ok_or(Unseen)?;
        let fine = is_fireproof(next)
            || is_log_pile(next)
            || block_kind(next) == BLOCK_CHARCOAL_PILE;
        if !fine {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Can a pile of logs be laid in this cell?
///
/// Air, over a whole floor. Not only over a fireproof one: a pile is laid
/// first and walled afterwards, and a pile on a plank floor is a pile the
/// cover rule will burn to ash, which is the lesson and not a refusal.
///
/// **A pile goes on a pile only once the one under it is full.** A pile is
/// drawn and collided as the logs in it (`pile_log_boxes`), and a pile of
/// three is a third of a cell tall: another laid over it would hang in the
/// air two thirds of a cell above them. A full pile is flat on top and
/// answers `has_full_top` like any block; a pile alight is its whole cell.
pub fn pile_fits(look: impl Fn(i32, i32, i32) -> Option<BlockId>, at: (i32, i32, i32)) -> bool {
    let (x, y, z) = at;
    look(x, y, z).is_some_and(|here| block_kind(here) == BLOCK_AIR)
        && look(x, y - 1, z).is_some_and(crate::types::has_full_top)
}

/// The ids of every stage, for the tests and for the tables that list
/// blocks by kind.
pub const KINDS: [BlockId; 7] = [
    BLOCK_PIT_KILN,
    BLOCK_PIT_KILN_FIBRE,
    BLOCK_PIT_KILN_LOGS,
    BLOCK_PIT_KILN_LIT,
    BLOCK_LOG_PILE,
    BLOCK_LOG_PILE_LIT,
    BLOCK_CHARCOAL_PILE,
];

/// May an id of this kind carry a number in its variant field?
///
/// Every kind here but the burning kiln spends all eight values. See
/// `types::may_carry_variant`, which asks this.
pub fn carries_variant(kind: BlockId) -> bool {
    matches!(
        block_kind(kind),
        BLOCK_PIT_KILN | BLOCK_PIT_KILN_FIBRE | BLOCK_PIT_KILN_LOGS | BLOCK_LOG_PILE | BLOCK_LOG_PILE_LIT | BLOCK_CHARCOAL_PILE
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_GRAVEL, BLOCK_LOG, BLOCK_PLANKS, BLOCK_SAND, BLOCK_STONE,
    };
    use std::collections::HashMap;

    /// A world of cells written by hand; everything unwritten is air, and
    /// nothing is unloaded unless a test says so.
    #[derive(Default)]
    struct Cells(HashMap<(i32, i32, i32), BlockId>);

    impl Cells {
        fn look(&self) -> impl Fn(i32, i32, i32) -> Option<BlockId> + '_ {
            |x, y, z| Some(*self.0.get(&(x, y, z)).unwrap_or(&BLOCK_AIR))
        }
        /// A one-deep hole at (0, 10, 0) in a field of `ground`.
        fn pit(ground: BlockId) -> Cells {
            let mut cells = Cells::default();
            for x in -2..=2 {
                for z in -2..=2 {
                    cells.0.insert((x, 9, z), ground);
                    if (x, z) != (0, 0) {
                        cells.0.insert((x, 10, z), ground);
                    }
                }
            }
            cells
        }
    }

    const AT: (i32, i32, i32) = (0, 10, 0);

    #[test]
    fn every_stage_of_a_pit_kiln_comes_back_as_itself_from_its_id() {
        let mut stages = Vec::new();
        for pieces in 1..=POTTERY_MAX {
            for fired in [false, true] {
                stages.push(Stage::Pottery { pieces, fired });
            }
        }
        stages.extend((1..=FIBRE_NEEDED).map(Stage::Fibre));
        stages.extend((1..=LOGS_NEEDED).map(Stage::Logs));
        stages.push(Stage::Burning);
        let mut seen = std::collections::HashSet::new();
        for stage in stages {
            let id = stage.block();
            assert_eq!(Stage::of(id), Some(stage), "{stage:?} did not survive its id");
            assert!(seen.insert(id), "{stage:?} shares an id with another stage");
            assert!(crate::types::is_known_block(id), "{stage:?} is an id the anti-cheat calls invented");
        }
        assert_eq!(Stage::of(BLOCK_DIRT), None);
    }

    #[test]
    fn a_pile_keeps_its_count_whether_it_is_lit_or_not() {
        for logs in 1..=PILE_LOGS_MAX {
            assert_eq!(pile_logs(log_pile(logs)), Some(logs));
            assert_eq!(pile_logs(log_pile_lit(logs)), Some(logs));
            assert!(crate::types::is_known_block(log_pile_lit(logs)));
        }
        for count in 1..=4 {
            assert_eq!(charcoal_in(charcoal_pile(count)), Some(count));
        }
    }

    #[test]
    fn a_hole_one_deep_in_earth_or_stone_is_a_pit_and_a_hole_in_planks_is_not() {
        // Turf first: the pit a player digs in a meadow has grass for walls.
        for ground in [crate::types::BLOCK_GRASS, BLOCK_DIRT, BLOCK_STONE, BLOCK_COBBLESTONE, BLOCK_SAND, BLOCK_GRAVEL] {
            let cells = Cells::pit(ground);
            assert_eq!(breach(cells.look(), AT), Ok(None), "a pit in block {ground} is refused");
        }
        let cells = Cells::pit(BLOCK_PLANKS);
        assert_eq!(breach(cells.look(), AT), Ok(Some(Breach::Floor)), "a wooden pit would burn");
    }

    #[test]
    fn a_pit_with_a_side_open_or_a_lid_on_it_is_not_a_pit() {
        let mut open = Cells::pit(BLOCK_DIRT);
        open.0.insert((1, 10, 0), BLOCK_AIR);
        assert_eq!(breach(open.look(), AT), Ok(Some(Breach::Wall)));

        let mut wooden_side = Cells::pit(BLOCK_DIRT);
        wooden_side.0.insert((0, 10, -1), BLOCK_PLANKS);
        assert_eq!(breach(wooden_side.look(), AT), Ok(Some(Breach::Wall)));

        let mut lid = Cells::pit(BLOCK_DIRT);
        lid.0.insert((0, 11, 0), BLOCK_STONE);
        assert_eq!(breach(lid.look(), AT), Ok(Some(Breach::Covered)));

        // A roof two up is a roof, not a lid.
        let mut roofed = Cells::pit(BLOCK_DIRT);
        roofed.0.insert((0, 12, 0), BLOCK_STONE);
        assert_eq!(breach(roofed.look(), AT), Ok(None));
        assert!(!open_to_the_sky(roofed.look(), AT), "the roof keeps no rain off");
        assert!(open_to_the_sky(Cells::pit(BLOCK_DIRT).look(), AT));
    }

    #[test]
    fn a_wall_nobody_has_loaded_is_not_a_breach() {
        let cells = Cells::pit(BLOCK_DIRT);
        let unloaded = |x: i32, y: i32, z: i32| if x > 0 { None } else { cells.look()(x, y, z) };
        assert_eq!(breach(unloaded, AT), Err(Unseen), "an evicted wall put a kiln out");
    }

    #[test]
    fn pottery_goes_into_an_empty_pit_and_not_onto_a_flat_field() {
        let cells = Cells::pit(BLOCK_DIRT);
        assert!(takes_pottery_above(cells.look(), (0, 9, 0)));
        // The field beside the pit: its cell above is open on every side.
        let mut field = Cells::default();
        field.0.insert((5, 9, 5), BLOCK_DIRT);
        assert!(!takes_pottery_above(field.look(), (5, 9, 5)));
    }

    #[test]
    fn a_pile_is_covered_only_when_all_six_faces_are() {
        let mut cells = Cells::pit(BLOCK_DIRT);
        cells.0.insert(AT, log_pile_lit(8));
        assert_eq!(pile_covered(cells.look(), AT), Ok(false), "the open top counted as cover");
        cells.0.insert((0, 11, 0), BLOCK_DIRT);
        assert_eq!(pile_covered(cells.look(), AT), Ok(true));
        // A neighbouring pile is cover, as in TerraFirmaCraft.
        cells.0.insert((1, 10, 0), log_pile(3));
        assert_eq!(pile_covered(cells.look(), AT), Ok(true));
    }

    #[test]
    fn a_covered_pile_gives_half_a_charcoal_a_log_and_an_open_one_gives_ash() {
        assert_eq!(charcoal_in(burnt_pile(8, true)), Some(4));
        assert_eq!(charcoal_in(burnt_pile(5, true)), Some(2));
        assert_eq!(burnt_pile(1, true), BLOCK_ASH, "one log made charcoal");
        assert_eq!(burnt_pile(8, false), BLOCK_ASH);
        // Better than the campfire's third, which is the whole reason to
        // wait an hour for it.
        let campfire = crate::crafting::RECIPES
            .iter()
            .find(|recipe| recipe.name == "charcoal")
            .expect("the campfire's charcoal row");
        let campfire_per_log = campfire.output.1 as f32 / campfire.inputs[0].1 as f32;
        assert!(charcoal_from(8) as f32 / 8.0 > campfire_per_log, "the pit gives no more than the fire");
    }

    #[test]
    fn everything_raw_a_pit_fires_comes_out_as_what_the_kiln_makes() {
        for raw in [BLOCK_VESSEL_RAW, BLOCK_MOULD_RAW, BLOCK_JUG_RAW, BLOCK_BRICK_RAW] {
            let fired = fires_into(raw).expect("raw pottery");
            assert!(
                crate::crafting::RECIPES
                    .iter()
                    .any(|recipe| recipe.output.0 == fired && recipe.station.is_hearth()),
                "a pit fires block {raw} into something no hearth makes"
            );
        }
        assert_eq!(fires_into(BLOCK_DIRT), None);
    }

    #[test]
    fn a_lone_log_is_stepped_over_and_only_a_full_pile_is_a_floor() {
        // A pile was a whole cube whatever it held: one log on the ground
        // came with a metre of invisible wall round it. Now it is its logs
        // (`pile_log_boxes`), so the heights are theirs -- and a pile laid
        // over a part-filled one would hang in the air over the gaps.
        use crate::types::{collision_height, has_full_top};
        assert!(collision_height(log_pile(1)) < crate::geometry::PLAYER_STEP_HEIGHT, "one log is a wall");
        assert!((collision_height(log_pile(4)) - 2.0 * PILE_LOG).abs() < 1e-6);
        assert_eq!(collision_height(log_pile(8)), 1.0);
        for logs in 1..PILE_LOGS_MAX {
            assert!(!has_full_top(log_pile(logs)), "a pile of {logs} is a floor");
        }
        assert!(has_full_top(log_pile(PILE_LOGS_MAX)));
        // Alight, a pile is drawn as its cell of fire, and collides as one.
        assert_eq!(pile_extent(log_pile_lit(1)), None);
        assert_eq!(collision_height(log_pile_lit(1)), 1.0);

        let mut cells = Cells::default();
        cells.0.insert((0, 9, 0), log_pile(3));
        assert!(!pile_fits(cells.look(), (0, 10, 0)), "a pile went onto a pile of three");
        cells.0.insert((0, 9, 0), log_pile(8));
        assert!(pile_fits(cells.look(), (0, 10, 0)), "a pile would not go onto a full one");
    }

    #[test]
    fn every_log_of_a_pile_lies_on_the_ground_or_on_logs_under_it() {
        // Laid in the order `PILE_LOGS_AT` gives, every prefix of it is a
        // pile somebody can see: no count may leave a log in the air over a
        // gap the next one was meant to fill.
        for logs in 1..=PILE_LOGS_MAX {
            let boxes: Vec<_> = pile_log_boxes(log_pile(logs)).collect();
            for (from, to) in &boxes {
                assert!(from.iter().all(|&c| c >= 0.0) && to.iter().all(|&c| c <= 1.0 + 1e-6), "a log out of its cell");
                let resting = from[1] == 0.0
                    || boxes.iter().any(|(under_from, under_to)| {
                        (under_to[1] - from[1]).abs() < 1e-6 && under_from[0] < to[0] && under_to[0] > from[0]
                    });
                assert!(resting, "a pile of {logs}: the log at {from:?} lies on nothing");
            }
        }
    }

    #[test]
    fn nothing_that_burns_is_a_wall() {
        for block in [BLOCK_LOG, BLOCK_PLANKS, log_pile(4), Stage::Fibre(3).block(), BLOCK_FIBER] {
            assert!(!is_fireproof(block), "block {block} would hold a fire in");
        }
    }
}
