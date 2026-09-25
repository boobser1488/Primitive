//! The clay, in the two places the ground keeps it: the alluvial bed lying
//! between a floodplain's soil and its rock (`WorldGen::clay_bed`), and the
//! parting on top of the sandstone in the deep rock (`Beds::at`).
//!
//! What these hold is the shape of a *bed* rather than the amount of clay.
//! A layer that hangs in the air, one that starts in the middle of the rock
//! with more rock over it, one that climbs a mountain: each of those is a
//! way the clay stops reading as geology and starts reading as a paint job,
//! and each has a test here.

use super::*;
use crate::types::block_kind;

const SEED: u32 = 1337;

/// The top solid block of a column, walking down from the sky.
fn top_of(chunk: &Chunk, x: usize, z: usize) -> Option<usize> {
    (0..CHUNK_SIZE_Y).rev().find(|&y| {
        let id = block_kind(chunk.get(x, y, z));
        id != BLOCK_AIR && id != BLOCK_WATER && id != crate::types::BLOCK_ICE
    })
}

/// Chunks laid out a `step` apart rather than in a block, for `ore_tests`'
/// reason: a solid square of chunks small enough to generate in a test is
/// one place, and one place is not the world.
fn spread_chunks(gen: &WorldGen, across: i32, step: i32) -> Vec<Chunk> {
    let mut out = Vec::new();
    for cx in 0..across {
        for cz in 0..across {
            out.push(gen.generate_chunk(ChunkPos::new((cx - across / 2) * step, (cz - across / 2) * step)));
        }
    }
    out
}

/// How thick a bed this column carries, asked the way the generator asks.
fn bed_at(gen: &WorldGen, gx: i32, gz: i32) -> i32 {
    let height = gen.height_at(gx, gz);
    let biome = gen.biome_from(gx, gz, height);
    gen.clay_bed(gx, gz, height, gen.water_level_at(gx, gz), gen.uphill_at(gx, gz), biome)
}

/// The beds a column was built with -- the same call `build_column_tile`
/// makes, so a test and a chunk cannot disagree about where the parting is.
fn beds_under(gen: &WorldGen, gx: i32, gz: i32) -> Beds {
    let height = gen.height_at(gx, gz);
    let biome = gen.biome_from(gx, gz, height);
    let surface = gen.surface_at(gx, gz, height);
    let (_, _, granite_from) = gen.stratum(gx, gz, biome, surface);
    gen.beds_at(gx, gz, granite_from)
}

/// **The alluvial bed lies under the soil and on the rock, and never
/// anywhere else.** The two failures it rules out are the two ways a bed
/// stops being one: a layer with rock over it as well as under it -- clay
/// cut arbitrarily into the stone, which is not how a bed is laid -- and a
/// layer with the soil *under* it, which is a sheet of clay floating in a
/// hillside.
///
/// Read over the columns of real chunks rather than off the function,
/// because the thing being asserted is about the blocks a player digs.
/// The deep parting is excluded by the column's own beds: it has rock over
/// it on purpose, and what says *it* is in the right place is
/// `the_deep_parting_lies_on_the_sandstone_under_the_limestone`.
#[test]
fn the_clay_bed_lies_between_the_soil_and_the_rock_and_never_in_the_air() {
    let gen = WorldGen::new(SEED);
    let mut seen = 0;
    for chunk in spread_chunks(&gen, 7, 23) {
        let (ox, oz) = (chunk.pos.x * CHUNK_SIZE_X as i32, chunk.pos.z * CHUNK_SIZE_Z as i32);
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                let (gx, gz) = (ox + x as i32, oz + z as i32);
                // The column's own beds, once: the parting is excluded by
                // name below, and asking per cell would be a noise sample
                // for every block of every chunk in the sweep.
                let beds = beds_under(&gen, gx, gz);
                for y in 1..CHUNK_SIZE_Y - 1 {
                    let deep = y as i32 >= beds.clay_from && (y as i32) < beds.clay_to;
                    if block_kind(chunk.get(x, y, z)) != BLOCK_CLAY || deep {
                        continue;
                    }
                    seen += 1;
                    let over = block_kind(chunk.get(x, y + 1, z));
                    let under = block_kind(chunk.get(x, y - 1, z));
                    // What may stand over a bed cell: more of the bed, the
                    // soil it was buried under, the sky or water where a
                    // bank has been cut back to it, and a cave's air where
                    // the carver opened it.
                    assert!(
                        over == BLOCK_CLAY
                            || over == BLOCK_AIR
                            || over == BLOCK_WATER
                            || !is_rock(over),
                        "clay at ({gx}, {y}, {gz}) has {} over it: the bed was cut into the rock",
                        crate::types::block_name(over)
                    );
                    // ...and what may not stand under it: the soil. A bed
                    // with earth beneath it is a bed laid in mid-column.
                    assert!(
                        !matches!(under, BLOCK_DIRT | BLOCK_GRASS | BLOCK_PEAT | crate::types::BLOCK_MUD)
                            && !crate::ground::is_soil(under),
                        "clay at ({gx}, {y}, {gz}) stands on {}: the bed is floating in the soil",
                        crate::types::block_name(under)
                    );
                }
            }
        }
    }
    assert!(seen > 500, "the sweep found only {seen} cells of alluvial clay to hold to anything");
}

