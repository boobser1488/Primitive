//! Finds: things in the world worth walking to that nobody built.
//!
//! ## What they are for
//!
//! **Each one is a reason to go somewhere, and each one is a decision
//! when you get there.** The ruins are what people left; these are what
//! the land offers a person with nothing, and every one of them is
//! something the stone age actually went looking for:
//!
//! * **A rock shelter** -- an overhang of limestone or sandstone with a
//!   back wall, cheeks and a roof, and the ash of old fires under it. A
//!   fire under a roof does not go out in the rain and a body under one
//!   keeps its warmth (`climate::shelter_at` reads the roof), so it is a
//!   camp that costs no building -- a day's walk from the resources, or a
//!   night in the open near them.
//! * **A fallen giant** -- a trunk two thick and a dozen long lying in a
//!   wood, its root plate torn up at one end and a limb under its flank to
//!   climb on by. Lying wood is gathered by hand (`types::break_seconds`),
//!   so it is forty logs before anybody has an axe, with tinder fungus on
//!   its flanks: the first night's fire and the first shelter's timber in
//!   one place, if you find it. **One in three is hollow** -- three across
//!   and three high, rotted out along its heart from the torn-off tip to
//!   the root: a tunnel a player walks into out of the rain, with the
//!   sticks and a mushroom the wood kept dry at its closed end. See
//!   `plan_giant`.
//! * **A berry thicket** -- a clump of a dozen or more bushes at the edge
//!   of a meadow wood. Berries grow back (`types::ripens_into`), so a
//!   thicket is a larder a player remembers and returns to rather than a
//!   meal they trip over.
//! * **A knapping floor** -- a scatter of flint nodules and struck flakes
//!   on bare rock where somebody sat and worked, twice as common over
//!   limestone, which is where flint forms (`flint_spacing`). A flake is a
//!   blade already knapped: a knife without the risk of shattering the
//!   nodule.
//! * **A ring of mushrooms** -- caps in a circle on bared earth in a dark
//!   wood, one in four of them the toadstool. The only fungus in daylight:
//!   food without a cave, and a quarter of it poison, so a ring is the one
//!   place the flecks on the cap (`BLOCK_TOADSTOOL`) are the whole question.
//!
//! ## Where they stand, and why every chunk agrees
//!
//! One candidate per `FeatureKind::cell`-square region, at a hashed spot
//! kept `reach` clear of the cell's edges, **judged from the column
//! tiles** rather than from the chunk's column cache -- the arrangement
//! `ruins` argues for. A fallen giant is fourteen blocks long and the cache
//! only reaches `FEATURE_MARGIN` past its chunk, so a verdict read from the
//! cache would come out differently in the two chunks a trunk crosses and
//! the log would stop at the seam. The tiles are the store the caches are
//! copied from, so every chunk reads the same ground.
//!
//! What a find writes is then worked out from the site and the columns
//! alone (`WorldGen::plan_feature`), and each chunk keeps the cells that
//! fall inside it. `a_find_is_whole_across_a_chunk_seam` holds that.
//!
//! ## Rejected
//!
//! * **Rooting them like boulders, per column, reading the cache.** Fine
//!   for a thing three wide; a feature fourteen wide judged that way needs
//!   a margin of twenty-eight, which would triple every chunk's column
//!   cache in every biome for a handful of logs.
//! * **Hot springs, dens and salt licks**, from the list these came out of.
//!   Each needs a new server mechanic (water that is warm, animals that
//!   return to a place) as well as the generator, and a feature that is only
//!   its shape until the mechanic exists is scenery. **Beehives were on this
//!   line and are not any more**: the mechanic they waited for turned out to
//!   be smoke the game already had. A raid stings (`bees::stings`), and
//!   anything burning near the hive -- a torch in the hand, a fire under the
//!   tree, a smoky room (`bees::smokes_bees`) -- calms the bees, so the hive
//!   is a decision rather than a shape. They are not a feature here either:
//!   a hive is one cell on a trunk, and the tree pass's own chunk already
//!   holds both (`WorldGen::place_hives`).
//! * **Abandoned campsites.** A cold hearth and a few things lying about is
//!   what a settlement ruin already is at its smallest (`RuinKind::Footing`
//!   and the chest the ruins keep). A second thing of the same meaning at a
//!   different density is clutter, and the ruins were already too common
//!   once.

use super::{
    column_tile_with, hash2, put_block, ruin_offered, Biome, Column, Preset, WorldGen, MAX_CANOPY_RADIUS,
    SEA_LEVEL, TILE,
};
use crate::types::{
    block_kind, faced, has_full_top, oriented, support_at, Axis, BlockId, ChunkPos, Facing, BLOCK_AIR,
    BLOCK_ASH, BLOCK_BERRY_BUSH, BLOCK_BIRCH_LOG, BLOCK_BRACKET_FUNGUS, BLOCK_BUSH_LEAVES,
    BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_FLINT, BLOCK_FLINT_FLAKE, BLOCK_GRANITE, BLOCK_GRASS,
    BLOCK_GRAVEL, BLOCK_LIMESTONE, BLOCK_LOG, BLOCK_MUSHROOM, BLOCK_PEBBLE, BLOCK_SANDSTONE,
    BLOCK_STICK, BLOCK_STONE, BLOCK_TOADSTOOL, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};

/// The five kinds of find. See the module note for what each is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureKind {
    RockShelter,
    FallenGiant,
    BerryThicket,
    KnappingFloor,
    MushroomRing,
}

impl FeatureKind {
    pub const ALL: [FeatureKind; 5] = [
        FeatureKind::RockShelter,
        FeatureKind::FallenGiant,
        FeatureKind::BerryThicket,
        FeatureKind::KnappingFloor,
        FeatureKind::MushroomRing,
    ];

    /// A word for a tool's log line.
    pub fn name(self) -> &'static str {
        match self {
            FeatureKind::RockShelter => "rock shelter",
            FeatureKind::FallenGiant => "fallen giant",
            FeatureKind::BerryThicket => "berry thicket",
            FeatureKind::KnappingFloor => "knapping floor",
            FeatureKind::MushroomRing => "mushroom ring",
        }
    }

    /// Side of the region cell one candidate is drawn from, in blocks.
    ///
    /// **Measured, not chosen** -- `each_find_is_a_long_walk_from_the_next`
    /// prints the walk per find on three seeds, and these are set so that
    /// none of them is on the skyline of every walk the way the ruins were
    /// at a cell of 128. The ground refuses most candidates (a shelter
    /// wants limestone or sandstone, a giant wants a level wood), so a
    /// cell this size is not the distance a player walks: see the test for
    /// that number.
    fn cell(self) -> i32 {
        match self {
            FeatureKind::RockShelter => 160,
            FeatureKind::FallenGiant => 128,
            FeatureKind::BerryThicket => 224,
            FeatureKind::KnappingFloor => 128,
            FeatureKind::MushroomRing => 144,
        }
    }

    /// The furthest any block of this find lands from its centre, on
    /// either axis. What keeps a find inside its own cell, and what a chunk
    /// asks before paying for a verdict.
    pub fn reach(self) -> i32 {
        match self {
            // Back wall three behind the middle, roof one in front, cheeks
            // three either side.
            FeatureKind::RockShelter => 3,
            // Half of thirteen, the root plate one past the butt, three
            // sticks past the tip.
            FeatureKind::FallenGiant => 10,
            FeatureKind::BerryThicket => 3,
            FeatureKind::KnappingFloor => 2,
            // A ring of radius four reaches four and a half.
            FeatureKind::MushroomRing => 5,
        }
    }

    fn salt(self) -> u32 {
        match self {
            FeatureKind::RockShelter => 0x5E17_E400,
            FeatureKind::FallenGiant => 0x0DDF_A11E,
            FeatureKind::BerryThicket => 0xBE44_71C7,
            FeatureKind::KnappingFloor => 0xF117_F100,
            FeatureKind::MushroomRing => 0xFA1E_0419,
        }
    }
}

/// A find that stands: what, where, on what ground, and the roll its
/// shape is drawn from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Feature {
    pub kind: FeatureKind,
    /// The middle of it, in world columns.
    pub x: i32,
    pub z: i32,
    /// The height of the ground it is built on: the top solid cell of the
    /// middle column, so it stands at `ground + 1`.
    pub ground: i32,
    pub biome: Biome,
    roll: u32,
    /// The rock a shelter is made of, or the wood a giant is.
    material: BlockId,
    /// Whether a giant is hollow. See `plan_giant`.
    hollow: bool,
}

