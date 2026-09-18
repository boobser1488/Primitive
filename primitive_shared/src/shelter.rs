//! What a room does to the warmth in it: how shut it is, which way its gaps
//! face the wind, what its walls and roof are made of, and how long it
//! holds a fire's heat once the fire is out.
//!
//! ## The decision this is for
//!
//! Before this, "indoors" was a count: a roof, and something solid within
//! three cells in three of the four directions (`climate::has_walls` on the
//! server). Every such place was the same house. A hut of branches with the
//! door facing the storm and a log cabin with the door in its lee kept out
//! the same four fifths of the night, and a smoke hole cost nothing but the
//! smoke -- so there was one right way to build and a smoke hole was it.
//!
//! Now the room is the air a player can reach ([`survey`], the same walk
//! the smoke takes: `wildfire::walk_room`), and three things about it are
//! choices with a price on each side:
//!
//! - **Openings.** A doorway into the wind is a draught ([`Room::draught`]):
//!   it lets the night in and blows the fire's warmth out. The same doorway
//!   on the lee side costs almost nothing. Where the door goes is a
//!   decision, and the wind veers (`raft::wind`), so a door can be *shut*.
//! - **The smoke hole.** Smoke goes up and so does heat. A hole in the roof
//!   clears the smoke (`wildfire::smoke_kept`) and lets part of the
//!   hearth's warmth out with it ([`Room::heat_kept`]). Warm and smoky, or
//!   clear and cooler -- the choice every hut with a hearth in it has made.
//! - **Material.** Thatch and earth keep the night out, stone and earth
//!   keep a fire's heat after it dies, planks do neither well ([`material`]).
//!   Thatch is the warm roof that burns; stone is the cold wall that is
//!   still warm at dawn.
//!
//! Pure rules, no I/O: the server owns the clock and the cache, and the
//! tests here read the same numbers the server does.

use crate::raft::Wind;
use crate::types::{
    block_kind, BlockId, BLOCK_BRANCH_ROOF, BLOCK_BRANCH_SLAB, BLOCK_BRICK, BLOCK_BRICKS,
    BLOCK_CHARRED_LOG, BLOCK_CHARRED_PLANKS, BLOCK_CLAY, BLOCK_COBBLESTONE_STAIRS, BLOCK_DIRT,
    BLOCK_DOOR, BLOCK_DOOR_TOP, BLOCK_DRIED_PEAT, BLOCK_DRY_TURF, BLOCK_GRASS, BLOCK_ICE, BLOCK_MUD,
    BLOCK_PEAT, BLOCK_PLANK_STAIRS, BLOCK_SANDSTONE_BRICKS, BLOCK_SNOW, BLOCK_STRIPPED_LOG,
    BLOCK_THATCH_ROOF, BLOCK_THATCH_SLAB, BLOCK_TILE_ROOF, BLOCK_TILE_SLAB,
};
use crate::wildfire::{walk_room, Face, ROOM_MAX_RISE};

type Cell = (i32, i32, i32);

/// What one kind of wall or roof does for the warmth behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// How much of the night it keeps out, 0..1: insulation.
    pub keeps_out: f32,
    /// How long it holds a dead fire's heat, in days, as the time the
    /// room's warmth takes to fall to about a third: thermal mass.
    pub holds_days: f32,
}