/// **A floodplain has a bed and the mountains have none.** The one sentence
/// the whole surface rule is for: clay is what still water dropped, so it
/// belongs where water stood and nowhere a slope sheds it.
///
/// Measured as a share of columns rather than as a count, because the two
/// countries are not the same size in any one sweep.
#[test]
fn the_low_flat_country_carries_a_clay_bed_and_the_mountains_do_not() {
    let gen = WorldGen::new(SEED);
    let (mut flat, mut flat_clay) = (0, 0);
    let mut mountain_clay = 0;
    let mut mountains = 0;
    for gx in (-4_000..4_000).step_by(37) {
        for gz in (-4_000..4_000).step_by(41) {
            let height = gen.height_at(gx, gz);
            let biome = gen.biome_from(gx, gz, height);
            let bed = bed_at(&gen, gx, gz);
            if matches!(biome, Biome::Mountains | Biome::SnowyPeaks) {
                mountains += 1;
                mountain_clay += i32::from(bed > 0);
            }
            // The floodplain: ground within a few blocks of the water it
            // belongs to, with nothing rising from it, which is where the
            // mud a river carries stops.
            if (SEA_LEVEL..=SEA_LEVEL + 4).contains(&height)
                && gen.uphill_at(gx, gz) == 0
                && !matches!(biome, Biome::Ocean | Biome::River)
            {
                flat += 1;
                flat_clay += i32::from(bed > 0);
            }
        }
    }
    assert!(flat > 200 && mountains > 200, "the sweep found no country to compare: {flat} flat, {mountains} mountain");
    assert_eq!(mountain_clay, 0, "{mountain_clay} mountain columns of {mountains} carry a clay bed");
    let share = flat_clay as f64 / flat as f64;
    assert!(
        (0.20..0.75).contains(&share),
        "{:.0}% of the floodplain has clay under it -- a bed nobody finds, or the ground itself",
        share * 100.0
    );
}

/// **The bed wedges out rather than ending.** Walk out of a clay district in
/// a straight line and the thickness comes down through its values before it
/// reaches nought: a sheet that went from four layers to none in one column
/// is a wall of clay in a bank, which is what a bed laid by a threshold
/// alone looks like.
///
/// Read off the columns because thickness is what is being asserted, and a
/// chunk only shows the blocks.
#[test]
fn a_clay_bed_thins_out_at_its_edge_instead_of_ending_in_a_wall() {
    let gen = WorldGen::new(SEED);
    let mut steps = std::collections::BTreeMap::new();
    let mut thickest = 0;
    for gz in (-3_000..3_000).step_by(53) {
        let mut last = 0;
        for gx in -3_000..3_000 {
            let bed = bed_at(&gen, gx, gz);
            thickest = thickest.max(bed);
            *steps.entry((last - bed).abs()).or_insert(0u32) += 1;
            last = bed;
        }
    }
    assert!(thickest == CLAY_BED_MAX, "the thickest bed in three thousand blocks of sweep is {thickest}");
    // Every step between neighbouring columns is a layer at a time. Two at
    // once is not forbidden in principle -- the ground can drop under the
    // field -- but a bed that routinely stepped from four to nothing is the
    // wall this is here to rule out.
    let cliffs: u32 = steps.iter().filter(|(&step, _)| step > 1).map(|(_, &n)| n).sum();
    let total: u32 = steps.values().sum();
    assert!(
        cliffs * 200 < total,
        "{cliffs} of {total} steps in the bed's thickness are more than a layer: {steps:?}"
    );
}

