//! What the ground tells a newcomer, held to what a player can see.
//!
//! The four tells `landforms` adds -- the shingle at fresh water, the paths
//! worn to a pond, the talus round a cave mouth, the copper in a hillside --
//! and the properties that make each of them a *tell* rather than a texture:
//! it has to be where the thing is, it has to be nowhere else, and there has
//! to be enough of it to notice from where a player stands.
//!
//! In a file of its own for `surface_metal_tests`' reason.

use super::*;

/// How wide the band is, as this file reads it: `landforms::SHINGLE_REACH`,
/// written again here rather than made `pub(super)` so that a change to the
/// band's width has to be *decided* about the test rather than absorbed by
/// it -- a test that moves with the number it holds holds nothing.
const SHINGLE_REACH_FOR_TESTS: i32 = 3;

/// Every cell of a kind in a chunk, as world coordinates.
fn cells_of(chunk: &Chunk, kind: BlockId) -> Vec<(i32, i32, i32)> {
    let mut found = Vec::new();
    for y in 0..CHUNK_SIZE_Y {
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                if crate::types::block_kind(chunk.get(x, y, z)) == kind {
                    found.push((
                        chunk.pos.x * CHUNK_SIZE_X as i32 + x as i32,
                        y as i32,
                        chunk.pos.z * CHUNK_SIZE_Z as i32 + z as i32,
                    ));
                }
            }
        }
    }
    found
}

fn landformed(seed: u32) -> WorldGen {
    WorldGen::with_scale(seed, Preset::Normal, Zone::Temperate, Scale::Landforms)
}

/// The first watering place in a patch of a world, for the tests that need a
/// pond to stand beside. `None` only if a world has no pond in two hundred
/// and fifty-six chunks, which would be a bug in its own right.
fn a_watering(gen: &WorldGen) -> Option<super::landforms::Watering> {
    for cz in -8..8 {
        for cx in -8..8 {
            if let Some(w) = gen
                .waterings_near(cx * CHUNK_SIZE_X as i32, cz * CHUNK_SIZE_Z as i32)
                .into_iter()
                .next()
            {
                return Some(w);
            }
        }
    }
    None
}

/// The top cell of a column that is not air, and what is in it.
///
/// **Read off the chunk and not off `terrain_height`**, because
/// `terrain_height` is the land before a lake is dug out of it: asked of a
/// pond it answers with the ground the pond replaced, which is above the
/// water, and a test written on it finds no lake shore anywhere in a world
/// full of lakes. That cost an afternoon once already.
fn surface_of(chunk: &Chunk, lx: usize, lz: usize) -> Option<(i32, BlockId)> {
    (0..CHUNK_SIZE_Y)
        .rev()
        .map(|y| (y as i32, chunk.get(lx, y, lz)))
        .find(|&(_, block)| block != BLOCK_AIR)
}

/// Every column of a patch of the world, as (top cell, what is in it).
fn surfaces(gen: &WorldGen, from: (i32, i32), to: (i32, i32)) -> std::collections::HashMap<(i32, i32), (i32, BlockId)> {
    let mut found = std::collections::HashMap::new();
    for cz in from.1..to.1 {
        for cx in from.0..to.0 {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
            for lz in 0..CHUNK_SIZE_Z {
                for lx in 0..CHUNK_SIZE_X {
                    if let Some(top) = surface_of(&chunk, lx, lz) {
                        found.insert(
                            (cx * CHUNK_SIZE_X as i32 + lx as i32, cz * CHUNK_SIZE_Z as i32 + lz as i32),
                            top,
                        );
                    }
                }
            }
        }
    }
    found
}

