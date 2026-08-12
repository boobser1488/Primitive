//! Этап 5: noise-based terrain, plus the extras the lighting system
//! needs something to actually light: caves, trees and glowstone veins.
//!
//! Noise fields, all independent so features don't visibly correlate:
//! - `height_noise` + `detail_noise`: a cheap 2-octave fBm shaping the
//!   surface height.
//! - `biome_noise`: a much lower-frequency field picking surface
//!   material, deliberately decorrelated from height so biome edges
//!   don't just trace the mountains.
//! - `cave_noise`: 3D. Carves tunnels underground -- the reason block
//!   light (glowstone) is worth having at all.
//! - `ore_noise`: 3D, narrow-band, sprinkles glowstone into deep stone.
//!
//! Deterministic per world seed: same seed -> byte-identical chunk, every
//! time. That's what lets the server *evict* a chunk from its cache and
//! regenerate it later instead of keeping every chunk ever visited in RAM
//! forever (see the server's `world` module).
//!
//! Chunk-border safety: trees are only rooted where their whole canopy
//! fits inside the chunk, so generation never needs to write into a
//! neighbour. That keeps `generate_chunk` a pure function of (seed, pos)
//! -- no cross-chunk ordering dependency, which is what makes parallel
//! generation across worker threads safe.

use noise::{NoiseFn, Perlin};

use crate::types::{
    Chunk, ChunkPos, BLOCK_AIR, BLOCK_DIRT, BLOCK_GLOWSTONE, BLOCK_GRASS, BLOCK_LEAVES, BLOCK_LOG,
    BLOCK_SAND, BLOCK_SNOW, BLOCK_STONE, BLOCK_WATER, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
    CHUNK_VOLUME,
};

pub const SEA_LEVEL: i32 = 20;
const SNOW_LINE: i32 = SEA_LEVEL + 14;
const DIRT_DEPTH: i32 = 4;
const BEDROCK_TOP: i32 = 2;

/// Canopy radius of a tree; trunks are only rooted at least this far from
/// the chunk edge.
const TREE_CANOPY_RADIUS: i32 = 2;
const TREE_MIN_TRUNK: i32 = 4;
const TREE_MAX_TRUNK: i32 = 6;

pub struct WorldGen {
    seed: u32,
    height_noise: Perlin,
    detail_noise: Perlin,
    biome_noise: Perlin,
    cave_noise: Perlin,
    ore_noise: Perlin,
}