impl Feature {
    /// One of the four ways a find can be turned, off its roll: the step a
    /// shelter opens toward, or the way a giant's butt points.
    fn step(&self) -> (i32, i32) {
        [(1, 0), (0, 1), (-1, 0), (0, -1)][((self.roll >> 8) & 3) as usize]
    }

    /// A find's own axes to a world column: `u` along its step, `v` across.
    fn world(&self, u: i32, v: i32) -> (i32, i32) {
        let (fx, fz) = self.step();
        let (px, pz) = (-fz, fx);
        (self.x + fx * u + px * v, self.z + fz * u + pz * v)
    }

    /// A number for one column of this find, from the world position, so
    /// every chunk that holds the column rolls the same.
    fn hash_at(&self, gx: i32, gz: i32, salt: u32) -> u32 {
        hash2(gx, gz, self.roll ^ salt)
    }

    /// How long a fallen giant is: nine to thirteen.
    fn length(&self) -> i32 {
        9 + ((self.roll >> 12) % 5) as i32
    }

    /// How many logs across and high a giant's trunk is: two, or three for
    /// a hollow one -- a hollow needs a wall round it.
    fn width(&self) -> i32 {
        if self.hollow {
            3
        } else {
            2
        }
    }

    /// Whether this giant's roll asks for it to be hollow: one in
    /// `HOLLOW_SHARE`. Whether it *is* is the judge's, from the ground.
    fn rolls_hollow(&self) -> bool {
        (self.roll >> 20).is_multiple_of(HOLLOW_SHARE)
    }

    /// A ring's radius: three or four.
    fn radius(&self) -> i32 {
        3 + ((self.roll >> 16) & 1) as i32
    }
}

/// One fallen giant in this many is hollow, where the ground allows it.
///
/// **Rare on purpose, and not rarer than that.** A giant is already a find a
/// long walk from the last (`each_find_is_a_long_walk_from_the_next`), so a
/// third of them is a hollow log every few giants a player meets -- often
/// enough that one is remembered as a place to sleep dry, seldom enough that
/// it is a find and not what fallen trees are. Rejected: *a find of its own*,
/// with its own cell and judge. It is a giant in every rule -- the wood it is
/// found in, the ground it needs, the tinder on its flanks -- and a second
/// kind would have been a second copy of all of it, judged separately, and
/// free to stand beside the first.
const HOLLOW_SHARE: u32 = 3;

/// Where a cell's candidate is, and the roll its site is judged with.
fn candidate(seed: u32, kind: FeatureKind, cx: i32, cz: i32) -> (i32, i32, u32) {
    let salt = seed.wrapping_add(kind.salt());
    let pad = kind.reach() + 1;
    let span = (kind.cell() - 2 * pad) as u32;
    let x = cx * kind.cell() + pad + (hash2(cx, cz, salt) % span) as i32;
    let z = cz * kind.cell() + pad + (hash2(cz, cx, salt ^ 0x9E37) % span) as i32;
    // The shape's roll off the position rather than off the cell, so the
    // facing is not correlated with where in its cell a find landed.
    (x, z, hash2(x, z, salt ^ 0xD1CE))
}

/// One column, from the tile store -- the copy every chunk's cache is
/// assembled from. The same four lines `ruins` has, which is private there.
fn column(gen: &WorldGen, gx: i32, gz: i32) -> Column {
    let (tx, tz) = (gx.div_euclid(TILE), gz.div_euclid(TILE));
    let (lx, lz) = (gx.rem_euclid(TILE), gz.rem_euclid(TILE));
    column_tile_with(gen, tx, tz, |columns| columns[(lz * TILE + lx) as usize])
}

/// Land a find may stand on: above the tide, above its own water, and not
/// a river bed or a beach.
fn dry(here: &Column) -> bool {
    !matches!(here.biome, Biome::Ocean | Biome::Beach | Biome::River)
        && here.height > SEA_LEVEL + 1
        && here.height > here.water
}

impl WorldGen {
    /// Whether a column's ground is really there: a cave that reached the
    /// surface takes the top block, or the one under it, and a thing laid
    /// on the terrain height there hangs over the hole. `place_deadfall`'s
    /// rule, for its reason.
    fn ground_is_whole(&self, gx: i32, gz: i32, here: &Column) -> bool {
        !self.is_cave(gx, here.height, gz) && !self.is_cave(gx, here.height - 1, gz)
    }

    /// Whether no tree is rooted within `margin` of a box of columns, the
    /// corners given in either order.
    ///
    /// **Refused rather than cut round.** A shelter's roof through a crown
    /// is half a tree standing in rock, and a giant laid through a trunk is
    /// a log with a tree growing out of its middle. The tree pass runs
    /// first and cannot be asked to step aside without teaching it about
    /// every find, which is `ruin_claims`' arrangement repeated five times.
    ///
    /// **The margin is what the find can collide with, not the widest
    /// crown in the world.** The first version asked every root within an
    /// old tree's reach of the find's whole reach square, and in a wood
    /// that is always a tree: over three seeds not one fallen giant stood.
    /// A shelter's roof is at crown height, so its margin is a crown's; a
    /// giant lies below every crown but a fir's and asks only about the
    /// trunks on its own cells (`judge_giant`).
    fn clear_of_trees(&self, a: (i32, i32), b: (i32, i32), margin: i32) -> bool {
        for gz in a.1.min(b.1) - margin..=a.1.max(b.1) + margin {
            for gx in a.0.min(b.0) - margin..=a.0.max(b.0) + margin {
                if self.tree_at(gx, gz, column(self, gx, gz).biome) {
                    return false;
                }
            }
        }
        true
    }

    /// The verdict on one cell's candidate: the find that stands there, or
    /// nothing.
    pub(super) fn feature_site(&self, kind: FeatureKind, cx: i32, cz: i32) -> Option<Feature> {
        if self.preset == Preset::Test {
            return None;
        }
        let (x, z, roll) = candidate(self.seed, kind, cx, cz);
        let middle = column(self, x, z);
        if !dry(&middle) || middle.height + 8 >= CHUNK_SIZE_Y as i32 {
            return None;
        }
        // **Not where a ruin is**, which clears its room and would sweep
        // half a find out of the way or build a wall through it; and not on
        // an old brick ruin's chunk, which clears to the sky. The ruin's own
        // tree guard is the right distance: anything that could lean a
        // crown over a wall could lay a log across one.
        if self.ruin_claims(x, z) {
            return None;
        }
        let reach = kind.reach();
        for chunk_z in (z - reach).div_euclid(CHUNK_SIZE_Z as i32)..=(z + reach).div_euclid(CHUNK_SIZE_Z as i32) {
            for chunk_x in (x - reach).div_euclid(CHUNK_SIZE_X as i32)..=(x + reach).div_euclid(CHUNK_SIZE_X as i32) {
                if ruin_offered(self, ChunkPos::new(chunk_x, chunk_z)) {
                    return None;
                }
            }
        }
        let site = Feature {
            kind,
            x,
            z,
            ground: middle.height,
            biome: middle.biome,
            roll,
            material: BLOCK_AIR,
            hollow: false,
        };
        match kind {
            FeatureKind::RockShelter => self.judge_shelter(site, &middle),
            FeatureKind::FallenGiant => self.judge_giant(site),
            FeatureKind::BerryThicket => self.judge_thicket(site),
            FeatureKind::KnappingFloor => self.judge_knapping(site, &middle),
            FeatureKind::MushroomRing => self.judge_ring(site),
        }
    }