/// **The deep parting lies on top of the sandstone, under the limestone.**
/// Where it is, is the whole of what it teaches: a player who meets clay in
/// a gallery knows which bed they are in and which way to drive the tunnel.
/// A lens that could turn up at any depth teaches nothing.
///
/// Also that it never eats the bed it lies on: there is always sandstone
/// under the clay, so the parting reads as a seam *in* the sandstone rather
/// than as the sandstone being clay.
#[test]
fn the_deep_parting_lies_on_the_sandstone_under_the_limestone() {
    let gen = WorldGen::new(SEED);
    let mut found = 0;
    for gx in (-2_000..2_000).step_by(97) {
        for gz in (-2_000..2_000).step_by(89) {
            let beds = beds_under(&gen, gx, gz);
            if beds.clay_from > beds.clay_to {
                continue;
            }
            found += 1;
            assert_eq!(beds.clay_to, beds.sandstone, "the parting at ({gx}, {gz}) is not against the limestone over it");
            assert!(
                beds.clay_to - beds.clay_from <= CLAY_LENS_MAX,
                "the parting at ({gx}, {gz}) is {} layers thick",
                beds.clay_to - beds.clay_from
            );
            assert!(
                beds.clay_from > beds.granite + 2,
                "the parting at ({gx}, {gz}) reaches down to the granite with no sandstone left under it"
            );
            // Read back through the generator's own function, which is what
            // the chunk loop calls: a cell in the band is clay, the cell
            // under the band is the sandstone it was laid on, and the cell
            // over it is the limestone.
            assert_eq!(WorldGen::bedded_rock(beds.clay_from, beds), BLOCK_CLAY);
            assert_eq!(WorldGen::bedded_rock(beds.clay_from - 1, beds), crate::types::BLOCK_SANDSTONE);
            assert_eq!(WorldGen::bedded_rock(beds.clay_to, beds), BLOCK_LIMESTONE);
        }
    }
    assert!(found > 20, "only {found} partings in a two-thousand-block sweep");
}

/// **The parting is found by the shaft that happens to land on it, and not
/// by every shaft.** Both bounds are the test, as they are for the surface
/// deposits: a lens under every column is another layer of the basement and
/// teaches nothing; one under none of them is a mechanic nobody meets.
#[test]
fn a_shaft_sunk_anywhere_finds_the_deep_clay_sometimes_and_not_usually() {
    let gen = WorldGen::new(SEED);
    let (mut columns, mut with) = (0, 0);
    for gx in (-6_000..6_000).step_by(151) {
        for gz in (-6_000..6_000).step_by(149) {
            let beds = beds_under(&gen, gx, gz);
            columns += 1;
            with += i32::from(beds.clay_from < beds.clay_to);
        }
    }
    let share = with as f64 / columns as f64;
    assert!(
        (0.04..0.30).contains(&share),
        "{:.1}% of the world's columns hold a deep parting ({with} of {columns})",
        share * 100.0
    );
}