impl WorldGen {
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            // Distinct seeds per field so they don't correlate with each
            // other (one seed for all of them would make biome edges,
            // caves and ore veins suspiciously trace terrain contours).
            height_noise: Perlin::new(seed),
            detail_noise: Perlin::new(seed.wrapping_add(1)),
            biome_noise: Perlin::new(seed.wrapping_add(2)),
            cave_noise: Perlin::new(seed.wrapping_add(3)),
            ore_noise: Perlin::new(seed.wrapping_add(4)),
        }
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    pub fn height_at(&self, gx: i32, gz: i32) -> i32 {
        let x = gx as f64;
        let z = gz as f64;
        let base = self.height_noise.get([x * 0.01, z * 0.01]) * 18.0;
        let detail = self.detail_noise.get([x * 0.06, z * 0.06]) * 4.0;
        let h = SEA_LEVEL as f64 + base + detail;
        h.round().clamp(1.0, (CHUNK_SIZE_Y - 2) as f64) as i32
    }

    /// Roughly "-1 (cold) .. +1 (hot)", independent of terrain height.
    fn biome_value(&self, gx: i32, gz: i32) -> f64 {
        self.biome_noise.get([gx as f64 * 0.004, gz as f64 * 0.004])
    }

    fn is_cave(&self, gx: i32, y: i32, gz: i32) -> bool {
        if y <= BEDROCK_TOP + 1 {
            return false; // never carve through the floor of the world
        }
        let v = self.cave_noise.get([
            gx as f64 * 0.045,
            y as f64 * 0.075,
            gz as f64 * 0.045,
        ]);
        v.abs() < 0.11
    }

    fn is_glowstone(&self, gx: i32, y: i32, gz: i32) -> bool {
        if y > SEA_LEVEL - 2 {
            return false; // deep only, so it lights caves rather than hillsides
        }
        let v = self
            .ore_noise
            .get([gx as f64 * 0.11, y as f64 * 0.11, gz as f64 * 0.11]);
        v > 0.72
    }

    /// A safe standing height for a spawn point at (gx, gz): one block
    /// above the surface, never below sea level + 1 (so you don't spawn
    /// inside an ocean).
    pub fn spawn_y(&self, gx: i32, gz: i32) -> f32 {
        let surface = self.height_at(gx, gz).max(SEA_LEVEL);
        (surface + 1) as f32
    }

    pub fn generate_chunk(&self, pos: ChunkPos) -> Chunk {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        let origin_x = pos.x * CHUNK_SIZE_X as i32;
        let origin_z = pos.z * CHUNK_SIZE_Z as i32;

        // Pass 1: terrain column by column.
        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in 0..CHUNK_SIZE_X as i32 {
                let gx = origin_x + lx;
                let gz = origin_z + lz;
                let height = self.height_at(gx, gz);
                let biome = self.biome_value(gx, gz);

                let surface = if height <= SEA_LEVEL + 1 {
                    BLOCK_SAND // beaches around the waterline, regardless of biome
                } else if height >= SNOW_LINE && biome < 0.1 {
                    BLOCK_SNOW // only the colder biome gets snow caps up high
                } else {
                    BLOCK_GRASS
                };

                for y in 0..CHUNK_SIZE_Y as i32 {
                    let mut id = if y > height {
                        if y <= SEA_LEVEL {
                            BLOCK_WATER
                        } else {
                            BLOCK_AIR
                        }
                    } else if y == height {
                        surface
                    } else if y > height - DIRT_DEPTH {
                        BLOCK_DIRT
                    } else {
                        BLOCK_STONE
                    };

                    // Caves: carve only through solid ground, and never
                    // through the seabed (a cave opening under the ocean
                    // would drain into an unlit void the client has no way
                    // to render sensibly yet).
                    if id != BLOCK_AIR
                        && id != BLOCK_WATER
                        && y > BEDROCK_TOP
                        && height > SEA_LEVEL + 1
                        && self.is_cave(gx, y, gz)
                    {
                        id = BLOCK_AIR;
                    } else if id == BLOCK_STONE && self.is_glowstone(gx, y, gz) {
                        id = BLOCK_GLOWSTONE;
                    }

                    blocks[Chunk::index(lx as usize, y as usize, lz as usize)] = id;
                }

                // Bedrock floor: guarantees no cave ever opens into the
                // void below y=0.
                for y in 0..=BEDROCK_TOP {
                    blocks[Chunk::index(lx as usize, y as usize, lz as usize)] = BLOCK_STONE;
                }
            }
        }

        // Pass 2: trees. Only rooted where the whole canopy fits inside
        // this chunk, so we never write outside our own block array.
        for lz in TREE_CANOPY_RADIUS..CHUNK_SIZE_Z as i32 - TREE_CANOPY_RADIUS {
            for lx in TREE_CANOPY_RADIUS..CHUNK_SIZE_X as i32 - TREE_CANOPY_RADIUS {
                let gx = origin_x + lx;
                let gz = origin_z + lz;
                if !self.tree_here(gx, gz) {
                    continue;
                }
                let ground = self.height_at(gx, gz);
                if ground <= SEA_LEVEL + 1 || ground >= SNOW_LINE {
                    continue; // no trees on beaches or above the snow line
                }
                if blocks[Chunk::index(lx as usize, ground as usize, lz as usize)] != BLOCK_GRASS {
                    continue; // cave mouth or otherwise not grassy ground
                }
                let trunk = TREE_MIN_TRUNK
                    + (hash2(gx, gz, self.seed.wrapping_add(9)) % (TREE_MAX_TRUNK - TREE_MIN_TRUNK + 1) as u32) as i32;
                place_tree(&mut blocks, lx, ground, lz, trunk);
            }
        }

        Chunk { pos, blocks }
    }

    /// Deterministic, cheap "is there a tree rooted at this exact column"
    /// test. A hash rather than noise so neighbouring trees don't clump
    /// into solid forests.
    fn tree_here(&self, gx: i32, gz: i32) -> bool {
        hash2(gx, gz, self.seed) % 110 == 0
    }
}