/// **Two numbers and not one**, because the two are different things and
/// the decisions live in the difference. A single "warmth" per material
/// would rank them, and a ranking has one right answer. Insulation keeps
/// the cold out while nothing is burning; mass keeps the heat in once the
/// fire has gone out. Thatch is good at the first and has nothing of the
/// second; stone is the other way round; earth is both, and it is a hole
/// in a hillside you have to find.
///
/// The numbers were set against a fifteen-minute day: stone's fifth of a
/// day is a room that has kept over a quarter of a midnight fire's warmth
/// at dawn and most of it two hours on, and a fiftieth -- planks -- is
/// gone within the hour.
pub fn material(block: BlockId) -> Material {
    let m = |keeps_out, holds_days| Material { keeps_out, holds_days };
    let kind = block_kind(block);
    match kind {
        // Snow is air in ice: the best insulator a stone age has, with no
        // mass at all. An igloo is warm with a body in it, and a fire in one
        // does not outlast the fire.
        BLOCK_SNOW | BLOCK_ICE => m(0.95, 0.03),
        BLOCK_THATCH_ROOF | BLOCK_THATCH_SLAB => m(0.9, 0.03),
        // A roof of branches over leaves is the first night's roof, and it
        // is what it is.
        BLOCK_BRANCH_ROOF | BLOCK_BRANCH_SLAB => m(0.3, 0.01),
        BLOCK_TILE_ROOF | BLOCK_TILE_SLAB => m(0.4, 0.02),
        BLOCK_PLANK_STAIRS | BLOCK_DOOR | BLOCK_DOOR_TOP | BLOCK_CHARRED_PLANKS => m(0.55, 0.02),
        BLOCK_STRIPPED_LOG | BLOCK_CHARRED_LOG => m(0.9, 0.12),
        BLOCK_BRICK | BLOCK_BRICKS | BLOCK_SANDSTONE_BRICKS | BLOCK_COBBLESTONE_STAIRS => m(0.6, 0.2),
        BLOCK_DIRT | BLOCK_GRASS | BLOCK_CLAY | BLOCK_MUD | BLOCK_PEAT | BLOCK_DRIED_PEAT | BLOCK_DRY_TURF => {
            m(1.0, 0.25)
        }
        _ if crate::wood::is_log(block) => m(0.9, 0.12),
        _ if crate::wood::wood_of(block).is_some() => m(0.55, 0.02),
        _ if crate::ground::rock_of(block).is_some() => m(0.6, 0.2),
        _ if crate::ground::is_soil(block) || crate::ground::is_grass(block) => m(1.0, 0.25),
        // Anything else -- a chest against the wall, a block nobody thought
        // of -- is taken as middling, so a new block is never the warmest
        // wall in the game by accident.
        _ => m(0.6, 0.05),
    }
}

/// How much of the night a shut room of the best material keeps out.
///
/// Nine tenths, against the eight tenths `climate::WALLS_EVENS_OUT` gives
/// any three walls: the best-built room is a little better than the old
/// one-size house, and a room of planks or stone a little worse. The tenth
/// that always gets in is what a fire is for.
pub const BEST_EVENS_OUT: f32 = 0.9;

/// How many open faces in the walls a place may have and still be a room:
/// a doorway (two) and a window on each of the other walls. More, and it is
/// a shed with the wind through it -- `climate::Shelter::Roofed`.
pub const SIDE_OPENINGS_FOR_A_ROOM: usize = 6;

/// ...and in the roof: a smoke hole two across is still a roof with a hole
/// in it; a roof that is mostly hole is a yard.
pub const ROOF_OPENINGS_FOR_A_ROOM: usize = 4;

/// How fast an open face lets a draught in, per unit of the wind's push on
/// it. A doorway square to a fair-weather wind makes a draught of about two
/// fifths; the same doorway in a storm, two thirds.
pub const DRAUGHT_PER_FACE: f32 = 0.6;

/// The share of an opening's draught that does not care which way it
/// faces: air moves through any gap a little. A fifth, so a door in the
/// lee is not free, only cheap.
pub const ANY_SIDE_DRAUGHT: f32 = 0.2;

/// How much of the hearth's warmth a full draught blows out of a room.
pub const DRAUGHT_TAKES_HEAT: f32 = 0.6;

