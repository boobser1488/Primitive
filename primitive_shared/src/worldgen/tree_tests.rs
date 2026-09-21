//! Leaves and the wood that holds them, counted over generated woods.

use super::*;
use crate::types::{block_kind, block_name, is_branch, is_leafy, BlockId, BLOCK_PALM_TRUNK};
use std::collections::{HashMap, HashSet};

/// A square of generated chunks, read as one grid.
struct Square {
    blocks: HashMap<(i32, i32), Chunk>,
    min: (i32, i32),
    max: (i32, i32),
}

impl Square {
    fn around(generator: &WorldGen, centre: ChunkPos, span: i32) -> Square {
        let mut blocks = HashMap::new();
        for dz in -span..=span {
            for dx in -span..=span {
                let pos = ChunkPos::new(centre.x + dx, centre.z + dz);
                blocks.insert((pos.x, pos.z), generator.generate_chunk(pos));
            }
        }
        let size = CHUNK_SIZE_X as i32;
        Square {
            blocks,
            min: ((centre.x - span) * size, (centre.z - span) * size),
            max: ((centre.x + span + 1) * size - 1, (centre.z + span + 1) * size - 1),
        }
    }

    fn at(&self, x: i32, y: i32, z: i32) -> BlockId {
        if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
            return crate::types::BLOCK_AIR;
        }
        let size = CHUNK_SIZE_X as i32;
        self.blocks.get(&(x.div_euclid(size), z.div_euclid(size))).map_or(crate::types::BLOCK_AIR, |chunk| {
            chunk.get(x.rem_euclid(size) as usize, y as usize, z.rem_euclid(size) as usize)
        })
    }

    fn inner(&self, x: i32, z: i32, margin: i32) -> bool {
        x >= self.min.0 + margin && x <= self.max.0 - margin && z >= self.min.1 + margin && z <= self.max.1 - margin
    }
}

fn is_wood(b: BlockId) -> bool {
    is_branch(b) || crate::wood::is_log(b) || block_kind(b) == BLOCK_PALM_TRUNK
}

/// A cell, as an offset or in the world.
type At = (i32, i32, i32);

/// A clump of leaves found: its leaf, its size, whether wood holds it, and
/// one of its cells.
type Clump = (BlockId, usize, bool, At);

/// Per leaf: clumps, clumps off the wood, leaves in them, a few of them.
type Tally = (usize, usize, usize, Vec<At>);

const FACES: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];

/// Every clump of leaves (face-connected) wholly inside the square's inner
/// part, with whether any leaf of it has wood across a face.
fn clumps(square: &Square) -> Vec<Clump> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for x in square.min.0..=square.max.0 {
        for z in square.min.1..=square.max.1 {
            for y in 1..CHUNK_SIZE_Y as i32 {
                let b = square.at(x, y, z);
                if !is_leafy(b) || seen.contains(&(x, y, z)) {
                    continue;
                }
                let mut stack = vec![(x, y, z)];
                seen.insert((x, y, z));
                let (mut size, mut held, mut inside) = (0, false, true);
                while let Some((cx, cy, cz)) = stack.pop() {
                    size += 1;
                    inside &= square.inner(cx, cz, 6);
                    for (dx, dy, dz) in FACES {
                        let n = (cx + dx, cy + dy, cz + dz);
                        let nb = square.at(n.0, n.1, n.2);
                        if is_wood(nb) {
                            held = true;
                        }
                        if is_leafy(nb) && seen.insert(n) {
                            stack.push(n);
                        }
                    }
                }
                if inside {
                    out.push((block_kind(b), size, held, (x, y, z)));
                }
            }
        }
    }
    out
}

