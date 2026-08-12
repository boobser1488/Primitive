use serde::{Deserialize, Serialize};

// Этап 2: real 3D chunks, per the plan's "16x16x16" example. Height is
// now 64 (Этап 5 terrain needs real vertical room -- 16 was fine for a
// flat world but way too short for hills).
pub const CHUNK_SIZE_X: usize = 16;
pub const CHUNK_SIZE_Y: usize = 64;
pub const CHUNK_SIZE_Z: usize = 16;
pub const CHUNK_VOLUME: usize = CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z;

pub type BlockId = u16;

pub const BLOCK_AIR: BlockId = 0;
pub const BLOCK_GRASS: BlockId = 1;
pub const BLOCK_DIRT: BlockId = 2;
pub const BLOCK_STONE: BlockId = 3;
pub const BLOCK_SAND: BlockId = 4;
pub const BLOCK_SNOW: BlockId = 5;
pub const BLOCK_WATER: BlockId = 6;
pub const BLOCK_LOG: BlockId = 7;
pub const BLOCK_LEAVES: BlockId = 8;
pub const BLOCK_GLOWSTONE: BlockId = 9;
pub const BLOCK_PLANKS: BlockId = 10;
pub const BLOCK_COBBLESTONE: BlockId = 11;

/// Every block type that has a texture, in a stable order -- used by the
/// client's texture system to build a name<->id lookup, by the hotbar,
/// and by the server's anti-cheat to reject `SetBlock` carrying a block
/// id that doesn't exist. Keep in sync when adding a new BLOCK_*.
pub const ALL_BLOCK_IDS: &[(BlockId, &str)] = &[
    (BLOCK_GRASS, "grass"),
    (BLOCK_DIRT, "dirt"),
    (BLOCK_STONE, "stone"),
    (BLOCK_SAND, "sand"),
    (BLOCK_SNOW, "snow"),
    (BLOCK_WATER, "water"),
    (BLOCK_LOG, "log"),
    (BLOCK_LEAVES, "leaves"),
    (BLOCK_GLOWSTONE, "glowstone"),
    (BLOCK_PLANKS, "planks"),
    (BLOCK_COBBLESTONE, "cobblestone"),
];

/// What a player is allowed to put into the world: the client hotbar
/// offers exactly these, and the server validates against the same list
/// (see `is_placeable`) rather than trusting the client's choice.
pub const PLACEABLE_BLOCKS: &[BlockId] = &[
    BLOCK_STONE,
    BLOCK_DIRT,
    BLOCK_GRASS,
    BLOCK_SAND,
    BLOCK_SNOW,
    BLOCK_LOG,
    BLOCK_LEAVES,
    BLOCK_GLOWSTONE,
    BLOCK_PLANKS,
    BLOCK_COBBLESTONE,
];

/// Maximum light level, matching the 4 bits per light channel packed into
/// each vertex on the client (see `primitive_client::mesh`).
pub const MAX_LIGHT: u8 = 15;

#[inline]
pub fn is_air(id: BlockId) -> bool {
    id == BLOCK_AIR
}

/// Collision. Water is deliberately still solid: there's no swimming yet,
/// so making it passable would just drop the player onto the seabed with
/// no way back up.
#[inline]
pub fn is_collidable(id: BlockId) -> bool {
    // Water is deliberately *not* collidable: it used to be, which meant
    // players walked across lakes as if they were glass. Swimming is
    // handled by the physics code instead (buoyancy and drag), not by
    // treating the surface as a floor.
    !matches!(id, BLOCK_AIR | BLOCK_WATER)
}

/// A block you can move through but that resists you: swimmable, and
/// enough to slow a fall.
#[inline]
pub fn is_liquid(id: BlockId) -> bool {
    id == BLOCK_WATER
}

/// Blocks that fall when nothing holds them up.
#[inline]
pub fn is_affected_by_gravity(id: BlockId) -> bool {
    id == BLOCK_SAND
}

/// Can a falling block displace what's here? Air yes, water yes (it
/// gets flooded out of the way), anything solid no.
#[inline]
pub fn can_be_displaced_by_falling(id: BlockId) -> bool {
    id == BLOCK_AIR || is_liquid(id)
}

/// Fully blocks light and line of sight. Non-opaque blocks still get
/// their own faces drawn, and light propagates through them (attenuated
/// by `light_opacity`).
#[inline]
pub fn is_opaque(id: BlockId) -> bool {
    !matches!(id, BLOCK_AIR | BLOCK_WATER | BLOCK_LEAVES)
}

