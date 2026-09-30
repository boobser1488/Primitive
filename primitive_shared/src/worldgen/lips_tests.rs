use super::lips::takes_a_lip;
use super::*;
use crate::dig;
use crate::geometry::PLAYER_STEP_HEIGHT;
use crate::types::{block_kind, collision_height, is_collidable, is_liquid, BLOCK_GRASS};

/// Chunks of the hill country of seed 1337 that `what_the_landforms_cost_a_
/// chunk` times and the golden prints hold: downs, which is where a gentle
/// slope is most of the ground.
const HILLS: (i32, i32) = (-1875, -1875);
const SPAN: i32 = 6;

/// What a walker meets at the top of a column: the whole-block height, the
/// height of the surface they stand on, and the block that is it. `None`
/// for a column whose top is not bare ground a lip could be -- a tree, a
/// stone, water.
#[derive(Clone, Copy)]
struct Top {
    y: i32,
    surface: f32,
    block: BlockId,
    above: BlockId,
}

fn tops(gen: &WorldGen) -> (i32, i32, i32, Vec<Option<Top>>) {
    tops_at(gen, HILLS, SPAN)
}

/// `tops`, over `span` chunks square from `corner`.
fn tops_at(gen: &WorldGen, corner: (i32, i32), span: i32) -> (i32, i32, i32, Vec<Option<Top>>) {
    let side = span * CHUNK_SIZE_X as i32;
    let (ox, oz) = (corner.0 * CHUNK_SIZE_X as i32, corner.1 * CHUNK_SIZE_Z as i32);
    let mut tops = vec![None; (side * side) as usize];
    for cz in 0..span {
        for cx in 0..span {
            let chunk = gen.generate_chunk(ChunkPos::new(corner.0 + cx, corner.1 + cz));
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let top = (0..CHUNK_SIZE_Y).rev().find(|&y| is_collidable(chunk.get(x, y, z)) || is_liquid(chunk.get(x, y, z)));
                    let Some(y) = top else { continue };
                    let block = chunk.get(x, y, z);
                    let above = chunk.get(x, y + 1, z);
                    // Bare ground: nothing over it but air or a plant, which
                    // is what a walker crosses. What stands on the slopes is
                    // looked at by `features_on_the_downs`.
                    if !takes_a_lip(dig::whole(block)) || !(above == crate::types::BLOCK_AIR || crate::types::is_cross(above)) {
                        continue;
                    }
                    let (gx, gz) = (cx * CHUNK_SIZE_X as i32 + x as i32, cz * CHUNK_SIZE_Z as i32 + z as i32);
                    // The height field's own ground, not a cave's mouth or a
                    // hollow something dug into it after: those are holes,
                    // and a hole's edge is not a slope.
                    if gen.height_at(ox + gx, oz + gz) != y as i32 {
                        continue;
                    }
                    tops[(gz * side + gx) as usize] =
                        Some(Top { y: y as i32, surface: y as f32 + collision_height(block), block, above: chunk.get(x, y + 1, z) });
                }
            }
        }
    }
    (ox, oz, side, tops)
}

/// Every one-block rise of the height field between two bare columns, as
/// (the surface step a walker climbs, whether the terrace above is at least
/// two wide along the way -- a slope of a half or gentler).
fn rises(gen: &WorldGen) -> Vec<(f32, bool, bool)> {
    let (_, _, side, tops) = tops(gen);
    let at = |x: i32, z: i32| {
        if x < 0 || z < 0 || x >= side || z >= side {
            None
        } else {
            tops[(z * side + x) as usize]
        }
    };
    let mut rises = Vec::new();
    for z in 0..side {
        for x in 0..side {
            let Some(low) = at(x, z) else { continue };
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let Some(high) = at(x + dx, z + dz) else { continue };
                if high.y != low.y + 1 {
                    continue;
                }
                let gentle = at(x + 2 * dx, z + 2 * dz).is_some_and(|beyond| beyond.y == high.y);
                // A straight stretch of the slope: the contour runs square
                // across the way up, on both sides of the rise.
                let straight = [(dz, dx), (-dz, -dx)].iter().all(|&(px, pz)| {
                    at(x + px, z + pz).is_some_and(|t| t.y == low.y)
                        && at(x + dx + px, z + dz + pz).is_some_and(|t| t.y == high.y)
                });
                rises.push((high.surface - low.surface, gentle, gentle && straight));
            }
        }
    }
    rises
}

