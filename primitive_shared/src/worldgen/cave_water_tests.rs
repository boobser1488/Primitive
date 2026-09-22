//! Underground lakes and flooded passages, checked in the blocks a chunk
//! actually holds. See `cave_water` for what they are and why they are
//! filled the way they are.

use super::cave_water::CavePool;
use super::*;
use crate::types::{can_be_displaced_by_falling, is_liquid};
use std::rc::Rc;

const SEED: u32 = 2024;

/// Every pool of a world over a square of site cells, with its cell.
fn pools(gen: &WorldGen, cells: i32) -> Vec<((i32, i32), Rc<CavePool>)> {
    let mut found = Vec::new();
    for cz in -cells..cells {
        for cx in -cells..cells {
            if let Some(pool) = gen.cave_pool(cx, cz) {
                found.push(((cx, cz), pool));
            }
        }
    }
    found
}

#[test]
#[ignore = "diagnostic: prints how many pools a world has and what they are like"]
fn cave_pool_census() {
    let started = std::time::Instant::now();
    let gen = WorldGen::new(SEED);
    let found = pools(&gen, 6);
    for ((cx, cz), pool) in &found {
        let lowest = pool.cells.iter().map(|c| c.1).min().unwrap_or(0);
        let roofed = pool
            .cells
            .iter()
            .filter(|&&(x, y, z)| {
                y < pool.level && pool.cells.binary_search(&(x, y + 1, z)).is_err()
            })
            .count();
        println!(
            "site ({cx},{cz}): {} cells, level {}, {} deep, {} under a drowned roof, at {:?}",
            pool.cells.len(),
            pool.level,
            pool.level - lowest + 1,
            roofed,
            pool.cells[0]
        );
    }
    println!(
        "{} pools in 144 sites, {:.1} s",
        found.len(),
        started.elapsed().as_secs_f64()
    );
}

/// The chunks a pool lies in, generated, and a reader over them in world
/// coordinates.
struct Blocks {
    chunks: std::collections::HashMap<(i32, i32), Chunk>,
}

impl Blocks {
    fn around(gen: &WorldGen, pool: &CavePool) -> Self {
        let mut chunks = std::collections::HashMap::new();
        for &(px, _, pz) in &pool.cells {
            // One ring further out, so every neighbour of a cell at a seam is
            // read out of the chunk that really holds it.
            for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (gx, gz) = gen.off_planet(px + dx, pz + dz);
                let pos = ChunkPos::from_global(gx, gz).0;
                chunks
                    .entry((pos.x, pos.z))
                    .or_insert_with(|| gen.generate_chunk(pos));
            }
        }
        Self { chunks }
    }

    fn get(&self, gen: &WorldGen, (px, y, pz): (i32, i32, i32)) -> BlockId {
        let (gx, gz) = gen.off_planet(px, pz);
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        self.chunks[&(pos.x, pos.z)].get(lx, y as usize, lz)
    }
}

/// The first few pools of the test world, smallest first, so the chunks
/// behind them stay few in a debug build.
fn some_pools(gen: &WorldGen, how_many: usize) -> Vec<Rc<CavePool>> {
    let mut found: Vec<_> = pools(gen, 6).into_iter().map(|(_, pool)| pool).collect();
    found.sort_by_key(|pool| {
        let (lo, hi) = pool.cells.iter().fold((i32::MAX, i32::MIN), |(lo, hi), c| {
            (lo.min(c.0).min(c.2), hi.max(c.0).max(c.2))
        });
        hi - lo
    });
    found.truncate(how_many);
    found
}

#[test]
fn an_ordinary_world_has_underground_lakes() {
    let gen = WorldGen::new(SEED);
    let found = pools(&gen, 6);
    // Nine at the time of writing, in a hundred and forty-four sites: a find,
    // not a rule, and not so rare that a test world of this size has none.
    assert!(
        found.len() >= 3,
        "{} cave pools in 144 sites of seed {SEED}",
        found.len()
    );
    assert!(
        found.len() <= 40,
        "{} cave pools in 144 sites: every cave is a lake",
        found.len()
    );
}