/// How much of the hearth's warmth one open face in the roof lets out, as
/// the weight in [`Room::heat_kept`] -- the heat's own `ROOF_LEAK`.
///
/// **Less than the smoke's.** One hole halves the smoke
/// (`wildfire::ROOF_LEAK`) and takes about a quarter of the warmth: smoke
/// is carried up by the heat and goes all of it, while most of a fire's
/// warmth is in the walls, the floor and the air at head height, which the
/// hole does not reach. That difference is what makes the hole worth
/// cutting -- equal, and no smoke hole would ever be the better hut.
pub const ROOF_HEAT_LEAK: f32 = 0.35;

/// A room, as the warmth sees it. Found by [`survey`].
#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    /// Which room this is, for remembering its warmth between looks: the
    /// first of its cells, how many there are and how many ways out.
    ///
    /// **The shape is in the key on purpose.** Knock a hole in a warm room
    /// and it is a different room, and a cold one -- the heat went out of
    /// the hole. The rejected key was the first cell alone, which carried
    /// a room's warmth through the wall somebody had just taken down.
    pub key: ((i32, i32, i32), u32, u16, u16),
    /// Cells of air in it.
    pub cells: usize,
    /// The least and the greatest corner of those cells: the box an edit
    /// has to land in to change this room (`logic::shelters` on the
    /// server). The cells themselves are not kept -- the warmth has no use
    /// for them.
    pub bounds: ((i32, i32, i32), (i32, i32, i32)),
    /// The open faces in its walls, each as the way out of it, (dx, dz).
    pub side: Vec<(i32, i32)>,
    /// Holes in its roof, one a column of sky.
    pub roof: usize,
    /// The walls' and roof's insulation, averaged over their faces.
    pub keeps_out: f32,
    /// ...and their mass, likewise.
    pub holds_days: f32,
}

/// The room a player's feet are in, if they are in one.
///
/// **The smoke's walk, from the feet** -- the call `comfort::survey` makes
/// through `smoke_room`, so a room is the same room to the smoke, to comfort
/// and to the warmth. `None` for open air, a hall past
/// `wildfire::ROOM_MAX_CELLS`, and a place whose neighbourhood is not
/// loaded; the caller falls back to the plain roof-and-walls count for all
/// three.
///
/// Every solid face is a vote for its material, so a stone room with a
/// thatch roof is mostly stone: a roof is one face a cell, a wall is one
/// per cell of wall.
///
/// ## A hole in the roof is not a hole in the wall
///
/// The walk only enters air with a ceiling over it, so the column under a
/// smoke hole is not in the room, and the room meets it *sideways*, from
/// every cell round it at every height -- a hole one block across in a hut
/// three high reads as a dozen gaps in the walls. To the smoke that is
/// near enough (it thins the room either way); to the warmth it was a hut
/// with a smoke hole counted as a shed with no walls, draughty in every
/// wind. So a side gap is sorted by what is beyond it: air whose four
/// neighbours are all wall or room is a shaft up through the roof, and
/// counts once per column as a hole in it; anything else is the outside.
pub fn survey(look: impl Fn(i32, i32, i32) -> Option<crate::types::BlockId>, feet: (i32, i32, i32)) -> Option<Room> {
    let mut gaps: Vec<(Cell, (i32, i32))> = Vec::new();
    let mut ups: Vec<(i32, i32)> = Vec::new();
    let mut faces = 0u32;
    let (mut keeps_out, mut holds_days) = (0.0f32, 0.0f32);
    let walk = walk_room(&look, feet, feet.1 - 1 + ROOM_MAX_RISE, |face| match face {
        Face::Opening { from, dir } if dir.1 == 0 => gaps.push((from, (dir.0, dir.2))),
        Face::Opening { from, .. } => ups.push((from.0, from.2)),
        Face::Solid { block, .. } => {
            let m = material(block);
            faces += 1;
            keeps_out += m.keeps_out;
            holds_days += m.holds_days;
        }
    })?;
    let room: std::collections::HashSet<(i32, i32, i32)> = walk.cells.iter().copied().collect();
    let shut = |at: (i32, i32, i32)| {
        room.contains(&at) || look(at.0, at.1, at.2).is_some_and(crate::types::blocks_the_sky)
    };
    let mut side = Vec::new();
    let mut holes: std::collections::HashSet<(i32, i32)> = ups.into_iter().collect();
    for (from, (dx, dz)) in gaps {
        let beyond = (from.0 + dx, from.1, from.2 + dz);
        let shaft = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .all(|(nx, nz)| shut((beyond.0 + nx, beyond.1, beyond.2 + nz)));
        if shaft {
            holes.insert((beyond.0, beyond.2));
        } else {
            side.push((dx, dz));
        }
    }
    let per = 1.0 / faces.max(1) as f32;
    let bounds = walk.cells.iter().fold((feet, feet), |(low, high), &(x, y, z)| {
        ((low.0.min(x), low.1.min(y), low.2.min(z)), (high.0.max(x), high.1.max(y), high.2.max(z)))
    });
    Some(Room {
        bounds,
        key: (
            walk.cells[0],
            walk.cells.len() as u32,
            side.len().min(u16::MAX as usize) as u16,
            holes.len().min(u16::MAX as usize) as u16,
        ),
        cells: walk.cells.len(),
        side,
        roof: holes.len(),
        keeps_out: if faces == 0 { 0.0 } else { keeps_out * per },
        holds_days: if faces == 0 { 0.0 } else { holds_days * per },
    })
}