/// Drawn with alpha blending rather than in the opaque pass -- you can
/// see through it, and what's behind it has to be drawn first.
///
/// This is deliberately *not* the same question as `is_opaque`. Leaves
/// are see-through in the lighting and face-culling sense but are still
/// drawn in the opaque pass, as an alpha *cutout*: every one of their
/// texels is either fully solid or fully absent, so they need no
/// blending, no sorting, and they can keep writing depth. Water is the
/// only block that actually needs the transparent pass.
#[inline]
pub fn is_translucent(id: BlockId) -> bool {
    id == BLOCK_WATER
}

/// Drawn with an alpha cutout: every texel is either fully solid or
/// fully absent, so the empty ones are discarded.
///
/// Kept apart from `is_translucent` because the two need opposite things
/// from the renderer. A cutout writes depth and needs no sorting; what it
/// does need is a fragment shader containing `discard`, and a shader
/// containing `discard` costs the GPU its early depth rejection for
/// *every* draw that uses it. So these get their own pass, and the
/// terrain -- which is almost all of the triangles -- keeps early-Z.
#[inline]
pub fn is_cutout(id: BlockId) -> bool {
    id == BLOCK_LEAVES
}

/// Extra light level lost when crossing one cell of this block, on top of
/// the usual 1-per-step. `MAX_LIGHT` = light stops dead.
#[inline]
pub fn light_opacity(id: BlockId) -> u8 {
    match id {
        BLOCK_AIR => 0,
        BLOCK_WATER => 2,
        BLOCK_LEAVES => 1,
        _ => MAX_LIGHT,
    }
}

/// Light this block emits on its own, independent of the sun -- what
/// makes caves and night-time builds visible.
#[inline]
pub fn light_emission(id: BlockId) -> u8 {
    match id {
        BLOCK_GLOWSTONE => 14,
        _ => 0,
    }
}

#[inline]
pub fn is_known_block(id: BlockId) -> bool {
    id == BLOCK_AIR || ALL_BLOCK_IDS.iter().any(|&(known, _)| known == id)
}

/// Anti-cheat helper: may a client ask to *place* this block?
#[inline]
pub fn is_placeable(id: BlockId) -> bool {
    PLACEABLE_BLOCKS.contains(&id)
}

pub fn block_name(id: BlockId) -> &'static str {
    if id == BLOCK_AIR {
        return "air";
    }
    ALL_BLOCK_IDS
        .iter()
        .find(|&&(known, _)| known == id)
        .map(|&(_, name)| name)
        .unwrap_or("unknown")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Converts a global block coordinate into (chunk position, local x, local z).
    ///
    /// FIX (per plan warning "Работа с отрицательными координатами"): must use
    /// div_euclid/rem_euclid rather than plain `/` and `%`, otherwise negative
    /// global coordinates map to the wrong chunk/local index.
    pub fn from_global(gx: i32, gz: i32) -> (ChunkPos, usize, usize) {
        let cx = gx.div_euclid(CHUNK_SIZE_X as i32);
        let cz = gz.div_euclid(CHUNK_SIZE_Z as i32);
        let lx = gx.rem_euclid(CHUNK_SIZE_X as i32) as usize;
        let lz = gz.rem_euclid(CHUNK_SIZE_Z as i32) as usize;
        (ChunkPos::new(cx, cz), lx, lz)
    }

    /// Chunk containing a world-space position.
    pub fn from_world(x: f32, z: f32) -> ChunkPos {
        let (pos, _, _) = ChunkPos::from_global(x.floor() as i32, z.floor() as i32);
        pos
    }

    /// Chebyshev distance in chunks -- the natural metric for a square
    /// render/interest area.
    pub fn chebyshev_distance(&self, other: ChunkPos) -> i32 {
        (self.x - other.x).abs().max((self.z - other.z).abs())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub pos: ChunkPos,
    // FIX: a fixed-size array ([T; 256]) is the "flat array, not Vec<Vec<T>>"
    // layout the plan calls for, but serde's built-in derive only covers
    // arrays up to length 32 without pulling in an extra crate (e.g.
    // serde_arrays). A single Vec<BlockId> of exactly CHUNK_VOLUME elements
    // keeps the same one-flat-buffer property (contiguous, no nesting,
    // O(1) indexed access) while staying trivially (de)serializable.
    pub blocks: Vec<BlockId>,
}

impl Chunk {
    #[inline]
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        (y * CHUNK_SIZE_Z + z) * CHUNK_SIZE_X + x
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        self.blocks[Self::index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, id: BlockId) {
        self.blocks[Self::index(x, y, z)] = id;
    }