/// Small integer hash -- deterministic across runs and platforms (unlike
/// `DefaultHasher`, whose output is explicitly not guaranteed stable).
fn hash2(x: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed ^ 0x9E37_79B9;
    h = h.wrapping_mul(0x85EB_CA6B) ^ (x as u32).wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x27D4_EB2F) ^ (z as u32).wrapping_mul(0x165667B1);
    h ^= h >> 13;
    h.wrapping_mul(0x9E37_79B1)
}

fn place_tree(blocks: &mut [u16], lx: i32, ground: i32, lz: i32, trunk_height: i32) {
    let top = ground + trunk_height;
    if top + 2 >= CHUNK_SIZE_Y as i32 {
        return;
    }

    // Canopy first, so the trunk overwrites any leaf that lands on it.
    for dy in -1..=2 {
        let y = top + dy;
        let radius = if dy >= 1 { 1 } else { TREE_CANOPY_RADIUS };
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs() == radius && dz.abs() == radius && dy < 1 {
                    continue; // round off the corners
                }
                let idx = Chunk::index((lx + dx) as usize, y as usize, (lz + dz) as usize);
                if blocks[idx] == BLOCK_AIR {
                    blocks[idx] = BLOCK_LEAVES;
                }
            }
        }
    }

    for y in ground + 1..=top {
        blocks[Chunk::index(lx as usize, y as usize, lz as usize)] = BLOCK_LOG;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_for_same_seed() {
        let a = WorldGen::new(42).generate_chunk(ChunkPos::new(3, -2));
        let b = WorldGen::new(42).generate_chunk(ChunkPos::new(3, -2));
        assert_eq!(a.blocks, b.blocks);
    }

    #[test]
    fn different_seeds_usually_differ() {
        let a = WorldGen::new(1).generate_chunk(ChunkPos::new(0, 0));
        let b = WorldGen::new(2).generate_chunk(ChunkPos::new(0, 0));
        assert_ne!(a.blocks, b.blocks);
    }

    #[test]
    fn always_has_a_stone_floor() {
        // Bedrock layer means caves can never open into the void, which
        // the client's physics has no answer for.
        for cx in -2..2 {
            let chunk = WorldGen::new(7).generate_chunk(ChunkPos::new(cx, cx));
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    assert_eq!(chunk.get(x, 0, z), BLOCK_STONE);
                }
            }
        }
    }

    #[test]
    fn caves_actually_carve_something() {
        // Across a handful of chunks there should be at least one pocket
        // of air below the surface -- otherwise the cave threshold has
        // drifted and block lighting has nothing to illuminate.
        let gen = WorldGen::new(2024);
        let mut air_underground = 0;
        for cx in 0..4 {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, 0));
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let h = gen.height_at(cx * 16 + x as i32, z as i32);
                    for y in (BEDROCK_TOP + 1)..h {
                        if chunk.get(x, y as usize, z) == BLOCK_AIR {
                            air_underground += 1;
                        }
                    }
                }
            }
        }
        assert!(air_underground > 0, "no caves generated at all");
    }

    #[test]
    fn spawn_is_above_water() {
        let gen = WorldGen::new(1337);
        assert!(gen.spawn_y(0, 0) > SEA_LEVEL as f32);
    }
}