impl Room {
    /// Is this a room -- the thing `climate::Shelter::Enclosed` is -- or a
    /// roof with too much sky round it?
    pub fn is_enclosed(&self) -> bool {
        self.side.len() <= SIDE_OPENINGS_FOR_A_ROOM && self.roof <= ROOF_OPENINGS_FOR_A_ROOM
    }

    /// How much the wind gets in, 0 (still air) .. 1.
    ///
    /// Every open face in a wall is pushed on by the wind as far as it
    /// faces into it (the dot of its way out against where the wind comes
    /// from), plus a little whatever way it faces ([`ANY_SIDE_DRAUGHT`]).
    /// Exponential in the sum, so two doorways are worse than one and ten
    /// are not ten times worse.
    ///
    /// Rejected: a draught as degrees off the air. The open field has no
    /// wind chill of its own, so a draught that subtracted degrees would
    /// make a hut with its door in the wind colder than the meadow outside
    /// it. As a share of what the room keeps out, the worst a draught can
    /// do is make the room the roof it is.
    pub fn draught(&self, wind: Wind) -> f32 {
        let strength = if wind.strength.is_finite() { wind.strength.clamp(0.0, 1.0) } else { 0.0 };
        if strength == 0.0 {
            return 0.0;
        }
        let (tz, tx) = wind.toward.sin_cos();
        let (tx, tz) = if tx.is_finite() && tz.is_finite() { (tx, tz) } else { (0.0, 0.0) };
        // The raft's convention (`Wind::vector`): `toward` is where the
        // wind goes, so a gap faces into it when its way out points the
        // other way.
        let push: f32 = self
            .side
            .iter()
            .map(|&(dx, dz)| {
                let into = (-(dx as f32 * tx + dz as f32 * tz)).max(0.0);
                strength * (ANY_SIDE_DRAUGHT + (1.0 - ANY_SIDE_DRAUGHT) * into)
            })
            .sum();
        (1.0 - (-DRAUGHT_PER_FACE * push).exp()).clamp(0.0, 1.0)
    }

    /// How much of the night this room keeps out, 0..1: what
    /// `climate::Shelter::evens_out` is for the plain count.
    pub fn evens_out(&self, wind: Wind) -> f32 {
        let roof_only = crate::shelter::ROOF_ALONE;
        let keeps = self.keeps_out.clamp(0.0, 1.0) * (1.0 - self.draught(wind));
        roof_only + (BEST_EVENS_OUT - roof_only) * keeps
    }

