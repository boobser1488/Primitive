use super::*;

/// Where a river's line ends when it is followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// At the coast: the land under the line has come down to the sea.
    Sea,
    /// In high country, where the order fades out: a source.
    Source,
    /// Back where it started: a river running in a ring.
    Ring,
    /// Into the channel of a wider order: a tributary's mouth.
    Joins,
    /// Still going when the walk gave up.
    Far,
}

/// The ground an order is cut into: the land with every wider order's valley
/// and channel already in it, as `cut_rivers` hands it over. A brook runs on
/// down a river's valley side where the land before the river stood too high
/// for one, and a line followed over the uncut land would stop there.
fn ground_for(gen: &WorldGen, order: &scale::RiverOrder, gx: i32, gz: i32) -> f64 {
    let mut height = gen.land_before_rivers(gx, gz).0;
    for wider in gen.river_orders().iter().take_while(|wider| wider.frequency < order.frequency) {
        let cut = gen.river(wider, gx, gz, height);
        height = gen.cut_order(wider, gx, gz, height, &cut);
    }
    height
}

/// Follows one order's line from a point near it, one way, over planet
/// coordinates, and says how it ends.
///
/// The line is the order's own field's zero (`river_field`), walked along its
/// tangent a step at a time and pulled back onto the zero by Newton after
/// every step -- the same line `WorldGen::river` cuts, before any fade.
fn follow(gen: &WorldGen, order: &scale::RiverOrder, start: (f64, f64), way: f64, reach: f64) -> End {
    let field = |x: f64, z: f64| gen.river_field(order, x, z);
    let d = 4.0;
    let gradient = |x: f64, z: f64| {
        ((field(x + d, z) - field(x - d, z)) / (2.0 * d), (field(x, z + d) - field(x, z - d)) / (2.0 * d))
    };
    let settle = |(mut x, mut z): (f64, f64)| {
        for _ in 0..4 {
            let (gx, gz) = gradient(x, z);
            let g2 = gx * gx + gz * gz;
            if g2 <= 0.0 {
                break;
            }
            let f = field(x, z);
            x -= f * gx / g2;
            z -= f * gz / g2;
        }
        (x, z)
    };
    let step = (0.02 / order.frequency).clamp(6.0, 400.0);
    let first = settle(start);
    let mut at = first;
    let mut walked = 0.0;
    while walked < reach {
        let (gx, gz) = gradient(at.0, at.1);
        let norm = (gx * gx + gz * gz).sqrt().max(1e-12);
        at = settle((at.0 - gz / norm * step * way, at.1 + gx / norm * step * way));
        walked += step;
        // Into a wider order's water: its field within a channel's width of
        // zero, measured the way `WorldGen::river` measures a bank. First,
        // because the wider channel has taken the ground under the sea's
        // level, and that is a mouth, not a hollow.
        for wider in gen.river_orders().iter().take_while(|wider| wider.frequency < order.frequency) {
            let f = gen.river_field(wider, at.0, at.1);
            let e = 8.0;
            let gx = (gen.river_field(wider, at.0 + e, at.1) - f) / e;
            let gz = (gen.river_field(wider, at.0, at.1 + e) - f) / e;
            if f.abs() / (gx * gx + gz * gz).sqrt().max(1e-12) < wider.half_width * 0.5 {
                return End::Joins;
            }
        }
        let land = ground_for(gen, order, at.0.round() as i32, at.1.round() as i32);
        // Down at the water's level: the sea, if this is the coast. Inland it
        // is a flat at the level the channel already holds -- a floodplain
        // the river runs on across, not the end of it.
        if land < SEA_LEVEL as f64 + 0.5 {
            let continent = gen.continent(at.0.round() as i32, at.1.round() as i32);
            if spline(CONTINENT_SPLINE, continent) <= SEA_LEVEL as f64 + 2.5 {
                return End::Sea;
            }
        }
        if land > SEA_LEVEL as f64 + order.fades.0 {
            return End::Source;
        }
        // ...and a tributary run out of the reach of what it feeds has risen.
        if gen.tributary_reach(order, at.0, at.1) <= 0.0 {
            return End::Source;
        }
        if walked > 20.0 * step && ((at.0 - first.0).powi(2) + (at.1 - first.1).powi(2)).sqrt() < 2.0 * step {
            return End::Ring;
        }
    }
    End::Far
}