    /// A shelter wants rock that makes shelters and level ground to stand
    /// the room on.
    ///
    /// **Limestone and sandstone and nothing else.** Those are the rocks
    /// real overhangs weather out of -- a soft bed eaten back under a hard
    /// one -- and granite does not do it. It is also the rule the boulders
    /// keep: a block of granite or cobble standing on a meadow has to be
    /// explained by a slope or a cliff
    /// (`a_boulder_lies_on_a_bank_or_under_a_cliff_and_never_in_a_meadow`),
    /// and a shelter on the flat is exactly what that test exists to refuse.
    fn judge_shelter(&self, mut site: Feature, middle: &Column) -> Option<Feature> {
        // **Open country only.** A shelter is rock written over whatever
        // stood in its cells, and in a wood what stands there is deadfall:
        // two shelters in the sweep `a_fallen_trunk_lies_on_the_ground_rather_than_over_a_hole`
        // counts took that test's fallen logs under a hundred. Out on the
        // grass nothing lies there to take, and a shelter is a thing seen
        // from across the country rather than found behind a tree.
        if !matches!(site.biome, Biome::Plains | Biome::Steppe | Biome::Hills | Biome::Tundra | Biome::Savanna | Biome::Desert) {
            return None;
        }
        if middle.height >= middle.granite_from {
            return None;
        }
        // Plain stone as well, as the ruins build in it: it is neither of
        // the two blocks the boulder rule is about, and a shelter kept to
        // the pale rocks alone was one per seven to twenty thousand blocks
        // of walking -- a rumour rather than a find.
        site.material = match (site.biome, middle.rock) {
            (Biome::Desert | Biome::Savanna, _) => BLOCK_SANDSTONE,
            (_, BLOCK_LIMESTONE) => BLOCK_LIMESTONE,
            (_, BLOCK_SANDSTONE) => BLOCK_SANDSTONE,
            (_, BLOCK_GRANITE) => return None,
            _ => BLOCK_STONE,
        };
        for u in -3..=1 {
            for v in -3..=3 {
                let (gx, gz) = site.world(u, v);
                let here = column(self, gx, gz);
                // The floor under the roof within a step of the middle, so
                // there is headroom over all of it; the walls and cheeks
                // may stand on ground two lower, because they are built up
                // from wherever it is.
                let floor = (-1..=1).contains(&u) && v.abs() <= 2;
                let fall = if floor { 1 } else { 2 };
                if !dry(&here) || (here.height - site.ground).abs() > fall || !self.ground_is_whole(gx, gz, &here) {
                    return None;
                }
            }
        }
        self.clear_of_trees(site.world(-3, -3), site.world(1, 3), MAX_CANOPY_RADIUS)
            .then_some(site)
    }

    /// A giant wants a level wood: every column under the trunk, the root
    /// plate and the limb at the ground's own height, on turf.
    ///
    /// **Ground a step either way of the middle, and the trunk follows it.**
    /// A trunk laid across a dip is a log in the air, which
    /// `a_fallen_trunk_lies_on_the_ground_rather_than_over_a_hole` refuses,
    /// and one through a hump is a log in the ground. So a column one below
    /// gets a cell of earth under the log -- a trunk that came down on soft
    /// ground pressed into it -- and a column one above takes the place of
    /// the lower log, with the upper one lying on the hump (`plan_giant`).
    /// No cell of trunk has air under it either way. Exactly level was
    /// tried first, and then level or a step down: over three seeds one
    /// giant stood, and a bump of one refused more candidates than every
    /// other rule together (`why_finds_are_refused` counted it).
    ///
    /// **Turf or bare earth under it**, the floor of a wood. Turf alone was
    /// required once, on the argument that a log at ground height is what
    /// `trees_stand_on_grass_and_nothing_else` reads as a trunk base -- and
    /// it is not: that test looks for an upright `BLOCK_LOG`, and a lying
    /// log's id carries its axis. Not in the taiga: a fir is clothed to the
    /// ground, and its lowest boughs are where the trunk would lie.
    fn judge_giant(&self, mut site: Feature) -> Option<Feature> {
        site.material = match site.biome {
            Biome::BirchForest => BLOCK_BIRCH_LOG,
            Biome::Forest | Biome::DeadForest | Biome::Swamp => BLOCK_LOG,
            _ => return None,
        };
        // **Hollow where the roll says and the ground lets it, and a plain
        // giant where it does not.** A hollow one is a column wider, and its
        // floor is the ground itself, so a hump under the hollow would fill
        // the tunnel with earth: those are refused as hollow and judged again
        // as the solid giant the same place would have had.
        if site.rolls_hollow() {
            let hollow = Feature { hollow: true, ..site };
            let (butt, tip) = giant_span(&hollow);
            let level_floor = (butt + 1..=tip).all(|u| {
                let (gx, gz) = hollow.world(u, 1);
                column(self, gx, gz).height <= hollow.ground
            });
            if level_floor && self.giant_stands(&hollow) {
                return Some(hollow);
            }
        }
        self.giant_stands(&site).then_some(site)
    }

    /// Does the ground under every cell a giant stands on take it? See
    /// `judge_giant`.
    fn giant_stands(&self, site: &Feature) -> bool {
        for (u, v) in giant_cells(site) {
            let (gx, gz) = site.world(u, v);
            let here = column(self, gx, gz);
            let floor = matches!(block_kind(here.surface.top), BLOCK_GRASS | BLOCK_DIRT);
            if !dry(&here)
                || (here.height - site.ground).abs() > 1
                || !floor
                || !self.ground_is_whole(gx, gz, &here)
            {
                return false;
            }
        }
        // **A trunk on a cell the giant lies on, and nothing further**:
        // every crown but a fir's is above a log two high. An old tree's
        // bole is two by two from its root, so a root one column west or
        // north of a cell stands on it too. A box round the footprint, even
        // two wide, holds a tree in every wood there is -- measured, no
        // giant stood on three seeds with one.
        let rooted = giant_cells(site).into_iter().any(|(u, v)| {
            let (gx, gz) = site.world(u, v);
            [(0, 0), (-1, 0), (0, -1), (-1, -1)]
                .into_iter()
                .any(|(dx, dz)| self.tree_at(gx + dx, gz + dz, column(self, gx + dx, gz + dz).biome))
        });
        !rooted
    }

    /// A thicket wants a meadow wood's edge: most of a small disc of turf.
    fn judge_thicket(&self, site: Feature) -> Option<Feature> {
        if !matches!(site.biome, Biome::Forest | Biome::BirchForest | Biome::Plains | Biome::Hills) {
            return None;
        }
        let good = disc(3, 10)
            .filter(|&(dx, dz)| self.thicket_column(&site, site.x + dx, site.z + dz).is_some())
            .count();
        (good * 3 >= disc(3, 10).count() * 2).then_some(site)
    }

    fn thicket_column(&self, site: &Feature, gx: i32, gz: i32) -> Option<Column> {
        let here = column(self, gx, gz);
        (dry(&here)
            && (here.height - site.ground).abs() <= 1
            && matches!(block_kind(here.surface.top), BLOCK_GRASS | BLOCK_DIRT)
            && self.ground_is_whole(gx, gz, &here))
        .then_some(here)
    }

    /// A knapping floor wants bare rock, twice as readily over limestone.
    fn judge_knapping(&self, site: Feature, middle: &Column) -> Option<Feature> {
        if matches!(site.biome, Biome::Swamp | Biome::Bog) {
            return None;
        }
        if !matches!(
            block_kind(middle.surface.top),
            BLOCK_STONE | BLOCK_COBBLESTONE | BLOCK_LIMESTONE | BLOCK_SANDSTONE | BLOCK_GRANITE | BLOCK_GRAVEL
        ) {
            return None;
        }
        // **Twice as common over limestone**, the ratio the scattered
        // nodules keep (`a_limestone_outcrop_has_two_to_three_times_the_flint_of_any_other_rock`),
        // so the rule a knapper learns from single stones holds for the
        // floors as well.
        if middle.rock != BLOCK_LIMESTONE && (site.roll >> 20) & 1 == 1 {
            return None;
        }
        let good = disc(2, 6)
            .filter(|&(dx, dz)| self.knapping_column(&site, site.x + dx, site.z + dz).is_some())
            .count();
        (good * 2 >= disc(2, 6).count()).then_some(site)
    }

    fn knapping_column(&self, site: &Feature, gx: i32, gz: i32) -> Option<Column> {
        let here = column(self, gx, gz);
        (dry(&here)
            && (here.height - site.ground).abs() <= 1
            && has_full_top(here.surface.top)
            && self.ground_is_whole(gx, gz, &here))
        .then_some(here)
    }

    /// A ring wants the floor of a dark wood, most of the way round.
    fn judge_ring(&self, site: Feature) -> Option<Feature> {
        if !matches!(site.biome, Biome::Forest | Biome::BirchForest | Biome::Taiga) {
            return None;
        }
        let cells: Vec<(i32, i32)> = ring(site.radius()).collect();
        let good = cells
            .iter()
            .filter(|&&(dx, dz)| self.ring_column(&site, site.x + dx, site.z + dz).is_some())
            .count();
        (good * 4 >= cells.len() * 3).then_some(site)
    }