/// A diagnostic: where leaves hang with no wood under them, and where a
/// piece of branch ends with no leaves on it, per leaf and per biome.
///
/// ```text
/// cargo test -p primitive_shared --lib tree_tests::where_leaves_let_go_of_the_wood -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: counts leaves off their wood across the biomes"]
fn where_leaves_let_go_of_the_wood() {
    let generator = WorldGen::with_preset(1337, Preset::Normal);
    for &biome in Biome::ALL {
        let spot = (-4000..4000)
            .step_by(96)
            .flat_map(|gz| (-4000..4000).step_by(96).map(move |gx| (gx, gz)))
            .find(|&(gx, gz)| {
                [(0, 0), (24, 0), (-24, 0), (0, 24), (0, -24)].iter().all(|(dx, dz)| generator.biome_at(gx + dx, gz + dz) == biome)
            });
        let Some((gx, gz)) = spot else {
            println!("{biome:?}: none found");
            continue;
        };
        let square = Square::around(&generator, ChunkPos::new(gx.div_euclid(16), gz.div_euclid(16)), 1);
        let mut per: HashMap<BlockId, Tally> = HashMap::new();
        for (kind, size, held, at) in clumps(&square) {
            let e = per.entry(kind).or_default();
            e.0 += 1;
            if !held {
                e.1 += 1;
                e.2 += size;
                if e.3.len() < 4 {
                    e.3.push(at);
                }
            }
        }
        // Pieces of branch that end (one wood neighbour) with no leaf beside.
        let mut bare = 0;
        let mut ends = 0;
        let mut bare_at = Vec::new();
        for x in square.min.0 + 6..=square.max.0 - 6 {
            for z in square.min.1 + 6..=square.max.1 - 6 {
                for y in 1..CHUNK_SIZE_Y as i32 {
                    let b = square.at(x, y, z);
                    if !is_branch(b) {
                        continue;
                    }
                    let wood = FACES.iter().filter(|(dx, dy, dz)| is_wood(square.at(x + dx, y + dy, z + dz))).count();
                    // A tip: one piece of wood beside it, and not over it --
                    // a foot on the ground or a stump is not a limb's end.
                    let below = square.at(x, y - 1, z);
                    let footed = crate::types::has_full_top(below) && !is_wood(below) && !is_leafy(below);
                    if wood != 1 || is_wood(square.at(x, y + 1, z)) || footed {
                        continue;
                    }
                    ends += 1;
                    if !FACES.iter().any(|(dx, dy, dz)| is_leafy(square.at(x + dx, y + dy, z + dz))) {
                        bare += 1;
                        if bare_at.len() < 4 {
                            bare_at.push((x, y, z, block_name(b)));
                        }
                    }
                }
            }
        }
        println!("{biome:?} at ({gx}, {gz}): branch ends {ends}, bare {bare} {bare_at:?}");
        for (kind, (n, floating, leaves, at)) in per {
            println!("    {:<22} clumps {n:>4}, off the wood {floating:>3} ({leaves} leaves) {at:?}", block_name(kind));
        }
    }
}

/// Prints the blocks round a cell, layer by layer, for the tool above.
///
/// ```text
/// TREE_AT=x,y,z cargo test -p primitive_shared --lib tree_tests::what_is_round_a_cell -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: prints the blocks round TREE_AT"]
fn what_is_round_a_cell() {
    let Ok(at) = std::env::var("TREE_AT") else { return };
    let v: Vec<i32> = at.split(',').map(|s| s.trim().parse().expect("x,y,z")).collect();
    let generator = WorldGen::with_preset(1337, Preset::Normal);
    let square = Square::around(&generator, ChunkPos::new(v[0].div_euclid(16), v[2].div_euclid(16)), 1);
    let r = std::env::var("TREE_R").ok().and_then(|r| r.parse().ok()).unwrap_or(3);
    for y in (v[1] - r..=v[1] + r).rev() {
        println!("y={y}");
        for z in v[2] - r..=v[2] + r {
            let row: Vec<String> = (v[0] - r..=v[0] + r)
                .map(|x| {
                    let b = square.at(x, y, z);
                    let short: String = if b == crate::types::BLOCK_AIR {
                        ".".into()
                    } else if is_leafy(b) {
                        "L".into()
                    } else if let Some(w) = crate::types::branch_width(b) {
                        format!("b{w}")
                    } else {
                        block_name(b).chars().take(3).collect()
                    };
                    format!("{short:>4}")
                })
                .collect();
            println!("{}", row.join(""));
        }
    }
}