/// Both ends of the lines of every order that pass near a handful of points
/// round a world's origin, counted by how they end.
fn river_ends(gen: &WorldGen, reach_in_spacings: f64) -> Vec<(usize, [End; 2])> {
    let origin = gen.on_planet(0, 0);
    let mut ends = Vec::new();
    for (index, order) in gen.river_orders().iter().enumerate() {
        let spacing = 0.5 / order.frequency;
        let reach = spacing * reach_in_spacings;
        // Six places this order actually has water, found by walking out
        // from the origin along six bearings: a line followed from where no
        // channel is cut says nothing about the rivers anybody meets.
        for k in 0..6 {
            let angle = f64::from(k) * 1.047 + 0.3;
            let step = (order.half_width * 0.4).max(2.0);
            let found = (1..)
                .map(|i| f64::from(i) * step)
                .take_while(|&r| r < spacing * 20.0)
                .map(|r| (f64::from(origin.0) + angle.cos() * r, f64::from(origin.1) + angle.sin() * r))
                .find(|&(x, z)| {
                    let (gx, gz) = (x.round() as i32, z.round() as i32);
                    let (_, island) = gen.land_before_rivers(gx, gz);
                    // Out of the mountain belts: a great river's stretch
                    // between two ranges is a gorge lake of the range's own,
                    // and the question here is the lowland's water.
                    !island && gen.highland(gx, gz) < 0.6 && gen.river(order, gx, gz, ground_for(gen, order, gx, gz)).channel > 0.5
                });
            if let Some(start) = found {
                ends.push((index, [follow(gen, order, start, 1.0, reach), follow(gen, order, start, -1.0, reach)]));
            }
        }
    }
    ends
}

#[test]
#[ignore = "a measurement, not an assertion"]
fn probe_river_ends() {
    for scale in [Scale::Earth, Scale::Landforms] {
        // Per order: lines, rings, lines reaching water, lines closed at both ends.
        let mut tally = [[0u32; 4]; 3];
        for seed in [1337u32, 4242, 99, 7, 2024, 31337, 5, 777] {
            let gen = WorldGen::with_scale(seed, Preset::Normal, Zone::Temperate, scale);
            for (order, ends) in river_ends(&gen, 60.0) {
                let row = &mut tally[order];
                row[0] += 1;
                row[1] += u32::from(ends.contains(&End::Ring));
                row[2] += u32::from(ends.iter().any(|end| matches!(end, End::Sea | End::Joins | End::Far)));
                row[3] += u32::from(ends == [End::Source, End::Source]);
            }
        }
        println!("{scale:?}: [lines, rings, reach water or run on, closed at both ends] by order {tally:?}");
    }
}

/// A fingerprint of every block of a chunk, for holding a generator to the
/// ground it drew before.
///
/// FNV-1a written out rather than `DefaultHasher`, whose output the standard
/// library does not promise across releases: a toolchain update would turn
/// this red with not one block of any world changed.
fn fingerprint(gen: &WorldGen, pos: ChunkPos) -> u64 {
    let chunk = gen.generate_chunk(pos);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for y in 0..CHUNK_SIZE_Y {
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                for byte in u32::from(chunk.get(x, y, z)).to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
    }
    hash
}

const OLD_CHUNKS: [(i32, i32); 4] = [(0, 0), (5, -3), (-40, 90), (313, -77)];