    /// How much of a hearth's warmth stays in the room, 0..1: the draught
    /// and the smoke hole take the rest.
    pub fn heat_kept(&self, wind: Wind) -> f32 {
        (1.0 - DRAUGHT_TAKES_HEAT * self.draught(wind)) / (1.0 + ROOF_HEAT_LEAK * self.roof as f32)
    }

    /// How long this room holds a dead fire's heat, in days: its walls'
    /// mass, shortened by whatever lets the warm air out.
    pub fn holds_days(&self, wind: Wind) -> f32 {
        self.holds_days.max(0.0) * self.heat_kept(wind)
    }
}

/// How much of the night a roof alone keeps out: the floor a room's share
/// never falls under. The server's `climate::ROOF_EVENS_OUT`, restated here
/// because this crate cannot see the server; a test there holds the two
/// equal.
pub const ROOF_ALONE: f32 = 0.5;

/// What is left of a room's stored warmth after `elapsed_days` with no fire,
/// in a room that holds it for `holds_days`.
///
/// Exponential, which is how a warm thing cools in cold air: fast at first
/// and slower after. Written against elapsed time so a night slept through
/// in one step lands where the same night ticked would.
pub fn afterglow(share: f32, elapsed_days: f32, holds_days: f32) -> f32 {
    if !share.is_finite() || share <= 0.0 {
        return 0.0;
    }
    // Infinite time is not a nonsense clock, it is a room gone cold, and
    // `exp` below says so.
    if elapsed_days.is_nan() || elapsed_days <= 0.0 {
        return share.clamp(0.0, 1.0);
    }
    if !holds_days.is_finite() || holds_days <= 0.0 {
        return 0.0;
    }
    (share * (-elapsed_days / holds_days).exp()).clamp(0.0, 1.0)
}

/// What the health page is told about the place the player stands in.
/// See `protocol::ServerMessage::Shelter`.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Reading {
    /// The air the body is drifting towards, in degrees: fire, room, night
    /// and all. The skin is on the page already; this is *why* it is going
    /// where it is going.
    pub air_c: f32,
    /// In a room ([`Room::is_enclosed`]).
    pub indoors: bool,
    /// [`Room::draught`], 0..1.
    pub draught: f32,
    /// [`Room::keeps_out`], 0..1: what the walls are worth.
    pub keeps_out: f32,
    /// A hole in the roof over a room with a fire in reach: the heat going
    /// out with the smoke.
    pub roof_open: bool,
}

impl Reading {
    /// Worth sending again: a degree of air, a tenth of draught, or any
    /// change of state. The page is opened on purpose and read slowly, and
    /// the sample behind it is taken twice a second.
    pub fn differs(&self, other: &Reading) -> bool {
        (self.air_c - other.air_c).abs() >= 1.0
            || (self.draught - other.draught).abs() >= 0.1
            || (self.keeps_out - other.keeps_out).abs() >= 0.1
            || self.indoors != other.indoors
            || self.roof_open != other.roof_open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BLOCK_AIR, BLOCK_LOG, BLOCK_PLANKS, BLOCK_STONE, BLOCK_THATCH_SLAB,
    };
    use std::collections::HashMap;

    /// A hut on a stone floor at y 0: inside is x and z 0..=3, air at y 1..=3,
    /// walls of `wall` round it and a roof of `roof` at y 4, and a doorway,
    /// if asked for, in the east wall at (4, 1..=2, 1). Everything else in
    /// a sixteen-cell square is air, and past it nothing is loaded.
    fn hut(wall: BlockId, roof: BlockId, door_east: bool) -> HashMap<(i32, i32, i32), BlockId> {
        let mut cells = HashMap::new();
        for x in -6..=10 {
            for z in -6..=10 {
                for y in 0..=12 {
                    cells.insert((x, y, z), if y == 0 { BLOCK_STONE } else { BLOCK_AIR });
                }
            }
        }
        for x in -1..=4 {
            for z in -1..=4 {
                cells.insert((x, 4, z), roof);
                if x == -1 || x == 4 || z == -1 || z == 4 {
                    for y in 1..=3 {
                        cells.insert((x, y, z), wall);
                    }
                }
            }
        }
        if door_east {
            cells.insert((4, 1, 1), BLOCK_AIR);
            cells.insert((4, 2, 1), BLOCK_AIR);
        }
        cells
    }