/// What is wrong with how a tree's leaves hold on to its wood: the wood's
/// ends with no leaf across a face, and the clumps of leaves (joined through
/// faces) with no wood across any face of any of their leaves.
///
/// The cells as the generator writes them: wood overwrites, leaves fill air.
pub(super) fn crown_faults(cells: &[super::branches::Cell]) -> (Vec<At>, Vec<At>) {
    let mut grid: HashMap<(i32, i32, i32), BlockId> = HashMap::new();
    for &(at, id) in cells {
        if is_branch(id) {
            grid.insert(at, id);
        } else {
            grid.entry(at).or_insert(id);
        }
    }
    let wood = |at: &(i32, i32, i32)| grid.get(at).is_some_and(|&b| is_branch(b));
    let leaf = |at: &(i32, i32, i32)| grid.get(at).is_some_and(|&b| is_leafy(b));
    let round = |(x, y, z): (i32, i32, i32)| FACES.map(|(dx, dy, dz)| (x + dx, y + dy, z + dz));
    let bare_ends = grid
        .keys()
        .filter(|&&at| wood(&at) && at != (0, 1, 0))
        .filter(|&&at| round(at).iter().filter(|n| wood(n)).count() == 1)
        .filter(|&&at| !round(at).iter().any(leaf))
        .copied()
        .collect();
    let mut seen = HashSet::new();
    let mut loose = Vec::new();
    for &start in grid.keys().filter(|at| leaf(at)) {
        if !seen.insert(start) {
            continue;
        }
        let (mut stack, mut held) = (vec![start], false);
        while let Some(at) = stack.pop() {
            for n in round(at) {
                held |= wood(&n);
                if leaf(&n) && seen.insert(n) {
                    stack.push(n);
                }
            }
        }
        if !held {
            loose.push(start);
        }
    }
    (bare_ends, loose)
}

/// The crowds a tree is grown in, as the roots of the trees round it: none,
/// and woods of the spacing the generator's own trees keep.
pub(super) const CROWDS: [&[(i32, i32)]; 4] = [
    &[],
    &[(4, 1), (-2, 4), (-4, -2), (1, -4)],
    &[(3, 0), (0, 3), (-3, 0), (0, -3)],
    &[(3, 2), (-3, 2), (2, -3), (-2, -3), (5, 0)],
];

/// A diagnostic: the faults of every branching builder, counted.
///
/// ```text
/// cargo test -p primitive_shared --lib tree_tests::how_the_builders_hold_their_leaves -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: counts the faults of every tree builder"]
fn how_the_builders_hold_their_leaves() {
    use super::branches::*;
    let level = |_: i32, _: i32| 0;
    let mut report: HashMap<&str, (usize, usize, usize, Vec<String>)> = HashMap::new();
    let mut note = |name: &'static str, what: String, cells: Vec<Cell>| {
        let (bare, loose) = crown_faults(&cells);
        let e = report.entry(name).or_default();
        e.0 += 1;
        e.1 += bare.len();
        e.2 += loose.len();
        if !bare.is_empty() && e.3.len() < 4 {
            e.3.push(format!("{what}: bare {bare:?} loose {loose:?}"));
        }
    };
    for crowd in CROWDS {
        let neighbour = |dx: i32, dz: i32| crowd.contains(&(dx, dz));
        for variant in (0..4096u32).step_by(7) {
            for height in [4, 6, 8] {
                let oak = branch_tree_cells(height, 2, variant, crate::types::BLOCK_LEAVES, level, neighbour);
                note("oak", format!("{height}/{variant:#x}/{crowd:?}"), oak);
                let maple = branch_tree_cells(height + 2, MAX_CANOPY_RADIUS, variant, crate::types::BLOCK_MAPLE_LEAVES, level, neighbour);
                note("maple", format!("{height}/{variant:#x}/{crowd:?}"), maple);
                let birch = birch_tree_cells(height + 2, variant, crate::types::BLOCK_BIRCH_LEAVES, level, neighbour);
                note("birch", format!("{height}/{variant:#x}/{crowd:?}"), birch);
            }
        }
    }
    for variant in (0..4096u32).step_by(7) {
        note("acacia", format!("{variant:#x}"), acacia_branch_cells(5, 3, variant, level));
        note("baobab", format!("{variant:#x}"), baobab_branch_cells(variant, level));
        for stage in 0..3 {
            if let Some(cells) = tree_stage_cells(stage, variant, crate::types::BLOCK_LEAVES, level) {
                note(["sapling", "young 1", "young 2"][stage as usize], format!("{variant:#x}"), cells);
            }
        }
    }
    let mut names: Vec<_> = report.keys().copied().collect();
    names.sort();
    for name in names {
        let (trees, bare, loose, examples) = &report[name];
        println!("{name}: {trees} trees, {bare} bare ends, {loose} loose clumps");
        for e in examples {
            println!("    {e}");
        }
    }
}