/// **The clay a player digs is the clay they always dug.** Wherever the bed
/// or the parting put a cell, it is `BLOCK_CLAY` and nothing else: the same
/// block the riverbank patches are made of, so the pick, the yield, the
/// recipes and the kiln know nothing about any of this.
#[test]
fn clay_out_of_a_bed_is_the_same_block_as_clay_off_a_bank() {
    use crate::types::{is_affected_by_gravity, is_loose};
    // The properties the potter's whole chain rests on, restated here
    // because a bed of clay in a bank *above* a player's head is new: clay
    // that fell would bury the hole it was dug out of.
    assert!(!is_affected_by_gravity(BLOCK_CLAY));
    assert!(!is_loose(BLOCK_CLAY));
    assert!(crate::dig::digs_in_slices(BLOCK_CLAY), "clay stopped coming away a slice at a time");
    assert!(!is_rock(BLOCK_CLAY), "clay reads as rock, and then an ore body would be cut into a bed of it");
    // What a cell puts in the pack, which is the whole of what the potter's
    // chain sees of any of this: the block if it is broken whole, and a
    // lump for each of the four slices it is worked away in
    // (`build::handfuls_left`) -- which is the usual way, and the way the
    // scenario digs it out of a bank.
    assert_eq!(crate::types::block_drop(BLOCK_CLAY), Some(BLOCK_CLAY));
    assert_eq!(crate::types::block_drop_count(BLOCK_CLAY), 1);
    let bitten = crate::dig::next_bite(BLOCK_CLAY, crate::dig::Side::PosY).expect("clay takes a bite");
    assert_eq!(
        crate::build::handfuls_left(bitten).map(|(h, _)| h),
        Some(crate::types::BLOCK_HANDFUL_CLAY),
        "a worked cell of clay stopped giving lumps of clay"
    );

    // ...and that the generator writes the plain block, with no variant in
    // it: a bed cell and a bank cell stack in one slot.
    let gen = WorldGen::new(SEED);
    let mut seen = 0;
    for chunk in spread_chunks(&gen, 5, 29) {
        for &id in chunk.blocks.iter() {
            if block_kind(id) == BLOCK_CLAY {
                assert_eq!(id, BLOCK_CLAY, "a cell of clay carries a variant");
                seen += 1;
            }
        }
    }
    assert!(seen > 500, "only {seen} cells of clay to check");
}

/// **A bank cut into a floodplain shows the clay as a band.** The player's
/// half of the whole change: the wall of a cut reads turf, then earth, then
/// an unbroken stripe of clay, then rock -- in that order, so the layer is
/// something a player *sees* rather than something they are told about.
///
/// ## Why this test goes looking at the water and not over the country
///
/// It was written as a uniform sweep first, and what that measured is worth
/// keeping: of 3662 columns of clay country in a ten-thousand-block sweep,
/// **3049 had no lower neighbour at all** and 601 had one a single block
/// down. Clay country is flat -- that is what makes it clay country -- and
/// flat country has no cliffs in it. Twelve of the 3662 stood at a brink,
/// and all twelve were at water.
///
/// Which is the true answer rather than a defeat: the one thing that cuts a
/// floodplain is the river that made it. So a bank is looked for where a
/// bank is -- at the water's edge, where a potter has always dug -- and what
/// is asserted is the same thing either way.
#[test]
fn a_cut_bank_in_the_clay_country_shows_the_bed_as_a_band_in_its_face() {
    let gen = WorldGen::new(SEED);
    let mut faces = 0;
    let mut water = 0;
    for gx in (-9_000..9_000).step_by(23) {
        for gz in (-9_000..9_000).step_by(19) {
            // A column with water standing in it, and the dry ground round
            // it: a river's channel, a lake's bowl, a pond.
            let height = gen.height_at(gx, gz);
            if height >= gen.water_level_at(gx, gz).max(SEA_LEVEL) {
                continue;
            }
            water += 1;
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (2, 0), (-2, 0), (0, 2), (0, -2)] {
                let (bx, bz) = (gx + dx, gz + dz);
                let bank = gen.height_at(bx, bz);
                let bed = bed_at(&gen, bx, bz);
                if bed < 2 {
                    continue;
                }
                let soil = gen.surface_at(bx, bz, bank).soil;
                // The band has to stand in the open: the water beside it is
                // at least as far down as the soil over it, so the first
                // layer of clay is in the face rather than behind the bank.
                if bank - height < soil || bank - soil - bed < 1 {
                    continue;
                }
                let chunk = gen.generate_chunk(ChunkPos::new(bx.div_euclid(16), bz.div_euclid(16)));
                let (lx, lz) = (bx.rem_euclid(16) as usize, bz.rem_euclid(16) as usize);
                let at = |y: i32| block_kind(chunk.get(lx, y as usize, lz));
                // A cave mouth, a ruin, a pool or a feature may have had this
                // column; what is asserted is the bank the generator laid.
                if at(bank) == BLOCK_AIR || at(bank) == BLOCK_WATER {
                    continue;
                }
                // Walk down the face: the soil, then the band, then the rock.
                // Read as a *run* rather than against a computed depth,
                // because by the time a chunk has it the depth of the soil is
                // several rules' business (a river margin, a bank stripped by
                // its slope, a bog's peat), and what is asserted here is the
                // shape of the face.
                let Some(first) = (bank - soil - bed..=bank).rev().find(|&y| at(y) == BLOCK_CLAY) else {
                    continue;
                };
                let run = (1..).take_while(|k| at(first - k) == BLOCK_CLAY).count() as i32 + 1;
                faces += 1;
                // At least the bed's own thickness. More where the bank's
                // own surface is clay as well (`clay_site`): the patch the
                // water left on top and the bed under it are one face of
                // clay, which is what a clay bank at a river ought to be
                // and what the old world had a skin of.
                assert!(run >= bed, "the band at ({bx}, {bz}) is {run} layers where the column says {bed}");
                assert!(
                    !is_rock(at(first + 1)),
                    "the band at ({bx}, {bz}) has {} over it: it is cut into the rock, not lying on it",
                    crate::types::block_name(at(first + 1))
                );
                // ...and it ends: the cell under the run is not more earth,
                // so what a player sees is a stripe between two other
                // things rather than a column of clay to the bedrock. That
                // it ends on *rock* is
                // `the_clay_bed_lies_between_the_soil_and_the_rock_and_never_in_the_air`'s
                // to say, over every cell rather than over a face -- a cave
                // may have opened under this one.
                let below = at(first - run);
                assert!(
                    !crate::ground::is_soil(below) && below != BLOCK_DIRT,
                    "the band at ({bx}, {bz}) runs straight into {}",
                    crate::types::block_name(below)
                );
                if faces >= 30 {
                    return;
                }
            }
        }
    }
    panic!("only {faces} banks showing the band beside {water} columns of water");
}