    fn room(cells: &HashMap<(i32, i32, i32), BlockId>) -> Room {
        survey(|x, y, z| cells.get(&(x, y, z)).copied(), (1, 1, 1)).expect("the hut is not a room")
    }

    /// Wind blowing toward +x (from the west) at full strength, and the
    /// other way.
    fn east_wind(strength: f32) -> Wind {
        Wind { toward: 0.0, strength }
    }
    fn west_wind(strength: f32) -> Wind {
        Wind { toward: std::f32::consts::PI, strength }
    }

    #[test]
    fn a_shut_hut_is_a_room_with_no_draught_whatever_the_wind() {
        let shut = room(&hut(BLOCK_STONE, BLOCK_STONE, false));
        assert!(shut.is_enclosed());
        assert!(shut.side.is_empty() && shut.roof == 0);
        assert_eq!(shut.draught(west_wind(1.0)), 0.0);
        assert_eq!(shut.heat_kept(west_wind(1.0)), 1.0);
    }

    #[test]
    fn a_doorway_into_the_wind_is_a_draught_and_the_same_doorway_in_the_lee_is_not() {
        let open = room(&hut(BLOCK_LOG, BLOCK_LOG, true));
        assert!(open.is_enclosed(), "a doorway unmade the room");
        assert_eq!(open.side.len(), 2, "a doorway two high is two open faces");
        // The door is in the east wall. A wind from the east blows into it.
        let into = open.draught(west_wind(1.0));
        let lee = open.draught(east_wind(1.0));
        assert!(into > 0.5, "a storm into the doorway was a draught of {into}");
        assert!(lee < into / 3.0, "the lee doorway ({lee}) was nearly as draughty as the windward one ({into})");
        assert_eq!(open.draught(Wind::CALM), 0.0, "still air made a draught");
        assert!(open.evens_out(west_wind(1.0)) < open.evens_out(east_wind(1.0)));
        assert!(open.heat_kept(west_wind(1.0)) < open.heat_kept(east_wind(1.0)));
    }

    #[test]
    fn a_smoke_hole_lets_out_some_of_the_heat_and_less_of_it_than_the_smoke() {
        let mut cells = hut(BLOCK_STONE, BLOCK_STONE, false);
        cells.insert((1, 4, 1), BLOCK_AIR);
        let holed = room(&cells);
        assert!(holed.is_enclosed(), "a smoke hole unmade the room");
        assert_eq!(holed.roof, 1);
        assert!(holed.side.is_empty(), "the shaft under a smoke hole read as {} gaps in the walls", holed.side.len());
        assert_eq!(holed.draught(west_wind(1.0)), 0.0, "a smoke hole made a draught");
        let heat = holed.heat_kept(Wind::CALM);
        let smoke = crate::wildfire::smoke_kept(0, 1);
        assert!(heat < 1.0, "a hole in the roof kept all the heat");
        assert!(heat > smoke, "the hole let out as much heat ({heat}) as smoke ({smoke}): nobody would cut one");
    }