/// **A world made before the landforms keeps drawing its new chunks the way
/// it drew its old ones.** A save is edits over regenerated ground, so a
/// chunk a player walks into for the first time in an old world has to come
/// out of the generator that world was made with -- a hill in a new chunk
/// that the old generator never put there is a seam at the edge of every
/// explored country, and a house built across that edge standing on air.
///
/// The prints were taken off the generator before `Scale::Landforms`
/// existed, and every block of four chunks of each old scale is held to them.
#[test]
fn an_old_worlds_new_chunks_are_the_old_generators_to_the_block() {
    let held = [
        (Scale::Regional, [0x99ab_061c_9367_039c, 0x4af1_4e54_a088_dea4, 0x5de2_7d8a_c859_9387, 0xa130_a842_464d_0838]),
        (Scale::Earth, [0xf759_bb4f_1f5a_338a, 0x7286_f6ee_afce_8d93, 0x0a60_1d08_b823_9ba7, 0xf79a_fe4f_8046_74a8]),
    ];
    for (scale, prints) in held {
        let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, scale);
        for (&(x, z), print) in OLD_CHUNKS.iter().zip(prints) {
            assert_eq!(fingerprint(&gen, ChunkPos::new(x, z)), print, "{scale:?} chunk {x},{z} is not the ground it was");
        }
    }
}

#[test]
#[ignore = "prints the fingerprints the golden test holds"]
fn print_fingerprints() {
    for scale in [Scale::Regional, Scale::Earth] {
        let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, scale);
        let prints: Vec<u64> = OLD_CHUNKS.iter().map(|&(x, z)| fingerprint(&gen, ChunkPos::new(x, z))).collect();
        println!("{scale:?}: {prints:#x?}");
    }
}

/// The relief round a column: the range of the ground over a square
/// `2 * reach` across, and the tallest step to a neighbour at each sample,
/// every `step`.
///
/// **Dry ground only**: a brook's channel is cut to the sea's level wherever
/// it runs, so a square with one in it has the brook's depth for its range,
/// whatever the country round it is doing.
fn relief_round(gen: &WorldGen, gx: i32, gz: i32, reach: i32, step: i32) -> (i32, Vec<i32>) {
    let (mut low, mut high, mut steps) = (i32::MAX, i32::MIN, Vec::new());
    for dz in (-reach..=reach).step_by(step as usize) {
        for dx in (-reach..=reach).step_by(step as usize) {
            let (x, z) = (gx + dx, gz + dz);
            let h = gen.height_at(x, z);
            if h <= SEA_LEVEL + 1 {
                continue;
            }
            low = low.min(h);
            high = high.max(h);
            // The tallest step to a neighbour: what a player walking over
            // the ground meets, where a slope is an average of two.
            let tallest = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .map(|&(ox, oz)| (gen.height_at(x + ox, z + oz) - h).abs())
                .max()
                .unwrap_or(0);
            steps.push(tallest);
        }
    }
    ((high - low).max(0), steps)
}

/// Per biome over a square of a world round its origin, sixty kilometres a
/// side: how many samples, the median range of the ground a quarter of a
/// kilometre round every fourth of them, and the share of their ground with a
/// step of two blocks or more -- the step a player has to jump.
fn relief_by_biome(gen: &WorldGen) -> std::collections::HashMap<Biome, (u32, i32, f64)> {
    let mut raw: std::collections::HashMap<Biome, (u32, Vec<i32>, Vec<i32>)> = Default::default();
    for gz in (-30_000..30_000).step_by(600) {
        for gx in (-30_000..30_000).step_by(600) {
            let biome = gen.biome_at(gx, gz);
            let entry = raw.entry(biome).or_default();
            entry.0 += 1;
            if matches!(biome, Biome::Hills | Biome::Steppe | Biome::Plains | Biome::Forest) && entry.0 % 4 == 1 {
                let (range, steps) = relief_round(gen, gx, gz, 128, 16);
                entry.1.push(range);
                entry.2.extend(steps);
            }
        }
    }
    raw.into_iter()
        .map(|(biome, (n, mut ranges, steps))| {
            ranges.sort_unstable();
            let median = ranges.get(ranges.len() / 2).copied().unwrap_or(0);
            let jumps = steps.iter().filter(|&&s| s >= 2).count() as f64 / steps.len().max(1) as f64;
            (biome, (n, median, jumps))
        })
        .collect()
}

