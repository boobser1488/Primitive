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
    fingerprint_as(gen, pos, |id| id)
}

/// The same, of every block as `seen` reads it.
fn fingerprint_as(gen: &WorldGen, pos: ChunkPos, seen: impl Fn(BlockId) -> BlockId) -> u64 {
    let chunk = gen.generate_chunk(pos);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for y in 0..CHUNK_SIZE_Y {
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                for byte in u32::from(seen(chunk.get(x, y, z))).to_le_bytes() {
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

/// **The landforms draw the ground they drew before they were made cheaper**:
/// four chunks round the origin and four in the hill country of
/// `what_the_landforms_cost_a_chunk`, to the block.
///
/// The prints were taken from the generator as it was before `slow`
/// remembered its last column and the courses were turned once
/// (`drain_courses`) -- both changes are the same arithmetic done fewer
/// times, and this is what says so. A change that means to move the ground
/// changes these on purpose and says why.
///
/// **Read with every lip made whole again** (`dig::whole`): the lips on the
/// slopes (`lips`) are the one change made to the landforms' ground on
/// purpose since, and holding the rest of the chunk to these prints is what
/// says they are the *only* change -- a lip lowers a block of the ground
/// that was there and touches nothing else. What the lips themselves are is
/// held by `the_landforms_lay_the_same_lips_every_time`.
///
/// **...and with every feature on a slope on the whole block it was laid on**
/// (`lips::FEATURES_KEEP_THEIR_STEP`): where a lip meets a trunk, a boulder
/// or a bush it is that thing's foot and not a lowered block, which
/// `dig::whole` cannot read back -- and with that turned off, the rest of
/// the chunk is still to the block what it was.
/// **Read again with the beds of the deep rock in them** (`bedded_rock`)
/// and with dripstone grown in columns out of the carbonates only. Both
/// move the new world's ground on purpose: the deep rock was one grey block
/// at every depth, and a cave wall now shows the bands it is cut through.
/// The old scales' prints above did not move, which is what says the change
/// belongs to the new generator alone.
///
/// **All eight taken again when the country got its grain** and the beds of
/// the deep rock stopped being one thickness (`landforms::GRAIN_HEIGHT`,
/// `Beds::at`). Both move the new world's ground on purpose and neither
/// moves an older world's: the Regional and Earth prints above are the same
/// numbers they were, which is what says so.
///
/// **Three of the eight taken again when the clay became a bed** -- the
/// alluvial sheet under a floodplain's soil and the parting on top of the
/// sandstone (`WorldGen::clay_bed`, `Beds::at`). The five that did not move
/// are chunks with neither in them, and that is the other half of the
/// assertion: both are *somewhere*, not everywhere -- the deep field turns
/// over in about a hundred and fifty blocks, which is nine chunks, so a
/// chunk is wholly in a lens district or wholly out of one. The Regional
/// and Earth prints above did not move at all.
///
/// **Six of the eight taken again when the ground started telling a newcomer
/// where to go** (`landforms`, "What the ground tells a newcomer"): shingle
/// and flint on the banks of fresh water, bare paths worn to a pond, talus
/// round a cave mouth, copper showing in a steep hillside. The two that did
/// not move are chunks with none of those things in them, which is the other
/// half of the assertion -- the tells are *somewhere*, not everywhere. The
/// Regional and Earth prints above did not move at all.
#[test]
fn the_landforms_draw_the_ground_they_drew_before_they_were_made_cheaper() {
    let held: [((i32, i32), u64); 8] = [
        ((0, 0), 0xcdc1_48db_f291_184b),
        ((5, -3), 0x9faf_666e_2823_b1eb),
        ((-40, 90), 0x844e_9b4e_9783_f206),
        ((313, -77), 0xdffd_008f_e7cf_d068),
        ((-1875, -1875), 0x42b3_115c_25ea_d3e8),
        ((-1868, -1872), 0xa84e_8dee_614a_b222),
        ((-1864, -1864), 0xba6b_a426_448c_80a1),
        ((-1873, -1866), 0x5257_7fdd_6bfb_50a1),
    ];
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    super::lips::FEATURES_KEEP_THEIR_STEP.with(|keep| keep.set(true));
    for ((x, z), print) in held {
        assert_eq!(
            fingerprint_as(&gen, ChunkPos::new(x, z), crate::dig::whole),
            print,
            "landforms chunk {x},{z} is not the ground it was"
        );
    }
}

/// **The lips are laid the same every time**: the chunks above, as they are
/// with their lips, to the block. A lip is a function of the seed and the
/// heights round a column and of nothing a thread remembers, so a chunk
/// evicted and made again, or made by another thread, or made after its
/// neighbours rather than before, has the same slopes.
///
/// Two of the four were taken again when the clay became a bed: a lip is
/// the top block of a column and the clay is three below it, so what moved
/// there is the ground the lip was cut from and not the lip.
///
/// Three of the prints were taken again when the lips were let under the
/// trees, the boulders, the bushes and the stones lying on the slopes
/// (`lips`, "What stands on a lip"): that is the change they hold, and
/// `the_landforms_draw_the_ground_they_drew_before_they_were_made_cheaper`
/// says it is the only one. Round the origin nothing stood on a lip.
#[test]
fn the_landforms_lay_the_same_lips_every_time() {
    let held: [((i32, i32), u64); 4] = [
        // Taken again with the rest when the country got its grain
        // (`landforms::GRAIN_HEIGHT`): a slope with a grain in it is a slope
        // with other lips on it, which is most of the point.
        //
        // ...and again when the ground started telling a newcomer where to
        // go (`landforms`, "What the ground tells a newcomer"): shingle at
        // fresh water, bare paths to a pond, talus round a cave mouth and
        // copper in a hillside all write into cells a lip then reads.
        ((0, 0), 0x0fe5_0f0f_2c10_d0db),
        ((5, -3), 0xfe4f_9277_6a8d_ecdb),
        ((-1875, -1875), 0xd3ef_f6d9_0e79_0cc5),
        ((-1868, -1872), 0xe5b7_6067_47dc_6ff2),
    ];
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    for ((x, z), print) in held {
        assert_eq!(fingerprint(&gen, ChunkPos::new(x, z)), print, "landforms chunk {x},{z} laid other lips");
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
    // ...and the landforms' own two sets, which a change that means to move
    // the new world's ground has to write down again.
    // The first as their test reads them, with the features on the whole
    // block they were laid on (`lips::FEATURES_KEEP_THEIR_STEP`); printed
    // without it they were prints of another ground, and a change taken
    // again from them failed the test it was taken for.
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    super::lips::FEATURES_KEEP_THEIR_STEP.with(|keep| keep.set(true));
    for (x, z) in LANDFORM_CHUNKS {
        println!("((({x}, {z})), {:#018x}),", fingerprint_as(&gen, ChunkPos::new(x, z), crate::dig::whole));
    }
    super::lips::FEATURES_KEEP_THEIR_STEP.with(|keep| keep.set(false));
    for (x, z) in LANDFORM_CHUNKS.iter().take(2).chain(LANDFORM_CHUNKS[4..6].iter()) {
        println!("lips (({x}, {z})), {:#018x},", fingerprint(&gen, ChunkPos::new(*x, *z)));
    }
}

/// The eight chunks the landforms' golden prints are taken over.
const LANDFORM_CHUNKS: [(i32, i32); 8] =
    [(0, 0), (5, -3), (-40, 90), (313, -77), (-1875, -1875), (-1868, -1872), (-1864, -1864), (-1873, -1866)];

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
    // ...and the landforms once more with their lips off (`lips::LIPS_OFF`),
    // which is what the lips cost, measured in the same binary -- and once
    // with every feature on a slope keeping its whole block
    // (`lips::FEATURES_KEEP_THEIR_STEP`), which is what letting the lips
    // under them costs.
    // ...and once more with the clay's two rules off (`worldgen::CLAY_OFF`),
    // which is what the alluvial bed and the deep parting cost, measured in
    // the same binary as the ground with them in it.
    let mut spent = [[0f64; 2]; 5];
    for _round in 0..5 {
        for (index, (scale, lips, stepped, clay)) in [
            (Scale::Earth, true, false, true),
            (Scale::Landforms, true, false, true),
            (Scale::Landforms, false, false, true),
            (Scale::Landforms, true, true, true),
            (Scale::Landforms, true, false, false),
        ]
        .into_iter()
        .enumerate()
        {
            for (which, &(cx, cz)) in squares.iter().enumerate() {
                let elapsed = std::thread::spawn(move || {
                    super::lips::LIPS_OFF.with(|off| off.set(!lips));
                    super::lips::FEATURES_KEEP_THEIR_STEP.with(|keep| keep.set(stepped));
                    super::CLAY_OFF.with(|off| off.set(!clay));
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
    for (index, name) in ["earth", "landforms", "no lips", "stepped", "no clay"].iter().enumerate() {
        println!(
            "{name:>9}: {:.2} ms a chunk round the origin, {:.2} ms in hill country",
            spent[index][0] * 1000.0 / (5.0 * 144.0),
            spent[index][1] * 1000.0 / (5.0 * 144.0)
        );
    }
}

/// How banded a patch of ground is, and how much grain it has: three numbers
/// off a square `2 * reach + 1` across, dry land only.
///
/// The ground is a height field rounded to whole blocks, so every slope is a
/// set of contour bands, and how those bands read is the whole of what the
/// player called "слои ландшафта".
///
/// * **The shelf share** -- the columns whose four neighbours all stand at
///   their own height. That is the inside of a band; on a field smooth enough
///   it is nearly all of the ground, and a hillside of wide flat steps is
///   what that looks like from the ground.
/// * **The band** -- the mean length of a run of one height along an
///   east-west line, in blocks. A band four wide is a ramp (`lips` puts three
///   quarter steps in it); a band ten wide is a terrace with a rim.
/// * **The grain** -- the root mean square of a column against the mean of
///   its four neighbours four blocks off. A plane has none of it and a dome
///   next to none: only detail at a few blocks' wavelength shows here, which
///   is the thing that breaks a contour line into a coast instead of a curve.
fn banding_round(gen: &WorldGen, gx: i32, gz: i32, reach: i32) -> Option<Banding> {
    const WAYS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let at = |x: i32, z: i32| gen.height_at(x, z);
    let (mut shelves, mut counted, mut runs, mut grain) = (0u32, 0u32, 0u32, 0f64);
    for dz in -reach..=reach {
        let mut last: Option<i32> = None;
        for dx in -reach..=reach {
            let (x, z) = (gx + dx, gz + dz);
            let h = at(x, z);
            // Dry ground only: a channel is cut level at the sea wherever it
            // runs, so a band across one is the water's and not the country's.
            if h <= SEA_LEVEL + 1 {
                last = None;
                continue;
            }
            counted += 1;
            shelves += u32::from(WAYS.iter().all(|&(ox, oz)| at(x + ox, z + oz) == h));
            if last != Some(h) {
                runs += 1;
            }
            last = Some(h);
            let far: i32 = WAYS.iter().map(|&(ox, oz)| at(x + ox * 4, z + oz * 4)).sum();
            let curve = f64::from(h) - f64::from(far) / 4.0;
            grain += curve * curve;
        }
    }
    (counted >= 400).then(|| {
        (
            f64::from(shelves) / f64::from(counted),
            f64::from(counted) / f64::from(runs.max(1)),
            (grain / f64::from(counted)).sqrt(),
        )
    })
}

/// The same three numbers per biome, over the first `patches` columns of each
/// biome found in a coarse sweep of a world, a square 49 across at each.
fn banding_by_biome(gen: &WorldGen, want: &[Biome], patches: usize) -> Vec<(Biome, Banding, usize)> {
    want.iter().map(|&biome| biome_banding(gen, biome, patches).0).collect()
}

/// A patch's shelf share, mean band in blocks and grain. See `banding_round`.
type Banding = (f64, f64, f64);

/// A biome's three numbers over the patches measured, with how many there
/// were -- and where each patch was and what its band came out as.
type BiomeBanding = ((Biome, Banding, usize), Vec<(i32, i32, f64)>);

/// The same, and where each patch was: a probe that will not say where it
/// looked cannot be looked at.
fn biome_banding(gen: &WorldGen, biome: Biome, patches: usize) -> BiomeBanding {
    let (mut sum, mut where_) = ((0.0, 0.0, 0.0), Vec::new());
    for k in 0..1_600i32 {
        if where_.len() >= patches {
            break;
        }
        let (gx, gz) = ((k % 40) * 700 - 14_000, (k / 40) * 700 - 14_000);
        if gen.biome_at(gx, gz) != biome {
            continue;
        }
        if let Some((shelf, band, grain)) = banding_round(gen, gx, gz, 24) {
            sum = (sum.0 + shelf, sum.1 + band, sum.2 + grain);
            where_.push((gx, gz, band));
        }
    }
    let d = where_.len().max(1) as f64;
    ((biome, (sum.0 / d, sum.1 / d, sum.2 / d), where_.len()), where_)
}

/// ```text
/// cargo test -p primitive_shared --lib -- --ignored --nocapture probe_banding
/// ```
#[test]
#[ignore = "a measurement, not an assertion"]
fn probe_banding() {
    for seed in [1337u32, 99] {
        let gen = WorldGen::new(seed);
        println!("seed {seed}");
        for want in [Biome::Hills, Biome::Steppe, Biome::Plains, Biome::Forest] {
            let ((biome, (shelf, band, grain), n), patches) = biome_banding(&gen, want, 5);
            println!(
                "  {:>13} ({n} patches)  shelf {:5.1}%  band {band:5.2}  grain {grain:5.3}",
                biome.name(),
                shelf * 100.0
            );
            for (gx, gz, band) in patches {
                println!("      at {gx:>7},{gz:>7}  band {band:5.2}");
            }
        }
    }
}

/// **Open country is not a flight of terraces.** The ground is a height field
/// rounded to whole blocks and the bands that rounding leaves are what a
/// player reads a hillside by -- the complaint that began this was "слои
/// ландшафта странные и слишком гладкие", and it was about these bands.
///
/// What is asserted is the thing that was wrong, measured
/// (`banding_round`): a band that ran **twenty-two blocks** along a line in a
/// steppe and eighteen in a forest, with four columns in five having all
/// their neighbours at their own height. A country like that has one scale
/// of shape in it and the other is the rounding.
///
/// Each bound sits between what was measured before the grain
/// (`landforms::GRAIN_HEIGHT`) and what is measured after it, with the
/// margin on the side of the ground that is there now: the widest band
/// before was 21.9 and the widest after is 10.9; the fullest shelf share
/// before was 88.8 per cent and after 65.2; the least grain before was 0.23
/// and the least after 0.46.
#[test]
fn open_country_is_not_a_flight_of_terraces() {
    for seed in [1337u32, 99] {
        let gen = WorldGen::new(seed);
        for (biome, (shelf, band, grain), n) in
            banding_by_biome(&gen, &[Biome::Hills, Biome::Steppe, Biome::Plains, Biome::Forest], 5)
        {
            let name = biome.name();
            assert!(n >= 3, "seed {seed}: only {n} patches of {name} to measure");
            assert!(band < 13.0, "seed {seed}: a band of {name} runs {band:.1} blocks -- a terrace, not a slope");
            assert!(shelf < 0.72, "seed {seed}: {:.0}% of a {name} has every neighbour at its own height", shelf * 100.0);
            // ...and the ground has detail at a few blocks' wavelength at
            // all, which is what a smooth field of any amplitude has none
            // of, whatever its bands come out as.
            assert!(grain > 0.42, "seed {seed}: a {name} has a grain of {grain:.2} -- a smooth field, whatever its size");
        }
    }
}

/// The thickness of each bed of the deep rock over a sweep of columns.
///
/// ```text
/// cargo test -p primitive_shared --lib -- --ignored --nocapture probe_beds
/// ```
#[test]
#[ignore = "a measurement, not an assertion"]
fn probe_beds() {
    let gen = WorldGen::new(1337);
    let mut thick: [Vec<i32>; 3] = Default::default();
    for k in 0..900i32 {
        let (gx, gz) = ((k % 30) * 53 - 800, (k / 30) * 61 - 800);
        let beds = gen.beds_at(gx, gz, gen.stratum(gx, gz, Biome::Plains, Biome::Plains.surface()).2);
        for (which, rock) in [BLOCK_GRANITE, BLOCK_SANDSTONE, BLOCK_LIMESTONE].into_iter().enumerate() {
            let n = (BEDROCK_TOP..SEA_LEVEL).filter(|&y| WorldGen::bedded_rock(y, beds) == rock).count();
            thick[which].push(n as i32);
        }
    }
    for (which, name) in ["granite", "sandstone", "limestone"].iter().enumerate() {
        let v = &thick[which];
        let mean = f64::from(v.iter().sum::<i32>()) / v.len() as f64;
        let sd = (v.iter().map(|&t| (f64::from(t) - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        println!("{name:>10}: {:>3}..{:<3} mean {mean:5.2} sd {sd:4.2}", v.iter().min().unwrap(), v.iter().max().unwrap());
    }
}