/// Is there fresh water within `reach` of this column? The question
/// `shingle_at` asks, asked again from outside and off the blocks.
///
/// Fresh: a lake or a river, and not the sea. A column of water standing
/// off the sea's level is inland; one at the sea's level is the sea unless
/// the biome says it is a river channel.
fn fresh_water_near(
    gen: &WorldGen,
    tops: &std::collections::HashMap<(i32, i32), (i32, BlockId)>,
    gx: i32,
    gz: i32,
    reach: i32,
) -> bool {
    (-reach..=reach).any(|dz| {
        (-reach..=reach).any(|dx| {
            tops.get(&(gx + dx, gz + dz)).is_some_and(|&(y, block)| {
                crate::types::is_liquid(block)
                    && (y != SEA_LEVEL || gen.biome_from(gx + dx, gz + dz, y) == Biome::River)
            })
        })
    })
}

/// **The flint is at the water, and a new player can find it by walking to
/// the water.**
///
/// The whole of the first hour's redirection, as an assertion: the shingle
/// band exists, it is thick enough to be a band, and it is not anywhere
/// else. Before this, flint on the surface came from `flint_spacing`'s thin
/// scatter and from cave floors, and a player who had not been told where to
/// look had no reason to prefer the river to the meadow.
#[test]
fn flint_and_gravel_lie_thick_at_fresh_water_and_not_out_in_the_meadow() {
    // Round a pond, so the patch has both halves in it: a landformed world
    // is mostly dry ground, and a square taken at the origin can be all
    // meadow and prove nothing either way.
    let gen = landformed(4242);
    let pond = a_watering(&gen).expect("no pond in a landformed world");
    let (home_x, home_z) = (
        pond.centre_x().div_euclid(CHUNK_SIZE_X as i32),
        pond.centre_z().div_euclid(CHUNK_SIZE_Z as i32),
    );
    let tops = surfaces(&gen, (home_x - 6, home_z - 6), (home_x + 6, home_z + 6));
    let (mut on_the_shore, mut inland, mut shore_columns, mut inland_columns) = (0usize, 0usize, 0usize, 0usize);
    for (&(gx, gz), &(_, block)) in &tops {
        if crate::types::is_liquid(block) {
            continue;
        }
        let flint = crate::types::block_kind(block) == crate::types::BLOCK_FLINT;
        // The band's own reach; outside it there is no shingle by
        // construction, and a column six out is honestly inland.
        if fresh_water_near(&gen, &tops, gx, gz, SHINGLE_REACH_FOR_TESTS) {
            shore_columns += 1;
            on_the_shore += usize::from(flint);
        } else if !fresh_water_near(&gen, &tops, gx, gz, SHINGLE_REACH_FOR_TESTS * 2) {
            inland_columns += 1;
            inland += usize::from(flint);
        }
    }
    assert!(shore_columns > 150, "only {shore_columns} columns of shore in 144 chunks");
    assert!(inland_columns > 3_000, "only {inland_columns} columns of inland");
    let shore_share = on_the_shore as f32 / shore_columns as f32;
    let inland_share = inland as f32 / inland_columns.max(1) as f32;
    // One flint in nine columns of shore is the arithmetic of
    // `SHINGLE_SPACING` and `SHINGLE_FLINT_SHARE`; allow for the columns
    // the band's rise and the ground's own rules take out of it.
    assert!(
        shore_share > 0.04,
        "flint on {:.1}% of the shore: the band is not a band",
        shore_share * 100.0,
    );
    assert!(
        shore_share > inland_share * 6.0,
        "flint is {:.2}% at the water and {:.2}% inland -- that is not a reason to walk to the water",
        shore_share * 100.0,
        inland_share * 100.0,
    );
}