/// ```text
/// cargo test -p primitive_shared --lib -- --ignored --nocapture probe_landforms
/// ```
#[test]
#[ignore = "a measurement, not an assertion"]
fn probe_landforms() {
    for seed in [1337u32, 4242, 99, 7] {
        let gen = WorldGen::new(seed);
        let table = relief_by_biome(&gen);
        let land: u32 = table.iter().filter(|(b, _)| **b != Biome::Ocean).map(|(_, v)| v.0).sum();
        println!("seed {seed}: {land} samples of land");
        for (biome, (n, median, jumps)) in table {
            println!(
                "  {:>13} {:5.1}%  median range {median:3}  steps of 2+ {:4.1}%",
                biome.name(),
                100.0 * f64::from(n) / f64::from(land.max(1)),
                100.0 * jumps
            );
        }
    }
}

/// **A down country rolls and a steppe lies flat, and both are there to be
/// found.** Measured over two seeds with `relief_by_biome`: the ground round
/// a steppe column rises and falls ten or twelve blocks in a quarter of a
/// kilometre, a down's forty-odd, and an ordinary meadow's the same forty --
/// but a meadow's is benches and steps, and a down's is a slope: a third as
/// many of its columns have a step a player must jump.
#[test]
fn a_down_country_rolls_and_a_steppe_lies_flat() {
    for seed in [1337u32, 99] {
        let table = relief_by_biome(&WorldGen::new(seed));
        let of = |biome: Biome| table.get(&biome).copied().unwrap_or_else(|| panic!("seed {seed}: no {} at all", biome.name()));
        let (hills, steppe, plains) = (of(Biome::Hills), of(Biome::Steppe), of(Biome::Plains));
        println!("seed {seed}: hills {hills:?} steppe {steppe:?} plains {plains:?}");
        assert!(hills.0 >= 100 && steppe.0 >= 100, "seed {seed}: {} samples of hills and {} of steppe", hills.0, steppe.0);
        assert!(steppe.1 <= 16, "seed {seed}: the steppe rises and falls {} blocks in a quarter of a kilometre", steppe.1);
        assert!(hills.1 >= 30, "seed {seed}: the downs rise and fall only {} blocks", hills.1);
        assert!(hills.1 >= 2 * steppe.1 + 10, "seed {seed}: the downs ({}) are no hillier than the steppe ({})", hills.1, steppe.1);
        assert!(
            hills.2 * 2.0 < plains.2,
            "seed {seed}: {:.1}% of the downs is a jump, against {:.1}% of a meadow -- the downs are not rounded",
            hills.2 * 100.0,
            plains.2 * 100.0
        );
    }
}

/// **The steppe is open and the downs have woods in them.** Trees are rooted
/// by a hash on a rate (`WorldGen::tree_at`), so the rate is the claim: over
/// a sweep, the steppe grows a tree in some hundreds of columns and fewer
/// than a meadow does, and the downs grow many more than the steppe -- in
/// groves, since a column in a grove is many times likelier to hold one.
#[test]
fn the_steppe_is_open_and_the_downs_have_groves() {
    let gen = WorldGen::new(1337);
    let mut tally: std::collections::HashMap<Biome, (u32, u32)> = Default::default();
    let mut grove = (0u32, 0u32, 0u32, 0u32);
    for gz in (-20_000..20_000).step_by(37) {
        for gx in (-20_000..20_000).step_by(41) {
            let (px, pz) = gen.on_planet(gx, gz);
            let biome = gen.biome_on_planet(px, pz);
            if !matches!(biome, Biome::Steppe | Biome::Hills | Biome::Plains) {
                continue;
            }
            let tree = gen.tree_at(px, pz, biome);
            let entry = tally.entry(biome).or_default();
            entry.0 += 1;
            entry.1 += u32::from(tree);
            if biome == Biome::Hills {
                if gen.savanna_grove(px, pz) {
                    grove.0 += 1;
                    grove.1 += u32::from(tree);
                } else {
                    grove.2 += 1;
                    grove.3 += u32::from(tree);
                }
            }
        }
    }
    let rate = |biome: Biome| {
        let (n, trees) = tally[&biome];
        f64::from(trees) / f64::from(n.max(1))
    };
    println!("trees a column: steppe {:.4}, meadow {:.4}, downs {:.4}; downs in a grove {grove:?}", rate(Biome::Steppe), rate(Biome::Plains), rate(Biome::Hills));
    assert!(rate(Biome::Steppe) < 1.0 / 400.0, "a steppe with a tree in every {:.0} columns", 1.0 / rate(Biome::Steppe));
    assert!(rate(Biome::Steppe) < rate(Biome::Plains), "the steppe is more wooded than the meadow");
    assert!(rate(Biome::Hills) > 3.0 * rate(Biome::Steppe), "the downs are as bare as the steppe");
    let in_grove = f64::from(grove.1) / f64::from(grove.0.max(1));
    let outside = f64::from(grove.3) / f64::from(grove.2.max(1));
    assert!(in_grove > 10.0 * outside, "a grove on the downs ({in_grove:.4}) is no thicker than the open down ({outside:.4})");
}

