//! The edges of rivers, checked in the ground and the blocks a chunk holds.
//! See `banks` for what a bank is now and why it was not before.
//!
//! Everything here works in planet coordinates, as the generator does
//! inside (`WorldGen::on_planet`), so a chunk and the columns it was built
//! from are named by the same numbers.

use super::*;
use std::collections::BTreeMap;

const SEED: u32 = 1337;

/// Chunks with river water at their middle, found on a coarse sweep.
fn river_chunks(gen: &WorldGen, wanted: usize) -> Vec<ChunkPos> {
    let mut found = Vec::new();
    for step_z in 0..300 {
        for step_x in 0..300 {
            let (gx, gz) = (step_x * 96 - 14_400 + 8, step_z * 96 - 14_400 + 8);
            let height = gen.height_on_planet(gx, gz);
            if height < SEA_LEVEL && height > SEA_LEVEL - 6 && gen.biome_from(gx, gz, height) == Biome::River {
                found.push(ChunkPos::new(gx.div_euclid(CHUNK_SIZE_X as i32), gz.div_euclid(CHUNK_SIZE_Z as i32)));
                if found.len() == wanted {
                    return found;
                }
            }
        }
    }
    found
}

/// Is this column river water?
fn river_water(column: &Column) -> bool {
    column.biome == Biome::River && column.height < SEA_LEVEL && column.water == SEA_LEVEL
}

/// The dry columns of a chunk with river water against one of their four
/// sides, with their columns.
fn bank_columns(gen: &WorldGen, pos: ChunkPos) -> Vec<Column> {
    let (ox, oz) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
    let columns = ColumnCache::build(gen, ox, oz);
    let mut out = Vec::new();
    for lz in 0..CHUNK_SIZE_Z as i32 {
        for lx in 0..CHUNK_SIZE_X as i32 {
            let here = columns.at(lx, lz);
            if here.height < here.water || matches!(here.biome, Biome::Beach | Biome::Ocean) {
                continue;
            }
            if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dz)| river_water(&columns.at(lx + dx, lz + dz))) {
                out.push(here);
            }
        }
    }
    out
}

/// **A river is not one width from end to end.** Rivers are found on
/// straight walks and then followed along their own channel, re-centred at
/// every step on the run of water square to it, and each one's widths are
/// read along four hundred blocks: two rivers in three have to be at least
/// a third as wide again in their broadest reach as in their narrowest.
///
/// Before the banks wandered, a brook read 3 or 4 blocks at every step of
/// every walk and a river inside a block or two of one number.
#[test]
fn the_width_of_a_river_changes_along_it() {
    let gen = WorldGen::new(SEED);
    let wet = |x: f64, z: f64| gen.height_on_planet(x.floor() as i32, z.floor() as i32) < SEA_LEVEL;
    let mut rivers = 0;
    let mut wandering = 0;
    'walks: for line in 0..40 {
        let z = f64::from(line * 2_003 - 40_000) + 0.5;
        let mut run = 0;
        let mut x = -40_000.5;
        while x < 40_000.0 {
            x += 1.0;
            if wet(x, z) {
                run += 1;
                continue;
            }
            if !(3..=60).contains(&run) {
                run = 0;
                continue;
            }
            let (mut cx, mut cz) = (x - f64::from(run) / 2.0 - 0.5, z);
            run = 0;
            let height = gen.height_on_planet(cx as i32, cz as i32);
            if gen.biome_from(cx as i32, cz as i32, height) != Biome::River {
                continue;
            }
            // The order whose line is nearest, and its direction.
            let Some(order) = gen.river_orders().iter().min_by(|a, b| {
                let far = |o: &scale::RiverOrder| gen.river_field(o, cx, cz).abs() / o.frequency;
                far(a).total_cmp(&far(b))
            }) else {
                continue;
            };
            let mut widths = Vec::new();
            for _ in 0..50 {
                let sx = gen.river_field(order, cx + 2.0, cz) - gen.river_field(order, cx - 2.0, cz);
                let sz = gen.river_field(order, cx, cz + 2.0) - gen.river_field(order, cx, cz - 2.0);
                let length = sx.hypot(sz);
                if length <= f64::EPSILON || !wet(cx, cz) {
                    break;
                }
                let (nx, nz) = (sx / length, sz / length);
                let reach = |sign: f64| (1..300).take_while(|&d| wet(cx + nx * sign * f64::from(d), cz + nz * sign * f64::from(d))).count() as f64;
                let (ahead, behind) = (reach(1.0), reach(-1.0));
                widths.push(1.0 + ahead + behind);
                // Re-centre on the water, then step along the channel.
                cx += nx * (ahead - behind) / 2.0;
                cz += nz * (ahead - behind) / 2.0;
                cx += -nz * 8.0;
                cz += nx * 8.0;
            }
            if widths.len() < 40 {
                continue;
            }
            rivers += 1;
            let narrowest = widths.iter().copied().fold(f64::INFINITY, f64::min);
            let broadest = widths.iter().copied().fold(0.0, f64::max);
            if broadest >= narrowest * 1.35 {
                wandering += 1;
            }
            if rivers >= 24 {
                break 'walks;
            }
        }
    }
    println!("{wandering} of {rivers} rivers are a third as wide again in one reach as in another");
    assert!(rivers >= 8, "only {rivers} rivers followed");
    assert!(wandering * 3 >= rivers * 2, "only {wandering} of {rivers} rivers change width along their length");
}