    /// A column of a ring that can hold a cap: turf or bare earth at the
    /// ring's height, whole, and **not under a tree's own foot**. The ring
    /// bares the turf it stands on, and a trunk left rooted in bare earth
    /// is a tree the tree tests call planted wrong -- so a ring passes
    /// round a trunk the way a real one does.
    fn ring_column(&self, site: &Feature, gx: i32, gz: i32) -> Option<Column> {
        let here = column(self, gx, gz);
        if !dry(&here)
            || (here.height - site.ground).abs() > 1
            || !matches!(block_kind(here.surface.top), BLOCK_GRASS | BLOCK_DIRT)
            || !self.ground_is_whole(gx, gz, &here)
        {
            return None;
        }
        // An old tree's bole is two by two from its root, so a root one
        // column west or north still stands on this one.
        let rooted = [(0, 0), (-1, 0), (0, -1), (-1, -1)]
            .into_iter()
            .any(|(dx, dz)| self.tree_at(gx + dx, gz + dz, column(self, gx + dx, gz + dz).biome));
        (!rooted).then_some(here)
    }

    /// Everything a find writes, in world coordinates, as `(x, y, z, block,
    /// overwrite)`. `overwrite` false writes only into air, as `put_block`
    /// does.
    ///
    /// **From the site and the columns and nothing else** -- never from
    /// what a chunk being generated already holds -- so the plan is the
    /// same whichever chunk asks, and each keeps the cells that fall inside
    /// it. The into-air writes are what lets a tuft or a bush that got
    /// there first keep its cell.
    pub(super) fn plan_feature(&self, site: &Feature, emit: &mut dyn FnMut(i32, i32, i32, BlockId, bool)) {
        match site.kind {
            FeatureKind::RockShelter => self.plan_shelter(site, emit),
            FeatureKind::FallenGiant => self.plan_giant(site, emit),
            FeatureKind::BerryThicket => self.plan_thicket(site, emit),
            FeatureKind::KnappingFloor => self.plan_knapping(site, emit),
            FeatureKind::MushroomRing => self.plan_ring(site, emit),
        }
    }

    /// ```text
    ///   side view, opening to the right       from above
    ///     ###                                 #######   back wall, u -3..-2
    ///     #####                               #######
    ///     ######   roof at ground + 4         #.....#   cheeks at the sides
    ///     ##       headroom of three          #..a..#   a: the ash of old fires
    ///     ##  a                               ~~~~~~    roof overhangs one more
    /// ```
    ///
    /// Three cells of headroom over the middle of the floor and a roof one
    /// cell deeper than the floor it covers, so a fire laid at the ash is
    /// under rock with a cell of it to spare on the open side. The front of
    /// the roof is ragged, cut by a hash per column: a straight lintel of
    /// rock is a doorway somebody built.
    fn plan_shelter(&self, site: &Feature, emit: &mut dyn FnMut(i32, i32, i32, BlockId, bool)) {
        let (g, rock) = (site.ground, site.material);
        for u in -3..=1 {
            for v in -3..=3 {
                let (gx, gz) = site.world(u, v);
                let ground = column(self, gx, gz).height;
                if u <= -2 {
                    for y in ground + 1..=g + 3 {
                        emit(gx, y, gz, rock, true);
                    }
                } else if v.abs() == 3 && u <= 0 {
                    // The cheeks, a course or two high: what is left of the
                    // bed that the hollow was eaten out of.
                    let top = g + 1 + (site.hash_at(gx, gz, 0xC4EE) % 2) as i32;
                    for y in ground + 1..=top {
                        emit(gx, y, gz, rock, true);
                    }
                }
                let ragged = u == 1 && (v.abs() == 3 || site.hash_at(gx, gz, 0x2AFF).is_multiple_of(3));
                if !ragged {
                    emit(gx, g + 4, gz, rock, true);
                }
                if u <= -1 && v.abs() <= 2 {
                    emit(gx, g + 5, gz, rock, true);
                }
                if u <= -2 && v.abs() <= 1 {
                    emit(gx, g + 6, gz, rock, true);
                }
            }
        }
        // The ash of the fires people made here, and a flake one of them
        // struck and left. Into air, on a whole floor only.
        for (u, v, block) in [(0, 0, BLOCK_ASH), (-1, 1, BLOCK_FLINT_FLAKE)] {
            let (gx, gz) = site.world(u, v);
            let here = column(self, gx, gz);
            if has_full_top(here.surface.top) {
                emit(gx, here.height + 1, gz, block, false);
            }
        }
    }

    /// ```text
    ///   from above, butt to the left
    ///     d              the root plate: dirt round the torn trunk end
    ///     dLLLLLLLLLLLL  the trunk, two across and two high
    ///     dLLLLLLLLLLLL
    ///     d     ll       a limb under its flank: the step up
    ///                 s s s  sticks where the crown broke off
    /// ```
    ///
    /// **A hollow one** is three across and three high, and its middle column
    /// is open from the ground to under the top log for its whole length but
    /// the butt: a tunnel two high and one wide, the size of a person, closed
    /// at the root end and open at the tip.
    ///
    /// ```text
    ///   the tip end of a hollow giant
    ///     LLL
    ///     L.L      two cells of hollow, on the bare earth the log killed
    ///     L.L
    /// ```
    ///
    /// **What it is for is the roof.** A body under one keeps its warmth and
    /// a fire under one does not go out in the rain (`climate::shelter_at`
    /// reads the roof), so a hollow giant is a camp in a wood that costs no
    /// building -- the rock shelter's offer, in the woods where there is no
    /// rock. What lies at its closed end is what a dry hollow in a rotting
    /// log keeps: a couple of sticks the wind blew in and a mushroom on the
    /// rotted heartwood, which is kindling and a mouthful for the first
    /// night. Rejected: *a chest*. A chest in a log is somebody's cache, and
    /// this is not a place anybody has been.
    ///
    /// Two high because a player is: one high would be a hole to reach into,
    /// and "a log you could crawl into" in a game with no crawling is a log
    /// you look at.
    fn plan_giant(&self, site: &Feature, emit: &mut dyn FnMut(i32, i32, i32, BlockId, bool)) {
        let g = site.ground;
        let w = site.width();
        // The hollow: the middle column, two cells up, from past the butt to
        // the tip. Nothing for a solid giant.
        let (butt_u, _) = giant_span(site);
        let in_hollow = |u: i32, v: i32, dy: i32| site.hollow && v == 1 && dy <= 2 && u > butt_u;
        let (fx, _) = site.step();
        let (along, across) = if fx != 0 { (Axis::X, Axis::Z) } else { (Axis::Z, Axis::X) };
        let trunk = oriented(site.material, along);
        let limb_wood = oriented(site.material, across);
        let (butt, tip) = giant_span(site);
        // **Written over whatever stands there, and that is safe because of
        // what can.** The judge refused every trunk on the footprint, so
        // what a giant lands on is scrub, a smaller fallen log or a tuft --
        // and a giant that stopped wherever a bush stood was the first
        // draft's giant, a trunk with bites out of it.
        //
        // **Earth under the step-down columns** the judge allowed, first, so
        // no cell of trunk has air under it.
        for (u, v) in giant_cells(site) {
            let (gx, gz) = site.world(u, v);
            if column(self, gx, gz).height < g {
                emit(gx, g, gz, BLOCK_DIRT, true);
            }
        }
        // **The hump keeps its ground.** Where a column rises one above the
        // middle, its top block is where the lower log would go: the cell is
        // left to the ground, and the log above lies on it. Writing the log
        // there anyway put a cell of trunk in the hill with the turf gone
        // from over it.
        let humped = |gx: i32, gz: i32| column(self, gx, gz).height > g;
        for u in butt..=tip {
            for v in 0..w {
                let (gx, gz) = site.world(u, v);
                for dy in 1..=w {
                    if dy == 1 && humped(gx, gz) {
                        continue;
                    }
                    if in_hollow(u, v, dy) {
                        // Cleared, not merely left: a tuft or a stick the
                        // passes before laid there would stand in the
                        // tunnel.
                        emit(gx, g + dy, gz, BLOCK_AIR, true);
                        continue;
                    }
                    emit(gx, g + dy, gz, trunk, true);
                }
                // The floor of the hollow is the earth the log killed, which
                // is also what the mushroom at its end stands on.
                if in_hollow(u, v, 1) {
                    emit(gx, g, gz, BLOCK_DIRT, true);
                }
            }
        }
        // The root plate: the torn end of the trunk framed by the earth it
        // pulled up, four across and four high with its top corners off.
        for v in -1..=w {
            let (gx, gz) = site.world(butt - 1, v);
            for dy in 1..=w + 2 {
                let core = (0..w).contains(&v) && dy <= w;
                let corner = (v == -1 || v == w) && dy == w + 2;
                if corner || (dy == 1 && humped(gx, gz)) {
                    continue;
                }
                emit(gx, g + dy, gz, if core { trunk } else { BLOCK_DIRT }, true);
            }
        }
        // A limb lying under the flank, one high: the step onto a trunk two
        // high, which is a climb no player can jump. Over a hump the ground
        // itself is that step, and the limb is not written into it.
        let limb = (butt + tip) / 2;
        for v in w..=w + 1 {
            let (gx, gz) = site.world(limb, v);
            if !humped(gx, gz) {
                emit(gx, g + 1, gz, limb_wood, true);
            }
        }
        // ...and on a hollow giant, three high, a second step on the first:
        // a stair of two onto the top, which no jump makes in one.
        if site.hollow {
            let (gx, gz) = site.world(limb, w);
            emit(gx, g + 2, gz, limb_wood, true);
        }
        // Tinder on the flanks, rolled per cell of bark as the deadfall's
        // is, a shelf in five. Facing asked of `support_at`, so the shelf
        // hangs off the cell of trunk beside it and nothing else.
        for u in butt..=tip {
            for (inside, outside) in [(0, -1), (w - 1, w)] {
                let (lx, lz) = site.world(u, inside);
                let (sx, sz) = site.world(u, outside);
                let roll = site.hash_at(sx, sz, 0xF0AD);
                if !roll.is_multiple_of(5) {
                    continue;
                }
                let y = g + 1 + ((roll >> 8) % w as u32) as i32;
                // Not into ground, on either side of the bark: a shelf
                // planned inside a hump beside the trunk is a shelf that
                // never grew, and one on the lower log where a hump took its
                // place has no bark to hang from.
                if column(self, sx, sz).height >= y || (y == g + 1 && humped(lx, lz)) {
                    continue;
                }
                // Not on the flank over the limb at either height: low, the
                // limb is in that cell; high, the shelf stands exactly where
                // a player stepping off the limb puts their head, and
                // `a_fallen_giant_can_be_climbed_onto_by_its_limb` found one
                // there on seed 42.
                if (inside, outside) == (w - 1, w) && u == limb {
                    continue;
                }
                if let Some(bracket) = bracket_facing(lx - sx, lz - sz) {
                    emit(sx, y, sz, bracket, false);
                }
            }
        }
        // What the hollow kept at its closed end: two sticks, and a mushroom
        // between them, on the bare earth of its floor. Over the air the
        // hollow was cleared to, which is the one thing that can be there.
        if site.hollow {
            for (u, block) in [(butt + 1, BLOCK_STICK), (butt + 2, BLOCK_MUSHROOM), (butt + 3, BLOCK_STICK)] {
                let (gx, gz) = site.world(u, 1);
                emit(gx, g + 1, gz, block, true);
            }
        }
        // Sticks past the tip, where the crown broke up when it came down.
        for n in 1..=3 {
            let v = (site.hash_at(site.x, site.z, 0x571C + n as u32) % 3) as i32 - 1;
            let (gx, gz) = site.world(tip + n, v);
            let here = column(self, gx, gz);
            if dry(&here) && has_full_top(here.surface.top) && self.ground_is_whole(gx, gz, &here) {
                emit(gx, here.height + 1, gz, BLOCK_STICK, false);
            }
        }
    }