/// **Rivers run out to the sea.** The great rivers and rivers met near eight
/// worlds' origins, followed both ways along their own lines, reach the sea,
/// run into a wider river, or are still running a thousand kilometres on --
/// all but one in twenty, and none comes back round to where it started. And
/// the brooks, which run only near the river they feed
/// (`landforms::tributary_reach`), are closed at both ends in fewer of the
/// cases than the Earth's scale left them so.
///
/// Measured (`probe_river_ends`): the Earth's brooks 21 closed in 43, these
/// 8 in 27; one river in 75 stranded between two rises.
#[test]
fn rivers_run_out_to_the_sea_and_never_in_a_ring() {
    let seeds = [1337u32, 4242, 99, 7, 2024, 31337, 5, 777];
    let (mut rivers, mut stranded) = (0u32, 0u32);
    let mut closed_brooks = |scale: Scale| {
        let (mut brooks, mut closed) = (0u32, 0u32);
        for seed in seeds {
            let gen = WorldGen::with_scale(seed, Preset::Normal, Zone::Temperate, scale);
            for (order, ends) in river_ends(&gen, 60.0) {
                if scale == Scale::Landforms {
                    assert!(!ends.contains(&End::Ring), "seed {seed}: an order-{order} river runs in a ring");
                    if order < 2 {
                        rivers += 1;
                        stranded += u32::from(!ends.iter().any(|end| matches!(end, End::Sea | End::Joins | End::Far)));
                    }
                }
                if order == 2 {
                    brooks += 1;
                    closed += u32::from(ends == [End::Source, End::Source]);
                }
            }
        }
        (brooks, closed)
    };
    let (earth, landforms) = (closed_brooks(Scale::Earth), closed_brooks(Scale::Landforms));
    println!("brooks closed at both ends: Earth {earth:?}, landforms {landforms:?}; rivers stranded {stranded} of {rivers}");
    // A river rising in hills at both ends is a lowland basin walled all
    // round -- a lake's country, which the water here cannot make -- and one
    // in twenty is as many as the lowlands are allowed.
    assert!(rivers >= 40, "only {rivers} rivers found to follow");
    assert!(stranded * 20 <= rivers, "{stranded} of {rivers} rivers reach neither the sea nor a wider river");
    assert!(landforms.0 >= 15, "only {} brooks found to follow", landforms.0);
    let share = |(n, closed): (u32, u32)| f64::from(closed) / f64::from(n.max(1));
    assert!(share(landforms) < 0.4, "{} brooks of {} are trenches closed at both ends", landforms.1, landforms.0);
    assert!(share(landforms) < share(earth), "the landforms' brooks are no better drained than the Earth's");
}

