//! Where the first metal lies before anybody mines for it.
//!
//! Two rules, both about a find having a *place*: native copper over copper
//! country, where the lodes are, and stream tin on the banks of rivers inside a
//! tin district. Each is checked both ways -- every find stands where the rule
//! says, and the rule is not so tight that the world holds none.

use super::*;
use crate::types::{block_kind, BLOCK_NATIVE_COPPER, BLOCK_STREAM_TIN};

/// Every cell of `kind` in a chunk, as world coordinates.
fn cells_of(chunk: &Chunk, kind: BlockId) -> Vec<(i32, i32, i32)> {
    let ox = chunk.pos.x * CHUNK_SIZE_X as i32;
    let oz = chunk.pos.z * CHUNK_SIZE_Z as i32;
    let mut found = Vec::new();
    for lz in 0..CHUNK_SIZE_Z {
        for lx in 0..CHUNK_SIZE_X {
            for y in 1..CHUNK_SIZE_Y {
                if block_kind(chunk.get(lx, y, lz)) == kind {
                    found.push((ox + lx as i32, y as i32, oz + lz as i32));
                }
            }
        }
    }
    found
}

#[test]
fn native_copper_lies_only_where_the_hills_carry_copper() {
    // Chunks chosen by their ground before any is generated: the high ones,
    // where copper country is, and a handful of low ones, where it is not.
    let mut high_nuggets = 0;
    let mut low_chunks = 0;
    for seed in [1337u32, 42, 7] {
        let gen = WorldGen::new(seed);
        let mut high_chunks = 0;
        for cx in -24..24 {
            for cz in -24..24 {
                let centre_x = cx * CHUNK_SIZE_X as i32 + 8;
                let centre_z = cz * CHUNK_SIZE_Z as i32 + 8;
                let ground = gen.height_at(centre_x, centre_z);
                let high = WorldGen::copper_country(ground) > 0.3;
                let low = ground > SEA_LEVEL && ground < SEA_LEVEL + 8;
                if !(high && high_chunks < 6 || low && low_chunks < 6) {
                    continue;
                }
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for (gx, y, gz) in cells_of(&chunk, BLOCK_NATIVE_COPPER) {
                    // The nugget lies in the cell over the column's surface.
                    assert!(
                        WorldGen::copper_country(y - 1) > 0.0,
                        "a nugget at ({gx},{y},{gz}) of seed {seed} lies below the foot of copper country"
                    );
                    high_nuggets += usize::from(high);
                }
                if high {
                    high_chunks += 1;
                } else {
                    low_chunks += 1;
                }
            }
        }
    }
    assert!(low_chunks > 0, "no low country anywhere to check");
    assert!(high_nuggets > 0, "the hills of three seeds hold no native copper at all");
}

#[test]
fn stream_tin_lies_on_the_banks_of_tin_country_and_nowhere_else() {
    let mut found = 0;
    let mut river_chunks = 0;
    for seed in [1337u32, 42, 7, 2024, 99] {
        let gen = WorldGen::new(seed);
        for cx in -40..40 {
            for cz in -40..40 {
                let centre_x = cx * CHUNK_SIZE_X as i32 + 8;
                let centre_z = cz * CHUNK_SIZE_Z as i32 + 8;
                if gen.biome_at(centre_x, centre_z) != Biome::River
                    || gen.tin_country(centre_x, centre_z) < 0.3
                {
                    continue;
                }
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                river_chunks += 1;
                for (gx, y, gz) in cells_of(&chunk, BLOCK_STREAM_TIN) {
                    assert!(gen.tin_country(gx, gz) > 0.0, "stream tin at ({gx},{y},{gz}) outside tin country");
                    let river_near = (-BANK_WIDTH..=BANK_WIDTH).any(|dz| {
                        (-BANK_WIDTH..=BANK_WIDTH).any(|dx| {
                            gen.biome_at(gx + dx, gz + dz) == Biome::River
                                && gen.height_at(gx + dx, gz + dz) < SEA_LEVEL
                        })
                    });
                    assert!(river_near, "stream tin at ({gx},{y},{gz}) is not on a bank");
                    assert!(
                        crate::types::can_grow_on(BLOCK_PEBBLE, chunk.get(
                            (gx - chunk.pos.x * CHUNK_SIZE_X as i32) as usize,
                            (y - 1) as usize,
                            (gz - chunk.pos.z * CHUNK_SIZE_Z as i32) as usize,
                        )),
                        "stream tin at ({gx},{y},{gz}) lies on nothing a pebble lies on"
                    );
                    found += 1;
                }
                if river_chunks >= 8 {
                    assert!(found > 0, "eight river chunks in tin country and not one pebble of tin");
                    return;
                }
            }
        }
    }
    assert!(river_chunks > 0, "no river runs through tin country near the origin of five seeds");
    assert!(found > 0, "{river_chunks} river chunks in tin country and not one pebble of tin");
}

#[test]
fn a_riverbank_outside_tin_country_has_no_tin_on_it() {
    // The other half: the same banks, with the district taken away. A river
    // chunk wholly outside tin country holds none.
    // A wide net and several seeds: rivers are a thin share of any map, and
    // how thin depends on the world's scale, which is not this test's to
    // assume. The biome question is noise and cheap; only a chunk that
    // passes it is generated.
    let mut checked = 0;
    for seed in [1337u32, 42, 7, 2024, 99, 5] {
        let gen = WorldGen::new(seed);
        for cx in (-80..80).step_by(2) {
            for cz in (-80..80).step_by(2) {
                let ox = cx * CHUNK_SIZE_X as i32;
                let oz = cz * CHUNK_SIZE_Z as i32;
                // **Any river column in the chunk, not only its middle.** At the
                // Earth's scale (`worldgen::Scale`) the water near an origin is
                // mostly brooks a stride wide, which a chunk's middle column
                // almost never lands on: asked of the middle alone, two seeds
                // held no river chunk to check.
                let river = (0..16).step_by(4).any(|lz| (0..16).step_by(4).any(|lx| gen.biome_at(ox + lx, oz + lz) == Biome::River));
                if !river {
                    continue;
                }
                let corners = [(ox, oz), (ox + 15, oz), (ox, oz + 15), (ox + 15, oz + 15)];
                if corners.iter().any(|&(x, z)| gen.tin_country(x, z) > 0.0) {
                    continue;
                }
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                assert!(
                    cells_of(&chunk, BLOCK_STREAM_TIN).is_empty(),
                    "stream tin in a river chunk at ({cx},{cz}) of seed {seed}, outside tin country"
                );
                checked += 1;
                if checked >= 4 {
                    return;
                }
            }
        }
    }
    assert!(checked > 0, "no river outside tin country to check");
}