    fn plan_thicket(&self, site: &Feature, emit: &mut dyn FnMut(i32, i32, i32, BlockId, bool)) {
        for (dx, dz) in disc(3, 10) {
            let (gx, gz) = (site.x + dx, site.z + dz);
            let Some(here) = self.thicket_column(site, gx, gz) else {
                continue;
            };
            let roll = site.hash_at(gx, gz, 0xBE44);
            let near = dx * dx + dz * dz;
            // Two cells in ten a bush, and it was six: the thicket is a
            // third as generous as it was, on the same rule as
            // `worldgen::BERRY_THINNING` -- a thicket that still fed a
            // player for a week would be where every picker went instead.
            if (dx, dz) == (0, 0) || roll % 10 < 2 {
                emit(gx, here.height + 1, gz, BLOCK_BERRY_BUSH, false);
            } else if roll % 10 == 6 && near >= 4 {
                // A clump of scrub among them, which is what makes it a
                // thicket to push into rather than a row of bushes.
                emit(gx, here.height + 1, gz, BLOCK_BUSH_LEAVES, false);
                if (roll >> 8) & 1 == 1 {
                    emit(gx, here.height + 2, gz, BLOCK_BUSH_LEAVES, false);
                }
            }
        }
    }

    fn plan_knapping(&self, site: &Feature, emit: &mut dyn FnMut(i32, i32, i32, BlockId, bool)) {
        for (dx, dz) in disc(2, 6) {
            let (gx, gz) = (site.x + dx, site.z + dz);
            let Some(here) = self.knapping_column(site, gx, gz) else {
                continue;
            };
            let block = match site.hash_at(gx, gz, 0xF1A4) % 16 {
                _ if (dx, dz) == (0, 0) => BLOCK_FLINT,
                0..=3 => BLOCK_FLINT,
                4..=7 => BLOCK_FLINT_FLAKE,
                // The hammerstone they struck with.
                8 => BLOCK_PEBBLE,
                _ => continue,
            };
            emit(gx, here.height + 1, gz, block, false);
        }
    }

    fn plan_ring(&self, site: &Feature, emit: &mut dyn FnMut(i32, i32, i32, BlockId, bool)) {
        for (dx, dz) in ring(site.radius()) {
            let (gx, gz) = (site.x + dx, site.z + dz);
            let Some(here) = self.ring_column(site, gx, gz) else {
                continue;
            };
            // **The ring bares the turf it stands on**, which is what a
            // fairy ring does to a lawn, and it is also the only way a cap
            // can stand here: a mushroom does not grow on living turf
            // (`types::can_grow_on`).
            if block_kind(here.surface.top) == BLOCK_GRASS {
                emit(gx, here.height, gz, BLOCK_DIRT, true);
            }
            let roll = site.hash_at(gx, gz, 0x4105);
            if roll % 10 < 7 {
                // One cap in four the toadstool, the share the caves keep,
                // so the ring asks the question a cave floor does.
                let cap = if (roll >> 8).is_multiple_of(4) { BLOCK_TOADSTOOL } else { BLOCK_MUSHROOM };
                emit(gx, here.height + 1, gz, cap, false);
            }
        }
    }

    /// Every find that reaches into this chunk, its own cells of it.
    ///
    /// Runs after the boulders and the termite mounds, which it may stand
    /// over, and before the ruins and the ground cover: the ruins sweep
    /// their rooms of anything in the way, and the ground cover sees a
    /// cell already taken and leaves it.
    pub(super) fn place_features(&self, blocks: &mut [BlockId], origin_x: i32, origin_z: i32) {
        if self.preset == Preset::Test {
            return;
        }
        let (size_x, size_z) = (CHUNK_SIZE_X as i32, CHUNK_SIZE_Z as i32);
        for kind in FeatureKind::ALL {
            let (cell, reach) = (kind.cell(), kind.reach());
            for cz in (origin_z - reach).div_euclid(cell)..=(origin_z + size_z - 1 + reach).div_euclid(cell) {
                for cx in (origin_x - reach).div_euclid(cell)..=(origin_x + size_x - 1 + reach).div_euclid(cell) {
                    // The subtraction before the verdict: nearly every chunk
                    // in a cell is further than `reach` from its candidate.
                    let (x, z, _) = candidate(self.seed, kind, cx, cz);
                    if x + reach < origin_x
                        || x - reach >= origin_x + size_x
                        || z + reach < origin_z
                        || z - reach >= origin_z + size_z
                    {
                        continue;
                    }
                    let Some(site) = self.feature_site(kind, cx, cz) else {
                        continue;
                    };
                    self.plan_feature(&site, &mut |gx, y, gz, id, overwrite| {
                        put_block(blocks, gx - origin_x, y, gz - origin_z, id, overwrite);
                    });
                }
            }
        }
    }
}

/// The first and last `u` of a giant's trunk.
fn giant_span(site: &Feature) -> (i32, i32) {
    let butt = -site.length() / 2;
    (butt, butt + site.length() - 1)
}

/// Every `(u, v)` a giant stands on: the trunk two across (three, hollow),
/// the root plate a column wider each side at the butt, and the limb under
/// its flank. One list for the judge and the plan, so the ground that was
/// asked about is the ground
/// that is built on.
fn giant_cells(site: &Feature) -> Vec<(i32, i32)> {
    let (butt, tip) = giant_span(site);
    let w = site.width();
    let mut cells: Vec<(i32, i32)> = (butt..=tip).flat_map(|u| (0..w).map(move |v| (u, v))).collect();
    cells.extend((-1..=w).map(|v| (butt - 1, v)));
    let limb = (butt + tip) / 2;
    cells.extend([(limb, w), (limb, w + 1)]);
    cells
}