/// **A river's edge is made of more than one thing.** Over the banks of two
/// dozen river chunks, the ground at the water's edge -- the dry column
/// beside the water and the shallows against it -- is at least four
/// materials, and no one of them is more than three columns in five.
///
/// It was two: sand under the water, turf over it, and the odd patch of the
/// deposit field's clay.
#[test]
fn a_river_bank_is_made_of_several_things_in_patches() {
    let gen = WorldGen::new(SEED);
    let mut tops: BTreeMap<BlockId, usize> = BTreeMap::new();
    for pos in river_chunks(&gen, 24) {
        let (ox, oz) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        let columns = ColumnCache::build(&gen, ox, oz);
        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in 0..CHUNK_SIZE_X as i32 {
                let here = columns.at(lx, lz);
                let beside = [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dz)| columns.at(lx + dx, lz + dz));
                let dry_bank = here.height >= SEA_LEVEL && beside.iter().any(river_water);
                let shallows = river_water(&here) && here.height >= SEA_LEVEL - 2 && beside.iter().any(|c| c.height >= SEA_LEVEL);
                if dry_bank && !matches!(here.biome, Biome::Beach | Biome::Ocean) || shallows {
                    *tops.entry(block_kind(here.surface.top)).or_default() += 1;
                }
            }
        }
    }
    let total: usize = tops.values().sum();
    println!("the edge of the water, by what it is made of: {tops:?}");
    assert!(total > 200, "only {total} columns at the edge of river water");
    let common = tops.values().filter(|&&n| n * 50 >= total).count();
    assert!(common >= 4, "the edge of a river is {common} materials of any size: {tops:?}");
    let most = tops.values().copied().max().unwrap_or(0);
    assert!(most * 5 <= total * 3, "one material is {most} of {total} columns at the water: {tops:?}");
}

/// **Some banks are walls and some come down to the water.** Of the dry
/// columns against river water, at least one in ten stands two blocks or
/// more over it -- a cut bank, climbed out of rather than walked -- and at
/// least one in five lies level with it.
///
/// Every bank was the same ramp before, and a column two over the water with
/// the water against it was a rounding accident.
#[test]
fn rivers_have_cut_banks_and_banks_that_shelve_into_the_water() {
    let gen = WorldGen::new(SEED);
    let mut rises: BTreeMap<i32, usize> = BTreeMap::new();
    for pos in river_chunks(&gen, 32) {
        for column in bank_columns(&gen, pos) {
            *rises.entry(column.height - SEA_LEVEL).or_default() += 1;
        }
    }
    let total: usize = rises.values().sum();
    let walls: usize = rises.range(2..).map(|(_, n)| n).sum();
    let level = rises.get(&0).copied().unwrap_or(0);
    println!("dry columns against river water, by height over it: {rises:?}");
    assert!(total > 200, "only {total} bank columns");
    assert!(walls * 10 >= total, "{walls} of {total} bank columns are a wall: {rises:?}");
    assert!(level * 5 >= total, "{level} of {total} bank columns are level with the water: {rises:?}");
    // ...and no wall is a gorge.
    let tallest = rises.keys().copied().max().unwrap_or(0);
    assert!(tallest <= 4, "a bank stands {tallest} blocks over the water: {rises:?}");
}