/// **A gentle generated slope has no whole-block step a player must jump.**
/// Every one-block rise of the downs' height field with a terrace at least
/// two wide above it -- a slope of a half or gentler -- is climbed in steps
/// no taller than a body steps up without a jump, wherever the contour runs
/// straight across the way up. Where two rises meet at a corner a column
/// cannot be a ramp both ways, and one rise in fifty is allowed to stay a
/// block (measured: twelve of 1861). Without the lips every one of them is
/// a block.
#[test]
fn a_gentle_generated_slope_has_no_whole_block_step_a_player_must_jump() {
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    let all = rises(&gen);
    let gentle: Vec<_> = all.iter().filter(|r| r.1).collect();
    let straight: Vec<_> = all.iter().filter(|r| r.2).collect();
    assert!(
        gentle.len() > 1000 && straight.len() > 100,
        "the downs have {} gentle rises, {} straight",
        gentle.len(),
        straight.len()
    );
    let jumps = |set: &[&(f32, bool, bool)]| set.iter().filter(|r| r.0 > PLAYER_STEP_HEIGHT).count();
    assert_eq!(jumps(&straight), 0, "a straight gentle slope still has a step to jump");
    assert!(
        jumps(&gentle) * 50 <= gentle.len(),
        "{} of {} gentle rises still have to be jumped",
        jumps(&gentle),
        gentle.len()
    );
    // ...and without the lips, every one of them did.
    super::lips::LIPS_OFF.with(|off| off.set(true));
    let before = rises(&gen);
    super::lips::LIPS_OFF.with(|off| off.set(false));
    assert!(before.iter().filter(|r| r.1).all(|r| r.0 >= 1.0), "a rise was walkable before the lips");
}

/// **The lips of a meadow are turf** -- grass on top, what the ground round
/// them is -- and never the bare earth a spade leaves. "Учти голую землю": a
/// lip laid as dirt would draw every rise of every meadow as a brown stripe.
#[test]
fn the_lips_of_a_meadow_slope_are_grass_on_top() {
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    let (_, _, _, tops) = tops(&gen);
    let lips: Vec<Top> = tops.iter().flatten().copied().filter(|t| dig::is_dug(t.block)).collect();
    let turf = lips.iter().filter(|t| dig::is_turf_lip(t.block)).count();
    assert!(turf > 500, "only {turf} turf lips on the downs");
    for lip in &lips {
        // A lip is its column's own ground lowered: grass stays grass.
        assert!(takes_a_lip(dig::whole(lip.block)), "a lip of {} on the downs", crate::types::block_name(lip.block));
        if block_kind(lip.block) == BLOCK_GRASS {
            assert!(dig::is_turf_lip(lip.block));
        }
    }
    // ...and the meadow's tufts go on growing on them.
    let tufted = lips.iter().filter(|t| crate::types::is_cross(t.above)).count();
    assert!(tufted * 10 > turf, "only {tufted} of {turf} turf lips grew anything");
}

/// What stands on the ground of a column, sorted the way the lips meet it
/// (`lips`, "What stands on a lip").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Standing {
    Tree,
    Stone,
    Pebble,
    Other,
}

fn standing(above: BlockId, over_that: BlockId) -> Standing {
    use crate::ground::{rock_of, Form};
    if (crate::wood::is_log(above) && crate::types::block_axis(above) == crate::types::Axis::Y)
        || (crate::types::is_bough(above) && crate::types::is_branch(over_that))
    {
        Standing::Tree
    } else if matches!(rock_of(above), Some((_, None | Some(Form::Cobble)))) && over_that == crate::types::BLOCK_AIR {
        Standing::Stone
    } else if crate::types::is_flat(above) {
        Standing::Pebble
    } else {
        Standing::Other
    }
}