/// **"у деревьев проблема с креплением листвы на ветки".** Two ways a crown
/// let go of its wood, both in the generator and both counted here:
///
/// * a mass of leaves is torn at random round its rim, leaf by leaf, and the
///   rolls took the leaves round the very wood the mass hangs from -- a limb's
///   tip, a leader, the tip over a birch's hanging mass -- and cut outer
///   leaves off from the rest, so a tip stood bare with its leaves a cell off
///   and single leaves hung in the air beside the crown (`branches::blob`);
/// * a fork whose two arms were both refused in a close wood left the trunk
///   ending at the fork with no leader and no crown at all.
///
/// Every builder of a tree of pieces, in every crowd the woods grow them in:
/// no end of wood without a leaf across a face, and no clump of leaves
/// without wood across a face. Before the fix, 1177 bare ends and 3106 loose
/// clumps in 7032 oaks, 1766 and 6468 in as many maples.
#[test]
fn every_leaf_clump_of_a_generated_tree_touches_its_branch() {
    use super::branches::*;
    let level = |_: i32, _: i32| 0;
    let check = |name: &str, cells: Vec<Cell>| {
        let (bare, loose) = crown_faults(&cells);
        assert!(bare.is_empty(), "{name}: wood ends in the air at {bare:?}");
        assert!(loose.is_empty(), "{name}: leaves hang off the wood at {loose:?}");
    };
    for crowd in CROWDS {
        let neighbour = |dx: i32, dz: i32| crowd.contains(&(dx, dz));
        for variant in (0..4096u32).step_by(23) {
            for height in [4, 6, 8] {
                let at = format!("{height}/{variant:#x} in {crowd:?}");
                let oak = branch_tree_cells(height, 2, variant, crate::types::BLOCK_LEAVES, level, neighbour);
                check(&format!("oak {at}"), oak);
                let maple = branch_tree_cells(
                    height + 2,
                    MAX_CANOPY_RADIUS,
                    variant,
                    crate::types::BLOCK_MAPLE_LEAVES,
                    level,
                    neighbour,
                );
                check(&format!("maple {at}"), maple);
                let birch = birch_tree_cells(height + 2, variant, crate::types::BLOCK_BIRCH_LEAVES, level, neighbour);
                check(&format!("birch {at}"), birch);
            }
        }
    }
    for variant in (0..4096u32).step_by(23) {
        check(&format!("acacia {variant:#x}"), acacia_branch_cells(5, 3, variant, level));
        check(&format!("baobab {variant:#x}"), baobab_branch_cells(variant, level));
        for stage in 0..3 {
            let cells = tree_stage_cells(stage, variant, crate::types::BLOCK_LEAVES, level).expect("a stage");
            check(&format!("stage {stage} {variant:#x}"), cells);
        }
    }
}