/// **The outside of a bend is cut and the inside shelves.** Over the
/// columns of every order's channel on a sweep, a bank on the outside of a
/// bend is a cut bank much more often than one on the inside.
#[test]
fn the_outside_of_a_bend_is_steeper_than_the_inside() {
    let gen = WorldGen::new(SEED);
    let (mut outside, mut inside) = ((0.0, 0usize), (0.0, 0usize));
    // Closer than the ninety-odd blocks it was: the landforms' brooks run
    // only near their rivers (`landforms::tributary_reach`), and the sweep
    // found twenty-odd bends of each hand where it wants thirty.
    for gz in (-30_000..30_000).step_by(61) {
        for gx in (-30_000..30_000).step_by(59) {
            let (land, island) = gen.land_before_rivers(gx, gz);
            if island {
                continue;
            }
            let mut height = land;
            for order in gen.river_orders() {
                let cut = gen.river(order, gx, gz, height);
                if cut.channel > 0.05 && cut.bank.steep.is_finite() {
                    if cut.bank.bend > 0.5 {
                        outside.0 += cut.bank.steep;
                        outside.1 += 1;
                    } else if cut.bank.bend < -0.5 {
                        inside.0 += cut.bank.steep;
                        inside.1 += 1;
                    }
                }
                height = gen.cut_order(order, gx, gz, height, &cut);
            }
        }
    }
    let mean = |(sum, n): (f64, usize)| sum / n.max(1) as f64;
    println!(
        "steepness on the outside of bends {:.2} over {}, on the inside {:.2} over {}",
        mean(outside),
        outside.1,
        mean(inside),
        inside.1
    );
    assert!(outside.1 >= 30 && inside.1 >= 30, "too few bends: {} outside, {} inside", outside.1, inside.1);
    assert!(mean(outside) > mean(inside) + 0.3, "a bend's outside is {:.2} steep and its inside {:.2}", mean(outside), mean(inside));
}

/// **No river water stands against air.** In the blocks of river chunks and
/// the chunks round them, every cell of water at or under the waterline has
/// something under it and something on each of its four sides -- ground,
/// water, a plant or a stone -- or the server's water runs out of it on the
/// first tick and the bank a player walks up to is a dry trench.
#[test]
fn the_water_at_a_river_bank_is_held_on_every_side() {
    let gen = WorldGen::new(SEED);
    for pos in river_chunks(&gen, 6) {
        let mut chunks = BTreeMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                let at = ChunkPos::new(pos.x + dx, pos.z + dz);
                chunks.insert((at.x, at.z), gen.generate_on_planet(at));
            }
        }
        let block = |gx: i32, y: i32, gz: i32| -> Option<BlockId> {
            let (cx, cz) = (gx.div_euclid(CHUNK_SIZE_X as i32), gz.div_euclid(CHUNK_SIZE_Z as i32));
            let chunk = chunks.get(&(cx, cz))?;
            let (lx, lz) = (gx.rem_euclid(CHUNK_SIZE_X as i32), gz.rem_euclid(CHUNK_SIZE_Z as i32));
            Some(chunk.get(lx as usize, y as usize, lz as usize))
        };
        let (ox, oz) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        for gz in oz..oz + CHUNK_SIZE_Z as i32 {
            for gx in ox..ox + CHUNK_SIZE_X as i32 {
                for y in SEA_LEVEL - 12..=SEA_LEVEL {
                    if block(gx, y, gz) != Some(BLOCK_WATER) {
                        continue;
                    }
                    for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)] {
                        assert_ne!(
                            block(gx + dx, y + dy, gz + dz),
                            Some(BLOCK_AIR),
                            "water at {gx},{y},{gz} has air beside it at {},{},{}",
                            gx + dx,
                            y + dy,
                            gz + dz
                        );
                    }
                }
            }
        }
    }
}

/// **A river chunk is the same whichever of its neighbours was made
/// first.** One chunk made cold on a thread of its own, and made again on
/// another thread after the eight round it -- so every tile it reads was
/// built by a different chunk's request -- has to come out block for block
/// the same, and its edge columns have to agree with the heights the
/// neighbour on the far side of each seam reads for them.
#[test]
fn a_river_chunk_is_the_same_whichever_side_of_its_seams_was_made_first() {
    let gen = WorldGen::new(SEED);
    for pos in river_chunks(&gen, 4) {
        let cold = std::thread::scope(|scope| scope.spawn(|| gen.generate_on_planet(pos)).join().expect("cold chunk"));
        let warm = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            if dx != 0 || dz != 0 {
                                let _ = gen.generate_on_planet(ChunkPos::new(pos.x + dx, pos.z + dz));
                            }
                        }
                    }
                    gen.generate_on_planet(pos)
                })
                .join()
                .expect("warm chunk")
        });
        assert!(cold.blocks == warm.blocks, "the chunk at {pos:?} depends on which chunk was made first");
        // The seam from both sides: the columns this chunk was built from and
        // the ones its neighbour's cache holds for the same ground.
        let (ox, oz) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        let here = ColumnCache::build(&gen, ox, oz);
        let east = ColumnCache::build(&gen, ox + CHUNK_SIZE_X as i32, oz);
        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in CHUNK_SIZE_X as i32 - 2..CHUNK_SIZE_X as i32 + 2 {
                let (a, b) = (here.at(lx, lz), east.at(lx - CHUNK_SIZE_X as i32, lz));
                assert_eq!((a.height, a.surface.top), (b.height, b.surface.top), "the seam east of {pos:?} disagrees at {lx},{lz}");
            }
        }
    }
}