/// Every column of the downs whose ground something stands on, as (what
/// stands there, whether the slope made a lip of that column -- the thing
/// on a lip or its foot in the lip's cell -- and the column's cells from
/// under its ground up: under the ground, the ground, and the two over it).
fn features_on_the_downs(gen: &WorldGen) -> Vec<(Standing, bool, [BlockId; 4])> {
    let keep = super::lips::FEATURES_KEEP_THEIR_STEP.with(std::cell::Cell::get);
    let mut found = Vec::new();
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            let pos = ChunkPos::new(HILLS.0 + cx, HILLS.1 + cz);
            let chunk = gen.generate_chunk(pos);
            // The same chunk with every feature on the whole block it was
            // laid on: where the two differ under a feature, the lip met it.
            super::lips::FEATURES_KEEP_THEIR_STEP.with(|k| k.set(true));
            let stepped = gen.generate_chunk(pos);
            super::lips::FEATURES_KEEP_THEIR_STEP.with(|k| k.set(keep));
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let (gx, gz) = (pos.x * CHUNK_SIZE_X as i32 + x as i32, pos.z * CHUNK_SIZE_Z as i32 + z as i32);
                    let h = gen.height_at(gx, gz);
                    if h < 1 || h + 3 >= CHUNK_SIZE_Y as i32 {
                        continue;
                    }
                    let at = |c: &Chunk, y: i32| c.get(x, y as usize, z);
                    let whole = at(&stepped, h);
                    if !takes_a_lip(whole) || at(&stepped, h + 1) == crate::types::BLOCK_AIR {
                        continue;
                    }
                    let what = standing(at(&stepped, h + 1), at(&stepped, h + 2));
                    let met = at(&chunk, h) != whole;
                    found.push((what, met, [at(&chunk, h - 1), at(&chunk, h), at(&chunk, h + 1), at(&chunk, h + 2)]));
                }
            }
        }
    }
    found
}

/// **On a generated hillside trees and stones stand on the lips as well as
/// on whole tops.** "Деревья растут только на полных блоках, камни также
/// только так появляются": every trunk, boulder and pebble on the downs kept
/// the whole block it was laid on, a step left standing round each. Now a
/// trunk on a lip roots into it, a boulder is bedded in it and a pebble lies
/// on it -- and on the terraces, where the slope makes no lip, they stand on
/// the whole block as they always did.
#[test]
fn on_a_generated_hillside_trees_and_stones_stand_on_lips_as_well_as_on_whole_tops() {
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    let found = features_on_the_downs(&gen);
    let count = |what: Standing, met: bool| found.iter().filter(|f| f.0 == what && f.1 == met).count();
    let [trees, stones, pebbles] = [Standing::Tree, Standing::Stone, Standing::Pebble].map(|w| (count(w, true), count(w, false)));
    println!("on lips and on whole tops: trees {trees:?}, boulders {stones:?}, pebbles {pebbles:?}");
    assert!(trees.0 >= 10 && trees.1 >= 10, "trees on lips and on whole tops: {trees:?}");
    assert!(stones.0 + pebbles.0 >= 10 && stones.1 + pebbles.1 >= 10, "stones {stones:?}, pebbles {pebbles:?}");
    // ...and before, not one of them met a lip.
    super::lips::FEATURES_KEEP_THEIR_STEP.with(|k| k.set(true));
    let before = features_on_the_downs(&gen);
    super::lips::FEATURES_KEEP_THEIR_STEP.with(|k| k.set(false));
    assert!(
        before.iter().all(|f| !f.1 || f.0 == Standing::Other),
        "a tree or a stone met a lip before the lips were let under them"
    );
}

/// **Nothing on a generated hillside floats or sinks**: whatever stands on a
/// column's ground has its bottom at the real top of what is under it. A
/// plant or a thing lying on a lip is drawn on the lip (`types::stand_drop`);
/// a trunk, a boulder or a bush that met a lip has its foot in the lip's
/// cell, on the whole block under it; and nothing solid is left over a lip,
/// which would be a trunk on a gap a quarter to three quarters deep.
#[test]
fn nothing_on_a_generated_hillside_floats_over_a_lip_or_sinks_into_one() {
    use crate::types::{coating_rests_at, has_full_top, is_cross, is_flat, stand_drop, BLOCK_AIR};
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    let found = features_on_the_downs(&gen);
    let mut checked = 0;
    for &(what, met, [below, ground, above, over]) in &found {
        if is_cross(above) || is_flat(above) {
            // Drawn from `1 - drop` over the ground's cell floor: that is
            // where the ground's own top is.
            let top = coating_rests_at(ground).or_else(|| has_full_top(ground).then_some(1.0));
            let drawn = 1.0 - stand_drop(above, ground, BLOCK_AIR);
            assert_eq!(Some(drawn), top, "{} on {ground:#x} is drawn at {drawn}", crate::types::block_name(above));
            checked += 1;
        } else if met {
            // The foot of the thing, where the lip would have been, on a
            // whole floor -- and the thing going on up from it.
            assert!(has_full_top(below), "the foot of a {what:?} at the lip stands on {below:#x}");
            assert!(!dig::is_dug(ground), "a {what:?} was left over a lip");
            assert_ne!(above, BLOCK_AIR, "the foot of a {what:?} has nothing on it");
            checked += 1;
        } else {
            assert!(!dig::is_dug(ground), "a {} stands over a lip, {over:#x} on it", crate::types::block_name(above));
        }
    }
    assert!(checked > 100, "only {checked} things on the downs were looked at");
}

