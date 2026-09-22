//! Where the wild hives are: on standing trunks in warm woods, reachable from
//! the ground, and nowhere a bee could not live.

use super::*;
use crate::types::{block_kind, BLOCK_WILD_HIVE};

/// A hive found: where it is in the world, and where in its chunk.
type Found = ((i32, i32, i32), (usize, usize, usize));

/// Every hive in a chunk, as world coordinates, with the chunk-local cell.
fn hives_in(chunk: &Chunk) -> Vec<Found> {
    let ox = chunk.pos.x * CHUNK_SIZE_X as i32;
    let oz = chunk.pos.z * CHUNK_SIZE_Z as i32;
    let mut found = Vec::new();
    for lz in 0..CHUNK_SIZE_Z {
        for lx in 0..CHUNK_SIZE_X {
            for y in 1..CHUNK_SIZE_Y {
                if block_kind(chunk.get(lx, y, lz)) == BLOCK_WILD_HIVE {
                    found.push(((ox + lx as i32, y as i32, oz + lz as i32), (lx, y, lz)));
                }
            }
        }
    }
    found
}

#[test]
fn hives_hang_on_trunks_in_warm_woods_and_never_in_the_desert_or_the_tundra() {
    let mut warm_hives = 0;
    let mut cold_or_dry_chunks = 0;
    for seed in [1337u32, 42] {
        let gen = WorldGen::new(seed);
        let (mut forests, mut others) = (0, 0);
        for cx in -40..40 {
            for cz in -40..40 {
                let centre = (cx * CHUNK_SIZE_X as i32 + 8, cz * CHUNK_SIZE_Z as i32 + 8);
                let biome = gen.biome_at(centre.0, centre.1);
                let barren = matches!(biome, Biome::Desert | Biome::Tundra | Biome::Taiga);
                let wood = matches!(biome, Biome::Forest | Biome::BirchForest);
                if !(wood && forests < 24 || barren && others < 12) {
                    continue;
                }
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for ((gx, y, gz), (lx, ly, lz)) in hives_in(&chunk) {
                    // Hung on a trunk: a log beside it, in this chunk.
                    let trunk = [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)].into_iter().find(|&(dx, dz)| {
                        let (tx, tz) = (lx as i32 + dx, lz as i32 + dz);
                        (0..CHUNK_SIZE_X as i32).contains(&tx)
                            && (0..CHUNK_SIZE_Z as i32).contains(&tz)
                            && is_trunk(chunk.get(tx as usize, ly, tz as usize))
                    });
                    let Some((dx, dz)) = trunk else {
                        panic!("a hive at ({gx},{y},{gz}) of seed {seed} hangs on no trunk");
                    };
                    let trunk_biome = gen.biome_at(gx + dx, gz + dz);
                    assert!(
                        matches!(trunk_biome, Biome::Forest | Biome::BirchForest | Biome::Savanna),
                        "a hive at ({gx},{y},{gz}) of seed {seed} is in {}",
                        trunk_biome.name()
                    );
                    // ...five cells up the trunk: out of reach from the ground,
                    // a climb for the honey. See `place_hives`' `HIVE_RISE`.
                    let ground = gen.height_at(gx + dx, gz + dz);
                    assert_eq!(y - ground, 5, "a hive at ({gx},{y},{gz}) hangs at the wrong height");
                    assert_eq!(crate::bees::honey_in(chunk.get(lx, ly, lz)), crate::bees::HIVE_FULL, "a wild hive grew empty");
                    warm_hives += 1;
                }
                if wood {
                    forests += 1;
                } else {
                    cold_or_dry_chunks += 1;
                    others += 1;
                }
            }
        }
    }
    assert!(cold_or_dry_chunks > 0, "no desert, tundra or taiga anywhere to check");
    assert!(warm_hives > 0, "forty-eight chunks of wood hold no hive at all");
}

