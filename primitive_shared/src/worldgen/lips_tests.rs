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
    let side = SPAN * CHUNK_SIZE_X as i32;
    let (ox, oz) = (HILLS.0 * CHUNK_SIZE_X as i32, HILLS.1 * CHUNK_SIZE_Z as i32);
    let mut tops = vec![None; (side * side) as usize];
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            let chunk = gen.generate_chunk(ChunkPos::new(HILLS.0 + cx, HILLS.1 + cz));
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