/// The rhythm of a slope, in numbers: what a walker's eye meets along every
/// straight line of bare ground.
///
/// * **risers** -- every change of surface between two neighbouring bare
///   columns, by its height in quarters;
/// * **treads** -- how many columns lie between one riser and the next along
///   a line;
/// * **stairs** -- runs of three or more columns in a row each rising by the
///   same amount, which is a flight of identical steps;
/// * **wrapped** -- how many risers wear the turf's top picture on their
///   face (`ground::turf_wraps_the_side`), which is green on green.
#[derive(Default, Debug)]
struct Rhythm {
    risers: [u32; 6],
    treads: [u32; 8],
    stairs: u32,
    stair_columns: u32,
    columns: u32,
    wrapped: u32,
}

fn rhythm_of(gen: &WorldGen, corner: (i32, i32), span: i32) -> Rhythm {
    let (_, _, side, tops) = tops_at(gen, corner, span);
    let at = |x: i32, z: i32| {
        if x < 0 || z < 0 || x >= side || z >= side {
            None
        } else {
            tops[(z * side + x) as usize]
        }
    };
    let mut rhythm = Rhythm::default();
    for (dx, dz) in [(1, 0), (0, 1)] {
        for line in 0..side {
            let mut last_riser: Option<i32> = None;
            let (mut run, mut run_step) = (0, 0);
            for k in 0..side - 1 {
                let (x, z) = if dx == 1 { (k, line) } else { (line, k) };
                let (Some(a), Some(b)) = (at(x, z), at(x + dx, z + dz)) else {
                    last_riser = None;
                    run = 0;
                    continue;
                };
                rhythm.columns += 1;
                let step = ((b.surface - a.surface) * 4.0).round() as i32;
                if step == 0 {
                    run = 0;
                    continue;
                }
                rhythm.risers[(step.unsigned_abs() as usize).min(5)] += 1;
                let (high, low) = if step > 0 { (b, a) } else { (a, b) };
                let (beyond, diagonal) =
                    if high.y == low.y { (low.block, crate::types::BLOCK_AIR) } else { (low.above, low.block) };
                if crate::ground::turf_wraps_the_side(high.block, high.above, beyond, diagonal) {
                    rhythm.wrapped += 1;
                }
                if let Some(prev) = last_riser {
                    rhythm.treads[((k - prev) as usize).min(7)] += 1;
                }
                last_riser = Some(k);
                if step == run_step && run > 0 {
                    run += 1;
                } else {
                    run = 1;
                    run_step = step;
                }
                if run == 3 {
                    rhythm.stairs += 1;
                    rhythm.stair_columns += 3;
                } else if run > 3 {
                    rhythm.stair_columns += 1;
                }
            }
        }
    }
    rhythm
}