/// A picture of the banks of a stretch of river, from above: the water
/// shaded by depth, the ground at the water's edge in the colour of what it
/// is made of, and the land shaded by its height over the water, so a cut
/// bank reads as a bright edge against dark water.
///
/// ```text
/// cargo test -p primitive_shared --lib -- --ignored --nocapture picture_of_a_river_bank
/// ```
///
/// Writes `target/river_banks.png`, or `$PRIMITIVE_MAP_DUMP_banks.png`.
#[test]
#[ignore = "diagnostic: writes a picture of river banks"]
fn picture_of_a_river_bank() {
    const SPAN: i32 = 256;
    const SCALE: u32 = 3;
    let gen = WorldGen::new(SEED);
    // A river rather than a brook: the first chunk whose nearest line is an
    // order more than a few blocks wide.
    let centre = river_chunks(&gen, 60)
        .into_iter()
        .find(|pos| {
            let (x, z) = (f64::from(pos.x * 16 + 8), f64::from(pos.z * 16 + 8));
            gen.river_orders()
                .iter()
                .min_by(|a, b| (gen.river_field(a, x, z).abs() / a.frequency).total_cmp(&(gen.river_field(b, x, z).abs() / b.frequency)))
                .is_some_and(|order| (20.0..50.0).contains(&order.half_width))
        })
        .expect("no river in the sweep");
    let (cx, cz) = (centre.x * CHUNK_SIZE_X as i32, centre.z * CHUNK_SIZE_Z as i32);
    let mut map = image::RgbImage::new(SPAN as u32 * SCALE, SPAN as u32 * SCALE);
    let (x0, z0) = ((cx - SPAN / 2).div_euclid(16) * 16, (cz - SPAN / 2).div_euclid(16) * 16);
    for tz in 0..SPAN / 16 {
        for tx in 0..SPAN / 16 {
            let columns = ColumnCache::build(&gen, x0 + tx * 16, z0 + tz * 16);
            for lz in 0..16 {
                for lx in 0..16 {
                    let c = columns.at(lx, lz);
                    let rise = c.height - SEA_LEVEL;
                    let rgb = if c.height < c.water {
                        let depth = (c.water - c.height).min(8) as u8;
                        match block_kind(c.surface.top) {
                            BLOCK_SAND if depth <= 2 => [150, 170, 150],
                            BLOCK_GRAVEL if depth <= 2 => [110, 120, 130],
                            crate::types::BLOCK_MUD if depth <= 2 => [70, 90, 80],
                            BLOCK_CLAY if depth <= 2 => [120, 130, 150],
                            _ => [30, 60u8.saturating_sub(depth * 3), 150u8.saturating_sub(depth * 10)],
                        }
                    } else {
                        let shade = |rgb: [u8; 3]| rgb.map(|v| (f32::from(v) * (0.75 + 0.08 * rise.clamp(0, 5) as f32)).min(255.0) as u8);
                        shade(match block_kind(c.surface.top) {
                            BLOCK_SAND => [215, 200, 140],
                            BLOCK_GRAVEL => [140, 140, 140],
                            BLOCK_CLAY => [160, 165, 185],
                            crate::types::BLOCK_MUD => [95, 75, 55],
                            BLOCK_DIRT => [130, 95, 60],
                            BLOCK_COBBLESTONE | BLOCK_STONE => [110, 110, 105],
                            _ => [80, 140, 60],
                        })
                    };
                    for (px, pz) in (0..SCALE).flat_map(|px| (0..SCALE).map(move |pz| (px, pz))) {
                        map.put_pixel((tx * 16 + lx) as u32 * SCALE + px, (tz * 16 + lz) as u32 * SCALE + pz, image::Rgb(rgb));
                    }
                }
            }
        }
    }
    let path = std::env::var("PRIMITIVE_MAP_DUMP").map(|stem| format!("{stem}_banks.png")).unwrap_or_else(|_| "target/river_banks.png".to_string());
    map.save(&path).expect("write the picture");
    println!("wrote {path}, centred on {cx},{cz}");
}