/// Offsets inside a disc: every `(dx, dz)` within `reach` on both axes and
/// no further than the square root of `radius_squared`.
fn disc(reach: i32, radius_squared: i32) -> impl Iterator<Item = (i32, i32)> {
    (-reach..=reach).flat_map(move |dz| {
        (-reach..=reach).filter_map(move |dx| (dx * dx + dz * dz <= radius_squared).then_some((dx, dz)))
    })
}

/// The cells of a ring of `radius`: those whose squared distance is within
/// `radius` of the radius squared, which is a band one cell thick all the
/// way round with no gaps at the diagonals.
fn ring(radius: i32) -> impl Iterator<Item = (i32, i32)> {
    let reach = radius + 1;
    (-reach..=reach).flat_map(move |dz| {
        (-reach..=reach)
            .filter_map(move |dx| ((dx * dx + dz * dz - radius * radius).abs() <= radius).then_some((dx, dz)))
    })
}

/// The bracket fungus that hangs off wood at `(dx, dz)` from its own cell,
/// asked of `support_at` so the generator, the support rule and the model
/// are one answer.
fn bracket_facing(dx: i32, dz: i32) -> Option<BlockId> {
    [Facing::North, Facing::East, Facing::South, Facing::West]
        .into_iter()
        .map(|facing| faced(BLOCK_BRACKET_FUNGUS, facing))
        .find(|&block| {
            let (sx, _, sz) = support_at(block);
            (sx, sz) == (dx, dz)
        })
}