/// **Every shape a ramp can take is still walked.** The profile of a rise
/// varies from column to column (`lips::profile`) -- bent by a field and
/// pushed by a hash -- and the one thing that may not vary is the climb: a
/// lip is at least a quarter, less than a whole block, and within a stride
/// of whatever lip the column before or after it took, for any bend and any
/// push those two can hand it. Checked over the whole range of both rather
/// than over a world, because a world only visits the shapes its seed does.
#[test]
fn every_shape_of_a_ramp_climbs_no_more_than_a_stride_a_column() {
    let stride = (PLAYER_STEP_HEIGHT * f32::from(dig::SLICES)) as i32;
    let whole = i32::from(dig::SLICES);
    let bends: Vec<f64> = (0..=40).map(|i| 2f64.powf(-1.0 + f64::from(i) / 20.0)).collect();
    let jitters: Vec<f64> = (0..=24).map(|i| -0.6 + f64::from(i) * 0.05).collect();
    for width in 2..=whole {
        // Every lip each column of the terrace can take, whatever shape.
        let range: Vec<(i32, i32)> = (1..width)
            .map(|k| {
                let lips = bends.iter().flat_map(|&b| jitters.iter().map(move |&j| super::lips::profile(k, width, b, j)));
                (lips.clone().min().unwrap(), lips.max().unwrap())
            })
            .collect();
        for (k, &(low, high)) in range.iter().enumerate() {
            assert!(low >= 1 && high < whole, "column {} of {width}: lips {low}..={high}", k + 1);
        }
        // The climb onto the first lip, between any two neighbours, and off
        // the last one onto the whole block.
        assert!(range[0].1 <= stride, "a terrace {width} wide begins with a climb of {} quarters", range[0].1);
        for pair in range.windows(2) {
            assert!(pair[1].1 - pair[0].0 <= stride, "a terrace {width} wide climbs {pair:?} in one column");
            assert!(pair[0].1 - pair[1].0 <= stride, "a terrace {width} wide drops {pair:?} in one column");
        }
        assert!(whole - range[width as usize - 2].0 <= stride, "a terrace {width} wide ends with a climb");
    }
}

/// **A hillside of lips is not one flight of identical steps.** Before the
/// profiles varied, 53 per cent of the downs' risers stood in a run of three
/// or more identical steps and 80 per cent were a quarter tall: from the
/// middle distance a corrugated sheet, from far off one tone. Held with the
/// even ramp alongside, so the numbers say it is the profile that broke the
/// rhythm and not the ground under it.
#[test]
fn a_hillside_of_lips_is_not_one_flight_of_identical_steps() {
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    let share = |r: &Rhythm| {
        let risers: u32 = r.risers.iter().sum();
        (r.stair_columns as f32 / risers.max(1) as f32, r.risers[1] as f32 / risers.max(1) as f32)
    };
    super::lips::EVEN_LIPS.with(|e| e.set(true));
    let (even_stairs, even_quarters) = share(&rhythm_of(&gen, HILLS, SPAN));
    super::lips::EVEN_LIPS.with(|e| e.set(false));
    let (stairs, quarters) = share(&rhythm_of(&gen, HILLS, SPAN));
    assert!(even_stairs > 0.45, "the even ramp was not a flight to begin with: {even_stairs:.2}");
    assert!(stairs < 0.30, "{:.0}% of the downs' risers still stand in a flight of identical steps", stairs * 100.0);
    assert!(quarters < even_quarters - 0.1, "the risers are still all a quarter: {quarters:.2} against {even_quarters:.2}");
}

/// ```text
/// cargo test -p primitive_shared --lib -- --ignored --nocapture probe_the_rhythm_of_a_slope
/// ```
#[test]
#[ignore = "a measurement, not an assertion"]
fn probe_the_rhythm_of_a_slope() {
    let gen = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Landforms);
    for (name, corner, even) in
        [("downs, even", HILLS, true), ("downs", HILLS, false), ("origin, even", (-3, -3), true), ("origin", (-3, -3), false)]
    {
        super::lips::EVEN_LIPS.with(|e| e.set(even));
        let r = rhythm_of(&gen, corner, SPAN);
        super::lips::EVEN_LIPS.with(|e| e.set(false));
        let risers: u32 = r.risers.iter().sum();
        let treads: u32 = r.treads.iter().sum();
        println!("{name}: {} column pairs, {risers} risers", r.columns);
        println!(
            "  riser height in quarters 1..5+: {:?} ({:.0}% a quarter)",
            &r.risers[1..],
            100.0 * r.risers[1] as f32 / risers.max(1) as f32
        );
        println!(
            "  tread between risers 1..7+: {:?} ({:.0}% one column)",
            &r.treads[1..],
            100.0 * r.treads[1] as f32 / treads.max(1) as f32
        );
        println!(
            "  identical stairs of 3+: {} ({:.0}% of risers stand in one)",
            r.stairs,
            100.0 * r.stair_columns as f32 / risers.max(1) as f32
        );
        println!("  risers wearing the top's picture: {:.0}%", 100.0 * r.wrapped as f32 / risers.max(1) as f32);
    }
}

