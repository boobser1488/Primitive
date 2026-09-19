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
                    // Bare ground: nothing over it but air or a plant. A
                    // bush or a stone keeps the whole block it stands on.
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