/// Every find whose middle is in this rectangle of the world.
///
/// Public so a tool can photograph them without generating a world to
/// search, and so a test can ask whether a mushroom in daylight is
/// standing in a ring.
pub fn features_in(gen: &WorldGen, from: (i32, i32), to: (i32, i32)) -> Vec<Feature> {
    // **A boundary of the planet frame** (`WorldGen::on_planet`): the
    // rectangle asked about is the caller's world, the cells are the
    // planet's, and what comes back has to be in the caller's again --
    // a find reported at its planet row would be drawn four thousand
    // kilometres from the ground it stands on.
    let (from, to) = (gen.on_planet(from.0, from.1), gen.on_planet(to.0, to.1));
    let mut found = Vec::new();
    for kind in FeatureKind::ALL {
        let cell = kind.cell();
        for cz in from.1.div_euclid(cell)..=to.1.div_euclid(cell) {
            for cx in from.0.div_euclid(cell)..=to.0.div_euclid(cell) {
                if let Some(mut site) = gen.feature_site(kind, cx, cz) {
                    if (from.0..=to.0).contains(&site.x) && (from.1..=to.1).contains(&site.z) {
                        (site.x, site.z) = gen.off_planet(site.x, site.z);
                        found.push(site);
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{can_grow_on, is_cross, is_flat, Chunk, BLOCK_BUSH_LEAVES as SCRUB};
    use std::collections::HashMap;

    /// Up to `wanted` finds of one kind, from a few seeds, near enough the
    /// origin to generate.
    fn sample(kind: FeatureKind, wanted: usize) -> Vec<(u32, Feature)> {
        let mut out = Vec::new();
        for seed in [1337u32, 42, 7, 2024, 99, 31337] {
            let gen = WorldGen::new(seed);
            for cz in -12..12 {
                for cx in -12..12 {
                    if let Some(site) = gen.feature_site(kind, cx, cz) {
                        out.push((seed, site));
                        if out.len() >= wanted {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }

    fn planned(gen: &WorldGen, site: &Feature) -> HashMap<(i32, i32, i32), (BlockId, bool)> {
        let mut cells = HashMap::new();
        gen.plan_feature(site, &mut |x, y, z, id, overwrite| {
            let entry = cells.entry((x, y, z)).or_insert((id, overwrite));
            if overwrite {
                *entry = (id, overwrite);
            }
        });
        cells
    }

    fn chunks_under(gen: &WorldGen, site: &Feature) -> HashMap<ChunkPos, Chunk> {
        let reach = site.kind.reach();
        let mut chunks = HashMap::new();
        for gz in [site.z - reach, site.z, site.z + reach] {
            for gx in [site.x - reach, site.x, site.x + reach] {
                let (pos, _, _) = ChunkPos::from_global(gx, gz);
                chunks.entry(pos).or_insert_with(|| gen.generate_chunk(pos));
            }
        }
        chunks
    }

    fn block_at(chunks: &HashMap<ChunkPos, Chunk>, gx: i32, y: i32, gz: i32) -> BlockId {
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        chunks[&pos].get(lx, y as usize, lz)
    }

    /// What may already stand in a cell a find writes into air: whatever a
    /// pass before it put there -- a tree, a nest, a bush, a fallen trunk
    /// and its tinder, a boulder, a termite mound -- none of which the plan
    /// is allowed to replace.
    fn got_there_first(block: BlockId) -> bool {
        matches!(
            block_kind(block),
            BLOCK_LOG
                | BLOCK_BIRCH_LOG
                | crate::types::BLOCK_FIR_LOG
                | crate::types::BLOCK_SAXAUL_LOG
                | crate::types::BLOCK_PINE_LOG
                | crate::types::BLOCK_WILLOW_LOG
                | SCRUB
                | crate::types::BLOCK_NEST_EGGS
                | BLOCK_BRACKET_FUNGUS
                | BLOCK_COBBLESTONE
                | BLOCK_GRANITE
                | crate::types::BLOCK_TERMITE_MOUND
        ) || crate::types::is_canopy(block)
            // ...and a boulder of any rock's cobble (`ground::rubble_of`).
            || crate::ground::rock_of(block).is_some_and(|(_, form)| form == Some(crate::ground::Form::Cobble))
            // A tree of branches is a tree that got there first as much as
            // one of logs: the ordinary world grows its broadleaf, acacias and
            // saplings out of pieces now, and a limb or a young stem in a
            // giant's cell stays, as a log there always did.
            || crate::types::is_branch(block)
            || is_cross(block)
            || is_flat(block)
    }

    /// **The number each kind is tuned by**, measured the way the ruins
    /// are: straight lines eighty blocks apart across a 1920-block square
    /// on three seeds, a find counted as met on a line if its middle is
    /// within thirty-two blocks of it, and the walk counted on dry land only.
    ///
    /// Measured as set (debug build, seeds 1337 / 42 / 7), blocks of land
    /// walked per find met:
    ///
    /// ```text
    /// see the numbers this prints; the ranges below are what they allow
    /// ```
    #[test]
    fn each_find_is_a_long_walk_from_the_next() {
        const SIDE: i32 = 1920;
        const LINES: i32 = 24;
        let mut met_somewhere: HashMap<FeatureKind, bool> = HashMap::new();
        for seed in [1337u32, 42, 7] {
            let gen = WorldGen::new(seed);
            let finds = features_in(&gen, (-SIDE / 2, -SIDE / 2), (SIDE / 2, SIDE / 2));
            let mut walked = 0usize;
            for line in 0..LINES {
                let z = -SIDE / 2 + 40 + line * (SIDE / LINES);
                let dry = (0..SIDE / 16)
                    .filter(|step| {
                        let gx = -SIDE / 2 + step * 16 + 8;
                        let height = gen.height_at(gx, z);
                        height > SEA_LEVEL + 1
                            && height > gen.water_level_at(gx, z)
                            && !matches!(gen.biome_at(gx, z), Biome::Ocean | Biome::Beach | Biome::River)
                    })
                    .count();
                walked += dry * 16;
            }
            let mut line = format!("seed {seed}: land {}% |", walked * 100 / (LINES * SIDE) as usize);
            for kind in FeatureKind::ALL {
                let of_kind: Vec<&Feature> = finds.iter().filter(|f| f.kind == kind).collect();
                let met: usize = (0..LINES)
                    .map(|n| {
                        let z = -SIDE / 2 + 40 + n * (SIDE / LINES);
                        of_kind.iter().filter(|f| (f.z - z).abs() <= 32).count()
                    })
                    .sum();
                let per = walked / met.max(1);
                line.push_str(&format!(" {} {} (one per {per} walked) |", kind.name(), of_kind.len()));
                // **A find, not a feature of the landscape.** Under four
                // hundred blocks of land a find is in sight of the last one
                // on most walks, which is the complaint the ruins drew; over
                // nine thousand and a player can cross the country and never
                // meet one, which makes it a rumour. Each kind is refused by
                // its own ground, so each seed can be short of one kind: the
                // assertion is on the total and on the commonest.
                assert!(per >= 400 || met == 0, "seed {seed}: a {} every {per} blocks of walking", kind.name());
            }
            println!("{line}");
            assert!(finds.len() >= 8, "seed {seed}: only {} finds in a 1920-block square", finds.len());
            for kind in FeatureKind::ALL {
                met_somewhere.entry(kind).or_insert(false);
                if finds.iter().any(|f| f.kind == kind) {
                    met_somewhere.insert(kind, true);
                }
            }
        }
        // ...and the other end: every kind stands somewhere on the three
        // seeds. A find that the ground refuses everywhere is dead code that
        // reads as a feature -- which is what the fallen giant was, on its
        // first measurement, with not one standing.
        for (kind, met) in met_somewhere {
            assert!(met, "not one {} on three seeds", kind.name());
        }
    }

    #[test]
    fn a_find_is_whole_across_a_chunk_seam() {
        for kind in FeatureKind::ALL {
            let mut across = 0;
            for (seed, site) in sample(kind, 12) {
                let gen = WorldGen::new(seed);
                // **Across a seam means its cells are**, not its reach box:
                // a shelter reaches three behind its middle and one in
                // front, and a seam two in front of it is inside the box and
                // outside the shelter -- the first run counted exactly that
                // shelter as built on one side only.
                let plan = planned(&gen, &site);
                let touched: std::collections::HashSet<ChunkPos> =
                    plan.keys().map(|&(gx, _, gz)| ChunkPos::from_global(gx, gz).0).collect();
                if touched.len() < 2 {
                    continue;
                }
                across += 1;
                let chunks = chunks_under(&gen, &site);
                let mut built_in = std::collections::HashSet::new();
                for (&(gx, y, gz), &(id, overwrite)) in &plan {
                    let (pos, _, _) = ChunkPos::from_global(gx, gz);
                    let Some(chunk) = chunks.get(&pos) else {
                        continue;
                    };
                    let (_, lx, lz) = ChunkPos::from_global(gx, gz);
                    let actual = chunk.get(lx, y as usize, lz);
                    if actual == id {
                        built_in.insert(pos);
                        continue;
                    }
                    // **Air a find cleared is air the ground cover may lay on**:
                    // that pass runs after the finds (`generate_chunk`), and a
                    // tuft or a stick on the floor of a hollow giant is the
                    // floor of a wood, not half a find.
                    let covered = id == BLOCK_AIR && (is_cross(actual) || is_flat(actual));
                    assert!(
                        (!overwrite && got_there_first(actual)) || covered,
                        "a {} of seed {seed} at ({}, {}) planned {} at ({gx},{y},{gz}) and the chunk holds {}",
                        kind.name(),
                        site.x,
                        site.z,
                        crate::types::block_name(id),
                        crate::types::block_name(actual)
                    );
                }
                // **Every chunk the plan writes over is built in**, which is
                // the property a verdict made in one chunk only would break.
                // Not "built on both sides": a knapping floor writes only
                // into air, and the one or two cells of it past a seam can
                // all be under a bush or a boulder that got there first --
                // the first run failed on exactly that, with every one of
                // its cells accounted for.
                for (&(gx, _, gz), &(_, overwrite)) in &plan {
                    let pos = ChunkPos::from_global(gx, gz).0;
                    assert!(
                        !overwrite || !chunks.contains_key(&pos) || built_in.contains(&pos),
                        "a {} over a seam wrote nothing into chunk {pos:?}, which its plan overwrites",
                        kind.name()
                    );
                }
            }
            assert!(across > 0, "no sampled {} crosses a chunk seam", kind.name());
        }
    }

    /// Why candidates are refused, kind by kind, and whether any rock
    /// shelter stands in the sweep `a_fallen_trunk_lies_on_the_ground_rather_than_over_a_hole`
    /// counts deadfall in -- the one find that could write over a fallen log.
    ///
    /// ```text
    /// cargo test -p primitive_shared --lib -- --ignored --nocapture why_finds_are_refused
    /// ```
    #[test]
    #[ignore = "a diagnostic: prints why candidates are refused"]
    fn why_finds_are_refused() {
        for seed in [1337u32, 42, 7] {
            let gen = WorldGen::new(seed);
            let mut tally: HashMap<&'static str, usize> = HashMap::new();
            for cz in -8..8 {
                for cx in -8..8 {
                    let (x, z, roll) = candidate(seed, FeatureKind::FallenGiant, cx, cz);
                    let middle = column(&gen, x, z);
                    let site = Feature {
                        kind: FeatureKind::FallenGiant,
                        x,
                        z,
                        ground: middle.height,
                        biome: middle.biome,
                        roll,
                        material: BLOCK_LOG,
                        hollow: false,
                    };
                    let why = if !matches!(middle.biome, Biome::Forest | Biome::BirchForest | Biome::DeadForest | Biome::Swamp) {
                        "biome"
                    } else if !dry(&middle) {
                        "wet"
                    } else if gen.ruin_claims(x, z) {
                        "ruin"
                    } else {
                        let mut reason = "stands";
                        for (u, v) in giant_cells(&site) {
                            let (gx, gz) = site.world(u, v);
                            let here = column(&gen, gx, gz);
                            reason = if !dry(&here) {
                                "a cell wet"
                            } else if here.height > site.ground {
                                "a bump"
                            } else if here.height < site.ground - 1 {
                                "a deep dip"
                            } else if here.height == site.ground && block_kind(here.surface.top) != BLOCK_GRASS {
                                "not turf"
                            } else if !gen.ground_is_whole(gx, gz, &here) {
                                "a cave"
                            } else if [(0, 0), (-1, 0), (0, -1), (-1, -1)]
                                .into_iter()
                                .any(|(dx, dz)| gen.tree_at(gx + dx, gz + dz, column(&gen, gx + dx, gz + dz).biome))
                            {
                                "a trunk"
                            } else {
                                continue;
                            };
                            break;
                        }
                        reason
                    };
                    *tally.entry(why).or_default() += 1;
                }
            }
            let shelters_in_sweep = features_in(&gen, (-192, -192), (192, 192))
                .iter()
                .filter(|f| f.kind == FeatureKind::RockShelter)
                .count();
            println!("seed {seed}: giant candidates {tally:?}; shelters in the deadfall sweep {shelters_in_sweep}");
        }
    }

    #[test]
    fn what_a_find_leaves_lying_about_stands_on_ground_that_holds_it() {
        // The rule every scattered thing keeps: a cap, a bush, a flint and
        // a stick on the floor they can grow on or lie on. A find that laid
        // them on a roof edge or a hole would lose them to the first block
        // update beside them.
        for kind in FeatureKind::ALL {
            for (seed, site) in sample(kind, 6) {
                let gen = WorldGen::new(seed);
                let chunks = chunks_under(&gen, &site);
                for (&(gx, y, gz), &(id, _)) in &planned(&gen, &site) {
                    if !(is_cross(id) || is_flat(id)) || !chunks.contains_key(&ChunkPos::from_global(gx, gz).0) {
                        continue;
                    }
                    if block_at(&chunks, gx, y, gz) != id {
                        continue;
                    }
                    // A tinder shelf hangs off the bark beside it, not off
                    // the ground under it: its floor is the cell
                    // `support_at` names.
                    let (sx, sy, sz) = support_at(id);
                    let under = block_at(&chunks, gx + sx, y + sy, gz + sz);
                    assert!(
                        can_grow_on(id, under),
                        "a {} laid {} on {} at ({gx},{y},{gz})",
                        kind.name(),
                        crate::types::block_name(id),
                        crate::types::block_name(under)
                    );
                }
            }
        }
    }

    #[test]
    fn a_rock_shelter_keeps_the_sky_off_its_hearth_and_leaves_room_to_stand() {
        // The whole of what a shelter is for: a fire laid at the ash is
        // under rock, and a player can stand beside it.
        let shelters = sample(FeatureKind::RockShelter, 4);
        assert!(!shelters.is_empty(), "no rock shelter on any sampled seed");
        for (seed, site) in shelters {
            let gen = WorldGen::new(seed);
            let chunks = chunks_under(&gen, &site);
            let (hx, hz) = site.world(0, 0);
            let floor = column(&gen, hx, hz).height;
            assert!(
                (floor + 1..=site.ground + 4).any(|y| block_at(&chunks, hx, y, hz) == site.material),
                "seed {seed}: the hearth of the shelter at ({}, {}) is under the open sky",
                site.x,
                site.z
            );
            for y in floor + 2..=floor + 3 {
                assert!(
                    !crate::types::is_collidable(block_at(&chunks, hx, y, hz)),
                    "seed {seed}: no headroom over the hearth of the shelter at ({}, {})",
                    site.x,
                    site.z
                );
            }
        }
    }

    #[test]
    fn a_fallen_giant_can_be_climbed_onto_by_its_limb() {
        // Two logs high is a climb no jump makes, which is why the limb is
        // there. Without it the giant is a wall lying in a wood.
        for (seed, site) in sample(FeatureKind::FallenGiant, 4) {
            let gen = WorldGen::new(seed);
            let chunks = chunks_under(&gen, &site);
            let (butt, tip) = giant_span(&site);
            let limb = (butt + tip) / 2;
            let (lx, lz) = site.world(limb, 2);
            let (tx, tz) = site.world(limb, 1);
            // The step is the limb, or -- where the ground rises one under
            // it -- the hump itself (`plan_giant`). Either is a block a
            // player stands on one below the trunk's top.
            let step = block_at(&chunks, lx, site.ground + 1, lz);
            assert!(
                block_kind(step) == site.material || crate::types::is_collidable(step),
                "seed {seed}: nothing to step up on beside the giant at ({}, {}), only {}",
                site.x,
                site.z,
                crate::types::block_name(step)
            );
            assert_eq!(block_kind(block_at(&chunks, tx, site.ground + 2, tz)), site.material, "seed {seed}: no trunk top");
            assert_eq!(block_at(&chunks, lx, site.ground + 2, lz), BLOCK_AIR, "seed {seed}: nothing to stand on the step in");
        }
    }

    #[test]
    fn a_hollow_giant_is_a_dry_tunnel_a_player_walks_into_with_something_kept_at_its_end() {
        use crate::geometry::{PLAYER_HALF_WIDTH, PLAYER_HEIGHT};
        use crate::types::is_collidable;
        // A person fits the tunnel: one cell across and two high.
        const { assert!(PLAYER_HALF_WIDTH * 2.0 < 1.0 && PLAYER_HEIGHT < 2.0, "a player no longer fits a hollow two high") };
        let hollows: Vec<(u32, Feature)> =
            sample(FeatureKind::FallenGiant, 24).into_iter().filter(|(_, site)| site.hollow).take(4).collect();
        assert!(!hollows.is_empty(), "not one hollow giant among the giants sampled on six seeds");
        for (seed, site) in hollows {
            let gen = WorldGen::new(seed);
            let chunks = chunks_under(&gen, &site);
            let (butt, tip) = giant_span(&site);
            let g = site.ground;
            for u in butt + 1..=tip {
                let (hx, hz) = site.world(u, 1);
                for y in g + 1..=g + 2 {
                    let cell = block_at(&chunks, hx, y, hz);
                    assert!(
                        !is_collidable(cell),
                        "seed {seed}: the hollow of the giant at ({}, {}) is blocked at {:?} by {}",
                        site.x,
                        site.z,
                        (hx, y, hz),
                        crate::types::block_name(cell)
                    );
                }
                assert_eq!(
                    block_kind(block_at(&chunks, hx, g + 3, hz)),
                    site.material,
                    "seed {seed}: the hollow of the giant at ({}, {}) is open to the sky at u={u}",
                    site.x,
                    site.z
                );
                for v in [0, 2] {
                    let (wx, wz) = site.world(u, v);
                    assert!(
                        is_collidable(block_at(&chunks, wx, g + 2, wz)),
                        "seed {seed}: the wall of the hollow giant at ({}, {}) has a hole at u={u}, v={v}",
                        site.x,
                        site.z
                    );
                }
            }
            // Closed at the root: the butt is wood all the way across.
            let (bx, bz) = site.world(butt, 1);
            assert_eq!(block_kind(block_at(&chunks, bx, g + 1, bz)), site.material, "seed {seed}: the hollow runs out through the root");
            let kept = (butt + 1..=butt + 3)
                .filter(|&u| {
                    let (kx, kz) = site.world(u, 1);
                    matches!(block_kind(block_at(&chunks, kx, g + 1, kz)), BLOCK_STICK | BLOCK_MUSHROOM)
                })
                .count();
            assert!(kept >= 2, "seed {seed}: the hollow giant at ({}, {}) kept {kept} things at its end", site.x, site.z);
        }
    }

    #[test]
    fn a_hollow_giant_comes_apart_by_hand_into_logs_as_a_solid_one_does() {
        let hollows: Vec<(u32, Feature)> =
            sample(FeatureKind::FallenGiant, 24).into_iter().filter(|(_, site)| site.hollow).take(2).collect();
        assert!(!hollows.is_empty(), "not one hollow giant among the giants sampled on six seeds");
        for (seed, site) in hollows {
            let gen = WorldGen::new(seed);
            let mut logs = 0;
            for ((x, y, z), (id, _)) in planned(&gen, &site) {
                if block_kind(id) != site.material {
                    continue;
                }
                assert_ne!(crate::types::block_axis(id), crate::types::Axis::Y, "seed {seed}: a hollow giant stands a log upright at {:?}", (x, y, z));
                assert!(crate::types::break_seconds(id).is_some(), "seed {seed}: a hollow giant's log cannot be broken by hand");
                assert_eq!(crate::types::block_drop(id).map(block_kind), Some(site.material), "seed {seed}: a hollow giant's log gives something else");
                logs += 1;
            }
            assert!(logs >= 50, "seed {seed}: a hollow giant of {logs} logs is not a giant");
        }
    }

    #[test]
    fn nothing_the_generator_lays_in_the_world_is_a_thing_that_exists_only_in_a_pack() {
        // **The black block in the shelter.** An item has no shape in the
        // world: the mesher draws one as the cube it falls through to,
        // wearing its icon with the transparent corners opaque, and a ray
        // does not stop at it (`types::is_targetable`), so it cannot even
        // be picked up. The flake on every shelter's hearth was one, and a
        // player standing in the shelter reported a black cube in a ruin.
        //
        // Asked of the plans rather than of generated chunks, so every kind
        // is checked on several seeds for the price of a few hundred cells.
        let mut flaked = 0;
        for kind in FeatureKind::ALL {
            for (seed, site) in sample(kind, 3) {
                let gen = WorldGen::new(seed);
                let knapped = matches!(kind, FeatureKind::RockShelter | FeatureKind::KnappingFloor);
                flaked += usize::from(knapped);
                for ((x, y, z), (id, _)) in planned(&gen, &site) {
                    assert!(
                        !crate::types::is_item(id),
                        "seed {seed}: the find at ({}, {}) lays {} at ({x}, {y}, {z}), which exists only in a pack",
                        site.x,
                        site.z,
                        crate::types::block_name(id)
                    );
                }
            }
        }
        assert!(flaked > 0, "no shelter and no knapping floor on any sampled seed: the case this is for went unchecked");
        // ...and the test world, which builds the same shelter and knapping
        // floor by hand.
        for pos in crate::showcase::built_chunks() {
            let chunk = crate::showcase::generate_chunk(pos);
            for &id in chunk.blocks.iter() {
                assert!(
                    !crate::types::is_item(id),
                    "the test world lays {} in chunk ({}, {}), which exists only in a pack",
                    crate::types::block_name(id),
                    pos.x,
                    pos.z
                );
            }
        }
    }

    /// What the finds cost a chunk, against what the chunk costs.
    ///
    /// ```text
    /// cargo test -p primitive_shared --lib -- --ignored --nocapture what_the_finds_cost_to_generate
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn what_the_finds_cost_to_generate() {
        use std::time::Instant;
        let gen = WorldGen::new(1337);
        const CHUNKS: i32 = 12;
        let started = Instant::now();
        for cx in 0..CHUNKS {
            for cz in 0..CHUNKS {
                std::hint::black_box(gen.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }
        let whole = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let mut blocks = vec![BLOCK_AIR; crate::types::CHUNK_VOLUME];
        for cx in 0..CHUNKS {
            for cz in 0..CHUNKS {
                gen.place_features(&mut blocks, cx * CHUNK_SIZE_X as i32, cz * CHUNK_SIZE_Z as i32);
            }
        }
        let finds = started.elapsed().as_secs_f64();
        let n = (CHUNKS * CHUNKS) as f64;
        println!(
            "generate_chunk {:.3} ms/chunk, of which finds {:.4} ms/chunk ({:.1}%)",
            whole * 1000.0 / n,
            finds * 1000.0 / n,
            100.0 * finds / whole
        );
    }
}