/// How much clay the world holds, where it lies and what country it is in,
/// split by which of the three rules put it there. Not an assertion: the
/// numbers a change to any of them is argued from.
///
/// ```text
/// cargo test -p primitive_shared --lib -- --ignored --nocapture probe_clay
/// ```
#[test]
#[ignore = "prints how much clay the world has and where"]
fn probe_clay() {
    use std::collections::BTreeMap;
    let gen = WorldGen::new(SEED);
    let chunks = spread_chunks(&gen, 9, 17);
    // Cells, by rule, by ten blocks of depth under the top of their own
    // column -- which is the depth a player digs, not a height.
    let mut depth: [BTreeMap<i32, usize>; 3] = Default::default();
    let mut biomes: [BTreeMap<&'static str, usize>; 3] = Default::default();
    let mut cells = [0usize; 3];
    // ...and the bed itself, over the same columns: how many carry one and
    // how thick it is.
    let mut beds_by_thickness: BTreeMap<i32, usize> = BTreeMap::new();
    let mut partings_by_thickness: BTreeMap<i32, usize> = BTreeMap::new();
    let mut columns = 0usize;
    for chunk in &chunks {
        let (ox, oz) = (chunk.pos.x * CHUNK_SIZE_X as i32, chunk.pos.z * CHUNK_SIZE_Z as i32);
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                let (gx, gz) = (ox + x as i32, oz + z as i32);
                let height = gen.height_at(gx, gz);
                let biome = gen.biome_from(gx, gz, height);
                let beds = beds_under(&gen, gx, gz);
                let bed = bed_at(&gen, gx, gz);
                columns += 1;
                *beds_by_thickness.entry(bed).or_default() += 1;
                *partings_by_thickness.entry(beds.clay_to.saturating_sub(beds.clay_from).max(0)).or_default() += 1;
                let Some(top) = top_of(chunk, x, z) else { continue };
                let soil = gen.surface_at(gx, gz, height).soil;
                for y in 0..CHUNK_SIZE_Y {
                    if block_kind(chunk.get(x, y, z)) != BLOCK_CLAY {
                        continue;
                    }
                    let y = y as i32;
                    let which = if y >= beds.clay_from && y < beds.clay_to {
                        2
                    } else if bed > 0 && y <= height - soil && y > height - soil - bed {
                        1
                    } else {
                        0
                    };
                    cells[which] += 1;
                    *depth[which].entry((top as i32 - y) / 10 * 10).or_default() += 1;
                    *biomes[which].entry(biome.name()).or_default() += 1;
                }
            }
        }
    }
    let per_chunk = |n: usize| n as f64 / chunks.len() as f64;
    println!(
        "clay over {} chunks: {:.1} cells a chunk in all",
        chunks.len(),
        per_chunk(cells.iter().sum::<usize>())
    );
    for (which, name) in ["the old surface patches", "the alluvial bed", "the deep parting"].iter().enumerate() {
        let mut country: Vec<_> = std::mem::take(&mut biomes[which]).into_iter().collect();
        country.sort_unstable_by_key(|&(_, n)| std::cmp::Reverse(n));
        country.truncate(6);
        println!("  {name}: {:.1} a chunk ({} cells)", per_chunk(cells[which]), cells[which]);
        println!("    depth under the surface, in tens: {:?}", depth[which]);
        println!("    country: {country:?}");
    }
    let with: usize = beds_by_thickness.iter().filter(|(&t, _)| t > 0).map(|(_, &n)| n).sum();
    println!(
        "  a bed under {:.1}% of {columns} columns; thicknesses {beds_by_thickness:?}",
        with as f64 * 100.0 / columns as f64
    );
    let deep: usize = partings_by_thickness.iter().filter(|(&t, _)| t > 0).map(|(_, &n)| n).sum();
    println!(
        "  a parting under {:.1}% of them; thicknesses {partings_by_thickness:?}",
        deep as f64 * 100.0 / columns as f64
    );
}