/// **One seed is one world.** Two generators built apart from one seed make
/// the same chunks to the block, on two threads -- so no remembered tile of
/// the first can lend the second anything -- and another seed makes other
/// ground.
#[test]
fn two_worlds_from_one_seed_are_the_same_world_to_the_block() {
    let chunks = [(0, 0), (37, -12), (-250, 410)];
    let prints = |seed: u32| {
        std::thread::spawn(move || {
            let gen = WorldGen::with_zone(seed, Preset::Normal, Zone::Temperate);
            assert_eq!(gen.scale(), Scale::Landforms);
            chunks.map(|(x, z)| fingerprint(&gen, ChunkPos::new(x, z)))
        })
        .join()
        .expect("a generator thread")
    };
    let (first, second) = (prints(2718), prints(2718));
    assert_eq!(first, second, "one seed made two worlds");
    assert_ne!(first, prints(2719), "two seeds made one world");
}

/// **Every biome a species can declare is one the world grows.** The steppe
/// and the downs are the country a herd of horses or a flock is to be found
/// in, so each has to exist near a new player of an ordinary temperate world
/// -- within a few hours' walk -- in most seeds, not in one seed of fifty.
#[test]
fn a_steppe_and_a_down_country_lie_within_a_walk_of_most_spawns() {
    let mut both = 0;
    for seed in 0..8u32 {
        let gen = WorldGen::new(seed);
        let (mut steppe, mut hills) = (false, false);
        'sweep: for gz in (-8_000..8_000).step_by(250) {
            for gx in (-8_000..8_000).step_by(250) {
                match gen.biome_at(gx, gz) {
                    Biome::Steppe => steppe = true,
                    Biome::Hills => hills = true,
                    _ => {}
                }
                if steppe && hills {
                    break 'sweep;
                }
            }
        }
        both += u32::from(steppe && hills);
    }
    assert!(both >= 6, "only {both} of eight temperate worlds have both a steppe and downs within eight kilometres");
}

/// **What the landforms cost a chunk**, against the Earth's generator on the
/// same chunks in the same binary: a square of 144 round the origin and a
/// square of 144 in hill and plain country, each generated cold on a thread
/// of its own (the tiles are remembered per thread), the two generators in
/// turn for five rounds so neither has the warmer machine.
///
/// ```text
/// cargo test -p primitive_shared --release --lib -- --ignored --nocapture what_the_landforms_cost_a_chunk
/// ```
#[test]
#[ignore = "a measurement, not an assertion"]
fn what_the_landforms_cost_a_chunk() {
    use std::time::Instant;
    // Hill and plain country of seed 1337, found by the landforms' own
    // generator; the Earth's is timed on the same chunks.
    let probe = WorldGen::new(1337);
    let country = (0..400)
        .flat_map(|k| (0..400).map(move |j| (j * 150 - 30_000, k * 150 - 30_000)))
        .find(|&(gx, gz)| matches!(probe.biome_at(gx, gz), Biome::Hills) && matches!(probe.biome_at(gx + 400, gz + 400), Biome::Hills | Biome::Steppe))
        .expect("hill country near the origin");
    let squares = [(0, 0), (country.0.div_euclid(16), country.1.div_euclid(16))];
    let mut spent = [[0f64; 2]; 2];
    for _round in 0..5 {
        for (index, scale) in [Scale::Earth, Scale::Landforms].into_iter().enumerate() {
            for (which, &(cx, cz)) in squares.iter().enumerate() {
                let elapsed = std::thread::spawn(move || {
                    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, scale);
                    let clock = Instant::now();
                    for dz in 0..12 {
                        for dx in 0..12 {
                            std::hint::black_box(gen.generate_chunk(ChunkPos::new(cx + dx, cz + dz)));
                        }
                    }
                    clock.elapsed().as_secs_f64()
                })
                .join()
                .expect("a timing thread");
                spent[index][which] += elapsed;
            }
        }
    }
    for (index, name) in ["earth", "landforms"].iter().enumerate() {
        println!(
            "{name:>9}: {:.2} ms a chunk round the origin, {:.2} ms in hill country",
            spent[index][0] * 1000.0 / (5.0 * 144.0),
            spent[index][1] * 1000.0 / (5.0 * 144.0)
        );
    }
}