    #[test]
    fn thatch_keeps_the_night_out_and_stone_keeps_the_fire_in() {
        let thatch = material(BLOCK_THATCH_SLAB);
        let stone = material(BLOCK_STONE);
        let planks = material(BLOCK_PLANKS);
        let earth = material(BLOCK_DIRT);
        let snow = material(BLOCK_SNOW);
        assert!(thatch.keeps_out > stone.keeps_out, "stone kept the night out better than thatch");
        assert!(stone.holds_days > thatch.holds_days * 3.0, "stone held a fire no longer than thatch");
        assert!(planks.keeps_out < material(BLOCK_LOG).keeps_out, "boards were as warm as logs");
        assert!(earth.keeps_out >= thatch.keeps_out && earth.holds_days >= stone.holds_days, "earth was not the best of both");
        assert!(snow.keeps_out > stone.keeps_out && snow.holds_days < stone.holds_days, "an igloo was not warm and quick to cool");

        let log_room = room(&hut(BLOCK_LOG, BLOCK_LOG, false));
        let board_room = room(&hut(BLOCK_PLANKS, BLOCK_PLANKS, false));
        assert!(log_room.evens_out(Wind::CALM) > board_room.evens_out(Wind::CALM));
        assert!(log_room.evens_out(Wind::CALM) <= BEST_EVENS_OUT);
        assert!(board_room.evens_out(Wind::CALM) > ROOF_ALONE, "four walls of boards were worth no more than a roof");
    }

    #[test]
    fn a_stone_room_is_still_warm_after_the_fire_when_a_plank_one_is_not() {
        let stone = room(&hut(BLOCK_STONE, BLOCK_STONE, false)).holds_days(Wind::CALM);
        let planks = room(&hut(BLOCK_PLANKS, BLOCK_PLANKS, false)).holds_days(Wind::CALM);
        // A quarter of a day: from a fire gone out at midnight to dawn.
        let (warm_stone, warm_planks) = (afterglow(0.55, 0.25, stone), afterglow(0.55, 0.25, planks));
        assert!(warm_stone > 0.1, "a stone room kept {warm_stone} of the fire till dawn");
        assert!(warm_planks < 0.01, "a plank room kept {warm_planks} of the fire till dawn");
        assert_eq!(afterglow(0.5, 0.0, stone), 0.5);
        assert_eq!(afterglow(f32::NAN, 0.1, stone), 0.0);
        assert_eq!(afterglow(0.5, f32::INFINITY, stone), 0.0);
    }

    #[test]
    fn open_air_and_an_unloaded_neighbourhood_are_not_rooms() {
        let field: HashMap<(i32, i32, i32), BlockId> =
            (-4..=4).flat_map(|x| (-4..=4).flat_map(move |z| (0..=10).map(move |y| ((x, y, z), BLOCK_AIR)))).collect();
        assert!(survey(|x, y, z| field.get(&(x, y, z)).copied(), (0, 1, 0)).is_none());
        let mut cells = hut(BLOCK_STONE, BLOCK_STONE, false);
        cells.remove(&(2, 2, 2));
        assert!(survey(|x, y, z| cells.get(&(x, y, z)).copied(), (1, 1, 1)).is_none(), "a hole in the loaded world was walled");
    }

    #[test]
    fn a_shed_with_its_walls_gone_is_not_a_room() {
        let mut cells = hut(BLOCK_STONE, BLOCK_STONE, false);
        for z in 0..=3 {
            for y in 1..=3 {
                cells.insert((4, y, z), BLOCK_AIR);
            }
        }
        let shed = room(&cells);
        assert!(!shed.is_enclosed(), "a hut with a whole wall open was a room: {} side faces", shed.side.len());
    }

    #[test]
    fn a_reading_is_sent_again_on_a_degree_and_not_on_a_breath() {
        let a = Reading { air_c: 10.0, indoors: true, draught: 0.2, keeps_out: 0.8, roof_open: false };
        assert!(!a.differs(&Reading { air_c: 10.4, ..a }));
        assert!(a.differs(&Reading { air_c: 11.2, ..a }));
        assert!(a.differs(&Reading { roof_open: true, ..a }));
    }
}