#[test]
fn every_cell_of_cave_water_is_held_by_rock_or_water_below_and_beside() {
    // **This is the rest condition of the water itself.** A full cell trades
    // nothing with a full cell beside it or under it, and nothing with rock
    // (`fluid::fall_transfer`, `fluid::level_transfer`), so a pool that
    // passes this is one the server's water leaves exactly where it was
    // generated, however many of its cells are woken.
    //
    // Three worlds and every pool in each: the closure is a proof, and a
    // proof with a hole in it shows up in whichever cave finds the hole.
    for seed in [SEED, 7, 1337] {
        let gen = WorldGen::new(seed);
        for pool in some_pools(&gen, usize::MAX) {
            let blocks = Blocks::around(&gen, &pool);
            for &(x, y, z) in &pool.cells {
                let here = blocks.get(&gen, (x, y, z));
                // All of it written: nothing a later pass put down took a cell of
                // the lake, and no flint or stalagmite stands in one.
                assert_eq!(
                    here, BLOCK_WATER,
                    "the pool cell ({x},{y},{z}) holds {here:#x}"
                );
                for n in [
                    (x + 1, y, z),
                    (x - 1, y, z),
                    (x, y, z + 1),
                    (x, y, z - 1),
                    (x, y - 1, z),
                ] {
                    let block = blocks.get(&gen, n);
                    assert!(
                    is_liquid(block) || !can_be_displaced_by_falling(block),
                    "the water at ({x},{y},{z}) of seed {seed} has {block:#x} at {n:?}, and runs out into it"
                );
                }
            }
        }
    }
}

#[test]
fn cave_water_never_comes_near_the_surface() {
    let gen = WorldGen::new(SEED);
    for (_, pool) in pools(&gen, 6) {
        for &(x, y, z) in &pool.cells {
            assert!(
                y <= cave_water::MAX_LEVEL,
                "cave water at ({x},{y},{z}) stands above {}",
                cave_water::MAX_LEVEL
            );
            let ground = gen.column_anywhere(x, z).height;
            assert!(
                y <= ground - cave_water::SKIN,
                "cave water at ({x},{y},{z}) is {} under ground at {ground}",
                ground - y
            );
        }
    }
}

#[test]
fn some_cave_passages_are_flooded_to_the_roof() {
    // A siphon: water under a roof of rock, lower than the lake's surface --
    // a passage a player swims through rather than wades.
    let gen = WorldGen::new(SEED);
    let drowned: usize = pools(&gen, 6)
        .iter()
        .map(|(_, pool)| {
            pool.cells
                .iter()
                .filter(|&&(x, y, z)| {
                    y < pool.level
                        && pool.cells.binary_search(&(x, y + 1, z)).is_err()
                        && y < gen.column_anywhere(x, z).height - cave_water::SKIN
                        && !gen.is_cave(x, y + 1, z)
                })
                .count()
        })
        .sum();
    assert!(
        drowned > 0,
        "no cave water anywhere in 144 sites is under a drowned roof"
    );
}

#[test]
fn a_cave_lake_is_the_same_whichever_chunk_is_made_first_and_on_whichever_thread() {
    let gen = WorldGen::new(SEED);
    let pool = some_pools(&gen, 1).pop().expect("a pool to test");
    let blocks = Blocks::around(&gen, &pool);
    let mut order: Vec<(i32, i32)> = blocks.chunks.keys().copied().collect();
    order.sort_unstable();
    order.reverse();
    // A fresh thread has none of the tiles or pools this one remembers, and
    // it makes the chunks in the other order.
    let again = std::thread::spawn(move || {
        let gen = WorldGen::new(SEED);
        order
            .into_iter()
            .map(|(x, z)| ((x, z), gen.generate_chunk(ChunkPos::new(x, z))))
            .collect::<Vec<_>>()
    })
    .join()
    .expect("the other thread");
    for ((x, z), chunk) in again {
        assert!(
            chunk.blocks == blocks.chunks[&(x, z)].blocks,
            "chunk ({x},{z}) came out different the second time"
        );
    }
}

#[test]
fn a_player_can_ask_whether_a_cell_is_cave_water() {
    let gen = WorldGen::new(SEED);
    let pool = some_pools(&gen, 1).pop().expect("a pool to test");
    let &(px, y, pz) = pool.cells.last().expect("a cell");
    let (gx, gz) = gen.off_planet(px, pz);
    assert!(
        gen.cave_water_at(gx, y, gz),
        "the pool's own cell is not cave water"
    );
    assert!(
        !gen.cave_water_at(gx, pool.level + 1, gz),
        "the air over the pool is cave water"
    );
    assert!(
        !gen.cave_water_at(gx, SEA_LEVEL, gz),
        "the surface is cave water"
    );
}