/// Banks near the origin of the scenarios' world (seed 1337, temperate,
/// `Scale::Landforms`) where the clay stands in the face and a player has
/// dry ground to walk to the brink over.
///
/// What it is for: `a_player_who_walks_to_the_brink_of_a_bank_sees_the_clay`
/// in `primitive_client` names one of these places, and this is how another
/// is found when the ground moves under it.
///
/// ```text
/// cargo test -p primitive_shared --lib -- --ignored --nocapture probe_clay_banks
/// ```
#[test]
#[ignore = "prints the clay banks the scenario can be stood at"]
fn probe_clay_banks() {
    let gen = WorldGen::new(SEED);
    let mut found = 0;
    for gx in -600..600 {
        for gz in -600..600 {
            let height = gen.height_at(gx, gz);
            if height < gen.water_level_at(gx, gz).max(SEA_LEVEL) {
                continue;
            }
            let bed = bed_at(&gen, gx, gz);
            if bed < 2 {
                continue;
            }
            let soil = gen.surface_at(gx, gz, height).soil;
            // A face of at least the soil and one layer of clay, to the
            // west, and four columns of walkable ground to the east of it
            // at this column's own height for the player to come over.
            let face = height - gen.height_at(gx - 1, gz);
            if face < soil + 1 || (1..=4).any(|k| (gen.height_at(gx + k, gz) - height).abs() > 1) {
                continue;
            }
            let chunk = gen.generate_chunk(ChunkPos::new(gx.div_euclid(16), gz.div_euclid(16)));
            let (lx, lz) = (gx.rem_euclid(16) as usize, gz.rem_euclid(16) as usize);
            let band: Vec<&str> = (0..6)
                .map(|k| crate::types::block_name(block_kind(chunk.get(lx, (height - k) as usize, lz))))
                .collect();
            println!("brink at ({gx}, {gz}) h={height} soil={soil} bed={bed} face={face}: {band:?}");
            found += 1;
            if found >= 12 {
                return;
            }
        }
    }
    println!("only {found} brinks within six hundred blocks of the origin");
}