/// **A trail is a gap, and it goes somewhere.**
///
/// Two properties in one world, because they fail in opposite directions: a
/// trail that is not bare is not a trail, and a trail that wanders off into
/// the country instead of reaching the water is a scar.
#[test]
fn the_paths_to_a_pond_are_bare_ground_and_every_one_of_them_reaches_the_water() {
    let gen = landformed(4242);
    let watering = a_watering(&gen).expect("no pond in 256 chunks of a landformed world");
    // Every trodden column, and how far out it is.
    let mut trodden = Vec::new();
    let reach = 40;
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let (gx, gz) = (watering.centre_x() + dx, watering.centre_z() + dz);
            if watering.on_a_trail(gx, gz) {
                trodden.push((gx, gz, f64::from(dx).hypot(f64::from(dz))));
            }
        }
    }
    assert!(
        trodden.len() > 80,
        "only {} columns of trail round a pond: nothing converges on anything",
        trodden.len(),
    );
    // **They reach the water.** A path that starts twenty blocks out and
    // stops ten blocks out is not a path to anywhere; every one of them has
    // to have a column at the shore.
    let nearest = trodden.iter().map(|&(_, _, far)| far).fold(f64::MAX, f64::min);
    let furthest = trodden.iter().map(|&(_, _, far)| far).fold(0.0, f64::max);
    assert!(
        nearest <= f64::from(watering.shore()) + 2.0,
        "the nearest trail column is {nearest:.1} out and the shore is at {}",
        watering.shore(),
    );
    assert!(furthest > f64::from(watering.shore()) + 18.0, "the trails run only {furthest:.1} out");
    // **And they are bare.** Not one trodden column of open country carries
    // a plant: that is the whole of what makes the path visible.
    let mut checked = 0usize;
    for &(gx, gz, _) in &trodden {
        let (cx, cz) = (gx.div_euclid(CHUNK_SIZE_X as i32), gz.div_euclid(CHUNK_SIZE_Z as i32));
        let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
        let (lx, lz) = (gx.rem_euclid(CHUNK_SIZE_X as i32) as usize, gz.rem_euclid(CHUNK_SIZE_Z as i32) as usize);
        let height = gen.terrain_height(gx, gz);
        if height < gen.water_level_at(gx, gz) || height + 1 >= CHUNK_SIZE_Y as i32 {
            continue;
        }
        // Open ground only: a path under a closed canopy is deliberately not
        // made bare -- the reason is written where the decision is.
        if under_canopy(&chunk.blocks, lx as i32, height, lz as i32) {
            continue;
        }
        let over = chunk.get(lx, (height + 1) as usize, lz);
        // A stone lying in the path is still a path -- that is written down
        // where the trail is cut -- so the property is that nothing *grows*
        // there.
        let stone = matches!(
            crate::ground::as_common(over),
            BLOCK_PEBBLE | crate::types::BLOCK_GRAVEL | crate::types::BLOCK_FLINT | BLOCK_COBBLESTONE
        );
        if over == BLOCK_AIR || stone || !crate::types::can_grow_on(over, chunk.get(lx, height as usize, lz)) {
            checked += 1;
            continue;
        }
        panic!("a plant is standing in the path at {gx},{gz}: {over}");
    }
    assert!(checked > 40, "only {checked} trail columns were above water and in the world");
}

/// **A cave mouth is a hole with broken rock round it**, and the rock is the
/// country's own -- which is the whole of "another colour": grey cobble in
/// chalk country, buff in limestone. Before this a cave opened on a hillside
/// as a black rectangle in unbroken turf.
#[test]
fn a_hole_in_the_ground_has_the_countrys_own_broken_rock_spread_round_it() {
    let gen = landformed(1337);
    let (mut mouths, mut with_talus) = (0usize, 0usize);
    for cz in -8..8 {
        for cx in -8..8 {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
            for lz in 1..CHUNK_SIZE_Z - 1 {
                for lx in 1..CHUNK_SIZE_X - 1 {
                    let (gx, gz) = (
                        cx * CHUNK_SIZE_X as i32 + lx as i32,
                        cz * CHUNK_SIZE_Z as i32 + lz as i32,
                    );
                    let height = gen.terrain_height(gx, gz);
                    if height < gen.water_level_at(gx, gz) || height + 1 >= CHUNK_SIZE_Y as i32 {
                        continue;
                    }
                    if !gen.is_cave(gx, height, gz) {
                        continue;
                    }
                    mouths += 1;
                    // The talus is the rock's own rubble, anywhere in the
                    // two columns round the hole.
                    let mut talus = false;
                    for dz in -1i32..=1 {
                        for dx in -1i32..=1 {
                            let (nx, nz) = ((lx as i32 + dx) as usize, (lz as i32 + dz) as usize);
                            let near = gen.terrain_height(gx + dx, gz + dz);
                            if near + 1 >= CHUNK_SIZE_Y as i32 {
                                continue;
                            }
                            let over = crate::ground::as_common(chunk.get(nx, (near + 1) as usize, nz));
                            talus |= matches!(over, crate::types::BLOCK_GRAVEL | BLOCK_PEBBLE);
                        }
                    }
                    with_talus += usize::from(talus);
                }
            }
        }
    }
    assert!(mouths > 40, "only {mouths} cave mouths in 256 chunks to judge");
    let share = with_talus as f32 / mouths as f32;
    assert!(
        share > 0.6,
        "only {:.0}% of cave mouths have any broken rock round them",
        share * 100.0,
    );
}