    /// Highest non-air block in a column, or -1 for an entirely empty
    /// column. Used by the server to pick a safe spawn height.
    pub fn height_at(&self, x: usize, z: usize) -> i32 {
        for y in (0..CHUNK_SIZE_Y).rev() {
            if self.get(x, y, z) != BLOCK_AIR {
                return y as i32;
            }
        }
        -1
    }

    /// Этап 1 world generation, extended for Этап 2's real chunk height:
    /// a single grass layer at y=0, air above. Kept for tests and as a
    /// trivial fallback; `worldgen::WorldGen` is the real generator.
    pub fn generate_flat(pos: ChunkPos) -> Self {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for x in 0..CHUNK_SIZE_X {
            for z in 0..CHUNK_SIZE_Z {
                blocks[Self::index(x, 0, z)] = BLOCK_GRASS;
            }
        }
        Self { pos, blocks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_coords_map_correctly() {
        // -1 should land in chunk -1, local index 15 (last cell), not panic
        // or wrap into chunk 0 the way naive `%` would.
        let (pos, lx, lz) = ChunkPos::from_global(-1, -1);
        assert_eq!(pos, ChunkPos::new(-1, -1));
        assert_eq!(lx, 15);
        assert_eq!(lz, 15);
    }

    #[test]
    fn chunk_index_roundtrip() {
        let mut chunk = Chunk::generate_flat(ChunkPos::new(0, 0));
        chunk.set(3, 0, 7, BLOCK_STONE);
        assert_eq!(chunk.get(3, 0, 7), BLOCK_STONE);
        assert_eq!(chunk.get(0, 0, 0), BLOCK_GRASS);
    }

    #[test]
    fn block_properties_are_consistent() {
        assert!(!is_opaque(BLOCK_AIR));
        assert!(is_opaque(BLOCK_STONE));
        assert_eq!(light_opacity(BLOCK_AIR), 0);
        assert!(light_emission(BLOCK_GLOWSTONE) > 0);
        // A client must not be able to place air (that's what breaking is
        // for) or water (there's no bucket).
        assert!(!is_placeable(BLOCK_AIR));
        assert!(!is_placeable(BLOCK_WATER));
        assert!(is_placeable(BLOCK_STONE));
        for &id in PLACEABLE_BLOCKS {
            assert!(is_known_block(id), "{id} is placeable but unknown");
        }
    }

    #[test]
    fn chunk_distance_is_chebyshev() {
        assert_eq!(
            ChunkPos::new(0, 0).chebyshev_distance(ChunkPos::new(3, -5)),
            5
        );
    }
}

#[cfg(test)]
mod fluid_tests {
    use super::*;

    #[test]
    fn water_is_not_something_you_can_stand_on() {
        // Regression: water used to be collidable, so a lake behaved
        // like a sheet of glass.
        assert!(!is_collidable(BLOCK_WATER));
        assert!(is_liquid(BLOCK_WATER));
    }

    #[test]
    fn solids_are_still_solid() {
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG, BLOCK_LEAVES] {
            assert!(is_collidable(id), "{} should be solid", block_name(id));
            assert!(!is_liquid(id));
        }
        assert!(!is_collidable(BLOCK_AIR));
    }

    #[test]
    fn cutout_and_translucent_are_different_questions() {
        // Leaves are see-through but write depth and need no sorting;
        // water is blended and does. Conflating them puts one of them
        // in a pass that renders it wrong.
        assert!(is_cutout(BLOCK_LEAVES));
        assert!(!is_translucent(BLOCK_LEAVES));
        assert!(is_translucent(BLOCK_WATER));
        assert!(!is_cutout(BLOCK_WATER));
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG] {
            assert!(!is_cutout(id) && !is_translucent(id));
        }
    }

    #[test]
    fn water_still_dims_light_without_blocking_it() {
        assert!(!is_opaque(BLOCK_WATER));
        assert!(light_opacity(BLOCK_WATER) > 0);
        assert!(light_opacity(BLOCK_WATER) < MAX_LIGHT);
    }
}

#[cfg(test)]
mod gravity_tests {
    use super::*;

    #[test]
    fn only_sand_falls() {
        assert!(is_affected_by_gravity(BLOCK_SAND));
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG, BLOCK_GLOWSTONE] {
            assert!(!is_affected_by_gravity(id), "{} should not fall", block_name(id));
        }
    }

    #[test]
    fn sand_falls_through_air_and_water_but_not_through_solids() {
        assert!(can_be_displaced_by_falling(BLOCK_AIR));
        assert!(can_be_displaced_by_falling(BLOCK_WATER));
        assert!(!can_be_displaced_by_falling(BLOCK_STONE));
        assert!(!can_be_displaced_by_falling(BLOCK_SAND));
    }
}