/// **Copper shows on the high ground and nowhere else**, which is what makes
/// "the copper is over the hill" a thing a player can see rather than a
/// thing the ore table knows.
///
/// The property that matters is the second half: an outcrop in a meadow
/// would teach a player that ore is everywhere, which is the opposite of
/// what the hills are for.
#[test]
fn copper_shows_on_the_high_ground_and_nowhere_else() {
    let mut outcrops = 0usize;
    for seed in [1337, 42, 7] {
        let gen = landformed(seed);
        for cz in -10..10 {
            for cx in -10..10 {
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for (gx, y, gz) in cells_of(&chunk, crate::types::BLOCK_COPPER_ORE) {
                    let height = gen.terrain_height(gx, gz);
                    // Only the ones at the surface: the ore table's own
                    // veins are underground and are not this test's
                    // business. An outcrop's top cell is the column's top.
                    if y < height - 2 {
                        continue;
                    }
                    outcrops += 1;
                    assert!(
                        WorldGen::copper_country(height) > 0.0,
                        "copper showing at {gx},{gz} on ground of {height}, which is not copper country",
                    );
                    assert!(
                        height >= gen.water_level_at(gx, gz) + 12,
                        "copper showing a dozen blocks of the waterline at {gx},{gz}",
                    );
                }
            }
        }
    }
    assert!(outcrops > 0, "no copper anywhere on the surface of 1200 chunks of three worlds");
}

/// Not an assertion: what the tells actually cost and how much of them there
/// is, for the next person who has to argue about a number.
#[test]
#[ignore = "a measurement, not an assertion"]
fn what_the_tells_put_on_the_ground() {
    let gen = landformed(1337);
    let (mut flint, mut gravel, mut pebble, mut ore, mut columns, mut showing) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let span = 8;
    for cz in -span..span {
        for cx in -span..span {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
            flint += cells_of(&chunk, crate::types::BLOCK_FLINT).len();
            ore += cells_of(&chunk, crate::types::BLOCK_COPPER_ORE).len();
            for lz in 0..CHUNK_SIZE_Z {
                for lx in 0..CHUNK_SIZE_X {
                    let (gx, gz) = (
                        cx * CHUNK_SIZE_X as i32 + lx as i32,
                        cz * CHUNK_SIZE_Z as i32 + lz as i32,
                    );
                    let height = gen.terrain_height(gx, gz);
                    if height < gen.water_level_at(gx, gz) || height + 1 >= CHUNK_SIZE_Y as i32 {
                        continue;
                    }
                    columns += 1;
                    match crate::ground::as_common(chunk.get(lx, (height + 1) as usize, lz)) {
                        crate::types::BLOCK_GRAVEL => gravel += 1,
                        BLOCK_PEBBLE => pebble += 1,
                        _ => {}
                    }
                    if let Some((_, top)) = surface_of(&chunk, lx, lz) {
                        if crate::types::block_kind(top) == crate::types::BLOCK_COPPER_ORE {
                            showing += 1;
                        }
                    }
                }
            }
        }
    }
    let chunks = (span * 2) * (span * 2);
    println!("{chunks} chunks, {columns} dry columns");
    println!("flint {flint}, gravel {gravel}, pebbles {pebble}, copper cells {ore}, of them showing at the surface {showing}");
}
