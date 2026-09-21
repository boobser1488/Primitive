//! The scenarios themselves. Each is one thing a player does, named as
//! the sentence that should be true about it, and each asserts on what a
//! player would have seen or felt -- where they stood, what the server
//! said back, what was drawn -- rather than on the state of one part.
//!
//! Every one of them also asserts, at the end, that **the server never
//! corrected the player**: the validator is on (`scenario_settings`), and a
//! correction nobody asked for is a rubber-band in multiplayer whatever
//! else the scenario was about.

use super::*;
use primitive_shared::types as t;
use primitive_shared::types::Facing;
use primitive_shared::notice::Notice;

fn no_corrections(s: &Scenario) {
    assert!(s.corrections.is_empty(), "the server corrected the player: {:#?}", s.corrections);
}

/// Every solid-pass triangle with all three corners inside the cells from
/// `low` to `high`, as its three corners in world blocks, turned so the
/// smallest corner is first (the same triangle drawn twice compares equal;
/// its back face, wound the other way, does not).
fn triangles_in(s: &Scenario, low: (i32, i32, i32), high: (i32, i32, i32)) -> Vec<[[i32; 3]; 3]> {
    let pos = ChunkPos::from_world(low.0, low.2);
    let mesh = s.mesh(pos);
    let origin = [pos.x as f32 * 16.0, 0.0, pos.z as f32 * 16.0];
    let inside = |p: [f32; 3]| {
        (p[0] >= low.0 as f32 - 0.01 && p[0] <= high.0 as f32 + 1.0 + 0.01)
            && (p[1] >= low.1 as f32 - 0.01 && p[1] <= high.1 as f32 + 1.0 + 0.01)
            && (p[2] >= low.2 as f32 - 0.01 && p[2] <= high.2 as f32 + 1.0 + 0.01)
    };
    // In 1/1024ths, so two copies of one corner compare equal exactly.
    let key = |p: [f32; 3]| [(p[0] * 1024.0).round() as i32, (p[1] * 1024.0).round() as i32, (p[2] * 1024.0).round() as i32];
    let solid = &mesh.indices[..mesh.solid_index_count as usize];
    let mut out = Vec::new();
    for tri in solid.chunks(3) {
        let corners: Vec<[f32; 3]> = tri
            .iter()
            .map(|&i| {
                let v = mesh.vertices[i as usize].position;
                [v[0] + origin[0], v[1] + origin[1], v[2] + origin[2]]
            })
            .collect();
        if corners.iter().all(|&c| inside(c)) {
            let mut k = [key(corners[0]), key(corners[1]), key(corners[2])];
            let first = (0..3).min_by_key(|&i| k[i]).unwrap_or(0);
            k.rotate_left(first);
            out.push(k);
        }
    }
    out
}

fn duplicated(triangles: &[[[i32; 3]; 3]]) -> usize {
    let mut seen = std::collections::HashSet::new();
    triangles.iter().filter(|tri| !seen.insert(**tri)).count()
}

// ---------------------------------------------------------------- moving

#[test]
fn jumping_again_and_again_across_a_field_is_never_corrected() {
    let mut s = Scenario::new();
    s.stand_at(feet_on(FIELD.0, FIELD.1));
    s.face(0.0);
    s.hold(Action::Forward);
    for _ in 0..12 {
        s.hold(Action::Jump);
        s.frames(2);
        s.release(Action::Jump);
        let landed = s.until(2.0, |s| s.player.grounded);
        assert!(landed, "a jump never came down");
    }
    s.release_all();
    s.seconds(1.0);
    assert!(s.feet().x > FIELD.0 as f64 + 8.0, "the jumps went nowhere: {:?}", s.feet());
    no_corrections(&s);
}

#[test]
fn a_staircase_is_walked_up_half_a_block_at_a_time() {
    // The step's low side toward the climber, who comes from -x: a step
    // faced west has its upper box at the +x half (`geometry::step_boxes`).
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    let step = t::faced(t::BLOCK_PLANK_STAIRS, Facing::West);
    let mut cells = Vec::new();
    for k in 0..4 {
        let x = x0 + 2 + k;
        for y in g + 1..g + 1 + k {
            cells.push(((x, y, z), t::BLOCK_PLANKS));
        }
        cells.push(((x, g + 1 + k, z), step));
    }
    for y in g + 1..g + 5 {
        cells.push(((x0 + 6, y, z), t::BLOCK_PLANKS));
        cells.push(((x0 + 7, y, z), t::BLOCK_PLANKS));
    }
    s.stand_at(feet_on(x0, z));
    s.build(&cells);
    s.face(0.0);
    s.shot("staircase_before");
    s.hold(Action::Forward);
    let mut rises = Vec::new();
    let mut last = s.feet().y;
    for _ in 0..(6.0 / FRAME) as usize {
        s.frame();
        let y = s.feet().y;
        if y > last + 1e-4 {
            rises.push(y - last);
        }
        last = y;
        if s.feet().x > (x0 + 7) as f64 {
            break;
        }
    }
    s.release_all();
    s.seconds(0.5);
    let top = (g + 5) as f64;
    assert!(
        (s.feet().y - top).abs() < 0.05 && s.feet().x > (x0 + 6) as f64,
        "the climb did not reach the landing: at {:?}",
        s.feet()
    );
    let biggest = rises.iter().copied().fold(0.0, f64::max);
    assert!(
        biggest <= f64::from(primitive_shared::geometry::PLAYER_STEP_HEIGHT) + 0.02,
        "one frame lifted the player {biggest:.3} of a block: {rises:?}"
    );
    no_corrections(&s);
}

/// The surface a walker stands on at the top of a generated column: the
/// top block's y and the height of its floor, if it is bare meadow turf
/// (whole or a lip) with room for a body over it.
fn meadow_top(s: &Scenario, x: i32, z: i32) -> Option<(i32, f64, BlockId)> {
    let y = (0..t::CHUNK_SIZE_Y as i32).rev().find(|&y| {
        s.block((x, y, z)).is_some_and(|b| t::is_collidable(b) || t::is_liquid(b))
    })?;
    let block = s.block((x, y, z))?;
    let clear = (1..=2).all(|dy| s.block((x, y + dy, z)).is_some_and(|b| !t::is_collidable(b) && !t::is_liquid(b)));
    (t::block_kind(block) == t::BLOCK_GRASS && clear).then(|| (y, y as f64 + f64::from(t::collision_height(block)), block))
}

/// A new world of the scale that lays lips (`worldgen::lips`).
fn landforms_world() -> Scenario {
    Scenario::with(primitive_server::settings::ServerSettings {
        world_preset: primitive_shared::worldgen::Preset::Normal,
        world_scale: primitive_shared::worldgen::Scale::Landforms,
        ..scenario_settings()
    })
}

/// How many columns a rise is walked over.
const RISE_RUN: i32 = 10;

/// A straight line of meadow along +x round the spawn, [`RISE_RUN`] columns
/// long, climbing two whole blocks or more with no step taller than a body
/// takes and at least two lips on it: its first column, its z, and the
/// surface of each column.
fn gentle_rise(s: &Scenario) -> (i32, i32, Vec<(i32, f64, BlockId)>) {
    let spawn = cell_of(s.feet());
    let step = f64::from(primitive_shared::geometry::PLAYER_STEP_HEIGHT);
    for z in spawn.2 - 40..spawn.2 + 40 {
        for x0 in spawn.0 - 40..spawn.0 + 40 - RISE_RUN {
            let Some(line) = (0..RISE_RUN).map(|k| meadow_top(s, x0 + k, z)).collect::<Option<Vec<_>>>() else {
                continue;
            };
            let walkable = line.windows(2).all(|w| (w[1].1 - w[0].1).abs() <= step);
            let lips = line.iter().filter(|c| primitive_shared::dig::is_turf_lip(c.2)).count();
            if walkable && line[RISE_RUN as usize - 1].0 - line[0].0 >= 2 && lips >= 2 {
                return (x0, z, line);
            }
        }
    }
    panic!("no gentle rise of two blocks round the spawn of a landforms world");
}

/// Forward held up the rise from its foot, the jump never pressed: says the
/// player reached the top, and that no one frame lifted them more than a
/// step.
fn walk_up_the_rise(s: &mut Scenario, x0: i32, line: &[(i32, f64, BlockId)]) {
    let step = f64::from(primitive_shared::geometry::PLAYER_STEP_HEIGHT);
    let last_column = x0 + RISE_RUN - 1;
    s.hold(Action::Forward);
    let mut rises = Vec::new();
    let mut last = s.feet().y;
    for _ in 0..(8.0 / FRAME) as usize {
        s.frame();
        let y = s.feet().y;
        if y > last + 1e-4 {
            rises.push(y - last);
        }
        last = y;
        if s.feet().x > last_column as f64 + 0.5 {
            break;
        }
    }
    // **Where the walk ended, read where it ended.** Read after the half
    // second below, the body had coasted on from the middle of the last
    // column to its far edge, and a body at the edge stands on the column
    // beyond -- which is whatever the hill does next, half a block up in
    // one full run: the check was of the coast, not the climb.
    let (at, top) = (s.feet(), line[RISE_RUN as usize - 1].1);
    s.release_all();
    s.seconds(0.5);
    assert!(
        at.x > last_column as f64 && (at.y - top).abs() < 0.05,
        "the walk up the hill stopped at {at:?}, the top is {top} at x {last_column}",
    );
    let biggest = rises.iter().copied().fold(0.0, f64::max);
    assert!(biggest <= step + 0.02, "one frame lifted the player {biggest:.3} of a block: {rises:?}");
}

#[test]
fn a_generated_hill_is_walked_up_without_a_jump() {
    // **The lips on a landforms slope** (`worldgen::lips`): a new world's
    // meadow, a straight line up a rise of at least two whole blocks, found
    // in the ground round the spawn, and walked with forward held and the
    // jump never pressed. Before the lips every block of that rise was a
    // step a body does not take without jumping.
    let mut s = landforms_world();
    let (x0, z, line) = gentle_rise(&s);
    let start = (x0 as f64 + 0.5, line[0].1, z as f64 + 0.5);
    s.stand_at(start);
    s.face(0.0);
    s.camera.pitch = -0.25;
    s.shot("smooth_after");
    walk_up_the_rise(&mut s, x0, &line);
    no_corrections(&s);

    // The same hillside with every lip in sight made whole again: the stair
    // the generator drew before, from the same place, for the eye.
    if std::env::var("PRIMITIVE_SCENARIO_SHOTS").is_ok() {
        let mut whole = Vec::new();
        for dz in -8..=8 {
            for dx in -2..RISE_RUN + 12 {
                if let Some((y, _, block)) = meadow_top(&s, x0 + dx, z + dz) {
                    if primitive_shared::dig::is_dug(block) {
                        whole.push(((x0 + dx, y, z + dz), primitive_shared::dig::whole(block)));
                    }
                }
            }
        }
        s.build(&whole);
        s.stand_at((start.0, line[0].0 as f64 + 1.0, start.2));
        s.face(0.0);
        s.camera.pitch = -0.25;
        s.shot("smooth_before");
    }
}

#[test]
fn a_winter_hillside_of_lips_is_white_all_over_is_walked_up_and_is_swept_back_to_its_turf() {
    // **"Every lip of every hillside stays a green stripe in a white field."**
    // Snow wanted a whole top (`types::has_full_top`) and a lip is not one,
    // so a winter over the landforms' meadows left each rise green. It lies
    // on the lip's real top now (`types::coating_rests_at`), drawn and aimed
    // at there (`types::rest_drop`). The cover is laid here by the rule the
    // snowfall asks (`logic::snowfall`, whose own test lets it fall on a
    // field of lips); the rain keeps the thaw off it while the player works.
    let mut s = landforms_world();
    s.server().console_command("/weather rain");
    let (x0, z, line) = gentle_rise(&s);
    let start = (x0 as f64 + 0.5, line[0].1, z as f64 + 0.5);

    // The ground in sight, and the air over it.
    let (mut ground, mut mown) = (Vec::new(), Vec::new());
    for dz in -8..=8 {
        for dx in -2..RISE_RUN + 12 {
            let (x, z) = (x0 + dx, z + dz);
            let Some(y) = (1..t::CHUNK_SIZE_Y as i32 - 1)
                .rev()
                .find(|&y| s.block((x, y, z)).is_some_and(|b| t::is_collidable(b) || t::is_liquid(b)))
            else {
                continue;
            };
            let block = s.block((x, y, z)).expect("the column was just read");
            match s.block((x, y + 1, z)) {
                Some(t::BLOCK_AIR) => ground.push(((x, y, z), block)),
                // The tufts mown, so the picture is of the ground: snow keeps
                // off a plant on a lip as on whole turf, and a meadow in
                // leaf is green under any rule.
                Some(over) if t::is_cross(over) && !t::is_cross(s.block((x, y + 2, z)).unwrap_or(t::BLOCK_AIR)) => {
                    mown.push(((x, y + 1, z), t::BLOCK_AIR));
                    ground.push(((x, y, z), block));
                }
                _ => {}
            }
        }
    }
    let snow_where = |lies: &dyn Fn(BlockId) -> bool| -> Vec<((i32, i32, i32), BlockId)> {
        ground.iter().filter(|(_, b)| lies(*b)).map(|&((x, y, z), _)| ((x, y + 1, z), t::BLOCK_SNOW_COVER)).collect()
    };
    let look_up_the_hill = |s: &mut Scenario| {
        look_from(s, DVec3::new(start.0 - 3.0, start.1 + 5.0, start.2 - 5.0), DVec3::new(start.0 + 6.0, line[5].1, start.2));
    };

    // Before: where the old rule let snow lie, whole tops only.
    s.stand_at(start);
    s.build(&mown);
    s.build(&snow_where(&|b| t::has_full_top(b)));
    s.seconds(0.5);
    look_up_the_hill(&mut s);
    s.shot("winter_hill_before");

    // Now: every level top, lips and all.
    s.build(&snow_where(&|b| t::can_grow_on(t::BLOCK_SNOW_COVER, b)));
    s.seconds(0.5);
    look_up_the_hill(&mut s);
    s.shot("winter_hill_after");
    let lips: Vec<_> = ground.iter().filter(|(_, b)| primitive_shared::dig::is_turf_lip(*b)).collect();
    assert!(lips.len() >= 2, "no lips in sight of the rise");
    for &&((x, y, z), lip) in &lips {
        assert_eq!(s.block((x, y + 1, z)), Some(t::BLOCK_SNOW_COVER), "the lip at {x},{y},{z} stayed green");
        assert_eq!(s.block((x, y, z)), Some(lip), "the snow changed the lip under it at {x},{y},{z}");
    }

    // Up it with forward held, as up the green one: the snow is walked
    // through and the lips are still quarter steps.
    s.stand_at(start);
    s.face(0.0);
    walk_up_the_rise(&mut s, x0, &line);

    // A lip in sight, swept by hand from the ground beside it: the snow
    // comes off and the turf under it is the lip it was, at the height it
    // was. Stood on the column two to the west, looking down across the
    // edge of the lip, which is the ray that comes into the lip's own cell
    // through its side (`physics::raycast_in`).
    let floor_at = |x: i32, z: i32| {
        ground.iter().find(|((gx, _, gz), _)| (*gx, *gz) == (x, z)).map(|&((_, y, _), b)| f64::from(y) + f64::from(t::collision_height(b)))
    };
    let (&((lx, ly, lz), lip), stand) = lips
        .iter()
        .find_map(|l| Some((*l, floor_at(l.0 .0 - 2, l.0 .2)?)))
        .expect("a lip with ground beside it");
    let snow = (lx, ly + 1, lz);
    s.stand_at((f64::from(lx) - 1.5, stand, f64::from(lz) + 0.5));
    s.look_at(DVec3::new(f64::from(lx) + 0.5, f64::from(ly) + f64::from(t::collision_height(lip)) + 0.01, f64::from(lz) + 0.5));
    // **Once the move has landed.** `stand_at` is a teleport the server has
    // to confirm and the client has to catch up with; read the aim the same
    // frame and, on a busy machine, there is nothing under the crosshair yet
    // -- `None`, which is not a place the hand reaches but the absence of a
    // hand. What is under it, when there is something, is what this asks.
    s.until(3.0, |s| s.aimed().is_some());
    assert_eq!(s.aimed(), Some((snow, t::BLOCK_SNOW_COVER)), "the snow on the lip is not where the hand reaches for it");
    s.input.breaking = true;
    let swept = s.until(8.0, |s| s.block(snow) == Some(t::BLOCK_AIR));
    s.input.breaking = false;
    assert!(swept, "the snow on the lip was never swept off");
    s.seconds(0.5);
    assert_eq!(s.block((lx, ly, lz)), Some(lip), "sweeping the snow took the lip with it");
    assert_eq!(s.server().block_at(lx, ly, lz), Some(lip), "the server's lip is not the one swept");
    s.shot("winter_hill_swept");
    no_corrections(&s);
}

/// Is the cell the foot of a tree, a boulder or a bush that a lip met
/// (`worldgen::lips`, "What stands on a lip"): the thing's own block in the
/// cell the ground's lip would have been, standing on a whole floor, with a
/// turf lip beside it at the same level and the thing going on up over it.
fn met_by_a_lip(s: &Scenario, (x, y, z): (i32, i32, i32)) -> bool {
    use primitive_shared::ground::{rock_of, Form};
    let (Some(foot), Some(over), Some(floor)) = (s.block((x, y, z)), s.block((x, y + 1, z)), s.block((x, y - 1, z))) else {
        return false;
    };
    let trunk = primitive_shared::wood::is_log(foot) && t::block_axis(foot) == t::Axis::Y && (t::is_branch(over) || primitive_shared::wood::is_log(over));
    let stone = matches!(rock_of(foot), Some((_, None | Some(Form::Cobble)))) && over == foot;
    let bush = t::block_kind(foot) == t::BLOCK_BUSH_LEAVES && t::block_kind(over) == t::BLOCK_BUSH_LEAVES;
    let beside = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .any(|&(dx, dz)| s.block((x + dx, y, z + dz)).is_some_and(primitive_shared::dig::is_turf_lip));
    (trunk || stone || bush) && t::has_full_top(floor) && beside
}

#[test]
fn a_wooded_hillside_is_walked_up_without_a_jump_past_trees_and_stones_rooted_in_its_lips() {
    // **"Деревья растут только на полных блоках, камни также только так
    // появляются."** The lips left a whole block under everything standing
    // on a slope, so every tree and every stone on a hillside stood on a
    // step of its own. A trunk now roots into the lip's cell, a boulder is
    // bedded in it and a pebble or a stick lies on it (`worldgen::lips`).
    // What a player has to be able to do there is what they do on a bare
    // down: walk up with forward held, never jumping, never corrected --
    // past the trees, up lips that things now lie on.
    let mut s = landforms_world();
    let spawn = cell_of(s.feet());
    let step = f64::from(primitive_shared::geometry::PLAYER_STEP_HEIGHT);
    let mut feet_in_lips = Vec::new();
    for z in spawn.2 - 44..spawn.2 + 44 {
        for x in spawn.0 - 44..spawn.0 + 44 {
            if let Some(y) = (1..t::CHUNK_SIZE_Y as i32 - 2).rev().find(|&y| s.block((x, y, z)).is_some_and(t::is_collidable)) {
                // The foot is a cell under the top of what stands on it.
                if let Some(foot) = (y - 12..y).rev().find(|&fy| met_by_a_lip(&s, (x, fy, z))) {
                    feet_in_lips.push((x, foot, z));
                }
            }
        }
    }
    // A trunk's foot first, which is the picture the player described.
    let trunk = |s: &Scenario, (x, y, z): (i32, i32, i32)| s.block((x, y, z)).is_some_and(primitive_shared::wood::is_log);
    feet_in_lips.sort_by_key(|&cell| !trunk(&s, cell));
    // A rise like `gentle_rise`'s with a tree, a stone or a bush rooted in a
    // lip within three columns of it.
    let near_a_foot = |x0: i32, z: i32| {
        feet_in_lips.iter().copied().find(|&(fx, _, fz)| (fz - z).abs() <= 3 && fx >= x0 && fx < x0 + RISE_RUN)
    };
    let mut found = None;
    'search: for z in spawn.2 - 40..spawn.2 + 40 {
        for x0 in spawn.0 - 40..spawn.0 + 40 - RISE_RUN {
            let Some(foot) = near_a_foot(x0, z) else { continue };
            let Some(line) = (0..RISE_RUN).map(|k| meadow_top(&s, x0 + k, z)).collect::<Option<Vec<_>>>() else {
                continue;
            };
            let walkable = line.windows(2).all(|w| (w[1].1 - w[0].1).abs() <= step);
            let lips = line.iter().filter(|c| primitive_shared::dig::is_turf_lip(c.2)).count();
            if walkable && line[RISE_RUN as usize - 1].0 - line[0].0 >= 1 && lips >= 2 {
                if found.is_none() || trunk(&s, foot) {
                    found = Some((x0, z, line, foot));
                }
                if trunk(&s, foot) {
                    break 'search;
                }
            }
        }
    }
    let Some((x0, z, line, foot)) = found else {
        panic!("no wooded rise round the spawn of a landforms world: {} feet in lips", feet_in_lips.len());
    };
    let start = (x0 as f64 + 0.5, line[0].1, z as f64 + 0.5);
    println!("the rise at {x0},{z}; a {} rooted in a lip at {foot:?}", t::block_name(s.block(foot).unwrap_or(t::BLOCK_AIR)));
    let look_at_the_foot = |s: &mut Scenario| {
        let target = DVec3::new(foot.0 as f64 + 0.5, foot.1 as f64 + 0.8, foot.2 as f64 + 0.5);
        look_from(s, target + DVec3::new(-5.0, 2.2, if foot.2 >= z { -5.0 } else { 5.0 }), target);
    };
    s.stand_at(start);
    s.face(0.0);
    look_at_the_foot(&mut s);
    s.shot("lip_features_after");
    s.stand_at(start);
    s.face(0.0);
    walk_up_the_rise(&mut s, x0, &line);
    no_corrections(&s);

    // The same hillside as the lips left it before they were let under what
    // stands on it: every foot in sight turned back into the turf it stood
    // on, and every lip with a stone or a stick on it made whole -- from the
    // same place, for the eye.
    if std::env::var("PRIMITIVE_SCENARIO_SHOTS").is_ok() {
        let mut stepped: Vec<_> = feet_in_lips
            .iter()
            .filter(|&&(fx, _, fz)| (fx - foot.0).abs() <= 12 && (fz - foot.2).abs() <= 12)
            .map(|&cell| (cell, t::BLOCK_GRASS))
            .collect();
        for dz in -12..=12 {
            for dx in -12..=12 {
                let (x, z) = (foot.0 + dx, foot.2 + dz);
                for y in foot.1 - 6..foot.1 + 6 {
                    let (Some(ground), Some(on)) = (s.block((x, y, z)), s.block((x, y + 1, z))) else { continue };
                    if primitive_shared::dig::is_dug(ground) && t::is_flat(on) {
                        stepped.push(((x, y, z), primitive_shared::dig::whole(ground)));
                    }
                }
            }
        }
        s.build(&stepped);
        s.stand_at(start);
        s.seconds(0.5);
        look_at_the_foot(&mut s);
        s.shot("lip_features_before");
    }
}

/// Walks along +x at `z` from `x0` into whatever is in front, and says
/// where the front of the body stopped.
fn walk_into(s: &mut Scenario, x0: i32, z: i32) -> f64 {
    s.stand_at(feet_on(x0, z));
    s.face(0.0);
    s.hold(Action::Forward);
    let mut still = 0;
    let mut last = s.feet().x;
    for _ in 0..(6.0 / FRAME) as usize {
        s.frame();
        if (s.feet().x - last).abs() < 1e-4 {
            still += 1;
            if still > 20 {
                break;
            }
        } else {
            still = 0;
        }
        last = s.feet().x;
    }
    s.release_all();
    s.seconds(0.3);
    s.feet().x + f64::from(primitive_shared::geometry::PLAYER_HALF_WIDTH)
}

#[test]
fn walking_into_a_two_by_two_rack_stops_where_the_rack_is_drawn_and_it_is_drawn_once() {
    for facing in [Facing::North, Facing::East] {
        let mut s = Scenario::new();
        let (x0, z) = FIELD;
        let anchor = (x0 + 3, GROUND + 1, z);
        let cells = t::rack_cells(anchor, facing);
        s.stand_at(feet_on(x0, z));
        s.build(&cells);
        let (mut low, mut high) = (anchor, anchor);
        for ((x, y, z), _) in cells {
            low = (low.0.min(x), low.1.min(y), low.2.min(z));
            high = (high.0.max(x), high.1.max(y), high.2.max(z));
        }
        let triangles = triangles_in(&s, low, high);
        assert!(!triangles.is_empty(), "the {facing:?} rack is not drawn at all");
        assert_eq!(duplicated(&triangles), 0, "the {facing:?} rack is drawn more than once over itself");

        let stopped = walk_into(&mut s, x0, z);
        let (drawn_low, _) = s
            .drawn_bounds((low.0, GROUND + 1, z), (high.0, GROUND + 2, z))
            .expect("nothing drawn in the rack's cells");
        s.shot(&format!("rack_{facing:?}"));
        let front = f64::from(drawn_low[0]);
        assert!(
            (stopped - front).abs() < 0.1,
            "{facing:?}: the body stopped at x {stopped:.3} and the rack is drawn from x {front:.3}"
        );
        no_corrections(&s);
    }
}

#[test]
fn walking_into_a_hide_frame_stops_where_the_frame_is_drawn() {
    for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
        let mut s = Scenario::new();
        let (x0, z) = FIELD;
        let cell = (x0 + 3, GROUND + 1, z);
        s.stand_at(feet_on(x0, z));
        s.build(&[(cell, t::faced(t::BLOCK_HIDE_FRAME, facing))]);
        let stopped = walk_into(&mut s, x0, z);
        let (drawn_low, drawn_high) = s.drawn_bounds(cell, cell).expect("the frame is not drawn");
        let front = f64::from(drawn_low[0]);
        s.shot(&format!("hide_frame_{facing:?}"));
        if stopped > f64::from(drawn_high[0]) + 0.5 {
            // Walked past: a frame thin enough to go round is fine, one the
            // body went *through* is not -- the collider is somewhere.
            panic!("{facing:?}: walked straight through a frame drawn from x {front:.3}");
        }
        assert!(
            (stopped - front).abs() < 0.1,
            "{facing:?}: the body stopped at x {stopped:.3} and the frame is drawn from x {front:.3} -- \
             an invisible wall, or a frame walked into"
        );
        no_corrections(&s);
    }
}

#[test]
fn a_swimmer_climbs_out_onto_a_bank_one_block_up_without_a_correction() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0 - 2, z));
    // A pool whose water stands one block below the field: the cells at
    // the field's own level are cut out to air over it.
    s.fill((x0, g - 3, z - 2), (x0 + 5, g - 1, z + 2), t::BLOCK_WATER);
    s.fill((x0, g, z - 2), (x0 + 5, g, z + 2), t::BLOCK_AIR);
    s.stand_at((x0 as f64 + 1.5, (g - 1) as f64, z as f64 + 0.5));
    s.face(0.0);
    s.seconds(1.0);
    assert!(s.player.in_water, "the scenario did not put the player in the water");
    s.hold(Action::Forward);
    s.hold(Action::Jump);
    let out = s.until(10.0, |s| s.player.grounded && s.feet().y >= (g + 1) as f64 - 0.01 && s.feet().x > (x0 + 6) as f64);
    s.release_all();
    s.shot("swim_out");
    assert!(out, "never climbed out: at {:?}, in water {}", s.feet(), s.player.in_water);
    s.seconds(2.0);
    no_corrections(&s);
}

#[test]
fn stakes_are_waded_through_slowly_and_they_cut() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0, z));
    s.fill((x0 + 4, g + 1, z - 1), (x0 + 7, g + 1, z + 1), t::BLOCK_STAKE | t::STAKE_UPRIGHT);
    s.stand_at(feet_on(x0, z));
    s.face(0.0);
    s.hold(Action::Forward);
    // The open-ground pace, over the first two blocks.
    s.seconds(0.25);
    let a = s.feet().x;
    s.seconds(0.25);
    let open = (s.feet().x - a) / 0.25;
    let reached = s.until(4.0, |s| s.feet().x > (x0 + 5) as f64);
    assert!(reached, "never got into the stakes: at {:?}", s.feet());
    let b = s.feet().x;
    s.seconds(0.25);
    let among = (s.feet().x - b) / 0.25;
    s.until(8.0, |s| s.feet().x > (x0 + 9) as f64);
    s.release_all();
    s.seconds(1.0);
    assert!(
        among < open * 0.6,
        "a body pushed through stakes at {among:.2} b/s against {open:.2} b/s in the open"
    );
    let cut = s.heard_any(|m| matches!(m, ServerMessage::Staked { .. }));
    assert!(cut, "walked through a stand of stakes and nothing cut them");
    no_corrections(&s);
}

// ---------------------------------------------------------------- hands

#[test]
fn a_chest_lid_rises_when_it_is_opened_and_falls_when_it_is_shut() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let chest = (x0 + 2, GROUND + 1, z);
    s.stand_at(feet_on(x0, z));
    s.build(&[(chest, t::BLOCK_CHEST)]);
    s.look_at_face(chest, (-1, 0, 0));
    assert_eq!(s.aimed().map(|(cell, _)| cell), Some(chest), "not looking at the chest");
    let shut_key = s.chest_screen.ui_key();
    s.use_aimed();
    assert!(s.until(3.0, |s| s.chest_screen.is_open()), "the chest never opened");
    assert_ne!(s.chest_screen.ui_key(), shut_key, "the open chest screen would not be redrawn");
    let swing = |s: &Scenario| s.chunks.open_lids().find(|(cell, _, _)| *cell == chest).map(|(_, _, swing)| swing);
    let risen = s.until(3.0, |s| swing(s).is_some_and(|a| a > 0.95));
    s.shot("chest_open");
    assert!(risen, "the lid never rose: {:?}", swing(&s));
    s.close_screens();
    let fallen = s.until(3.0, |s| swing(s).is_none_or(|a| a < 0.05));
    assert!(fallen, "the lid never came down: {:?}", swing(&s));
    no_corrections(&s);
}

#[test]
fn an_anvil_is_opened_a_run_is_played_and_the_nails_come_out_of_it() {
    use primitive_shared::minigame::{self, Job};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let anvil = (x0 + 2, GROUND + 1, z);
    s.stand_at(feet_on(x0, z));
    s.build(&[(anvil, t::BLOCK_ANVIL)]);
    s.give(t::BLOCK_STONE_HAMMER, 1);
    s.give(t::BLOCK_IRON_INGOT, 1);
    s.select(t::BLOCK_STONE_HAMMER);
    s.look_at_face(anvil, (-1, 0, 0));
    let closed = s.station_screen.ui_key();
    s.use_aimed();
    assert!(s.until(3.0, |s| s.station_screen.is_open()), "the anvil never opened: {:?}", s.heard.last());
    let opened = s.station_screen.ui_key();
    assert_ne!(opened, closed, "the opened anvil would not be redrawn");

    s.station_begin(Job::Nails);
    assert!(
        s.until(3.0, |s| s.station_screen.is_running()),
        "the run never began: {:?}",
        s.heard.iter().rev().take(6).collect::<Vec<_>>()
    );
    let began = Instant::now();
    let seed = s
        .heard
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::StationBegun { seed } => Some(*seed),
            _ => None,
        })
        .expect("no seed");
    assert_ne!(s.station_screen.ui_key(), opened, "the running bar would not be redrawn");
    let game = Job::Nails.game();
    let step = game.step_ms();
    // A blow in each sweep, when the rising marker crosses the target.
    for k in 0..game.presses() {
        let at = k as u32 * step + (minigame::target(seed, k) * step as f32 / 2.0) as u32;
        while (began.elapsed().as_millis() as u32) < at {
            s.frame();
        }
        s.station_press();
    }
    let result = s.until(4.0, |s| s.heard_any(|m| matches!(m, ServerMessage::StationResult { .. })));
    assert!(result, "the run was never judged");
    let got = s.until(3.0, |s| s.inventory.count(t::BLOCK_NAILS) > 0);
    let verdict = s.heard.iter().rev().find_map(|m| match m {
        ServerMessage::StationResult { verdict, made } => Some((*verdict, *made)),
        _ => None,
    });
    assert!(got, "the anvil made nothing: {verdict:?}");
    no_corrections(&s);
}

/// Opens the station at `cell` with what is selected, begins `job` and plays
/// every stroke on its sweet spot, then waits for the verdict. The anvil
/// scenario's run, for the stations that came after it.
fn a_run_on_the_marker(s: &mut Scenario, cell: (i32, i32, i32), job: primitive_shared::minigame::Job) -> Option<(primitive_shared::minigame::Verdict, Option<(t::BlockId, u32)>)> {
    use primitive_shared::minigame;
    s.look_at_face(cell, (-1, 0, 0));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.station_screen.is_open()), "{job:?}: the station never opened: {:?}", s.heard.last());
    s.station_begin(job);
    assert!(s.until(3.0, |s| s.station_screen.is_running()), "{job:?}: the run never began: {:?}", s.heard.iter().rev().take(4).collect::<Vec<_>>());
    let began = Instant::now();
    let seed = s
        .heard
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::StationBegun { seed } => Some(*seed),
            _ => None,
        })
        .expect("no seed");
    let game = job.game();
    let step = game.step_ms();
    for k in 0..game.presses() {
        let at = k as u32 * step + (minigame::target(seed, k) * step as f32 / 2.0) as u32;
        while (began.elapsed().as_millis() as u32) < at {
            s.frame();
        }
        s.station_press();
    }
    assert!(s.until(4.0, |s| s.heard_any(|m| matches!(m, ServerMessage::StationResult { .. }))), "{job:?} was never judged");
    s.frames(10);
    s.heard.iter().rev().find_map(|m| match m {
        ServerMessage::StationResult { verdict, made } => Some((*verdict, *made)),
        _ => None,
    })
}

#[test]
fn a_chair_is_cut_at_the_sawhorse_from_pine_boards_and_the_saw_pays_for_it() {
    use primitive_shared::minigame::Job;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let horse = (x0 + 2, GROUND + 1, z);
    s.stand_at(feet_on(x0, z));
    s.build(&[(horse, t::BLOCK_SAWHORSE)]);
    // With nothing sharp in the hand the sawhorse is refused at the door.
    s.look_at_face(horse, (-1, 0, 0));
    s.use_aimed();
    s.seconds(1.0);
    assert!(!s.station_screen.is_open(), "a sawhorse opened for an empty hand");
    s.give(t::BLOCK_COPPER_SAW, 1);
    s.give(t::BLOCK_PINE_PLANKS, 2);
    s.give(t::BLOCK_FRAME, 1);
    s.give(t::BLOCK_STICK, 2);
    s.select(t::BLOCK_COPPER_SAW);
    let result = a_run_on_the_marker(&mut s, horse, Job::Chair);
    let chair = s
        .inventory
        .slots()
        .iter()
        .flatten()
        .find(|stack| t::block_kind(stack.block) == t::BLOCK_CHAIR)
        .map(|stack| stack.block);
    assert!(chair.is_some(), "the sawhorse made no chair: {result:?}");
    let pine = primitive_shared::wood::WOODS.iter().position(|w| w.planks == t::BLOCK_PINE_PLANKS).unwrap();
    assert_eq!(t::furniture_wood(chair.unwrap()), pine, "a chair of pine boards came off the sawhorse as another wood");
    assert_eq!(s.inventory.count(t::BLOCK_FRAME), 0, "the frame was not spent");
    let saw = s.inventory.slots().iter().flatten().find(|stack| t::block_kind(stack.block) == t::BLOCK_COPPER_SAW).copied();
    assert!(saw.is_some_and(|saw| saw.wear() >= 1), "the saw cut a chair for nothing: {saw:?}");
    no_corrections(&s);
}

#[test]
fn a_dull_axe_is_honed_sharp_at_the_honing_stone_for_less_than_the_whetstone_takes() {
    use primitive_shared::minigame::{Job, Verdict};
    use primitive_shared::tools::{blunt_step, edge_swings, with_edge};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let stone = (x0 + 2, GROUND + 1, z);
    s.stand_at(feet_on(x0, z));
    s.build(&[(stone, t::BLOCK_HONING_STONE)]);
    let dull = with_edge(t::BLOCK_BRONZE_AXE, 2);
    s.give(dull, 1);
    s.select(dull);
    let result = a_run_on_the_marker(&mut s, stone, Job::Hone);
    let axe = s
        .inventory
        .slots()
        .iter()
        .flatten()
        .find(|stack| t::block_kind(stack.block) == t::BLOCK_BRONZE_AXE)
        .copied()
        .expect("the axe went missing on the stone");
    assert_eq!(blunt_step(axe.block), 0, "the axe came off the stone still dull: {result:?}");
    // A run on the marker is a fine one, and a fine hone grinds less than
    // the whetstone's (which would carry the wear to a whole edge).
    assert_eq!(result.map(|r| r.0), Some(Verdict::Fine), "a run on the marker was not fine");
    assert!(axe.wear() < edge_swings(t::BLOCK_BRONZE_AXE).unwrap(), "the stone took a whole edge: {}", axe.wear());
    no_corrections(&s);
}

#[test]
fn turf_peels_to_earth_and_the_earth_comes_away_in_quarters() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let cell = (x0 + 1, GROUND, z);
    s.stand_at(feet_on(x0, z));
    assert_eq!(s.block(cell).map(t::block_kind), Some(t::BLOCK_GRASS), "the field is not turf here");
    s.look_at_face(cell, (0, 1, 0));
    s.input.breaking = true;
    let mut seen = vec![s.block(cell).unwrap_or(t::BLOCK_AIR)];
    let mut tops = Vec::new();
    for _ in 0..(40.0 / FRAME) as usize {
        s.frame();
        let now = s.block(cell).unwrap_or(t::BLOCK_AIR);
        if seen.last() != Some(&now) {
            seen.push(now);
            tops.push(s.drawn_bounds(cell, cell).map(|(_, hi)| hi[1]));
            // Keep looking at what is left of it as it sinks.
            s.look_at(glam::DVec3::new(cell.0 as f64 + 0.5, cell.1 as f64 + 0.2, cell.2 as f64 + 0.5));
        }
        if now == t::BLOCK_AIR {
            break;
        }
    }
    s.input.breaking = false;
    let names: Vec<String> = seen.iter().map(|&b| format!("{} {b:#x}", t::block_name(b))).collect();
    assert_eq!(seen.last(), Some(&t::BLOCK_AIR), "the dig never finished: {names:?}");
    assert_eq!(seen.get(1).map(|&b| t::block_kind(b)), Some(t::BLOCK_DIRT), "the first swing did not peel the turf: {names:?}");
    let bites = seen.iter().filter(|&&b| primitive_shared::dig::is_dug(b)).count();
    assert!(bites >= 2, "the earth came away whole rather than a slice at a time: {names:?}");
    // The floor sinks as it goes: every drawn top at or below the last.
    let drawn: Vec<f32> = tops.iter().flatten().copied().collect();
    assert!(drawn.windows(2).all(|w| w[1] <= w[0] + 1e-3), "the dug floor rose: {drawn:?} for {names:?}");
    no_corrections(&s);
}

#[test]
fn a_prop_goes_against_a_wall_and_a_second_one_goes_beside_it() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0, z));
    s.fill((x0 + 2, g + 1, z - 2), (x0 + 2, g + 3, z + 2), t::BLOCK_STONE);
    s.give(t::BLOCK_PROP, 4);
    s.select(t::BLOCK_PROP);
    let wall = (x0 + 2, g + 1, z);
    s.look_at_face(wall, (-1, 0, 0));
    s.use_aimed();
    let first = (x0 + 1, g + 1, z);
    assert!(
        s.until(3.0, |s| s.block(first).is_some_and(t::is_prop)),
        "the first prop never went up: {:?}",
        s.block(first).map(t::block_name)
    );
    let beside_wall = (x0 + 2, g + 1, z - 1);
    s.look_at_face(beside_wall, (-1, 0, 0));
    assert_eq!(s.aimed().map(|(c, _)| c), Some(beside_wall), "the first prop is in the way of the wall beside it");
    s.use_aimed();
    let second = (x0 + 1, g + 1, z - 1);
    assert!(
        s.until(3.0, |s| s.block(second).is_some_and(t::is_prop)),
        "the second prop never went up: {:?}",
        s.block(second).map(t::block_name)
    );
    s.shot("props");
    for cell in [first, second] {
        let block = s.block(cell).expect("prop");
        let (lo, hi) = t::prop_box(block);
        let (drawn_lo, drawn_hi) = s.drawn_bounds(cell, cell).expect("the prop is not drawn");
        let at = [cell.0 as f32, cell.1 as f32, cell.2 as f32];
        for i in [0, 2] {
            assert!(
                (drawn_lo[i] - (at[i] + lo[i])).abs() < 0.05 && (drawn_hi[i] - (at[i] + hi[i])).abs() < 0.05,
                "the prop at {cell:?} is drawn over {drawn_lo:?}..{drawn_hi:?} and stands over {lo:?}..{hi:?}"
            );
        }
        // Against the wall, both of them: the side toward x0 + 2.
        assert!(hi[0] > 0.99, "the prop at {cell:?} is not against the wall: {lo:?}..{hi:?}");
    }
    no_corrections(&s);
}

/// **A gallery driven under sand needs a post in it**, which is the whole
/// of what a pit prop is for (`types::BLOCK_PROP`, `falling::PROP_REACH`).
/// A post set on the floor of a working holds the sand three cells over a
/// player's head -- the height of a gallery you walk down -- and the cell
/// beside it with no post under it comes down at once. Take the post away
/// and what it was holding follows it.
#[test]
fn a_post_on_the_floor_of_a_gallery_holds_the_sand_over_it_and_the_cell_beside_it_caves_in() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0, z));

    // The post, set the way a miner sets one: a click on the floor of the
    // working, which stands it in the middle of its cell.
    s.give(t::BLOCK_PROP, 4);
    s.select(t::BLOCK_PROP);
    let post = (x0 + 1, g + 1, z);
    s.look_at_face((x0 + 1, g, z), (0, 1, 0));
    s.use_aimed();
    let stood = s.until(3.0, |s| s.block(post).is_some_and(t::is_prop));
    assert!(stood, "the post never went up: {:?}", s.block(post).map(t::block_name));

    // The roof of the working: sand three cells over the post, which is
    // over a player's head and was exactly the case the prop used to fail.
    let roof = (x0 + 1, g + 4, z);
    let bare = (x0 + 3, g + 4, z);
    s.server().place_block(roof.0, roof.1, roof.2, t::BLOCK_SAND);
    s.server().place_block(bare.0, bare.1, bare.2, t::BLOCK_SAND);

    // The cell with nothing under it caves in...
    let fell = s.until(5.0, |s| s.block(bare) == Some(t::BLOCK_AIR));
    assert!(fell, "sand hung in the air over an open gallery: {:?}", s.block(bare).map(t::block_name));
    let landed = s.until(3.0, |s| (g..g + 4).any(|y| s.block((x0 + 3, y, z)).map(t::block_kind) == Some(t::BLOCK_SAND)));
    assert!(landed, "the sand left the roof and never reached the floor");
    // ...and the one over the post does not.
    assert_eq!(s.block(roof).map(t::block_kind), Some(t::BLOCK_SAND), "the sand came down on the post as if it were not there");
    // The picture: the post, the sand it is holding three cells over it,
    // and the heap beside it where the same sand had nothing under it.
    s.look_at(glam::DVec3::new(x0 as f64 + 3.0, g as f64 + 3.0, z as f64 + 0.5));
    s.frames(4);
    s.shot("caves/propped-gallery");

    // Break the post out and the roof follows it in the same breath, rather
    // than the next time somebody digs nearby.
    // At the post itself and not at the face of its cell: a prop stands in
    // the middle of its cell and a ray at the cell's face passes beside it.
    s.look_at(glam::DVec3::new(post.0 as f64 + 0.5, post.1 as f64 + 0.5, post.2 as f64 + 0.5));
    assert_eq!(s.aimed().map(|(c, _)| c), Some(post), "the crosshair is not on the post");
    s.input.breaking = true;
    let taken = s.until(20.0, |s| s.block(post) == Some(t::BLOCK_AIR));
    s.input.breaking = false;
    assert!(taken, "the post never came out: {:?}", s.block(post).map(t::block_name));
    let buried = s.until(5.0, |s| s.block(roof) == Some(t::BLOCK_AIR));
    assert!(buried, "the roof stayed up with nothing under it: {:?}", s.block(roof).map(t::block_name));
    no_corrections(&s);
}

/// **A cave wall in bands, and a column of dripstone standing in it.** The
/// beds are what `worldgen::bedded_rock` lays and the column what
/// `dripstone::column` shapes; what this asks is that a player meets them
/// as the shapes they are -- the column is walked into and aimed at over
/// its whole height, and the tip of it is what the crosshair finds.
#[test]
fn a_cave_wall_reads_as_beds_of_rock_and_a_column_of_dripstone_stands_in_it() {
    use primitive_shared::dripstone::{sized, SHAFT, SIZES};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    // A chamber cut into a wall of rock, banded the way a shaft crosses
    // the beds: granite at the foot, sandstone over it, limestone at the
    // roof. Five cells high, so a column of four has room to stand.
    let wall = x0 + 4;
    s.stand_at(feet_on(x0, z));
    // **What it was**, for the picture beside the one below: one grey rock
    // from the roof to the floor of the world, and a spike of one cell
    // hanging in it. See `worldgen::bedded_rock` and `dripstone::column`
    // for what each of the two changes is.
    s.fill((wall, g + 1, z - 3), (wall + 1, g + 6, z + 3), t::BLOCK_STONE);
    s.fill((x0, g + 6, z - 3), (wall - 1, g + 6, z + 3), t::BLOCK_STONE);
    s.build(&[
        ((wall - 1, g + 5, z), sized(t::BLOCK_STALACTITE, SIZES - 2)),
        ((wall - 1, g + 1, z), sized(t::BLOCK_STALAGMITE, 1)),
    ]);
    s.look_at(glam::DVec3::new(wall as f64 - 0.5, g as f64 + 3.5, z as f64 + 0.5));
    s.frames(4);
    s.shot("caves/wall-before");
    // Left where they are rather than taken away: the column below covers
    // both cells, and a cell of this roof emptied under it would be a hole
    // with a shelf of stone over it, which is a collapse (`falling`) in the
    // middle of a scenario about something else.

    s.fill((wall, g + 1, z - 3), (wall + 1, g + 2, z + 3), t::BLOCK_GRANITE);
    s.fill((wall, g + 3, z - 3), (wall + 1, g + 4, z + 3), t::BLOCK_SANDSTONE);
    s.fill((wall, g + 5, z - 3), (wall + 1, g + 6, z + 3), t::BLOCK_LIMESTONE);
    // ...and a roof of limestone over the floor the player stands on.
    s.fill((x0, g + 6, z - 3), (wall - 1, g + 6, z + 3), t::BLOCK_LIMESTONE);

    // A column of three rising from the floor and one hanging to meet it.
    let column = (wall - 1, z);
    s.build(&[
        ((column.0, g + 1, column.1), sized(t::BLOCK_STALAGMITE, SHAFT)),
        ((column.0, g + 2, column.1), sized(t::BLOCK_STALAGMITE, SHAFT)),
        ((column.0, g + 3, column.1), sized(t::BLOCK_STALAGMITE, SIZES - 2)),
        ((column.0, g + 5, column.1), sized(t::BLOCK_STALACTITE, SHAFT)),
        ((column.0, g + 4, column.1), sized(t::BLOCK_STALACTITE, SIZES - 2)),
    ]);
    // The same wall from the same place, for the picture beside the one
    // above: three beds and a column of three under one of two.
    s.look_at(glam::DVec3::new(wall as f64 - 0.5, g as f64 + 3.5, z as f64 + 0.5));
    s.frames(4);
    s.shot("caves/wall-after");

    // **Every cell of it is drawn where it stands**, foot to tip, and the
    // whole column reads as one spike rather than as cones on cones: the
    // shafts are all the same width and the tip is narrower.
    let width = |y: i32| {
        let cell = (column.0, y, column.1);
        let (lo, hi) = s.drawn_bounds(cell, cell).unwrap_or_else(|| panic!("nothing drawn at {cell:?}"));
        hi[0] - lo[0]
    };
    let (foot, middle, tip) = (width(g + 1), width(g + 2), width(g + 3));
    assert!((foot - middle).abs() < 0.01, "the column steps between its shafts: {foot} then {middle}");
    // The tip is as wide *at its foot* as the shaft under it -- that is
    // what makes the column one spike -- so what says it comes to a point
    // is that it is drawn in tiers where a shaft is one box.
    assert!((tip - foot).abs() < 0.01, "the tip does not carry on the shaft's stone: {foot} then {tip}");
    let boxes = |y: i32| triangles_in(&s, (column.0, y, column.1), (column.0, y, column.1)).len();
    assert_eq!(boxes(g + 1), boxes(g + 2), "the two shafts of the column are not the same shape");
    assert!(boxes(g + 3) > boxes(g + 2), "the tip is a box like the shaft: {} against {}", boxes(g + 3), boxes(g + 2));

    // The crosshair finds the cell it is looking at, and not the rock
    // behind it: a column a player cannot break is a column they cannot
    // get past.
    s.look_at_face((column.0, g + 2, column.1), (-1, 0, 0));
    assert_eq!(s.aimed().map(|(c, _)| c), Some((column.0, g + 2, column.1)), "the shaft of the column is not aimed at");

    // ...and walked into, which a spike drawn but not collided is not. A
    // column is not a full cube to the rules that ask about cells (a
    // stalactite's box is up in the air), so what says a body meets it is
    // a body meeting it: the player walks at it and stops a hand's breadth
    // into its cell, well short of the rock face two cells further on.
    s.face(0.0);
    s.hold(Action::Forward);
    s.seconds(3.0);
    s.release_all();
    s.frames(4);
    let stopped = s.player.position.x;
    assert!(
        stopped > column.0 as f64 - 0.5 && stopped < column.0 as f64 + 0.2,
        "the player walked through the column and fetched up at {stopped}"
    );

    // The wall behind it is three rocks in bands, bottom to top.
    let at = |y: i32| s.block((wall, y, z - 2)).map(t::block_kind);
    assert_eq!(at(g + 1), Some(t::BLOCK_GRANITE));
    assert_eq!(at(g + 3), Some(t::BLOCK_SANDSTONE));
    assert_eq!(at(g + 5), Some(t::BLOCK_LIMESTONE));
    no_corrections(&s);
}

#[test]
fn peat_set_down_with_shift_lies_on_the_grass_and_dries_in_the_sun() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.server().console_command("/weather clear");
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_PEAT, 2);
    s.select(t::BLOCK_PEAT);
    let ground = (x0 + 2, GROUND, z);
    let lying = (x0 + 2, GROUND + 1, z);
    s.look_at_face(ground, (0, 1, 0));
    s.hold(Action::Sprint);
    s.use_aimed();
    s.release(Action::Sprint);
    let laid = s.until(3.0, |s| {
        s.chunks.set_down_items().any(|(cell, _, item)| cell == lying && t::block_kind(item) == t::BLOCK_PEAT)
    });
    assert!(laid, "the sod was not set down: {:?}", s.block(lying).map(t::block_name));
    assert!(
        s.block(lying).is_some_and(t::is_set_down),
        "a sod set down with the modifier was built as a block"
    );
    // A breath short of dry; the sun does the rest on the ordinary step.
    s.server().set_peat_progress(lying, 0.999);
    let dried = s.until(8.0, |s| {
        s.chunks.set_down_items().any(|(cell, _, item)| cell == lying && t::block_kind(item) == t::BLOCK_DRIED_PEAT)
    });
    assert!(dried, "the sod never dried in the noon sun");
    no_corrections(&s);
}

#[test]
fn a_wet_pot_set_down_in_the_sun_is_drawn_wet_then_leather_hard_then_bone_dry() {
    use primitive_shared::clay::{dryness, Dryness};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.server().console_command("/weather clear");
    s.stand_at(feet_on(x0, z));
    // Off the hands: the plain id is a wet pot.
    s.give(t::BLOCK_JUG_RAW, 1);
    s.select(t::BLOCK_JUG_RAW);
    let ground = (x0 + 2, GROUND, z);
    let lying = (x0 + 2, GROUND + 1, z);
    s.look_at_face(ground, (0, 1, 0));
    s.hold(Action::Sprint);
    s.use_aimed();
    s.release(Action::Sprint);
    let pot_is = |s: &Scenario, stage: Dryness| {
        s.chunks
            .set_down_items()
            .any(|(cell, _, item)| cell == lying && t::block_kind(item) == t::BLOCK_JUG_RAW && dryness(item) == stage)
    };
    assert!(s.until(3.0, |s| pot_is(s, Dryness::Wet)), "the wet jug was not set down: {:?}", s.block(lying).map(t::block_name));
    // A breath short of each stage; the noon sun does the rest on the
    // ordinary step, and every client near is told what is lying there.
    s.server().set_peat_progress(lying, 0.499);
    assert!(s.until(8.0, |s| pot_is(s, Dryness::LeatherHard)), "the jug never turned leather-hard in the sun");
    s.server().set_peat_progress(lying, 0.999);
    assert!(s.until(8.0, |s| pot_is(s, Dryness::BoneDry)), "the jug never dried through in the sun");
    no_corrections(&s);
}

#[test]
fn a_player_who_dies_in_a_rucksack_finds_everything_in_the_corpse() {
    use primitive_shared::inventory::SLOTS;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_RUCKSACK, 1);
    let slot = (0..SLOTS).find(|&i| s.inventory.block_in(i) == Some(t::BLOCK_RUCKSACK)).expect("rucksack");
    s.send(ClientMessage::Equip { slot: slot as u8 });
    assert!(
        s.until(3.0, |s| s.inventory.backpack_open()),
        "the rucksack went on and opened no squares"
    );
    // More kinds than the pack has squares, so the rucksack's own squares
    // are used too.
    let kinds: Vec<t::BlockId> = t::PLACEABLE_BLOCKS
        .iter()
        .copied()
        .filter(|&b| t::block_kind(b) == b && b != t::BLOCK_AIR && !t::is_liquid(b))
        .take(SLOTS + 4)
        .collect();
    for &kind in &kinds {
        assert_eq!(s.server().give(kind, 1), 0, "{} did not fit", t::block_name(kind));
    }
    assert!(
        s.until(3.0, |s| kinds.iter().all(|&k| s.inventory.count(k) >= 1)),
        "the pack never showed everything given"
    );
    let carried: Vec<(t::BlockId, u32)> = kinds.iter().map(|&k| (k, s.inventory.count(k))).collect();

    // A fall that kills: forty blocks, the real way down.
    let fall_x = x0 + 3;
    s.stand_at((fall_x as f64 + 0.5, (GROUND + 45) as f64, z as f64 + 0.5));
    assert!(s.until(10.0, |s| s.dead.is_some()), "a forty-block fall did not kill: health {}", s.health);
    s.send(ClientMessage::Respawn);
    assert!(s.until(5.0, |s| s.dead.is_none()), "never respawned");
    s.stand_at(feet_on(x0, z));

    let corpse = (fall_x - 3..=fall_x + 3)
        .flat_map(|x| (z - 3..=z + 3).flat_map(move |z| (GROUND..GROUND + 4).map(move |y| (x, y, z))))
        .find(|&c| s.block(c).is_some_and(|b| t::block_kind(b) == t::BLOCK_CORPSE))
        .expect("no corpse near where the player fell");
    s.stand_at(feet_on(corpse.0 - 2, corpse.2));
    s.look_at_face(corpse, (-1, 0, 0));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.chest_screen.is_open()), "the corpse never opened");
    let contents = s
        .heard
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::ChestState { inventory, .. } => Some(inventory.clone()),
            _ => None,
        })
        .expect("no contents");
    for (kind, count) in carried {
        assert_eq!(contents.count(kind), count, "{} was lost with the body", t::block_name(kind));
    }
    assert_eq!(contents.count(t::BLOCK_RUCKSACK), 1, "the rucksack itself was lost with the body");
    no_corrections(&s);
}

// ---------------------------------------------------------------- shelter

/// The last thing the server said about the smoke at the player's eyes.
fn last_smoke(s: &Scenario) -> f32 {
    s.heard
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::Smoke { thickness } => Some(*thickness),
            _ => None,
        })
        .unwrap_or(0.0)
}

/// ...and about the place they stand in.
fn last_shelter(s: &Scenario) -> Option<primitive_shared::shelter::Reading> {
    s.heard.iter().rev().find_map(|m| match m {
        ServerMessage::Shelter { reading } => Some(*reading),
        _ => None,
    })
}

/// A shut stone hut, inside `x..=x+3` by `z..=z+3` and three high, with a
/// lit campfire in the corner at `(x, z)`; with a smoke hole straight over
/// the fire if asked. Returns the fire's cell.
fn stone_hut_with_a_fire(s: &mut Scenario, x: i32, z: i32, smoke_hole: bool) -> (i32, i32, i32) {
    let floor = GROUND + 1;
    s.fill((x - 1, floor, z - 1), (x + 4, floor + 3, z + 4), t::BLOCK_STONE);
    s.fill((x, floor, z), (x + 3, floor + 2, z + 3), t::BLOCK_AIR);
    if smoke_hole {
        s.build(&[((x, floor + 3, z), t::BLOCK_AIR)]);
    }
    // Straight to the server rather than through `build`: a lit fire is
    // the fire map's from the moment it lands, and the block the client is
    // told about is not bound to be the one that was placed.
    let fire = (x, floor, z);
    s.server().place_block(fire.0, fire.1, fire.2, t::BLOCK_CAMPFIRE_LIT);
    s.seconds(0.5);
    fire
}

#[test]
fn a_hut_with_a_fire_and_no_smoke_hole_fills_with_smoke_and_with_one_it_clears_but_is_cooler() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.server().console_command("/weather clear");
    // Out in the field first, so the client has the chunks the huts go in.
    s.stand_at(feet_on(x0 - 3, z));
    let shut = stone_hut_with_a_fire(&mut s, x0, z, false);
    let holed = stone_hut_with_a_fire(&mut s, x0 + 7, z, true);
    let chokes = primitive_shared::wildfire::SMOKE_CHOKES;

    // In the shut hut, in the corner away from the fire. The smoke is set
    // a breath from full rather than waited for (`Server::set_smoke`): what
    // is under test is whether the room lets it go.
    s.stand_at(feet_on(x0 + 3, z + 3));
    s.server().set_smoke(shut, 0.95);
    s.seconds(6.0);
    let in_the_shut = last_smoke(&s);
    assert!(in_the_shut >= chokes, "a hut with no way out for the smoke held only {in_the_shut}");
    let warm = last_shelter(&s).expect("the page was never told about the place");
    assert!(warm.indoors, "a shut stone hut was not a room");
    assert!(!warm.roof_open);

    // The same hut with a hole over the hearth.
    s.stand_at(feet_on(x0 + 7 + 3, z + 3));
    s.server().set_smoke(holed, 0.95);
    let cleared = s.until(20.0, |s| last_smoke(s) < chokes);
    assert!(cleared, "the smoke hole never cleared the room: still {}", last_smoke(&s));
    s.seconds(1.0);
    let cooler = last_shelter(&s).expect("the page was never told about the second hut");
    assert!(cooler.indoors, "a smoke hole unmade the hut");
    assert!(cooler.roof_open, "the page was not told the heat goes up the hole");
    assert!(
        cooler.air_c < warm.air_c - 2.0,
        "the smoke hole cost no warmth: {} under it, {} in the shut hut",
        cooler.air_c,
        warm.air_c
    );
    no_corrections(&s);
}

#[test]
fn a_cairn_piled_on_the_meadow_asks_its_name_and_is_on_the_map_over_the_grass() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_CAIRN, 1);
    s.select(t::BLOCK_CAIRN);
    let ground = (x0 + 2, GROUND, z);
    let cairn = (x0 + 2, GROUND + 1, z);
    s.look_at_face(ground, (0, 1, 0));
    s.use_aimed();
    assert!(
        s.until(3.0, |s| s.block(cairn).is_some_and(|b| t::block_kind(b) == t::BLOCK_CAIRN)),
        "the cairn never went up: {:?}",
        s.block(cairn).map(t::block_name)
    );
    // The name is asked for once, when the server has agreed -- this is
    // what opens the chat box in the frame.
    assert_eq!(s.mining.take_piled_cairn(), Some(cairn), "the piled cairn did not ask for a name");
    assert_eq!(s.mining.take_piled_cairn(), None, "the name was asked for twice");
    // Drawn as tall as the box the feet stand on.
    let (lo, hi) = s.drawn_bounds(cairn, cairn).expect("the cairn is not drawn");
    assert!((hi[1] - lo[1] - 0.75).abs() < 0.05, "the cairn is drawn {} tall and stands 0.75", hi[1] - lo[1]);
    // The map, surveyed from the chunks this client holds, has it -- over
    // the meadow under it, not a patch of building.
    let mut map = crate::logic::map::ExploredMap::default();
    map.note_edit(cairn.0, cairn.2);
    map.catch_up(&s.chunks, std::time::Duration::from_secs(1));
    assert_eq!(map.mark_name(cairn), Some(""), "the cairn is not on the map");
    assert_eq!(map.at(cairn.0, cairn.2).map(|(g, _)| g), Some(crate::logic::map::Ground::Grass));
    map.name_mark(cairn, "the ford");
    assert_eq!(map.mark_name(cairn), Some("the ford"));
    s.shot("cairn");
    no_corrections(&s);
}


// ---------------------------------------------------------------- the stall

/// The pack square holding `block`, as the screen's hit test finds it.
fn pack_square(s: &Scenario, block: t::BlockId) -> (f32, f32) {
    let slot = (0..crate::logic::inventory::SLOTS)
        .find(|&slot| s.inventory.block_in(slot).is_some_and(|b| t::block_kind(b) == t::block_kind(block)))
        .unwrap_or_else(|| panic!("no {} in the pack", t::block_name(block)));
    let rect = crate::ui::chest_screen::slot_rect(primitive_shared::protocol::Side::Pack, slot);
    (rect.centre_x(), rect.centre_y())
}

fn centre(rect: crate::ui::widgets::Rect) -> (f32, f32) {
    (rect.centre_x(), rect.centre_y())
}

/// What the last `ChestState` a client was sent says is in the container.
fn last_contents(s: &Scenario) -> primitive_shared::inventory::Inventory {
    s.heard
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::ChestState { inventory, .. } => Some(inventory.clone()),
            _ => None,
        })
        .expect("never told what is in the container")
}

#[test]
fn a_stall_put_down_by_one_player_is_traded_at_by_another_through_both_screens() {
    use crate::ui::chest_screen::{stall_control_rect, stall_slot_rect, StallControl};
    use primitive_shared::stall::{Offer, Refusal, STOCK, TAKINGS};
    let mut owner = Scenario::new();
    let mut buyer = owner.join("buyer");
    let (x0, z) = FIELD;
    let stall = (x0 + 2, GROUND + 1, z);

    // The owner builds it, with their own hands, so the server knows whose.
    owner.stand_at(feet_on(x0, z));
    owner.give(t::BLOCK_STALL, 1);
    owner.select(t::BLOCK_STALL);
    owner.look_at_face((stall.0, GROUND, stall.2), (0, 1, 0));
    owner.use_aimed();
    assert!(
        owner.until_both(&mut buyer, 3.0, |o, _| o.block(stall).is_some_and(|b| t::block_kind(b) == t::BLOCK_STALL)),
        "the stall never went down"
    );

    // Stocks it, and prices it: four flint for a hide, named by holding a
    // flint off the counter and a hide out of the pack up to the squares.
    owner.give(t::BLOCK_FLINT, 8);
    owner.give(t::BLOCK_HIDE, 1);
    owner.look_at_face(stall, (-1, 0, 0));
    owner.use_aimed();
    assert!(
        owner.until_both(&mut buyer, 3.0, |o, _| o.chest_screen.stall().is_some_and(|v| v.yours)),
        "the owner's stall did not open as theirs"
    );
    let flint = pack_square(&owner, t::BLOCK_FLINT);
    owner.chest_shift_click(flint);
    assert!(
        owner.until_both(&mut buyer, 3.0, |o, _| last_contents(o).count_within(STOCK, t::BLOCK_FLINT) == 8),
        "the flint never went onto the counter"
    );
    owner.chest_click(centre(stall_slot_rect(STOCK.start).unwrap()));
    owner.chest_click(centre(stall_control_rect(StallControl::Give(0))));
    for _ in 0..3 {
        owner.chest_click(centre(stall_control_rect(StallControl::Step { row: 0, take: false, up: true })));
    }
    let hide = pack_square(&owner, t::BLOCK_HIDE);
    owner.chest_click(hide);
    owner.chest_click(centre(stall_control_rect(StallControl::Take(0))));
    let price = Offer { give: t::BLOCK_FLINT, give_count: 4, take: t::BLOCK_HIDE, take_count: 1 };
    assert!(
        owner.until_both(&mut buyer, 3.0, |o, _| o.chest_screen.stall().is_some_and(|v| v.offers[0] == Some(price))),
        "the price never reached the server: {:?}",
        owner.chest_screen.stall()
    );
    assert_eq!(owner.inventory.count(t::BLOCK_HIDE), 1, "naming the price spent the sample");
    owner.shot("stall_owner");

    // The buyer walks up with two hides, opens it, and sees it is not theirs.
    buyer.stand_at(feet_on(stall.0, stall.2 + 2));
    buyer.give(t::BLOCK_HIDE, 2);
    buyer.look_at_face(stall, (0, 0, 1));
    assert_eq!(buyer.aimed().map(|(cell, _)| cell), Some(stall), "the buyer is not looking at the stall");
    buyer.use_aimed();
    assert!(
        buyer.until_both(&mut owner, 3.0, |b, _| b.chest_screen.stall().is_some_and(|v| !v.yours && v.offers[0] == Some(price))),
        "the buyer never saw the price: {:?}",
        buyer.chest_screen.stall()
    );

    // TRADE, where the screen draws it.
    buyer.chest_click(centre(stall_control_rect(StallControl::Action(0))));
    assert!(
        buyer.until_both(&mut owner, 3.0, |b, _| b.inventory.count(t::BLOCK_FLINT) == 4),
        "the trade never came back"
    );
    assert_eq!(buyer.inventory.count(t::BLOCK_HIDE), 1, "the buyer did not pay exactly the price");
    // ...and the owner, still at the counter, saw it happen.
    assert!(
        owner.until_both(&mut buyer, 3.0, |o, _| {
            let store = last_contents(o);
            store.count_within(TAKINGS, t::BLOCK_HIDE) == 1 && store.count_within(STOCK, t::BLOCK_FLINT) == 4
        }),
        "the owner's screen never showed the sale"
    );

    // The buyer cannot help themselves to the rest.
    buyer.send(ClientMessage::ChestMove { from: (primitive_shared::protocol::Side::Chest, 0), to: (primitive_shared::protocol::Side::Pack, 25), half: false });
    assert!(
        buyer.until_both(&mut owner, 3.0, |b, _| b.heard_any(|m| matches!(m, ServerMessage::StallRefused { why: Refusal::NotYours }))),
        "a stranger reaching into the counter was not refused"
    );
    assert_eq!(buyer.inventory.count(t::BLOCK_FLINT), 4, "a stranger took goods off the counter");

    // The owner empties the till.
    owner.chest_shift_click(centre(stall_slot_rect(TAKINGS.start).unwrap()));
    assert!(
        owner.until_both(&mut buyer, 3.0, |o, _| o.inventory.count(t::BLOCK_HIDE) == 2),
        "the owner never collected the hide"
    );
    let store = owner.server().container_at(stall.0, stall.1, stall.2);
    assert_eq!(store.count_within(TAKINGS, t::BLOCK_HIDE), 0);
    assert_eq!(store.count_within(STOCK, t::BLOCK_FLINT), 4);
    no_corrections(&owner);
    no_corrections(&buyer);
}

// ---------------------------------------------------------------- building

/// Right-clicks `times` times to lay on `built`, waiting after each click
/// until the cell has changed -- one stage a click, the way a player lays a
/// wall. Aimed at the top of what stands there if anything does, and at the
/// top of `under` if nothing does: the gesture a player makes either way.
fn lay_on(s: &mut Scenario, under: (i32, i32, i32), built: (i32, i32, i32), times: usize) -> Vec<t::BlockId> {
    let mut seen = Vec::new();
    for _ in 0..times {
        let before = s.block(built);
        let top = s.drawn_bounds(built, built).map_or(0.0, |(_, hi)| f64::from(hi[1]) - built.1 as f64);
        if top > 0.0 {
            s.look_at(glam::DVec3::new(built.0 as f64 + 0.5, built.1 as f64 + top - 0.05, built.2 as f64 + 0.5));
        } else {
            s.look_at_face(under, (0, 1, 0));
        }
        s.use_aimed();
        let changed = s.until(3.0, |s| s.block(built) != before);
        assert!(changed, "a click did not lay anything on {:?}", before.map(t::block_name));
        seen.push(s.block(built).unwrap_or(t::BLOCK_AIR));
    }
    seen
}

#[test]
fn earth_dug_out_comes_as_four_handfuls_and_four_handfuls_heap_back_into_the_hole() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let cell = (x0 + 1, GROUND, z);
    s.stand_at(feet_on(x0, z));
    s.look_at_face(cell, (0, 1, 0));
    s.input.breaking = true;
    let dug = s.until(40.0, |s| s.block(cell) == Some(t::BLOCK_AIR));
    s.input.breaking = false;
    assert!(dug, "the dig never finished: {:?}", s.block(cell).map(t::block_name));
    let got = s.until(5.0, |s| s.inventory.count(t::BLOCK_HANDFUL_EARTH) >= 4);
    assert!(got, "a cell of earth gave {} handfuls", s.inventory.count(t::BLOCK_HANDFUL_EARTH));
    assert_eq!(s.inventory.count(t::BLOCK_DIRT), 0, "the whole block came out as well as its handfuls");

    // ...and back, a quarter a click, on the floor of the hole.
    s.select(t::BLOCK_HANDFUL_EARTH);
    let under = (cell.0, cell.1 - 1, cell.2);
    let stages = lay_on(&mut s, under, cell, 4);
    assert_eq!(stages.last().copied(), Some(t::BLOCK_DIRT), "four handfuls heaped into {stages:?}");
    assert!(primitive_shared::dig::is_dug(stages[0]), "the first handful was not a quarter of a cell");
    assert_eq!(s.inventory.count(t::BLOCK_HANDFUL_EARTH), 0);
    no_corrections(&s);
}

#[test]
fn a_brick_wall_is_laid_course_by_course_in_mortar_and_stands_as_high_as_it_is_drawn() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_BRICK, 4);
    s.give(t::BLOCK_MORTAR, 4);
    s.select(t::BLOCK_BRICK);
    let ground = (x0 + 2, GROUND, z);
    let wall = (x0 + 2, GROUND + 1, z);
    let stages = lay_on(&mut s, ground, wall, 2);
    let names: Vec<&str> = stages.iter().map(|&b| t::block_name(b)).collect();
    assert_eq!(t::block_kind(stages[1]), t::BLOCK_BRICK_COURSES, "{names:?}");
    // The top of what is drawn: a box as wide as its cell has its sides on
    // the cell's walls, which `drawn_bounds` leaves out, so its top is what
    // says how high it stands.
    let (_, hi) = s.drawn_bounds(wall, wall).expect("two courses are not drawn");
    assert!((hi[1] - wall.1 as f32 - 0.5).abs() < 0.05, "two courses are drawn {} high", hi[1] - wall.1 as f32);
    assert!(s.physics_solid(wall), "the courses are drawn and walked through");
    s.shot("brick courses");
    lay_on(&mut s, ground, wall, 2);
    assert_eq!(s.block(wall), Some(t::BLOCK_BRICKS), "four mortared courses are not brickwork");
    assert_eq!(s.inventory.count(t::BLOCK_MORTAR), 0, "a course went on without its trowel");
    assert_eq!(s.inventory.count(t::BLOCK_BRICK), 0);
    no_corrections(&s);
}

#[test]
fn a_cob_lift_is_refused_on_a_wet_one_and_the_wall_waits_as_it_was_left() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_COB, 2);
    s.select(t::BLOCK_COB);
    let ground = (x0 + 2, GROUND, z - 1);
    let wall = (x0 + 2, GROUND + 1, z - 1);
    let first = lay_on(&mut s, ground, wall, 1)[0];
    assert!(primitive_shared::build::is_wet(first), "a fresh lift of cob is not wet");
    let (_, hi) = s.drawn_bounds(wall, wall).expect("the lift is not drawn");
    assert!((hi[1] - wall.1 as f32 - 0.25).abs() < 0.05, "one lift is drawn {} high", hi[1] - wall.1 as f32);
    s.look_at(glam::DVec3::new(wall.0 as f64 + 0.5, wall.1 as f64 + 0.2, wall.2 as f64 + 0.5));
    s.use_aimed();
    let told = s.until(3.0, |s| s.heard_any(|m| matches!(m, ServerMessage::Notice { what: primitive_shared::notice::Notice::LiftStillWet })));
    assert!(told, "a lift went onto a wet one without a word");
    assert_eq!(s.block(wall), Some(first), "the wet lift changed under a refused one");
    assert_eq!(s.inventory.count(t::BLOCK_COB), 1, "the refused lump was spent");
    no_corrections(&s);
}

/// **A course is paid from the pack, not only from the square in the hand.**
/// A dry stone course takes two stones; with the last one in the hand and a
/// stack of them beside it, the server said "you are not carrying enough of
/// that" to a player carrying a hundred and twenty-nine.
#[test]
fn a_course_of_dry_stone_is_laid_with_one_stone_in_the_hand_and_the_rest_in_the_pack() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let stone = t::BLOCK_GRANITE_PEBBLE;
    s.stand_at(feet_on(x0, z));
    let ground = (x0 + 2, GROUND, z + 1);
    let wall = (x0 + 2, GROUND + 1, z + 1);
    let first = primitive_shared::build::lay(stone, stone, false, t::BLOCK_STONE, false).expect("a footing").result;
    s.build(&[(wall, first)]);
    assert!(s.until(3.0, |s| s.block(wall) == Some(first)), "the first course never arrived");
    // A full stack and one more: the one is a square of its own.
    let limit = t::stack_limit(stone);
    s.give(stone, limit + 1);
    let single = (0..crate::logic::inventory::HOTBAR_SLOTS)
        .find(|&slot| s.inventory.block_in(slot) == Some(stone) && s.inventory.count_in(slot) == 1)
        .expect("no square with a single stone");
    s.input.hotbar_slot = single;
    s.send(ClientMessage::SelectSlot { slot: single as u8 });
    s.frames(3);
    let laid = lay_on(&mut s, ground, wall, 1)[0];
    assert_eq!(primitive_shared::build::courses(laid), 2, "{}", t::block_name(laid));
    assert!(s.until(3.0, |s| s.inventory.count(stone) == limit - 1), "the course cost {} stones", limit + 1 - s.inventory.count(stone));
    no_corrections(&s);
}

// ---------------------------------------------------------------- horses

/// A broken horse with a saddle on, standing on the field two blocks along
/// from where the player stands, facing along the strip.
fn saddled_horse(s: &mut Scenario, x: i32, z: i32, bags: bool) -> primitive_shared::protocol::EntityId {
    use primitive_shared::husbandry::Keeping;
    let at = ((x + 2) as f32 + 0.5, (GROUND + 1) as f32, z as f32 + 0.5);
    let horse = s.server().spawn_animal(primitive_shared::animals::Species::Horse, at).expect("no room for a horse");
    let keep = Keeping { trust: 1.0, tame: true, home: Some(at), hunger: 0.0, well_fed: 1.0, ..Keeping::wild() };
    let gear = primitive_shared::horse::Gear {
        saddle: true,
        bags: bags.then(primitive_shared::inventory::Inventory::new),
        rides: 0,
    };
    s.server().keep_animal(horse, keep, Some(gear));
    s.server().face_animal(horse, 0.0);
    let seen = s.until(3.0, |s| s.entities.contains_key(&horse));
    assert!(seen, "the horse never reached the client");
    horse
}

/// Where the ridden horse is and what the server said about it, for a
/// failure message.
fn horse_story(s: &Scenario) -> String {
    format!(
        "horse at {:?} doing {:?} with {:?} wind, player at {:?}, corrections {:?}, said {:?}",
        s.horseback.as_ref().map(|h| h.feet()),
        s.horseback.as_ref().map(|h| h.body.speed()),
        s.horseback.as_ref().map(|h| h.body.wind),
        s.feet(),
        s.corrections,
        s.heard
            .iter()
            .filter(|m| matches!(m, ServerMessage::Chat { .. } | ServerMessage::Error(_) | ServerMessage::Notice { .. } | ServerMessage::Mounted { horse: None, .. }))
            .collect::<Vec<_>>()
    )
}

/// Gets on `horse` the way the click does, and waits until the client is
/// riding it.
fn mount(s: &mut Scenario, horse: primitive_shared::protocol::EntityId) {
    s.send(ClientMessage::Mount { horse });
    let on = s.until(3.0, |s| s.horseback.as_ref().is_some_and(|h| h.horse == horse));
    assert!(on, "never got on the horse: {:?}", s.heard.iter().rev().take(4).collect::<Vec<_>>());
    // The horse turned to look along the strip, the way a rider turns it.
    s.face(0.0);
    s.seconds(0.3);
}

#[test]
fn a_rider_gallops_a_saddled_horse_past_any_sprint_and_the_anticheat_never_corrects_them() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    let horse = saddled_horse(&mut s, x0, z, false);
    mount(&mut s, horse);
    assert_eq!(s.server().player_riding(), Some(horse), "the server does not have the player on the horse");
    let start = s.feet();
    s.hold(Action::Forward);
    s.hold(Action::Sprint);
    let mut top = 0.0f32;
    for _ in 0..(5.0 / FRAME) as usize {
        s.frame();
        top = top.max(s.horseback.as_ref().expect("fell off").body.speed());
    }
    s.release_all();
    let covered = (s.feet() - start).length();
    // A sprinting player covers 6.45 a second. Five seconds of a gallop from
    // a standstill -- two of them getting up to it -- is past what the same
    // five seconds on foot could be, and the pace at the end is half as fast
    // again as the sprint.
    let sprint = f64::from(primitive_shared::animals::NOMINAL_SPRINT_SPEED);
    assert!(covered > sprint * 5.0 * 1.2, "five seconds of gallop covered {covered:.1} blocks");
    assert!(f64::from(top) > sprint * 1.5, "a gallop under a rider was {top:.1} blocks a second: {}", horse_story(&s));
    // The server's horse is where the client rode it, give or take a round trip.
    s.seconds(1.5);
    let server = s.server().animal_position(horse).expect("the horse is gone");
    let client = s.horseback.as_ref().expect("fell off").feet();
    let apart = (client.x - f64::from(server.0)).hypot(client.z - f64::from(server.2));
    assert!(apart < 1.0, "the client's horse stopped {apart:.2} blocks from the server's");
    s.shot("horse_galloped");
    no_corrections(&s);
}

#[test]
fn a_horse_stops_at_the_bank_of_deep_water_and_its_rider_gets_down_beside_it() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    // A river three deep across the strip, eight blocks on.
    s.fill((x0 + 10, GROUND - 2, z - 4), (x0 + 13, GROUND, z + 4), t::BLOCK_WATER);
    let horse = saddled_horse(&mut s, x0, z, false);
    mount(&mut s, horse);
    s.hold(Action::Forward);
    s.seconds(4.0);
    s.release_all();
    s.seconds(1.0);
    let feet = s.horseback.as_ref().expect("fell off").feet();
    assert!(feet.x < (x0 + 10) as f64, "the horse went into the river: {feet:?}");
    assert!(feet.y >= (GROUND + 1) as f64 - 0.01, "the horse is in the water: {feet:?}");
    // Down, with the rein key, standing.
    s.hold(Action::Rein);
    s.frames(2);
    s.release(Action::Rein);
    let down = s.until(3.0, |s| s.horseback.is_none() && s.server().player_riding().is_none());
    assert!(down, "the rein key did not get the rider down");
    s.seconds(1.0);
    assert!(s.player.grounded, "got down into the air");
    let from_horse = (s.feet().x - feet.x).hypot(s.feet().z - feet.z);
    assert!(from_horse > 0.6 && from_horse < 2.0, "got down {from_horse:.2} from the horse");
    no_corrections(&s);
}

#[test]
fn a_horse_walks_up_a_step_and_jumps_a_ditch_it_could_not_climb_out_of() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    // A step up a block across the strip, and past it a ditch two wide and
    // three deep: walked into, it is a pit a horse cannot step out of.
    s.fill((x0 + 8, GROUND + 1, z - 3), (x0 + 40, GROUND + 1, z + 3), t::BLOCK_COBBLESTONE);
    s.fill((x0 + 18, GROUND - 1, z - 3), (x0 + 19, GROUND + 1, z + 3), t::BLOCK_AIR);
    let horse = saddled_horse(&mut s, x0, z, false);
    mount(&mut s, horse);
    s.hold(Action::Forward);
    s.hold(Action::Sprint);
    let up = s.until(4.0, |s| s.horseback.as_ref().is_some_and(|h| h.feet().y > (GROUND + 2) as f64 - 0.01));
    assert!(up, "the horse did not walk up a step: {}", horse_story(&s));
    // At the ditch, at a gallop, the jump asked two strides out.
    let near = s.until(4.0, |s| s.horseback.as_ref().is_some_and(|h| h.feet().x > (x0 + 18) as f64 - 2.4));
    assert!(near, "never came up to the ditch: {}", horse_story(&s));
    let at_the_jump = horse_story(&s);
    s.hold(Action::Jump);
    s.frames(2);
    s.release(Action::Jump);
    let over = s.until(3.0, |s| {
        s.horseback.as_ref().is_some_and(|h| h.feet().x > (x0 + 20) as f64 + 0.5 && h.body.on_ground)
    });
    assert!(over, "the horse did not jump the ditch: {} (at the jump {at_the_jump})", horse_story(&s));
    assert!(
        s.horseback.as_ref().is_some_and(|h| h.feet().y > (GROUND + 2) as f64 - 0.01),
        "it landed in the ditch: {}",
        horse_story(&s)
    );
    s.release_all();
    s.seconds(1.0);
    no_corrections(&s);
}

#[test]
fn saddlebags_on_a_horse_take_a_load_from_beside_it_and_the_load_slows_it() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    let horse = saddled_horse(&mut s, x0, z, true);
    s.give(t::BLOCK_COPPER_ORE, 64);
    s.give(t::BLOCK_COPPER_ORE, 64);
    s.send(ClientMessage::OpenBags { horse });
    let open = s.until(3.0, |s| s.chest_screen.is_open());
    assert!(open, "the saddlebags never opened");
    assert_eq!(s.chest_screen.layout(), crate::ui::chest_screen::Layout::Bags);
    s.send(ClientMessage::ChestBulkMove { to_chest: true });
    let loaded = s.until(3.0, |s| {
        s.server().horse_gear(horse).and_then(|g| g.bags).is_some_and(|b| b.count(t::BLOCK_COPPER_ORE) == 128)
    });
    assert!(loaded, "the ore did not go into the bags: {:?}", s.server().horse_gear(horse).and_then(|g| g.bags).map(|b| b.count(t::BLOCK_COPPER_ORE)));
    // Twelve squares and no more: a thirteenth stack stays in the pack.
    let bags = s.server().horse_gear(horse).and_then(|g| g.bags).expect("bags");
    assert!(bags.slots().iter().skip(primitive_shared::horse::BAGS_SLOTS).all(Option::is_none));
    s.send(ClientMessage::CloseChest);
    s.close_screens();
    s.seconds(0.3);
    mount(&mut s, horse);
    s.hold(Action::Forward);
    s.hold(Action::Sprint);
    s.seconds(3.0);
    let laden = s.horseback.as_ref().expect("fell off").body.speed();
    s.release_all();
    assert!(
        laden < primitive_shared::horse::GALLOP * 0.9,
        "a hundred and twenty-eight ore made no difference to the gallop: {laden:.1}"
    );
    no_corrections(&s);
}

/// **A knife on a living horse takes its tack off**: the bags first, with
/// their load, into the pack; then the saddle. Before this nothing came off a
/// horse but by killing it.
#[test]
fn a_knife_unbuckles_a_horses_bags_with_their_load_and_then_its_saddle() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    let horse = saddled_horse(&mut s, x0, z, true);
    s.give(t::BLOCK_COPPER_ORE, 20);
    s.send(ClientMessage::OpenBags { horse });
    assert!(s.until(3.0, |s| s.chest_screen.is_open()), "the saddlebags never opened");
    s.send(ClientMessage::ChestBulkMove { to_chest: true });
    let loaded = s.until(3.0, |s| s.server().horse_gear(horse).and_then(|g| g.bags).is_some_and(|b| b.count(t::BLOCK_COPPER_ORE) == 20));
    assert!(loaded, "the ore did not go into the bags");
    s.send(ClientMessage::CloseChest);
    s.close_screens();
    s.give(t::BLOCK_FLINT_KNIFE, 1);
    s.select(t::BLOCK_FLINT_KNIFE);
    s.send(ClientMessage::TendAnimal { animal: horse });
    let bags_off = s.until(3.0, |s| s.server().horse_gear(horse).is_some_and(|g| g.bags.is_none()));
    assert!(bags_off, "the knife took nothing off: {}", horse_story(&s));
    let packed = s.until(3.0, |s| s.inventory.count(t::BLOCK_SADDLEBAGS) == 1 && s.inventory.count(t::BLOCK_COPPER_ORE) == 20);
    assert!(packed, "the bags and their load did not come into the pack");
    s.send(ClientMessage::TendAnimal { animal: horse });
    let saddle_off = s.until(3.0, |s| s.inventory.count(t::BLOCK_SADDLE) == 1);
    assert!(saddle_off, "the saddle stayed on: {}", horse_story(&s));
    assert_eq!(s.server().horse_gear(horse).map(|g| g.tack()), Some(0), "the horse still wears something");
    no_corrections(&s);
}

#[test]
fn a_gentled_horse_throws_its_rider_until_it_is_broken_and_then_it_is_theirs() {
    use primitive_shared::husbandry::Keeping;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    let at = ((x0 + 2) as f32 + 0.5, (GROUND + 1) as f32, z as f32 + 0.5);
    let horse = s.server().spawn_animal(primitive_shared::animals::Species::Horse, at).expect("a horse");
    // Fed five times over two days, which a scenario does not wait for.
    s.server().keep_animal(horse, Keeping { trust: 1.0, tame: false, hunger: 0.0, ..Keeping::wild() }, None);
    assert!(s.until(3.0, |s| s.entities.contains_key(&horse)));
    let mut tries = 0;
    while s.horseback.is_none() {
        tries += 1;
        assert!(tries <= primitive_shared::husbandry::THROW_CHANCES.len(), "thrown {tries} times");
        // Back beside it, wherever it went after the last fall.
        if let Some(p) = s.server().animal_position(horse) {
            s.stand_at((f64::from(p.0) + 1.2, f64::from(p.1), f64::from(p.2)));
        }
        s.send(ClientMessage::Mount { horse });
        let answered = s.until(3.0, |s| {
            s.horseback.is_some() || s.heard.iter().any(|m| matches!(m, ServerMessage::Notice { what: Notice::HorseThrowsYou }))
        });
        assert!(answered, "a try on its back came to nothing");
        if s.horseback.is_none() {
            // Thrown: it hurt, and the next try waits for it to settle.
            s.heard.clear();
            s.seconds(primitive_shared::husbandry::SETTLE_SECONDS + 0.5);
        }
    }
    assert_eq!(s.server().player_riding(), Some(horse));
    assert!(
        s.heard.iter().any(|m| matches!(m, ServerMessage::Notice { what: Notice::HorseIsYours })),
        "breaking it was never said"
    );
}

/// **A horse broken against a wall does not throw its rider into the wall.**
/// The throw lands behind where the rider was looking, and it used to land
/// there whatever stood in the way: a player trying a gentled horse with a
/// wall at its far flank came down inside the stone.
#[test]
fn a_gentled_horse_by_a_wall_throws_its_rider_onto_open_ground_and_not_into_the_wall() {
    use primitive_shared::husbandry::Keeping;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0 + 4, z));
    let at = ((x0 + 2) as f32 + 0.5, (GROUND + 1) as f32, z as f32 + 0.5);
    let horse = s.server().spawn_animal(primitive_shared::animals::Species::Horse, at).expect("a horse");
    s.server().keep_animal(horse, Keeping { trust: 1.0, tame: false, hunger: 0.0, ..Keeping::wild() }, None);
    assert!(s.until(3.0, |s| s.entities.contains_key(&horse)));
    // The wall on its far side, once: the throw goes over its back.
    s.fill((x0 + 1, GROUND + 1, z - 1), (x0 + 1, GROUND + 3, z + 1), t::BLOCK_STONE);
    let mut throws = 0;
    for _ in 0..3 * primitive_shared::husbandry::THROW_CHANCES.len() {
        if s.horseback.is_some() || throws >= 2 {
            break;
        }
        // Beside it on its near side, looking away from it.
        let p = s.server().animal_position(horse).expect("the horse went");
        s.stand_at((f64::from(p.0) + 1.3, f64::from(p.1), f64::from(p.2)));
        s.face(0.0);
        s.seconds(0.3);
        s.send(ClientMessage::Mount { horse });
        let answered = s.until(3.0, |s| {
            s.horseback.is_some()
                || s.heard.iter().any(|m| matches!(m, ServerMessage::Notice { what: Notice::HorseThrowsYou | Notice::TooFarAway }))
        });
        assert!(answered, "a try on its back came to nothing: {:?}", s.heard.iter().rev().take(6).collect::<Vec<_>>());
        // It shifted its feet while the player walked round: stand beside it again.
        let too_far = s.heard.iter().any(|m| matches!(m, ServerMessage::Notice { what: Notice::TooFarAway }));
        if too_far {
            s.heard.clear();
        } else if s.horseback.is_none() {
            throws += 1;
            // Where the server put the body, which is the truth: the client
            // may shove itself out of the stone afterwards, and the server's
            // copy of it is still in the wall.
            let feet = s
                .heard
                .iter()
                .find_map(|m| match m {
                    ServerMessage::PositionCorrection { x, y, z, reason } if reason == "thrown" => Some(DVec3::new(*x, *y, *z)),
                    _ => None,
                })
                .expect("thrown without being put anywhere");
            for dx in [-0.29, 0.29] {
                for dz in [-0.29, 0.29] {
                    for dy in [0.1, 1.0, 1.7] {
                        let cell = cell_of(feet + DVec3::new(dx, dy, dz));
                        assert!(!s.physics_solid(cell), "thrown into the wall: feet at {feet:?}, stone at {cell:?}");
                    }
                }
            }
            s.heard.clear();
            s.seconds(primitive_shared::husbandry::SETTLE_SECONDS + 0.5);
        }
    }
    assert!(throws > 0 || s.horseback.is_some(), "never thrown and never on");
}

/// **A rider who leaves the game comes back beside the horse, not in it.**
/// The profile was written with the body on the saddle, so the rider
/// reconnected standing in the middle of their own horse, a metre and a half
/// up, and dropped through it.
#[test]
fn a_rider_who_disconnects_in_the_saddle_comes_back_standing_beside_the_horse() {
    let mut host = Scenario::new();
    let (x0, z) = FIELD;
    host.stand_at(feet_on(x0 - 3, z + 3));
    let mut rider = host.join("rider");
    rider.stand_at(feet_on(x0, z));
    let horse = saddled_horse(&mut rider, x0, z, false);
    mount(&mut rider, horse);
    drop(rider);
    host.seconds(1.0);
    let mut back = host.join("rider");
    assert!(back.until(5.0, |s| s.world_ready), "the rider never came back");
    back.seconds(1.0);
    let feet = back.feet();
    let at = host.server().animal_position(horse).expect("the horse went");
    let apart = (feet.x - f64::from(at.0)).hypot(feet.z - f64::from(at.2));
    assert!(
        apart > f64::from(primitive_shared::horse::HALF_WIDTH),
        "came back {apart:.2} from the horse's middle, inside it: feet {feet:?}, horse {at:?}"
    );
    no_corrections(&back);
}

// ---------------------------------------------------------------- a night out, and the wet

/// Whether every cell of a lean-to is standing, as this client has them.
fn lean_to_at(s: &Scenario, cells: &[((i32, i32, i32), t::BlockId)]) -> bool {
    cells.iter().all(|&(c, want)| s.block(c) == Some(want))
}

/// **A debris hut, from the item to the heap it leaves**: put down in front
/// of the player it is fifteen cells running away from them; walked at, it
/// stops them at its mouth, because the hollow is lower than a body; lain
/// down in, it keeps the rain off; and in the morning it falls in, all of
/// it, to half its sticks and leaves.
///
/// "какого хера шалаш размером с 2 блока": it was two cells and a body
/// walked straight through its roof. This plays the hut it became.
#[test]
fn a_lean_to_keeps_its_sleeper_out_of_the_rain_and_falls_in_at_dawn() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.server().console_command("/weather rain");
    s.server().console_command("/time night");
    s.give(t::BLOCK_LEAN_TO, 1);
    s.select(t::BLOCK_LEAN_TO);
    s.face(0.0);
    s.look_at_face((x0 + 2, GROUND, z), (0, 1, 0));
    s.use_aimed();
    // Its mouth where it was aimed, facing the placer (who looks along +x,
    // so the hut faces west), and the rest of it running on away from them.
    let mouth = (x0 + 2, GROUND + 1, z);
    let cells = primitive_shared::lean_to::cells_from_mouth(mouth, t::Facing::West);
    assert!(
        s.until(3.0, |s| lean_to_at(s, &cells)),
        "the lean-to was not put down whole: {:?}",
        cells.map(|(c, _)| s.block(c).map(|b| format!("{b:#x}")))
    );
    assert_eq!(cells.iter().map(|&((x, _, _), _)| x).max(), Some(x0 + 4), "it is not three cells long");
    assert_eq!(s.inventory.count(t::BLOCK_LEAN_TO), 0, "one item did not go to all fifteen cells");
    s.shot("lean_to");

    // **Nobody walks in upright.** Straight at the mouth for two seconds: the
    // player's middle never passes the mouth's end.
    s.face(0.0);
    s.hold(Action::Forward);
    s.seconds(2.0);
    s.release_all();
    assert!(s.feet().x < (x0 + 2) as f64, "a standing body walked into the hut: {:?}", s.feet());
    assert!(s.feet().x > (x0 + 1) as f64 + 0.3, "the mouth stopped the player a cell short: {:?}", s.feet());
    s.shot("lean_to_mouth");

    // Caught out in it first: the rain is on the player.
    let name = s.name.clone();
    // A long ceiling, which costs nothing when the rain does its work: the
    // wetting is sampled on the server's ticks, and a server starved by a
    // full test run takes fewer of them in six seconds of frames.
    assert!(s.until(20.0, |s| s.server().wetness_of(&name).unwrap_or(0.0) > 0.1), "standing in the rain wet nobody");

    // In, and down: at the bed of leaves inside the mouth. **With the
    // night's roll rigged to spare them**: a lean-to with no fire is found
    // one night in four (`animals::FOUND_UNDER_A_ROOF`), and this is about
    // the rain and the morning, not about wolves -- a found sleeper wakes
    // before dawn and the hut never falls in.
    s.server().set_sleeper_dice(Some(0.99));
    s.look_at(DVec3::new(mouth.0 as f64 + 0.5, mouth.1 as f64 + 0.1, mouth.2 as f64 + 0.5));
    s.use_aimed();
    let down = s.until(3.0, |s| s.heard.iter().any(|m| matches!(m, ServerMessage::Asleep { asleep: true })));
    let said: Vec<&String> = s.heard.iter().filter_map(|m| match m {
        ServerMessage::Error(text) => Some(text),
        _ => None,
    }).collect();
    assert!(down, "the lean-to could not be slept in: aimed at {:?}, told {said:?}", s.aimed().map(|(c, b)| (c, t::block_name(b))));
    // ...and down *inside* it: on the bed of leaves, not on the roof over it
    // -- the cell is as tall as its thatch now -- and not outside the mouth.
    let lying = s.server().position_of(&name).expect("the sleeper is somewhere");
    assert!(
        lying.0 > (x0 + 2) as f64 && lying.0 < (x0 + 4) as f64 && (lying.2 - (z as f64 + 0.5)).abs() < 0.1,
        "the sleeper was laid down outside the hut: {lying:?}"
    );
    assert!(
        (lying.1 - (GROUND + 1) as f64 - f64::from(primitive_shared::lean_to::BED_TOP)).abs() < 0.05,
        "the sleeper was laid on the roof, not on the bed: {lying:?}"
    );
    // Every sample while the leaves are still over the sleeper: none of
    // them wetter than the one before. (The sample taken as the player lay
    // down may have been read where they stood, so it is let go.) The night
    // passes on the server's clock, so this watches until the roof is gone
    // rather than for a fixed while.
    //
    // **Everything here is timed on the server's word, and it was not.** The
    // wait for the stale sample to pass was 0.6 s of client frames, and the
    // roof was the client's copy of the lean-to -- which hears of the
    // collapse a snapshot after the server has woken the sleeper into the
    // morning's rain. On a machine running the whole suite the server fell
    // behind the frames, a sample taken in that gap counted dawn's rain
    // against the roof, and one full run went red. Now: a sample interval of
    // the server's own ticks after lying down (the climate samples every ten
    // at the default rate), and each reading kept only if the server still
    // had the sleeper under a standing roof *after* it was read.
    let roofed = |s: &Scenario| {
        s.server().asleep(&name) && cells.iter().all(|&((x, y, z), want)| s.server().block_at(x, y, z) == Some(want))
    };
    let lay_down_at = s.server().ticks();
    assert!(s.until(10.0, |s| s.server().ticks() >= lay_down_at + 12 || !roofed(s)), "the server stopped ticking");
    let mut under = vec![s.server().wetness_of(&name).unwrap_or(0.0)];
    for _ in 0..40 {
        s.seconds(0.1);
        let wetness = s.server().wetness_of(&name).unwrap_or(1.0);
        if !roofed(&s) {
            break;
        }
        under.push(wetness);
    }
    assert!(
        under.windows(2).all(|w| w[1] <= w[0] + 1e-3),
        "it rained into the lean-to: {under:?}"
    );

    // The night passes -- everybody is asleep -- and the whole hut comes
    // down with the morning, the sleeper on their feet outside its mouth.
    let gone = s.until(30.0, |s| cells.iter().all(|&(c, _)| s.block(c) == Some(t::BLOCK_AIR)));
    assert!(gone, "the lean-to stood through the morning: {:?}", cells.map(|(c, _)| s.block(c).map(t::block_name)));
    s.seconds(0.5);
    let standing = s.feet();
    assert!(standing.x < (x0 + 2) as f64 + 0.05, "the sleeper woke inside the fallen hut, not out of its mouth: {standing:?}");
    // ...beside what is left of it: half its sticks and half its leaves,
    // lying there or already in the pack.
    let heaped = |s: &Scenario, block: t::BlockId| {
        s.inventory.count(block)
            + s.entities
                .values()
                .map(|e| match e.kind {
                    primitive_shared::protocol::EntityKind::Item { block: b, count } if b == block => count,
                    _ => 0,
                })
                .sum::<u32>()
    };
    for (block, count) in t::LEAN_TO_REMAINS {
        assert!(
            s.until(5.0, |s| heaped(s, block) >= count),
            "the fallen hut left {} of {count} {}",
            heaped(&s, block),
            t::block_name(block)
        );
    }
    s.shot("lean_to_fallen");
    no_corrections(&s);
}

#[test]
fn a_swim_soaks_the_kindling_and_a_fire_dries_it_again() {
    use primitive_shared::wet::{is_wet, wetted};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.server().console_command("/weather clear");
    s.stand_at(feet_on(x0 - 3, z));
    s.give(t::BLOCK_STICK, 4);
    s.give(t::BLOCK_STONE_AXE, 1);
    // A pool four deep, its water up to the field.
    s.fill((x0, g - 4, z - 2), (x0 + 5, g, z + 2), t::BLOCK_WATER);
    s.stand_at((x0 as f64 + 2.5, (g - 3) as f64, z as f64 + 0.5));
    let soaked = s.until(4.0, |s| s.inventory.slots().iter().flatten().any(|st| st.block == wetted(t::BLOCK_STICK)));
    assert!(soaked, "a swim left the sticks dry");
    assert!(
        s.inventory.slots().iter().flatten().all(|st| !is_wet(st.block) || t::block_kind(st.block) == t::BLOCK_STICK),
        "something that does not get wet came out of the river wet"
    );

    // Out, and sat by a fire: a breath short of a minute's drying, and the
    // ordinary sample does the rest.
    let fire = (x0 - 5, g + 1, z);
    s.server().place_block(fire.0, fire.1, fire.2, t::BLOCK_CAMPFIRE_LIT);
    s.stand_at(feet_on(x0 - 4, z));
    let name = s.name.clone();
    s.server().set_pack_drying(&name, primitive_shared::wet::PACK_DRIES_SECONDS - 0.5);
    let dry = s.until(4.0, |s| s.inventory.count(t::BLOCK_STICK) == 4 && s.inventory.slots().iter().flatten().all(|st| !is_wet(st.block)));
    assert!(dry, "the sticks never dried by the fire");
    no_corrections(&s);
}

#[test]
fn a_wet_log_laid_in_the_sun_stays_wet_dries_where_it_stands_and_breaks_out_dry() {
    // A wet log laid in a wall and cut out again came back dry, which made a
    // wall and an axe a two-second drying rack.
    use primitive_shared::wet::{is_wet, wetted};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.server().console_command("/weather clear");
    s.server().console_command("/time noon");
    s.stand_at(feet_on(x0, z));
    let log = wetted(primitive_shared::wood::green(t::BLOCK_LOG));
    s.give(log, 1);
    s.give(t::BLOCK_STONE_AXE, 1);
    // Laid on its side against a stone, the way timber goes into a wall --
    // and the way an axe takes it out in a few swings rather than felling it.
    let stone = (x0 + 3, GROUND + 1, z);
    let cell = (x0 + 2, GROUND + 1, z);
    s.build(&[(stone, t::BLOCK_STONE)]);
    let lay = |s: &mut Scenario| {
        s.select(log);
        s.look_at_face(stone, (-1, 0, 0));
        s.use_aimed();
        assert!(s.until(3.0, |s| s.block(cell).is_some_and(|b| t::block_kind(b) == t::BLOCK_LOG)), "the log did not go down");
        s.block(cell).unwrap_or(t::BLOCK_AIR)
    };
    let cut_out = |s: &mut Scenario| {
        s.select(t::BLOCK_STONE_AXE);
        s.look_at_face(cell, (0, 1, 0));
        s.input.breaking = true;
        let gone = s.until(30.0, |s| s.block(cell) == Some(t::BLOCK_AIR));
        s.input.breaking = false;
        assert!(gone, "the log never came out of the wall: {:?}, aimed {:?}, errors {:?}", s.block(cell), s.aimed(), s.heard.iter().filter(|m| matches!(m, ServerMessage::Error(_))).collect::<Vec<_>>());
    };

    let laid = lay(&mut s);
    assert!(is_wet(laid) && t::block_kind(laid) == t::BLOCK_LOG, "the log went down as {laid:#x}");
    assert!(s.server().drying_progress(cell).is_some(), "a wet log in the world is not drying");
    // Cut out at once, it comes back as wet as it went in.
    cut_out(&mut s);
    // In the pack or lying where it fell: the drop, whichever.
    let got = |s: &Scenario, block: t::BlockId| {
        s.inventory.count(block)
            + s.entities
                .values()
                .map(|e| match e.kind {
                    primitive_shared::protocol::EntityKind::Item { block: b, count } if b == block => count,
                    _ => 0,
                })
                .sum::<u32>()
    };
    assert!(s.until(5.0, |s| got(s, log) == 1), "a wet log cut out of the wall came back dry");
    s.stand_at(feet_on(x0 + 2, z));
    assert!(s.until(5.0, |s| s.inventory.count(log) == 1), "the wet log was never picked up");
    s.stand_at(feet_on(x0, z));

    // Laid again, a breath from dry in the noon sun: the ordinary step
    // dries it where it stands, and it comes out a dry log.
    lay(&mut s);
    s.server().set_drying_progress(cell, 0.999);
    let dried = s.until(8.0, |s| s.block(cell).is_some_and(|b| t::block_kind(b) == t::BLOCK_LOG && !is_wet(b)));
    assert!(dried, "a wet log in the noon sun never dried: {:?}", s.server().drying_progress(cell));
    cut_out(&mut s);
    let dry = primitive_shared::wood::green(t::BLOCK_LOG);
    assert!(s.until(5.0, |s| got(s, dry) == 1), "the dried log came out of the wall wet");
    no_corrections(&s);
}

#[test]
fn a_flower_on_a_lip_stands_on_the_lip_and_not_over_it() {
    // Tufts, flowers and tall plants on the lips the generator lays up a
    // slope were drawn from the floor of their own cell: a quarter to three
    // quarters of a block over the grass they grew in, daylight under every
    // stem. Drawn now from the lip's real top (`types::stand_drop`).
    use crate::engine::mesh::PLANTS_ON_THEIR_OWN_FLOOR;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.server().console_command("/time noon");
    let fireweed = t::BLOCK_FIREWEED;
    let plants = [t::BLOCK_TALL_GRASS, t::BLOCK_FLOWER, t::BLOCK_MUSHROOM, fireweed];
    let mut stands = Vec::new();
    for (i, &plant) in plants.iter().enumerate() {
        for quarters in 1..=3u8 {
            let (x, pz) = (x0 + 3 + quarters as i32, z - 3 + 2 * i as i32);
            s.server().place_block(x, g, pz, primitive_shared::dig::lowered(t::BLOCK_GRASS, quarters));
            s.server().place_block(x, g + 1, pz, plant);
            if plant == fireweed {
                s.server().place_block(x, g + 2, pz, plant | t::PLANT_TOP);
            }
            stands.push(((x, g + 1, pz), f32::from(quarters) / 4.0));
        }
    }
    s.stand_at(feet_on(x0, z));
    s.look_at(glam::DVec3::new(f64::from(x0 + 5), f64::from(g) + 1.0, f64::from(z) + 0.5));
    assert!(s.until(5.0, |s| stands.iter().all(|&(c, _)| s.block(c).is_some_and(t::is_cross))), "the plants never arrived");

    // The lowest sprite corner over each lip is the lip's top, to a hair.
    let foot = |s: &Scenario, (x, y, pz): (i32, i32, i32)| {
        let pos = ChunkPos::from_world(x, pz);
        let mesh = s.mesh(pos);
        let origin = [pos.x as f32 * 16.0, pos.z as f32 * 16.0];
        mesh.indices[mesh.leaf_end as usize..mesh.sprite_end as usize]
            .iter()
            .map(|&i| mesh.vertices[i as usize].position)
            .filter(|p| (p[0] + origin[0]).floor() as i32 == x && (p[2] + origin[1]).floor() as i32 == pz)
            .filter(|p| p[1] >= y as f32 - 1.0 && p[1] < y as f32 + 1.0)
            .map(|p| p[1])
            .fold(f32::MAX, f32::min)
    };
    PLANTS_ON_THEIR_OWN_FLOOR.with(|c| c.set(true));
    s.shot("tufts_on_lips_before");
    PLANTS_ON_THEIR_OWN_FLOOR.with(|c| c.set(false));
    s.shot("tufts_on_lips_after");
    for &(cell, top) in &stands {
        let lip = (cell.1 - 1) as f32 + top;
        let at = foot(&s, cell);
        assert!((at - lip).abs() < 0.01, "the plant at {cell:?} stands at {at}, the lip it grows on at {lip}");
    }
    no_corrections(&s);
}

// ---------------------------------------------------------------- water

/// A pond four cells long in a stone basin, full to the field, with a bank
/// of `bank` cells at its +x end and a dry trench past the bank.
fn a_pond_with_a_bank(s: &mut Scenario, bank: t::BlockId) -> (i32, i32) {
    let (x0, z) = FIELD;
    let g = GROUND;
    s.fill((x0 - 1, g - 2, z - 2), (x0 + 8, g, z + 2), t::BLOCK_STONE);
    s.fill((x0 + 4, g, z - 1), (x0 + 4, g, z + 1), bank);
    s.fill((x0 + 5, g, z - 1), (x0 + 7, g, z + 1), t::BLOCK_AIR);
    s.fill((x0, g, z - 1), (x0 + 3, g, z + 1), t::BLOCK_AIR);
    for x in x0..=x0 + 3 {
        for dz in -1..=1 {
            s.server().place_block(x, g, z + dz, t::BLOCK_WATER);
        }
    }
    (x0, z)
}

#[test]
fn a_trench_dug_down_beside_a_pond_is_filled_with_handfuls_and_holds_the_pond_back() {
    // The bank between a pond and a trench, dug down to its last quarter --
    // what a spade leaves -- used to be washed out by the still water beside
    // it the moment it was cut, and the pond ran into the trench. Still
    // water leaves a heap; the player heaps it back up a handful at a time.
    let mut s = Scenario::new();
    let g = GROUND;
    s.stand_at(feet_on(FIELD.0 + 5, FIELD.1 + 2));
    let (x0, z) = a_pond_with_a_bank(&mut s, primitive_shared::dig::heaped(t::BLOCK_DIRT));
    s.give(t::BLOCK_HANDFUL_EARTH, 3);
    s.select(t::BLOCK_HANDFUL_EARTH);
    s.seconds(3.0);
    for dz in -1..=1 {
        assert!(
            s.block((x0 + 4, g, z + dz)).is_some_and(primitive_shared::dig::is_dug),
            "still water took the heap at {dz}: {:?}",
            s.block((x0 + 4, g, z + dz)).map(t::block_name)
        );
    }
    let bank = (x0 + 4, g, z);
    let stages = lay_on(&mut s, (bank.0, g - 1, bank.2), bank, 3);
    assert_eq!(stages.last().copied(), Some(t::BLOCK_DIRT), "three handfuls heaped into {stages:?}");
    s.seconds(3.0);
    for dz in -1..=1 {
        let trench = (x0 + 5, g, z + dz);
        assert_eq!(s.block(trench), Some(t::BLOCK_AIR), "the pond got past the bank into the trench at {dz}");
    }
    s.shot("a_trench_by_a_pond_holds");
    no_corrections(&s);
}

#[test]
fn a_heap_in_a_running_stream_is_carried_downstream_as_its_handfuls() {
    // Running water still takes a heap -- and now what it takes goes down
    // the stream as the earth it was, rather than out of the world.
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0 + 6, z + 3));
    // A channel a cell wide, sixteen long, with a pond at its head.
    s.fill((x0 - 1, g - 2, z - 1), (x0 + 16, g, z + 1), t::BLOCK_STONE);
    s.fill((x0, g, z), (x0 + 15, g, z), t::BLOCK_AIR);
    let heap = (x0 + 6, g, z);
    s.server().place_block(heap.0, heap.1, heap.2, primitive_shared::dig::heaped(t::BLOCK_DIRT));
    for x in x0..=x0 + 3 {
        s.server().place_block(x, g, z, t::BLOCK_WATER);
    }
    let washed = s.until(20.0, |s| s.block(heap).is_some_and(|b| !primitive_shared::dig::is_dug(b)));
    assert!(washed, "the stream ran round a heap in its bed");
    let carried = |s: &Scenario| {
        s.entities.values().any(|e| {
            matches!(e.kind, primitive_shared::protocol::EntityKind::Item { block, .. } if block == t::BLOCK_HANDFUL_EARTH)
                && e.x >= f64::from(heap.0)
        })
    };
    assert!(s.until(5.0, carried), "the heap went out of the world instead of down the stream");
    no_corrections(&s);
}

#[test]
fn two_pools_joined_by_a_trench_come_to_one_level() {
    // The neighbour rules alone left a wedge in the trench and the far
    // pool three quarters of a block below the near one, for ever. Played
    // through the real server's flow, levelling and broadcast, and read
    // off what this client was told.
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0 - 3, z));
    s.fill((x0, g - 2, z - 2), (x0 + 12, g, z + 2), t::BLOCK_STONE);
    s.fill((x0 + 9, g, z - 1), (x0 + 11, g, z + 1), t::BLOCK_AIR);
    s.fill((x0 + 4, g, z), (x0 + 8, g, z), t::BLOCK_AIR);
    s.fill((x0 + 1, g, z - 1), (x0 + 3, g, z + 1), t::BLOCK_AIR);
    // The water straight into the world rather than through `fill`, which
    // waits to see the blocks it wrote -- and these start moving at once.
    for x in x0 + 1..=x0 + 3 {
        for dz in -1..=1 {
            s.server().place_block(x, g, z + dz, t::BLOCK_WATER);
        }
    }
    let depth = |s: &Scenario, x: i32| s.block((x, g, z)).map_or(0, primitive_shared::fluid::depth);
    let profile = |s: &Scenario| (x0 + 1..=x0 + 11).map(|x| depth(s, x)).collect::<Vec<u8>>();
    // Until two looks two seconds apart agree: it has stopped moving, and
    // it has to stop -- a pair trading an eighth for ever would never pass.
    let mut last = profile(&s);
    let mut settled = false;
    for _ in 0..30 {
        s.seconds(2.0);
        let now = profile(&s);
        if now == last && now.iter().any(|&d| d > 0) {
            settled = true;
            break;
        }
        last = now;
    }
    s.shot("two_pools_level");
    assert!(settled, "the pools were still moving after a minute: {last:?}");
    let (near, far) = (depth(&s, x0 + 2), depth(&s, x0 + 10));
    assert!(
        far >= 2 && near.abs_diff(far) <= 1 && (x0 + 4..=x0 + 8).all(|x| depth(&s, x) >= 2),
        "the two pools came to rest at different levels: {last:?}"
    );
    no_corrections(&s);
}

#[test]
fn a_swimmer_beside_a_cut_in_a_pond_is_carried_toward_it() {
    // A pond three deep, walled on the side of a deep pit; the player
    // floats in it, pressing nothing, and the wall is cut. Rivers carried
    // a swimmer already; water a player set running did not.
    //
    // **Beside the cut, not in the middle of the pond**, and that is what
    // the rule is rather than a convenience: a pond drawn on as one body
    // (`draw_on_the_body`) goes down evenly everywhere, so the only cells
    // handing water on across a difference the rules would move are the
    // ones at the lip -- which is where the water is visibly running, and
    // where a swimmer who lets it will be taken over.
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0 - 3, z));
    // Dug into the field's own ground, as a player would: a pond of 360
    // eighths, and a pit beside it that holds 504.
    s.fill((x0 + 7, g - 6, z - 1), (x0 + 9, g, z + 1), t::BLOCK_AIR);
    s.fill((x0 + 1, g - 3, z - 1), (x0 + 5, g - 1, z + 1), t::BLOCK_WATER);
    s.fill((x0 + 1, g, z - 1), (x0 + 5, g, z + 1), t::BLOCK_AIR);
    s.stand_at((x0 as f64 + 5.4, (g - 1) as f64, z as f64 + 0.5));
    s.seconds(1.0);
    assert!(s.player.in_water, "the scenario did not put the player in the water");
    let still = s.feet().x;
    s.seconds(1.0);
    let drift = s.feet().x - still;
    assert!(drift.abs() < 0.15, "a still pond carried the swimmer {drift:.2} blocks");

    let before = s.feet().x;
    for y in g - 3..=g - 1 {
        s.server().place_block(x0 + 6, y, z, t::BLOCK_AIR);
    }
    // **Until it has, not for a fixed while.** Two and a half seconds of
    // wall clock was enough alone and not in a full run, where the server
    // ticks behind a busy machine and the same pour takes longer to arrive:
    // the property is that the water takes the swimmer, not how fast this
    // machine is today.
    //
    // Twenty was still not enough, and the numbers say why: a run that
    // passes takes six seconds and one that fails spends the whole budget
    // and gets a third of the way, which is a server tick being starved
    // rather than water that does not run. Measured at one in three on a
    // machine with half a dozen builds on it. The wait costs nothing when
    // the water arrives, because `until` returns the moment it does.
    let taken = s.until(60.0, |s| s.feet().x - before > 0.5);
    let carried = s.feet().x - before;
    s.shot("carried_to_the_cut");
    assert!(taken, "the pond poured out beside the swimmer and carried them {carried:.2} blocks");
    no_corrections(&s);
}

// ---------------------------------------------------------------- the larder, the trapline and the pack

/// Crafts the row called `name` once, from the pack, the way the menu does.
fn craft(s: &mut Scenario, name: &str) {
    let index = primitive_shared::crafting::RECIPES
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("no row called {name}"));
    s.send(ClientMessage::Craft { index: index as u16, times: 1 });
}

/// The pack's slot holding something of `kind`.
fn slot_of(s: &Scenario, kind: t::BlockId) -> Option<usize> {
    (0..primitive_shared::inventory::SLOTS).find(|&i| s.inventory.block_in(i).map(t::block_kind) == Some(kind))
}

#[test]
fn a_snare_in_the_long_grass_takes_a_hare_only_while_nobody_is_near_and_gives_it_up_as_a_carcass() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let cell = (x0 + 2, GROUND + 1, z);
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_SNARE, 1);
    s.select(t::BLOCK_SNARE);
    s.look_at_face((cell.0, GROUND, cell.2), (0, 1, 0));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.block(cell) == Some(t::BLOCK_SNARE)), "the snare was not set: {:?}", s.block(cell).map(t::block_name));
    // Long grass all round it: a run a hare keeps to. The two cells toward
    // the trapper are left bare, so the snare can still be reached.
    let mut grass = Vec::new();
    for dx in -2..=2 {
        for dz in -2..=2 {
            if (dx, dz) != (0, 0) && (dx, dz) != (-2, 0) && (dx, dz) != (-1, 0) {
                grass.push(((cell.0 + dx, cell.1, cell.2 + dz), t::BLOCK_TALL_GRASS));
            }
        }
    }
    s.build(&grass);
    s.seconds(0.5);
    // A day of the clock with the trapper standing beside it: nothing comes.
    for _ in 0..8 {
        s.server().step_traps();
    }
    assert_eq!(s.block(cell), Some(t::BLOCK_SNARE), "a hare came to a snare with somebody standing over it");
    // Away across the field, out of the hare's nose, and the clock runs on.
    s.stand_at(feet_on(x0 - 20, z));
    s.seconds(0.3);
    let mut steps = 0;
    while s.block(cell).map(t::block_kind) != Some(t::BLOCK_SNARE_CAUGHT) && steps < 80 {
        s.server().step_traps();
        s.frame();
        steps += 1;
    }
    s.seconds(0.3);
    assert_eq!(s.block(cell).map(t::block_kind), Some(t::BLOCK_SNARE_CAUGHT), "no hare in {steps} steps in good cover");
    s.shot("snare_caught");
    // Back, and a hand at it: the hare lies where it hung, and the snare is
    // in the pack to be set again.
    s.stand_at(feet_on(x0, z));
    s.look_at(DVec3::new(cell.0 as f64 + 0.5, cell.1 as f64 + 0.03, cell.2 as f64 + 0.5));
    s.use_aimed();
    let hare = primitive_shared::animals::carcass_at_stage(primitive_shared::animals::Species::Hare, 0);
    assert!(s.until(3.0, |s| s.block(cell) == Some(hare)), "the hare was not laid down: {:?}", s.block(cell).map(t::block_name));
    assert!(s.until(2.0, |s| s.inventory.count(t::BLOCK_SNARE) == 1), "the snare did not come back into the pack");
    no_corrections(&s);
}

#[test]
fn a_salt_pan_of_the_sea_dries_to_salt_under_a_clear_sky_and_rain_puts_it_back() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let cell = (x0 + 2, GROUND + 1, z);
    s.server().console_command("/weather clear");
    s.server().console_command("/time noon");
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_SALT_PAN, 1);
    s.select(t::BLOCK_SALT_PAN);
    s.look_at_face((cell.0, GROUND, cell.2), (0, 1, 0));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.block(cell) == Some(t::BLOCK_SALT_PAN)), "the pan was not laid");
    // A jug of the sea, poured in: the jug comes back empty.
    s.give(t::jug_of(primitive_shared::body::Water::Salt), 1);
    s.select(t::BLOCK_JUG_WATER);
    s.look_at(DVec3::new(cell.0 as f64 + 0.5, cell.1 as f64 + 0.2, cell.2 as f64 + 0.5));
    s.use_aimed();
    assert!(
        s.until(3.0, |s| s.block(cell).map(t::block_kind) == Some(t::BLOCK_SALT_PAN_BRINE)),
        "the sea was not poured in: {:?}",
        s.block(cell).map(t::block_name)
    );
    assert!(s.until(2.0, |s| s.inventory.count(t::BLOCK_JUG) == 1), "the jug did not come back empty");
    // Two steps of sun, then a shower: back to the beginning.
    s.server().step_traps();
    s.server().step_traps();
    s.seconds(0.2);
    let dried = s.block(cell).map(primitive_shared::saltpan::dried).unwrap_or(0);
    assert!(dried > 0, "the noon sun dried nothing (the air is {:?})", s.server().player_ambient());
    s.server().console_command("/weather rain");
    s.seconds(1.0);
    s.server().step_traps();
    s.seconds(0.2);
    assert_eq!(s.block(cell), Some(primitive_shared::saltpan::brine(0)), "the rain did not put the pan back to the sea");
    // Clear again, and it dries through.
    s.server().console_command("/weather clear");
    s.seconds(1.0);
    for _ in 0..16 {
        if s.block(cell) == Some(t::BLOCK_SALT_PAN_SALT) {
            break;
        }
        s.server().step_traps();
        s.seconds(0.1);
    }
    assert_eq!(s.block(cell), Some(t::BLOCK_SALT_PAN_SALT), "two days of sun left no crust");
    s.shot("salt_pan_salt");
    s.select(t::BLOCK_JUG);
    s.use_aimed();
    assert!(
        s.until(3.0, |s| s.inventory.count(t::BLOCK_SALT) == primitive_shared::saltpan::YIELD),
        "the crust was not scraped into the pack"
    );
    assert!(s.until(2.0, |s| s.block(cell) == Some(t::BLOCK_SALT_PAN)), "the scraped pan was not empty");
    no_corrections(&s);
}

#[test]
fn a_deer_that_steps_on_a_pit_cover_falls_in_and_stays_and_so_does_a_player() {
    use primitive_shared::animals::Species;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    // Two pits two deep, each with a cover flush with the field.
    let deer_pit = (x0 + 8, z);
    let our_pit = (x0 + 3, z);
    s.stand_at(feet_on(x0, z));
    for (x, z) in [deer_pit, our_pit] {
        s.fill((x, GROUND - 1, z), (x, GROUND, z), t::BLOCK_AIR);
        s.server().place_block(x, GROUND + 1, z, t::BLOCK_PIT_COVER);
    }
    s.seconds(0.3);
    let deer = s
        .server()
        .spawn_animal(Species::Deer, (deer_pit.0 as f32 + 0.5, GROUND as f32 + 1.2, deer_pit.1 as f32 + 0.5))
        .expect("a deer");
    let fell = s.until(4.0, |s| s.block((deer_pit.0, GROUND + 1, deer_pit.1)) == Some(t::BLOCK_AIR));
    assert!(fell, "the cover held a deer");
    s.seconds(3.0);
    let at = s.server().animal_position(deer).expect("the deer");
    assert!(at.1 < GROUND as f32 + 0.5, "the deer is not in the pit: {at:?}");
    s.seconds(5.0);
    let later = s.server().animal_position(deer).expect("the deer");
    assert!(later.1 < GROUND as f32 + 0.5, "the deer climbed out of a pit two deep: {later:?}");
    // The cover does not know who stands on it.
    s.stand_at((our_pit.0 as f64 + 0.5, GROUND as f64 + 1.2, our_pit.1 as f64 + 0.5));
    assert!(
        s.until(3.0, |s| s.block((our_pit.0, GROUND + 1, our_pit.1)) == Some(t::BLOCK_AIR)),
        "a player walked over a pit's cover"
    );
    assert!(s.until(3.0, |s| s.feet().y < GROUND as f64 + 0.5), "the player did not fall in: {:?}", s.feet());
    s.shot("pit_trap");
}

#[test]
fn snowshoes_cross_a_drift_far_faster_than_bare_feet() {
    use primitive_shared::inventory::SLOTS;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0 - 4, z));
    s.fill((x0 - 2, GROUND + 1, z - 2), (x0 + 30, GROUND + 1, z + 2), t::BLOCK_SNOW);
    let walked = |s: &mut Scenario| {
        s.stand_at((x0 as f64 + 0.5, GROUND as f64 + 2.0, z as f64 + 0.5));
        s.seconds(0.4);
        let start = s.feet().x;
        s.face(0.0);
        s.hold(Action::Forward);
        s.seconds(2.0);
        s.release_all();
        s.seconds(0.2);
        s.feet().x - start
    };
    let bare = walked(&mut s);
    s.give(t::BLOCK_SNOWSHOES, 1);
    let slot = (0..SLOTS).find(|&i| s.inventory.block_in(i) == Some(t::BLOCK_SNOWSHOES)).expect("snowshoes");
    s.send(ClientMessage::Equip { slot: slot as u8 });
    assert!(s.until(3.0, |s| s.equipment.snowshoes()), "the snowshoes did not go on");
    let shod = walked(&mut s);
    assert!(shod > bare * 1.3, "snowshoes walked {shod:.2} where bare feet walked {bare:.2}");
    no_corrections(&s);
}

#[test]
fn milk_pressed_with_salt_ripens_to_cheese_in_a_cellar_and_mead_warms_a_cold_drinker() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    // Cheese: two bowls and a handful of salt, the bowls back.
    s.give(t::BLOCK_BOWL_MILK, 2);
    s.give(t::BLOCK_SALT, 1);
    craft(&mut s, "press cheese");
    assert!(s.until(3.0, |s| slot_of(s, t::BLOCK_CURD).is_some()), "no young cheese was pressed");
    assert_eq!(s.inventory.count(t::BLOCK_BOWL), 2, "the bowls were not given back");
    // Two days in a cellar's air.
    s.server().work_pack(8, 8.0);
    assert!(s.until(2.0, |s| s.inventory.count(t::BLOCK_CHEESE) == 1), "two days in the cellar did not ripen it");
    // Mead: two combs in a jug of the river, a day in the warm.
    s.give(t::BLOCK_HONEY, 2);
    s.give(t::jug_of(primitive_shared::body::Water::Fresh), 1);
    craft(&mut s, "set mead");
    assert!(s.until(3.0, |s| slot_of(s, t::BLOCK_JUG_MUST).is_some()), "no must was set");
    s.server().work_pack(4, 20.0);
    assert!(s.until(2.0, |s| s.inventory.count(t::BLOCK_JUG_MEAD) == 1), "a warm day did not make mead");
    // Drunk cold, on a full stomach: warmer, and the jug back.
    s.server().chill_player(primitive_shared::body::CHILLED);
    let slot = slot_of(&s, t::BLOCK_JUG_MEAD).expect("the mead");
    s.send(ClientMessage::Eat { slot: slot as u8 });
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_JUG) == 1), "the mead was not drunk, or its jug kept");
    let warm = s.server().player_body_c().expect("a body");
    assert!(warm > primitive_shared::body::CHILLED + 2.0, "the mead warmed nobody: {warm}");
    // Pemmican: the rack's meat, fat and berries, pounded by hand.
    s.give(t::BLOCK_DRIED_MEAT, 2);
    s.give(t::BLOCK_FAT, 1);
    s.give(t::BLOCK_BERRIES, 1);
    craft(&mut s, "pemmican");
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_PEMMICAN) == 2), "no pemmican was pounded");
    no_corrections(&s);
}

#[test]
fn a_knife_takes_bark_off_a_birch_and_a_willow_and_bast_off_a_nettle_and_the_bark_binds_a_bruise() {
    use primitive_shared::injury::{Kind, Part, Treatment};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.build(&[
        ((x0 + 2, GROUND + 1, z), t::BLOCK_BIRCH_LOG),
        ((x0 + 2, GROUND + 2, z), t::BLOCK_BIRCH_LOG),
        ((x0, GROUND + 1, z + 2), t::BLOCK_WILLOW_LOG),
        ((x0, GROUND + 2, z + 2), t::BLOCK_WILLOW_LOG),
        ((x0 - 2, GROUND + 1, z), t::BLOCK_NETTLE),
    ]);
    s.give(t::BLOCK_FLINT_KNIFE, 1);
    s.select(t::BLOCK_FLINT_KNIFE);
    s.look_at(DVec3::new(x0 as f64 + 2.0, GROUND as f64 + 2.2, z as f64 + 0.5));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_BIRCH_BARK) == 1), "the birch gave no bark");
    s.look_at(DVec3::new(x0 as f64 + 0.5, GROUND as f64 + 2.2, z as f64 + 2.0));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_WILLOW_BARK) == 1), "the willow gave no bark");
    assert_eq!(s.inventory.count(t::BLOCK_RESIN), 0, "a hardwood bled resin");
    // The nettle, cut with the knife: bast where the fibre would have been.
    let nettle = (x0 - 2, GROUND + 1, z);
    s.look_at(DVec3::new(nettle.0 as f64 + 0.5, nettle.1 as f64 + 0.3, nettle.2 as f64 + 0.5));
    s.input.breaking = true;
    let cut = s.until(8.0, |s| s.block(nettle) == Some(t::BLOCK_AIR));
    s.input.breaking = false;
    assert!(cut, "the nettle was not cut");
    s.stand_at(feet_on(nettle.0, nettle.2));
    assert!(s.until(4.0, |s| s.inventory.count(t::BLOCK_NETTLE_BAST) == 1), "the nettle gave no bast");
    assert_eq!(s.inventory.count(t::BLOCK_FIBER), 0, "the knife took fibre as well as bast");
    // A bruised leg, and the bark bound on it.
    s.server().injure(Part::LeftLeg, Kind::Bruise, 0.8);
    s.seconds(0.3);
    let slot = slot_of(&s, t::BLOCK_WILLOW_BARK).expect("the bark");
    s.send(ClientMessage::TreatInjury { slot: slot as u8, part: Part::LeftLeg.index() as u8 });
    let bound = s.until(3.0, |s| {
        s.server()
            .player_injuries()
            .is_some_and(|w| w.wound(Part::LeftLeg, Kind::Bruise).dressed == Some(Treatment::WillowBark))
    });
    assert!(bound, "the bark was not bound on the bruise");
    assert!(s.until(2.0, |s| s.inventory.count(t::BLOCK_WILLOW_BARK) == 0), "the bark was not spent");
    no_corrections(&s);
}

// ---------------------------------------------------------------- steps

/// Which way is *up* a step of this facing: the side its riser stands on
/// (`geometry::step_boxes`), as a unit step in x and z.
fn up_of(facing: Facing) -> (i32, i32) {
    match facing {
        Facing::North => (0, 1),
        Facing::East => (-1, 0),
        Facing::South => (0, -1),
        Facing::West => (1, 0),
    }
}

/// Holds forward along `yaw` until `done` or `seconds`, and answers every
/// rise of the feet from one frame to the next.
fn walk_until(s: &mut Scenario, yaw: f32, seconds: f32, done: impl Fn(&Scenario) -> bool) -> Vec<f64> {
    s.face(yaw);
    s.hold(Action::Forward);
    let mut rises = Vec::new();
    let mut last = s.feet().y;
    for _ in 0..(seconds / FRAME) as usize {
        s.frame();
        let y = s.feet().y;
        if y > last + 1e-4 {
            rises.push(y - last);
        }
        last = y;
        if done(s) {
            break;
        }
    }
    s.release_all();
    s.seconds(0.4);
    rises
}

/// **Up and down a flight of every kind of step, from each of the four
/// sides it can face, straight and at a slant, with the server watching.**
///
/// "у ступенек странная коллизия": a flight is three steps on a rising
/// fill, five cells wide, to a landing. Each is climbed from its low side
/// and walked back down, head-on and twenty-five degrees off, and every
/// frame's rise is at most a step's height -- never the whole block a
/// riser caught by the wrong side would give -- and nothing is ever
/// corrected. The kinds take turns by facing so every material and every
/// facing is walked.
#[test]
fn every_kind_of_step_is_walked_up_and_down_from_every_side_without_a_correction() {
    let mut s = Scenario::new();
    let (x0, z0) = FIELD;
    let g = GROUND + 1;
    for (i, facing) in FACINGS.into_iter().enumerate() {
        let kind = STEP_KINDS[i % STEP_KINDS.len()];
        let roof = STEP_KINDS[(i + 2) % STEP_KINDS.len()];
        for (j, kind) in [kind, roof].into_iter().enumerate() {
            let (ux, uz) = up_of(facing);
            let (px, pz) = (uz.abs(), ux.abs());
            let base = (x0 + 12 * i as i32 - 6, z0 + 12 * j as i32);
            let mut cells = Vec::new();
            for w in -2..=2 {
                for k in 1..=3 {
                    let (x, z) = (base.0 + ux * k + px * w, base.1 + uz * k + pz * w);
                    for y in g..g + k - 1 {
                        cells.push(((x, y, z), t::BLOCK_PLANKS));
                    }
                    cells.push(((x, g + k - 1, z), t::faced(kind, facing)));
                }
                for k in 4..=5 {
                    for y in g..g + 3 {
                        cells.push(((base.0 + ux * k + px * w, y, base.1 + uz * k + pz * w), t::BLOCK_PLANKS));
                    }
                }
            }
            s.stand_at(feet_on(base.0 - ux, base.1 - uz));
            s.build(&cells);
            let up_yaw = (uz as f32).atan2(ux as f32);
            for slant in [0.0_f32, 25.0, -25.0] {
                let yaw = up_yaw + slant.to_radians();
                s.stand_at(feet_on(base.0 - ux, base.1 - uz));
                let landing = f64::from(g + 3);
                let rises = walk_until(&mut s, yaw, 6.0, |s| (s.feet().y - landing).abs() < 0.01 && s.player.grounded);
                let biggest = rises.iter().copied().fold(0.0, f64::max);
                assert!(
                    (s.feet().y - landing).abs() < 0.05,
                    "{kind} {facing:?} slant {slant}: the climb did not reach the landing, stuck at {:?}",
                    s.feet()
                );
                assert!(
                    biggest <= f64::from(primitive_shared::geometry::PLAYER_STEP_HEIGHT) + 0.02,
                    "{kind} {facing:?} slant {slant}: one frame lifted the player {biggest:.3}: {rises:?}"
                );
                // And down again, facing the other way.
                let ground = f64::from(g);
                walk_until(&mut s, yaw + std::f32::consts::PI, 6.0, |s| (s.feet().y - ground).abs() < 0.01 && s.player.grounded);
                assert!(
                    (s.feet().y - ground).abs() < 0.05,
                    "{kind} {facing:?} slant {slant}: the walk down stopped at {:?}",
                    s.feet()
                );
            }
        }
    }
    no_corrections(&s);
}

/// **Into the corner of two flights and out again, on the diagonal**: an
/// outside corner, where the two flights' risers meet at a post, and an
/// inside one, where they run round the corner. Each is one course of
/// steps round a landing a block up; the walk goes at the corner along the
/// diagonal, reaches the landing, and comes back down, never lifted more
/// than a step in a frame and never corrected. The corner shape is decided
/// from the neighbours by the client's collider and the server's alike
/// (`geometry::step_shape`), and a corner the two disagreed about would be
/// a rubber-band here.
#[test]
fn the_corners_of_a_flight_are_walked_up_and_down_on_the_diagonal_without_a_correction() {
    let mut s = Scenario::new();
    let (x0, z0) = FIELD;
    let g = GROUND + 1;
    // Outside: a west-facing flight along z and a south-facing one along x,
    // their risers toward the landing in the angle between them.
    let (ox, oz) = (x0 + 4, z0 + 6);
    // Inside: the same two directions of rise with the landing round the
    // outside of the L, so the walker comes at the corner from inside it.
    let (ix, iz) = (x0 + 16, z0 + 6);
    s.stand_at(feet_on(ox - 3, oz + 3));
    let mut cells = Vec::new();
    for d in 0..4 {
        cells.push(((ox, g, oz - d), t::faced(t::BLOCK_PLANK_STAIRS, Facing::West)));
        cells.push(((ox + 1 + d, g, oz), t::faced(t::BLOCK_TILE_ROOF, Facing::South)));
        cells.push(((ix, g, iz - d), t::faced(t::BLOCK_COBBLESTONE_STAIRS, Facing::West)));
        cells.push(((ix - 1 - d, g, iz), t::faced(t::BLOCK_THATCH_ROOF, Facing::North)));
        for e in 1..4 {
            cells.push(((ox + e, g, oz - d - 1), t::BLOCK_PLANKS));
        }
    }
    for x in ix - 4..=ix + 3 {
        for z in iz - 4..=iz + 3 {
            if x > ix || z > iz {
                cells.push(((x, g, z), t::BLOCK_PLANKS));
            }
        }
    }
    s.build(&cells);
    let landing = f64::from(g + 1);
    let ground = f64::from(g);
    for (name, start, yaw) in [
        // From outside the angle, toward the landing in it: +x and -z.
        ("outside", (ox - 2, oz + 2), (-1.0_f32).atan2(1.0)),
        // From the angle, toward the landing round it: +x and +z.
        ("inside", (ix - 2, iz - 2), 1.0_f32.atan2(1.0)),
    ] {
        s.stand_at(feet_on(start.0, start.1));
        let rises = walk_until(&mut s, yaw, 4.0, |s| (s.feet().y - landing).abs() < 0.01 && s.player.grounded);
        let biggest = rises.iter().copied().fold(0.0, f64::max);
        s.shot(&format!("corner_{name}_climbed"));
        assert!((s.feet().y - landing).abs() < 0.05, "{name}: the walk at the corner stopped at {:?}", s.feet());
        assert!(
            biggest <= f64::from(primitive_shared::geometry::PLAYER_STEP_HEIGHT) + 0.02,
            "{name}: one frame lifted the player {biggest:.3}: {rises:?}"
        );
        walk_until(&mut s, yaw + std::f32::consts::PI, 4.0, |s| (s.feet().y - ground).abs() < 0.01 && s.player.grounded);
        assert!((s.feet().y - ground).abs() < 0.05, "{name}: the walk down from the corner stopped at {:?}", s.feet());
    }
    no_corrections(&s);
}

#[test]
fn a_block_set_beside_a_step_keeps_the_face_the_step_does_not_cover() {
    // **"если поставить с блоком, то грань блока будет пустая".** A step
    // counted as a whole block to the mesher, so a cobblestone set beside
    // one lost its whole face toward it -- and the step covers only the
    // lower half of it and a quarter of the upper, so the rest was a hole
    // into the cobblestone, with the grass beyond showing through. Every
    // kind, every facing, a block on each of its four sides: the area of
    // that block's face drawn toward the step is at least what the step
    // leaves open of it.
    let mut s = Scenario::new();
    let (x0, z0) = FIELD;
    let g = GROUND + 1;
    s.stand_at(feet_on(x0, z0));
    let mut cells = Vec::new();
    let mut cases = Vec::new();
    for (k, kind) in STEP_KINDS.into_iter().enumerate() {
        for (f, facing) in FACINGS.into_iter().enumerate() {
            let step = (x0 + 3 + 4 * k as i32, g, z0 + 3 + 4 * f as i32);
            cells.push((step, t::faced(kind, facing)));
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let beside = (step.0 + dx, g, step.2 + dz);
                cells.push((beside, t::BLOCK_COBBLESTONE));
                cases.push((kind, facing, beside, (-dx, -dz)));
            }
        }
    }
    s.build(&cells);
    s.seconds(0.5);
    for (kind, facing, cell, (nx, nz)) in cases {
        // The face of `cell` looking along (nx, nz), toward the step.
        let plane = if nx > 0 || nz > 0 { 1024 } else { 0 };
        let axis = if nx != 0 { 0 } else { 2 };
        let at = [cell.0 * 1024, cell.1 * 1024, cell.2 * 1024];
        let area: f64 = triangles_in(&s, cell, cell)
            .iter()
            .filter(|tri| tri.iter().all(|p| p[axis] - at[axis] == plane))
            .map(|tri| {
                let (s_axis, t_axis) = (if axis == 0 { 2 } else { 0 }, 1);
                let a = [f64::from(tri[1][s_axis] - tri[0][s_axis]), f64::from(tri[1][t_axis] - tri[0][t_axis])];
                let b = [f64::from(tri[2][s_axis] - tri[0][s_axis]), f64::from(tri[2][t_axis] - tri[0][t_axis])];
                (a[0] * b[1] - a[1] * b[0]).abs() * 0.5 / (1024.0 * 1024.0)
            })
            .sum();
        assert!(
            area > 0.999,
            "{} facing {facing:?}: the cobblestone at {cell:?} draws {area:.3} of its face toward the step",
            t::block_name(kind)
        );
    }
    let eye = DVec3::new(f64::from(x0) + 1.0, f64::from(g) + 1.62, f64::from(z0) + 1.0);
    look_from(&mut s, eye, DVec3::new(f64::from(x0) + 5.0, f64::from(g) + 0.5, f64::from(z0) + 5.0));
    s.shot("steps_beside_blocks");
    no_corrections(&s);
}

#[test]
fn nothing_is_put_or_set_down_over_the_air_of_a_partial_top() {
    // **"исправь баг наложения мешей и 3D-моделей на неполные блоки".** A
    // thing that rests on the block below is drawn from the floor of its own
    // cell -- the top of a *whole* block. Put on a slab, the tread of a step,
    // a floor dug down a quarter, a heap of a handful or a campfire, a chair
    // stood half a block over the slab and a knife lay in the air over the
    // tread, sunk into nothing and floating at once. They are refused there
    // now (`types::has_full_top`, `types::can_grow_on`), and the same things
    // on a whole floor still go down -- which is what says the refusal is
    // the rule and not a harness that places nothing.
    let mut s = Scenario::new();
    let (x0, z0) = FIELD;
    let g = GROUND + 1;
    s.stand_at(feet_on(x0, z0));
    let dug = primitive_shared::dig::next_bite(t::BLOCK_DIRT, primitive_shared::dig::Side::PosY).expect("dirt bites");
    let grounds = [
        t::BLOCK_STONE,
        t::BLOCK_TILE_SLAB,
        t::faced(t::BLOCK_PLANK_STAIRS, Facing::West),
        dug,
        primitive_shared::dig::heaped(t::BLOCK_DIRT),
        t::BLOCK_CAMPFIRE,
    ];
    // Grounds along z, two cells in front of the player, one per row.
    let ground_at = |i: usize| (x0 + 2, g, z0 - 5 + 2 * i as i32);
    s.build(&grounds.iter().enumerate().map(|(i, &b)| (ground_at(i), b)).collect::<Vec<_>>());
    s.give(t::BLOCK_CHAIR, 8);
    s.give(t::BLOCK_BARREL, 8);
    s.give(t::BLOCK_PEAT, 8);
    for (i, &ground) in grounds.iter().enumerate() {
        let cell = ground_at(i);
        let over = (cell.0, cell.1 + 1, cell.2);
        s.stand_at(feet_on(x0, cell.2));
        // Where a player aims to put a thing on a step: its tread.
        let aim = if t::is_step(ground) {
            DVec3::new(f64::from(cell.0) + 0.25, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5)
        } else {
            let top = f64::from(t::collision_height(ground));
            DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + top, f64::from(cell.2) + 0.5)
        };
        for (thing, set_down) in [(t::BLOCK_CHAIR, false), (t::BLOCK_BARREL, false), (t::BLOCK_PEAT, true)] {
            s.select(thing);
            s.look_at(aim);
            assert_eq!(s.aimed().map(|(c, _)| c), Some(cell), "not aiming at the {}", t::block_name(ground));
            if set_down {
                s.hold(Action::Sprint);
            }
            s.use_aimed();
            s.release_all();
            s.seconds(0.6);
            let there = s.server().block_at(over.0, over.1, over.2);
            // ...except a thing set down, which has no foot to hang and lies
            // on any level top at its height (`types::set_down_drop`): the
            // slab, the tread, the floor dug down and the heap. Not the fire.
            let whole = ground == t::BLOCK_STONE || (set_down && t::set_down_drop(ground).is_some());
            let went = there.is_some_and(|b| !t::is_air(b));
            assert_eq!(
                went,
                whole,
                "a {} {} a {}: the cell over it holds {:?}",
                t::block_name(thing),
                if whole { "was refused on" } else { "went down over" },
                t::block_name(ground),
                there.map(t::block_name)
            );
            if went {
                s.server().place_block(over.0, over.1, over.2, t::BLOCK_AIR);
                s.until(3.0, |s| s.block(over).is_some_and(t::is_air));
            }
        }
    }
    let eye = DVec3::new(f64::from(x0) - 1.0, f64::from(g) + 2.2, f64::from(z0));
    look_from(&mut s, eye, DVec3::new(f64::from(x0) + 2.5, f64::from(g) + 0.5, f64::from(z0)));
    s.shot("nothing_over_partial_tops");
    no_corrections(&s);
}

#[test]
fn a_thing_set_down_on_a_lip_a_slab_and_a_stair_lies_on_it_and_is_picked_up_again() {
    // **"через шифт можно ставить только на полные блоки".** Setting down
    // asked for a whole top, and the ground is full of tops that are not:
    // the lip every generated slope is edged with, a slab, a stair. A knife
    // goes down on each, is drawn and aimed at on the surface it lies on --
    // not a quarter or half a block over it -- and comes back to the hand.
    let mut s = Scenario::new();
    let (x0, z0) = FIELD;
    let g = GROUND + 1;
    let lip = primitive_shared::dig::lowered(t::BLOCK_GRASS, 1);
    s.stand_at(feet_on(x0, z0));
    let grounds = [lip, t::BLOCK_TILE_SLAB, t::faced(t::BLOCK_PLANK_STAIRS, Facing::West)];
    let ground_at = |i: usize| (x0 + 2, g, z0 - 3 + 3 * i as i32);
    s.build(&grounds.iter().enumerate().map(|(i, &b)| (ground_at(i), b)).collect::<Vec<_>>());
    s.give(t::BLOCK_COPPER_KNIFE, 4);
    for (i, &ground) in grounds.iter().enumerate() {
        let cell = ground_at(i);
        let over = (cell.0, cell.1 + 1, cell.2);
        let top = if t::is_step(ground) { 0.5 } else { 1.0 - t::set_down_drop(ground).expect("no floor") };
        s.stand_at(feet_on(x0, cell.2));
        // At the top a player can see: the tread of the stair, toward them.
        let across = if t::is_step(ground) { 0.25 } else { 0.5 };
        let aim = DVec3::new(f64::from(cell.0) + across, f64::from(cell.1) + f64::from(top), f64::from(cell.2) + 0.5);
        s.select(t::BLOCK_COPPER_KNIFE);
        s.look_at(aim);
        assert_eq!(s.aimed().map(|(c, _)| c), Some(cell), "not aiming at the {}", t::block_name(ground));
        let before = s.inventory.count(t::BLOCK_COPPER_KNIFE);
        s.hold(Action::Sprint);
        s.use_aimed();
        s.release_all();
        let laid = s.until(3.0, |s| s.chunks.set_down_laid().any(|(c, _, item, _)| c == over && t::block_kind(item) == t::BLOCK_COPPER_KNIFE));
        assert!(laid, "a knife was not set down on a {}: {:?}", t::block_name(ground), s.block(over).map(t::block_name));
        let (_, _, _, rest) = s.chunks.set_down_laid().find(|&(c, ..)| c == over).expect("laid");
        let lies_at = f64::from(over.1) + f64::from(rest[1]);
        let surface = f64::from(cell.1) + f64::from(top);
        assert!(
            (lies_at - surface).abs() < 1e-4,
            "a knife on a {} is drawn at {lies_at}, the surface is at {surface}",
            t::block_name(ground)
        );
        // Drawn over the tread, not the riser: the front half of a stair
        // facing west is its -x half.
        if t::is_step(ground) {
            assert!(rest[0] < 0.0 && rest[2].abs() < 1e-6, "a knife on a stair lies at {rest:?}, not on its tread");
        }
        // Aimed at where it lies, and taken back.
        let knife = DVec3::new(
            f64::from(over.0) + 0.5 + f64::from(rest[0]),
            lies_at + 0.05,
            f64::from(over.2) + 0.5 + f64::from(rest[2]),
        );
        s.look_at(knife);
        assert_eq!(s.aimed().map(|(c, _)| c), Some(over), "the knife on a {} is not aimed at", t::block_name(ground));
        s.shot(&format!("set_down_on_{}", ["lip", "slab", "stair"][i]));
        s.use_aimed();
        s.release_all();
        let back = s.until(3.0, |s| s.inventory.count(t::BLOCK_COPPER_KNIFE) == before && s.block(over).is_some_and(t::is_air));
        assert!(back, "the knife on a {} did not come back to the hand", t::block_name(ground));
    }
    no_corrections(&s);
}

/// Looks from `eye` at `target` for a picture, without moving the body.
fn look_from(s: &mut Scenario, eye: DVec3, target: DVec3) {
    let dir = (target - eye).as_vec3().normalize();
    s.camera.position = eye;
    s.camera.yaw = dir.z.atan2(dir.x);
    s.camera.pitch = dir.y.asin();
}

const STEP_KINDS: [BlockId; 5] =
    [t::BLOCK_PLANK_STAIRS, t::BLOCK_COBBLESTONE_STAIRS, t::BLOCK_TILE_ROOF, t::BLOCK_THATCH_ROOF, t::BLOCK_BRANCH_ROOF];
const FACINGS: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];

/// **Every step, every facing, alone, in rows, at corners, against a wall
/// and stacked**, from eye height on four sides and from above, through the
/// real renderer. A diagnostic, not a check:
///
/// ```text
/// PRIMITIVE_SCENARIO_SHOTS=<absolute dir> cargo test -p primitive_client --lib scenario::tests::a_gallery_of_steps -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn a_gallery_of_steps() {
    let mut s = Scenario::new();
    let (x0, z0) = FIELD;
    let g = GROUND + 1;
    s.stand_at(feet_on(x0 + 16, z0 + 8));
    let mut cells = Vec::new();
    // Kinds along x, facings along z: every one alone.
    for (k, kind) in STEP_KINDS.into_iter().enumerate() {
        for (f, facing) in FACINGS.into_iter().enumerate() {
            cells.push(((x0 + 3 * k as i32, g, z0 + 3 * f as i32), t::faced(kind, facing)));
        }
    }
    // Rows of four, one per facing: north and south along x, east and west
    // along z.
    let rx = x0 + 18;
    for facing in FACINGS {
        for i in 0..4 {
            let (x, z) = match facing {
                Facing::North => (rx + i, z0),
                Facing::South => (rx + i, z0 + 3),
                Facing::East => (rx + 6, z0 + 6 + i),
                Facing::West => (rx + 9, z0 + 6 + i),
            };
            cells.push(((x, g, z), t::faced(t::BLOCK_PLANK_STAIRS, facing)));
        }
    }
    // Corners: an L of steps meeting with the low sides outward (the
    // corner an outside one) and one with them inward (an inside one).
    let cx = x0 + 30;
    for (i, kind) in [t::BLOCK_PLANK_STAIRS, t::BLOCK_TILE_ROOF].into_iter().enumerate() {
        let z = z0 + 7 * i as i32;
        for d in 0..4 {
            cells.push(((cx, g, z + d), t::faced(kind, Facing::West)));
            cells.push(((cx + 1 + d, g, z + 3), t::faced(kind, Facing::South)));
            cells.push(((cx + 10, g, z + d), t::faced(kind, Facing::West)));
            cells.push(((cx + 6 + d, g, z + 3), t::faced(kind, Facing::North)));
        }
    }
    // A roof: two slopes meeting at a ridge, two courses high.
    let px = x0 + 12;
    let pz = z0 + 16;
    for d in 0..4 {
        for h in 0..2 {
            cells.push(((px + h, g + h, pz + d), t::faced(t::BLOCK_TILE_ROOF, Facing::West)));
            cells.push(((px + 3 - h, g + h, pz + d), t::faced(t::BLOCK_TILE_ROOF, Facing::East)));
        }
    }
    // A staircase up to a wall, and steps stacked on steps.
    let (wx, wz) = (x0, z0 + 16);
    for k in 0..3 {
        for y in g..g + k {
            cells.push(((wx + k, y, wz), t::BLOCK_PLANKS));
        }
        cells.push(((wx + k, g + k, wz), t::faced(t::BLOCK_COBBLESTONE_STAIRS, Facing::West)));
    }
    for y in g..g + 4 {
        cells.push(((wx + 3, y, wz), t::BLOCK_PLANKS));
    }
    for y in g..g + 3 {
        cells.push(((wx + 6, y, wz), t::faced(t::BLOCK_PLANK_STAIRS, Facing::West)));
    }
    s.build(&cells);
    s.seconds(0.5);

    let eye_h = f64::from(g) + 1.62;
    let shoot = |s: &mut Scenario, name: &str, centre: DVec3, dist: f64| {
        for (i, (dx, dz)) in [(-1.0, -0.6), (0.6, -1.0), (1.0, 0.6), (-0.6, 1.0)].into_iter().enumerate() {
            let eye = DVec3::new(centre.x + dx * dist, eye_h, centre.z + dz * dist);
            look_from(s, eye, centre);
            s.shot(&format!("{name}_side{i}"));
        }
        look_from(s, centre + DVec3::new(-0.8, dist, -0.5), centre);
        s.shot(&format!("{name}_above"));
    };
    let c = |x: f64, z: f64| DVec3::new(x, f64::from(g) + 0.5, z);
    shoot(&mut s, "alone", c(f64::from(x0) + 7.5, f64::from(z0) + 5.0), 9.0);
    for (k, _) in STEP_KINDS.iter().enumerate() {
        let at = c(f64::from(x0) + 3.0 * k as f64 + 0.5, f64::from(z0) + 3.5);
        look_from(&mut s, at + DVec3::new(-1.6, 1.3, -2.2), at);
        s.shot(&format!("kind{k}_close"));
    }
    shoot(&mut s, "rows", c(f64::from(rx) + 5.0, f64::from(z0) + 5.0), 8.0);
    shoot(&mut s, "corners", c(f64::from(cx) + 5.5, f64::from(z0) + 5.0), 9.0);
    // Each corner close, from outside the L and from inside it.
    for (i, _) in ["plank", "tile"].iter().enumerate() {
        let z = f64::from(z0 + 7 * i as i32) + 3.5;
        for (name, x, out) in [("outside", f64::from(cx) + 0.5, (-1.0, 1.0)), ("inside", f64::from(cx + 10) + 0.5, (1.0, 1.0))] {
            let at = c(x, z);
            look_from(&mut s, at + DVec3::new(out.0 * 2.2, 2.0, out.1 * 2.2), at);
            s.shot(&format!("corner{i}_{name}_from_out"));
            look_from(&mut s, at + DVec3::new(-out.0 * 2.2, 2.0, -out.1 * 2.2), at);
            s.shot(&format!("corner{i}_{name}_from_in"));
        }
    }
    shoot(&mut s, "roof", c(f64::from(px) + 2.0, f64::from(pz) + 2.0), 6.0);
    shoot(&mut s, "wall", c(f64::from(wx) + 3.0, f64::from(wz) + 0.5), 6.0);
}

/// The same server with the keepalive and the timeout at their floor (a
/// second and two), so a pause twice as long as the timeout is four
/// seconds of a test rather than a minute -- what is being tested is the
/// pause being longer than the timeout, not how long either is.
fn impatient_world() -> Scenario {
    Scenario::with(primitive_server::settings::ServerSettings {
        keepalive_interval_secs: 1.0,
        client_timeout_secs: 2.0,
        ..super::scenario_settings()
    })
}

fn kicks(s: &Scenario) -> Vec<&ServerMessage> {
    s.heard.iter().filter(|m| matches!(m, ServerMessage::Kick(_))).collect()
}

fn around(s: &Scenario, feet: DVec3) -> Vec<Option<BlockId>> {
    let (x, y, z) = (feet.x.floor() as i32, feet.y.floor() as i32, feet.z.floor() as i32);
    let mut cells = Vec::new();
    for dx in -2..=2 {
        for dz in -2..=2 {
            for dy in -2..=1 {
                cells.push(s.block((x + dx, y + dy, z + dz)));
            }
        }
    }
    cells
}

/// What a player coming back to the game would check: still in it, still
/// where they were, the ground still the ground, and nobody pulled them.
fn came_back_to_the_same_world(s: &Scenario, feet: DVec3, ground: &[Option<BlockId>]) {
    assert!(kicks(s).is_empty(), "the player was thrown out for putting the game down: {:?}", kicks(s));
    let on_server = s.server().position_of("scenario").expect("the player is no longer on the server");
    assert!(
        DVec3::new(on_server.0, on_server.1, on_server.2).distance(feet) < 0.1,
        "the server has the player somewhere else: {on_server:?}, not {feet:?}"
    );
    assert!(s.feet().distance(feet) < 0.1, "the player moved while the game was down: {:?}", s.feet());
    assert_eq!(around(s, feet), ground, "the ground changed while the game was down");
    no_corrections(s);
}

#[test]
fn minimising_the_game_on_a_phone_is_not_a_disconnection() {
    // "ошибка internal server error на Android при сворачивании": the
    // activity went to the background, the loop stopped answering, and the
    // embedded server -- still ticking -- timed its only player out. Now
    // `Suspended` holds the server still (`hold_the_world`), and this is
    // that: the frames stop, the server is held, the frames start again.
    let mut s = impatient_world();
    s.stand_at(feet_on(FIELD.0, FIELD.1));
    s.seconds(1.0);
    let feet = s.feet();
    let ground = around(&s, feet);

    s.server().set_paused(true);
    std::thread::sleep(Duration::from_millis(200));
    let held_at = s.server().ticks();
    std::thread::sleep(Duration::from_secs(4));
    assert!(s.server().ticks() <= held_at + 1, "the world ran on while the game was in the background");
    // Resumed: the first frame that draws lets the world go.
    s.server().set_paused(false);
    s.seconds(3.0);

    came_back_to_the_same_world(&s, feet, &ground);
}

#[test]
fn a_computer_that_slept_wakes_up_still_in_the_world() {
    // "...или на ПК при сне": the whole process froze, the server's clock
    // counted the sleep, and its first tick awake found the player silent
    // for all of it. The tick loop's thread is stopped here the way sleep
    // stops it, and the client says nothing for as long.
    let mut s = impatient_world();
    s.stand_at(feet_on(FIELD.0, FIELD.1));
    s.seconds(1.0);
    let feet = s.feet();
    let ground = around(&s, feet);

    s.server().stall_tick_loop_for(Duration::from_secs(4));
    std::thread::sleep(Duration::from_secs(4));
    s.seconds(3.0);

    came_back_to_the_same_world(&s, feet, &ground);
}

// ------------------------------------------------------------- the storm

/// A lone tree standing on the field: four logs and a crown over them.
///
/// Planted rather than found, for the reason the huts and the anvils in
/// these scenarios are: what is being tested is what a bolt does to a
/// tree, and a scenario that had to go and look for one would fail for
/// the wrong reason on a seed that grew none in sight.
fn plant_a_tree(s: &mut Scenario, x: i32, z: i32) -> (i32, i32, i32) {
    let trunk_top = (x, GROUND + 4, z);
    for y in GROUND + 1..=trunk_top.1 {
        s.server().place_block(x, y, z, t::BLOCK_LOG);
    }
    for dx in -1..=1 {
        for dz in -1..=1 {
            s.server().place_block(x + dx, GROUND + 5, z + dz, t::BLOCK_LEAVES);
        }
    }
    s.seconds(0.5);
    trunk_top
}

#[test]
fn a_bolt_out_of_a_storm_sets_a_lone_tree_alight_and_the_client_sees_the_flash() {
    // «добавь грозу с молниями». A storm was a darker shower with an
    // ambience track of thunder over it, and nothing in the world ever
    // happened. What a bolt does is here: it finds the tallest thing, it
    // lights it, and the wildfire takes it from there.
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    let tree = plant_a_tree(&mut s, x0 + 6, z);
    s.server().console_command("/weather storm");
    s.seconds(0.5);

    let said = s.server().console_command(&format!("/lightning {} {}", tree.0, tree.2));
    assert!(said.iter().any(|line| line.contains("struck")), "the bolt was refused: {said:?}");

    // **The crown flashes and the trunk burns.** A bolt that lit only the
    // leaves would be a tree struck by lightning that did not burn, which
    // is the one thing everybody knows lightning does -- see `strike`.
    let alight = s.until(3.0, |s| s.block(tree).map(t::block_kind) == Some(t::BLOCK_BURNING_LOG));
    assert!(alight, "the tree did not catch: {:?}", s.block(tree).map(t::block_name));

    // ...and the client was told, so the sky flashed and the crack is on
    // its way (`ServerMessage::Lightning`).
    assert!(
        s.heard_any(|m| matches!(m, ServerMessage::Lightning { .. })),
        "the client never heard the bolt it was standing under"
    );
    no_corrections(&s);
}

#[test]
fn the_rain_puts_out_the_fire_a_bolt_started() {
    // «сделай горение лесов»: a fire that could not be stopped would be
    // a disaster rather than a risk, and what stops one is water falling
    // on it. The other half of the rule -- that a storm over the desert
    // brings no water and stops nothing -- is
    // `a_storm_over_the_desert_does_not_put_the_fire_out`, where it can
    // be stated against a climate a test can choose.
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    let tree = plant_a_tree(&mut s, x0 + 6, z);
    s.server().console_command("/weather clear");
    s.server().console_command(&format!("/lightning {} {}", tree.0, tree.2));
    let alight = s.until(3.0, |s| s.block(tree).map(t::block_kind) == Some(t::BLOCK_BURNING_LOG));
    assert!(alight, "the tree did not catch under a clear sky");

    // The sky opens, and the tree under it is out as char long before its
    // three minutes are up.
    s.server().console_command("/weather rain");
    let out = s.until(8.0, |s| s.block(tree).map(t::block_kind) == Some(t::BLOCK_CHARRED_LOG));
    assert!(out, "the rain did not put the fire out: {:?}", s.block(tree).map(t::block_name));
    no_corrections(&s);
}

#[test]
fn biometp_steppe_puts_the_player_on_solid_ground_in_the_steppe() {
    // «сделай /biometp». Finding a biome used to be twenty minutes of
    // walking, or a world generated over and over until one turned up
    // under the spawn point -- and a scenario cannot walk at all. See
    // `Command::BiomeTeleport`.
    use primitive_shared::worldgen::Biome;
    let mut s = Scenario::with(primitive_server::settings::ServerSettings {
        world_preset: primitive_shared::worldgen::Preset::Normal,
        ..scenario_settings()
    });
    s.seconds(0.5);
    let from = cell_of(s.feet());
    // **Typed into the chat, the way a player types it**, and not run at
    // the console: the console is not standing anywhere, so it is the
    // one caller this command refuses. A world of your own makes you its
    // operator (`commands::permission_for`), which is what lets the
    // scenario ask at all.
    s.send(ClientMessage::Chat("/biometp steppe".to_string()));

    // Long enough for the search and the teleport.
    s.seconds(2.0);
    assert_ne!(cell_of(s.feet()), from, "the command moved nobody");
    // **Asked of the server, not of the client**, and that is not a
    // dodge: a teleport a kilometre and a half away lands in country
    // nobody has streamed, and where the client *thinks* it is while the
    // chunks are still arriving is a question about the loading screen
    // rather than about the command.
    let landed = s.server().position_of("scenario").expect("the scenario is online");
    let feet = cell_of(DVec3::new(landed.0, landed.1, landed.2));
    assert_eq!(
        s.server().biome_at(feet.0, feet.2),
        Biome::Steppe,
        "the player landed somewhere that is not a steppe"
    );

    // **On the ground**: there is something under the soles, and they
    // are resting on it rather than falling through country nobody has
    // loaded. Asserted as "they stopped" rather than as a cell being
    // solid, because a player standing on a turf lip stands at the
    // *middle* of a cell and the cell under their feet is the lip --
    // which reads as "inside the ground" to anything that only looks at
    // whole blocks, and is the ordinary way to stand on the downs.
    let under = cell_of(DVec3::new(landed.0, landed.1 - 0.6, landed.2));
    assert!(
        s.server().block_at(under.0, under.1, under.2).is_some_and(t::is_collidable),
        "the player landed over a hole: {:?}",
        s.server().block_at(under.0, under.1, under.2).map(t::block_name)
    );
    s.seconds(2.0);
    let rested = s.server().position_of("scenario").expect("the scenario is online");
    assert!(
        (rested.1 - landed.1).abs() < 0.2,
        "the player was still moving two seconds after landing: {} then {}",
        landed.1,
        rested.1
    );
}

// ---------------------------------------------------------------- the shore

#[test]
fn a_player_wades_out_to_a_mussel_bed_strips_it_and_is_left_looking_at_bare_rock() {
    // **The whole of what a shore is worth, played as a player plays it.**
    // Every unit test of `shore::gather` passed while the gesture itself was
    // unreachable: a bed is picked with the right click the berry bush uses
    // (`types::picks_by_hand`), and whether *that* is what a click on this
    // block does is a question about the client's gesture table, the
    // server's `use_block` and the block's own id all agreeing.
    //
    // It also states the thing the mechanic is for: the rock changes picture
    // when the last mussel comes off it, so a stripped headland is something
    // a player can see rather than a number they have to remember.
    use primitive_shared::shore;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let rock = (x0 + 2, GROUND + 1, z);
    s.stand_at(feet_on(x0, z));
    // **A stick in the hand, and it is not decoration.** The first handful
    // of mussels lands in the selected slot, and from that moment a right
    // click on anything is a player eating raw shellfish (`UseGesture::Eat`
    // outranks the pick) -- which is what this scenario did on its first
    // run, and is also exactly what would happen to a player. Holding
    // something that is not food is the answer both of them have.
    s.give(t::BLOCK_STICK, 1);
    s.select(t::BLOCK_STICK);
    // One cell, and it is the sea floor itself: a bed *is* the rock
    // (`types::BLOCK_MUSSEL_BED`), which is what the generator lays in the
    // shallows (`worldgen::place_seabed`).
    s.build(&[(rock, shore::bed_holding(shore::BED_FULL))]);
    s.look_at_face(rock, (-1, 0, 0));
    assert_eq!(s.aimed().map(|(cell, _)| cell), Some(rock), "not looking at the bed");

    // Once for every mussel on it, and one more for luck: the spare gesture
    // has to give nothing rather than a fifth mussel off a bare rock.
    for _ in 0..usize::from(shore::BED_FULL) + 1 {
        s.use_aimed();
        s.seconds(0.4);
    }
    let want = u32::from(shore::BED_FULL);
    assert!(
        s.until(3.0, |s| s.inventory.count(t::BLOCK_MUSSELS) >= want),
        "the pack holds {} mussels after stripping a bed",
        s.inventory.count(t::BLOCK_MUSSELS)
    );
    assert_eq!(s.inventory.count(t::BLOCK_MUSSELS), want, "a bare rock went on giving");
    // ...and what is left is the other picture, on the client's own copy of
    // the world: this is the half a server-side test cannot see.
    assert!(
        s.until(3.0, |s| s.block(rock) == Some(t::BLOCK_MUSSEL_ROCK)),
        "the rock still looks full: {:?}",
        s.block(rock).map(t::block_name)
    );
    s.shot("mussel_bed_stripped");
    no_corrections(&s);
}

#[test]
fn a_monkey_that_reaches_a_player_holding_food_takes_one_and_the_player_is_told() {
    use primitive_shared::animals::Species;
    // **The troop's whole point, end to end.** A monkey decides in the
    // animals module (`animals::raid`), the theft is settled in the tick loop
    // against a pack the animals cannot reach (`rob_the_hand`), and the
    // player learns about it from a notice -- three parts that have to agree
    // about one event, which is exactly what a scenario is for.
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_APPLE, 3);
    s.select(t::BLOCK_APPLE);
    // Standing still for a moment, so the sign the animals read already says
    // "holding an apple" before the monkey is asked anything.
    s.seconds(0.5);
    let feet = s.feet();
    let at = (feet.x as f32 + 4.0, feet.y as f32, feet.z as f32);
    s.server().spawn_animal(Species::Monkey, at).expect("no monkey would come");

    let robbed = s.until(15.0, |s| {
        s.heard.iter().any(|m| matches!(m, ServerMessage::Notice { what: Notice::MonkeyTakesIt }))
    });
    assert!(robbed, "a monkey stood four blocks from an apple for fifteen seconds and did nothing");
    assert!(
        s.until(2.0, |s| s.inventory.count(t::BLOCK_APPLE) == 2),
        "the monkey took {} apples",
        3 - s.inventory.count(t::BLOCK_APPLE)
    );
    no_corrections(&s);
}

// ---------------------------------------------------------------- the first two minutes

/// What the client's journal knows, rebuilt from the last list the server
/// sent -- which is exactly how `lib.rs` rebuilds it
/// (`ServerMessage::Discovered`). Read through the wire rather than off the
/// server's own state, because the line over the belt is drawn from the
/// client's copy and a client that never got the list would show the wrong
/// prompt forever.
fn knowledge(s: &Scenario) -> primitive_shared::discovery::Discovered {
    s.heard
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::Discovered { kinds } => {
                Some(primitive_shared::discovery::Discovered::from_kinds(kinds.iter().copied()))
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// Breaks whatever is at `cell` with bare hands, the way a player does:
/// look at it, hold the button, and wait for it to go.
fn break_by_hand(s: &mut Scenario, cell: (i32, i32, i32)) {
    // **Aimed until it is actually aimed at.** A pebble and a tuft of grass
    // are sprites that stand on the floor of their cell, so the middle of
    // the cell is over the top of them and the ray goes through -- which is
    // a player squinting at the ground, and the same three heights a player
    // would try.
    let found = [0.12f64, 0.35, 0.6].into_iter().any(|up| {
        s.look_at(DVec3::new(cell.0 as f64 + 0.5, cell.1 as f64 + up, cell.2 as f64 + 0.5));
        s.frame();
        s.aimed().map(|(at, _)| at) == Some(cell)
    });
    assert!(found, "{:?} could not be aimed at", s.block(cell).map(t::block_name));
    s.input.breaking = true;
    let gone = s.until(8.0, |s| s.block(cell).is_none_or(|b| b == t::BLOCK_AIR));
    s.input.breaking = false;
    s.seconds(0.2);
    assert!(gone, "{:?} would not break by hand", s.block(cell).map(t::block_name));
}

/// Breaks what is at `cell` from two steps away and then walks over the
/// spot to pick it up.
///
/// **The walk is not a formality.** A break leaves the thing lying on the
/// ground as an item (`spawn_block_drop`); nothing goes straight into the
/// pack. A test that skipped the walk would be testing a game this is not.
fn take_by_hand(s: &mut Scenario, cell: (i32, i32, i32)) {
    s.stand_at((cell.0 as f64 + 2.5, cell.1 as f64, cell.2 as f64 + 0.5));
    s.seconds(0.3);
    break_by_hand(s, cell);
    s.stand_at((cell.0 as f64 + 0.5, cell.1 as f64, cell.2 as f64 + 0.5));
    s.seconds(0.8);
}

/// **A new player is told the first three things and does them.**
///
/// The player's complaint was that the progression is completely unclear,
/// and its sharpest end is the first two minutes: a meadow, empty hands,
/// and nothing saying which of the ten thousand blocks in sight is the one
/// to touch. The answer is one line over the belt at a time
/// (`ladder::first_step`), and this is that line driven the whole way
/// through the real server -- mined, dropped, learned, sent over the wire
/// and read back off the client's own copy of the knowledge.
///
/// The prompt has to *stop*, which is the second half of the test: a line
/// that is still telling a knapper to pick up a stone is a line every
/// player learns to look past, and then the next one is wasted too.
#[test]
fn a_player_who_has_just_woken_up_is_shown_three_things_to_do_and_then_left_alone() {
    use primitive_shared::ladder::{first_step, FirstStep};

    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0, z));
    s.seconds(0.5);
    assert_eq!(first_step(&knowledge(&s)), Some(FirstStep::Stone), "an empty-handed player was told nothing");

    // A stone on the grass, two steps away, and picked up.
    let stone = (x0 + 2, g + 1, z);
    s.build(&[(stone, t::BLOCK_PEBBLE)]);
    s.seconds(0.3);
    take_by_hand(&mut s, stone);
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_PEBBLE) >= 1), "the stone did not come up");
    assert!(
        s.until(3.0, |s| first_step(&knowledge(s)) == Some(FirstStep::Fibre)),
        "the stone was in the pack and the game was still asking for a stone",
    );

    // Tall grass, torn until it gives fibre: a tuft does not always, which
    // is the game and not the test being unlucky.
    let mut tufts = Vec::new();
    for step in 0..8 {
        tufts.push(((x0 + 2, g + 1, z + 1 + step), t::BLOCK_TALL_GRASS));
    }
    s.build(&tufts);
    s.seconds(0.3);
    for &(cell, _) in &tufts {
        if s.inventory.count(t::BLOCK_FIBER) > 0 {
            break;
        }
        take_by_hand(&mut s, cell);
    }
    assert!(s.inventory.count(t::BLOCK_FIBER) > 0, "eight tufts of grass gave no fibre at all");
    assert!(
        s.until(3.0, |s| first_step(&knowledge(s)) == Some(FirstStep::Flake)),
        "fibre was in the pack and the game was still asking for grass",
    );

    // A flint off the gravel, knapped: a third of the strikes shatter the
    // nodule, so there are several of them, exactly as there would be on a
    // riverbank.
    let flints: Vec<_> = (0..6).map(|n| ((x0 + 3, g + 1, z + 1 + n), t::BLOCK_FLINT)).collect();
    s.build(&flints);
    s.seconds(0.3);
    for &(cell, _) in &flints {
        take_by_hand(&mut s, cell);
    }
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_FLINT) >= 4), "the flint did not come up");
    for _ in 0..6 {
        if s.inventory.count(t::BLOCK_FLINT_FLAKE) > 0 {
            break;
        }
        craft(&mut s, "flint flakes");
        s.seconds(0.4);
    }
    assert!(s.inventory.count(t::BLOCK_FLINT_FLAKE) > 0, "six nodules and not one flake");
    assert!(
        s.until(3.0, |s| first_step(&knowledge(s)).is_none()),
        "all three were done and the game was still prompting: {:?}",
        first_step(&knowledge(&s)),
    );

    // ...and the ladder page agrees with the belt: the stone age is behind
    // this player and the page says what is above it.
    let held = knowledge(&s);
    assert_eq!(
        primitive_shared::ladder::standing_on(&held).map(|r| r.age),
        Some(primitive_shared::ladder::Age::Flint),
        "a knapper's ladder page did not mark the flint age",
    );
    s.shot("first_three_things_done");
    no_corrections(&s);
}

// ---------------------------------------------------------------- winter feed

/// The world's first day of winter, off the calendar.
fn first_winter_day() -> f32 {
    use primitive_shared::season::Season;
    (0..400).map(|q| q as f32 * 0.25).find(|&day| Season::at(day) == Season::Winter).expect("a year with no winter")
}

#[test]
fn hay_built_into_a_stack_by_the_pen_keeps_a_ewe_through_a_winter_week_and_the_ewe_without_one_goes_wild() {
    // **The whole of the winter feed, as a player meets it.** Eight hay
    // built into a stack, the stack set down by the pen, and then a week of
    // winter -- turf that feeds nothing (`husbandry::grazes`) -- in one jump
    // of the calendar, as a week away on the road would be. The ewe by the
    // stack is still tame and fed, and the stack a player looks at is lower
    // by what she ate; the ewe twelve blocks off, on the same turf with
    // nothing put up for her, has gone wild.
    use primitive_shared::animals::Species;
    use primitive_shared::husbandry::Keeping;
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_HAY, t::HAYSTACK_HOLDS as u32);
    craft(&mut s, "haystack");
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_HAYSTACK) == 1), "eight hay did not build a stack");
    assert_eq!(s.inventory.count(t::BLOCK_HAY), 0, "the stack did not take the hay");
    s.select(t::BLOCK_HAYSTACK);
    let ground = (x0 + 2, g, z);
    s.look_at_face(ground, (0, 1, 0));
    s.use_aimed();
    let stack = (x0 + 2, g + 1, z);
    assert!(
        s.until(3.0, |s| s.block(stack).and_then(t::hay_in_stack) == Some(t::HAYSTACK_HOLDS)),
        "the stack never stood by the pen: {:?}",
        s.block(stack).map(t::block_name)
    );

    // Winter first, and the ewes after: the jump from the world's first
    // day to its first winter is three weeks, and a flock kept before it
    // would have lived those too.
    let winter = first_winter_day();
    s.server().set_world_days(winter + 0.5);
    s.seconds(0.5);
    let tame_here = |at: (f32, f32, f32)| Keeping { trust: 1.0, tame: true, home: Some(at), hunger: 0.0, ..Keeping::wild() };
    let by_stack = ((x0 + 3) as f32 + 0.5, (g + 1) as f32, z as f32 + 0.5);
    let far_off = ((x0 - 12) as f32 + 0.5, (g + 1) as f32, z as f32 + 0.5);
    let fed = s.server().spawn_animal(Species::Sheep, by_stack).expect("no room for a ewe");
    let unfed = s.server().spawn_animal(Species::Sheep, far_off).expect("no room for a ewe");
    s.server().keep_animal(fed, tame_here(by_stack), None);
    s.server().keep_animal(unfed, tame_here(far_off), None);
    s.seconds(0.5);
    s.server().set_world_days(winter + 7.5);
    let eaten = |s: &Scenario| t::HAYSTACK_HOLDS - s.block(stack).and_then(t::hay_in_stack).unwrap_or(0);
    assert!(s.until(5.0, |s| eaten(s) >= 5), "a winter week took {} bites of the stack the player can see", eaten(&s));
    s.shot("haystack_after_a_winter_week");
    let kept = s.server().animal_keeping(fed).expect("the ewe by the stack is gone");
    assert!(kept.tame && !kept.is_hungry(), "the ewe by the stack went hungry or wild: {kept:?}");
    let left = s.server().animal_keeping(unfed);
    assert!(!left.is_some_and(|k| k.tame), "a ewe with nothing put up for her kept tame through a winter week: {left:?}");
    // A bite a day and not faster: eight bites last a ewe a week.
    assert!(
        s.block(stack).and_then(t::hay_in_stack).is_some(),
        "one ewe ate a stack of eight in a week: {:?}",
        s.block(stack).map(t::block_kind)
    );
    no_corrections(&s);
}

// ---------------------------------------------------------------- downed

/// How far the feet go across the ground in `seconds` of holding forward.
fn crawled(s: &mut Scenario, seconds: f32, sprint: bool) -> f64 {
    let from = s.feet();
    s.face(0.0);
    s.hold(Action::Forward);
    if sprint {
        s.hold(Action::Sprint);
    }
    s.seconds(seconds);
    s.release_all();
    let to = s.feet();
    ((to.x - from.x).powi(2) + (to.z - from.z).powi(2)).sqrt()
}

/// **A fall that used to kill breaks you instead**: twenty blocks onto the
/// meadow puts the player on the ground with the eye at the grass, crawling
/// at a tenth of a walk whatever they hold down, and -- left there -- dead
/// when the clock runs out, of the fall, with the pack in a corpse.
///
/// The clock is ninety seconds, and a scenario runs on the wall's time, so
/// most of it is taken off by blows the way a wolf would take it (two
/// seconds a point, `downed::SECONDS_PER_HEALTH`) -- what is asserted is
/// that the last few seconds run out on their own and not faster.
#[test]
fn a_player_who_falls_from_a_height_is_downed_crawls_slowly_and_dies_when_the_time_runs_out() {
    use primitive_shared::downed::{Cause, CRAWL_EYE};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at((x0 as f64 + 0.5, (GROUND + 21) as f64, z as f64 + 0.5));
    assert!(s.until(10.0, |s| s.downed.is_some() || s.dead.is_some()), "a twenty-block fall did nothing");
    assert!(s.dead.is_none(), "a twenty-block fall killed outright: {:?}", s.dead);
    assert_eq!(s.downed.map(|d| d.cause), Some(Cause::Fall));
    s.seconds(0.5);
    let eye = s.camera.position.y - s.feet().y;
    assert!((eye - f64::from(CRAWL_EYE)).abs() < 0.01, "the eye stayed at {eye} over the feet");

    // The crawl, measured: three seconds of forward, then the same with the
    // sprint key held, which a body on the ground does not have.
    let walk = f64::from(crate::settings::ClientSettings::default().move_speed);
    let slow = crawled(&mut s, 3.0, false) / 3.0;
    let sprinting = crawled(&mut s, 3.0, true) / 3.0;
    assert!(slow > walk * 0.03, "a downed player could not crawl at all: {slow} blocks/s");
    assert!(slow < walk * 0.2, "a broken body crawled at {slow} blocks/s against a walk of {walk}");
    assert!(sprinting < walk * 0.2, "the sprint key ran a downed player at {sprinting} blocks/s");

    // Most of the clock taken off by blows, then the last seconds left alone.
    let left = s.server().player_downed().expect("the server lost the downed body").left;
    let leave = 5.0;
    // In bites under a bar each: one blow of a bar or more on a downed body
    // is `downed::OVERKILL`, which is a death and not a clock.
    let mut owed = (left - leave) / primitive_shared::downed::SECONDS_PER_HEALTH;
    while owed > 0.0 {
        let bite = owed.min(10.0);
        s.server().hurt_player(bite, "was pulled down by a wolf");
        owed -= bite;
    }
    assert!(s.until(2.0, |s| s.downed.is_some_and(|d| d.left < leave + 1.0)), "the blow never reached the client's clock");
    s.seconds(leave - 2.0);
    assert!(s.dead.is_none(), "died before the clock ran out");
    assert!(s.until(6.0, |s| s.dead.is_some()), "the clock ran out and nobody died");
    assert!(s.dead.as_deref().is_some_and(|c| c.contains("fell")), "died of {:?}", s.dead);
    assert!(s.downed.is_none());
    no_corrections(&s);
}

/// **A starving player on the ground eats and gets up**: the bread in the
/// pack is the whole rescue, and afterwards they walk at a walk.
#[test]
fn a_player_downed_by_hunger_who_eats_gets_up_and_walks() {
    use primitive_shared::downed::{Cause, RAISED_HEALTH};
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.give(t::BLOCK_BREAD, 2);
    s.server().set_player_nourishment(0.0);
    s.server().hurt_player(30.0, "starved");
    assert!(s.until(3.0, |s| s.downed.is_some()), "a starving body at nought never went down");
    assert_eq!(s.downed.map(|d| d.cause), Some(Cause::Hunger));
    let crawl = crawled(&mut s, 1.5, false) / 1.5;

    let slot = slot_of(&s, t::BLOCK_BREAD).expect("the bread");
    s.send(ClientMessage::Eat { slot: slot as u8 });
    assert!(s.until(3.0, |s| s.downed.is_none()), "the bread went down and the body stayed down");
    assert!(s.dead.is_none());
    assert!(s.until(2.0, |s| s.health * 20.0 >= RAISED_HEALTH - 0.01), "raised on {} of a bar", s.health);
    let walk = crawled(&mut s, 1.5, false) / 1.5;
    assert!(walk > crawl * 2.5, "up again, and still crawling: {walk} against {crawl}");
    no_corrections(&s);
}

/// **A body on the ground breathes at the ground.** Knee-deep water, which
/// a standing player wades through without a thought, closes over the face
/// of one who is down in it: the server used to ask about the standing
/// head, a block up in the air, and the downed player lay under the water
/// line on their own screen and never lost a breath.
#[test]
fn a_player_downed_in_knee_deep_water_runs_out_of_breath_and_one_standing_in_it_does_not() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0, z));
    s.fill((x0 - 2, g, z - 2), (x0 + 2, g, z + 2), t::BLOCK_WATER);
    // The field's own turf dug out and filled: the feet on the bed a
    // block down, the water up to the knee.
    s.stand_at((x0 as f64 + 0.5, g as f64, z as f64 + 0.5));
    s.seconds(3.0);
    assert!(
        !s.heard_any(|m| matches!(m, ServerMessage::Breath { .. })),
        "standing in water to the knee took the breath"
    );
    s.server().hurt_player(30.0, "starved");
    assert!(s.until(3.0, |s| s.downed.is_some()), "the blow never put the body down");
    assert!(
        s.until(5.0, |s| s.heard_any(|m| matches!(m, ServerMessage::Breath { fraction } if *fraction < 1.0))),
        "a face down in knee-deep water was never short of air"
    );
}

/// The age the ladder page says this player stands on, off the client's own
/// copy of what they have held.
fn age_of(s: &Scenario) -> Option<primitive_shared::ladder::Age> {
    primitive_shared::ladder::standing_on(&knowledge(s)).map(|rung| rung.age)
}

/// Puts the pack's stack of `block` into hearth square `slot` the way a
/// player drags it: picked up off the pack, put down on the square.
fn load_hearth(s: &mut Scenario, block: t::BlockId, slot: usize) {
    let square = crate::ui::chest_screen::hearth_slot_rect(slot).expect("a hearth square");
    let from = pack_square(s, block);
    s.chest_click(from);
    s.seconds(0.2);
    s.chest_click(centre(square));
    let landed = s.until(3.0, |s| last_contents(s).block_in(slot).map(t::block_kind) == Some(t::block_kind(block)));
    assert!(landed, "{} never reached hearth square {slot}: {:?}", t::block_name(block), s.heard.iter().rev().take(4).collect::<Vec<_>>());
}

/// **The first hour, played to its end: bare hands, flint, fire, clay and a
/// copper ingot out of a kiln.**
///
/// `a_player_who_has_just_woken_up_is_shown_three_things_to_do_and_then_left_alone`
/// is the first two minutes and `progression` is the whole walk on paper;
/// neither had ever put a player in front of a kiln. This is the spine of
/// the ladder page (`ladder::LADDER`) done through the real server and the
/// real screens -- sticks and a log thrown down and struck alight, a kiln made and set
/// down, its squares filled by dragging, struck, and the ingot taken out of
/// its tray -- with the page asked at every rung whether it agrees.
///
/// **What is given rather than gathered**, and why: stone and flint are
/// taken off the ground as a player takes them (the test above has the rest
/// of the gathering), but the cobbles, clay and ore come from the script's
/// hand, because digging is a scenario of its own
/// (`earth_dug_out_comes_as_four_handfuls_and_four_handfuls_heap_back_into_the_hole`)
/// and a mine is a walk this harness cannot make in real time. The pot and
/// the mould are given fired: a firing is 110 seconds of a hearth's clock
/// each (`hearth::batch_seconds`), the smelt here runs the same batch code,
/// and a scenario that took eight minutes would be one nobody runs.
#[test]
fn the_first_hour_goes_from_bare_hands_through_flint_fire_and_clay_to_a_copper_ingot() {
    use primitive_shared::hearth::{FUEL_SLOT, INPUT_SLOTS, OUTPUT_SLOTS};
    use primitive_shared::ladder::Age;

    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let g = GROUND;
    s.stand_at(feet_on(x0, z));
    s.seconds(0.5);
    assert_eq!(age_of(&s), None, "an empty-handed player already stands on a rung");

    // ---- bare hands: stones off the grass ----
    let stones: Vec<_> = (0..2).map(|n| ((x0 + 2, g + 1, z + n), t::BLOCK_PEBBLE)).collect();
    s.build(&stones);
    s.seconds(0.3);
    for &(cell, _) in &stones {
        take_by_hand(&mut s, cell);
    }
    assert!(s.until(3.0, |s| age_of(s) == Some(Age::BareHands)), "stones in the pack and no rung: {:?}", age_of(&s));

    // ---- flint: nodules off the gravel, knapped ----
    let flints: Vec<_> = (0..6).map(|n| ((x0 + 3, g + 1, z + n), t::BLOCK_FLINT)).collect();
    s.build(&flints);
    s.seconds(0.3);
    for &(cell, _) in &flints {
        take_by_hand(&mut s, cell);
    }
    s.stand_at(feet_on(x0, z));
    for _ in 0..6 {
        if s.inventory.count(t::BLOCK_FLINT_FLAKE) > 0 {
            break;
        }
        craft(&mut s, "flint flakes");
        s.seconds(0.4);
    }
    assert!(s.until(3.0, |s| age_of(s) == Some(Age::Flint)), "flakes in the pack and not the flint age: {:?}", age_of(&s));
    assert!(s.inventory.count(t::BLOCK_FLINT) >= 2, "knapping left no nodule to strike a fire with");

    // ---- fire: a firepit, the fire that wants no cobblestone ----
    //
    // Sticks and a log thrown down in front of the player and struck where
    // they lie: the first fire there is, because the campfire's ring of
    // cobbles wants a pick the flint age has not got. It used to leave the
    // path page in the flint age -- nothing about a firepit passes through
    // the pack -- which is why the rung is asked here.
    s.give(t::BLOCK_STICK, 3);
    s.give(t::BLOCK_LOG, 1);
    // Thrown the way a player throws: looking ahead and a little down, so
    // the wood lands a couple of steps off rather than at the feet -- where
    // `items::PICKUP_DELAY` hands it straight back to its thrower.
    s.look_at(DVec3::new(x0 as f64 + 4.0, g as f64 + 1.2, z as f64 + 0.5));
    s.seconds(0.3);
    for kind in [t::BLOCK_STICK, t::BLOCK_LOG] {
        let slot = slot_of(&s, kind).expect("in the pack");
        s.send(ClientMessage::DropSlot { slot: slot as u8, whole_stack: true });
        s.seconds(0.2);
    }
    s.seconds(1.5);
    assert_eq!(s.inventory.count(t::BLOCK_STICK), 0, "the thrown sticks came straight back into the pack");
    // Struck at the ground under the sticks: where the client sees them lie.
    let under_the_sticks = s
        .entities
        .values()
        .find_map(|e| match e.kind {
            primitive_shared::protocol::EntityKind::Item { block, .. } if t::block_kind(block) == t::BLOCK_STICK => {
                Some((e.x.floor() as i32, e.y.floor() as i32 - 1, e.z.floor() as i32))
            }
            _ => None,
        })
        .expect("the sticks are nowhere on the ground");
    let over = (under_the_sticks.0, under_the_sticks.1 + 1, under_the_sticks.2);
    s.select(t::BLOCK_FLINT);
    s.look_at_face(under_the_sticks, (0, 1, 0));
    assert!(s.until(1.0, |s| s.aimed().map(|(cell, _)| cell) == Some(under_the_sticks)), "the ground under the sticks cannot be aimed at");
    // What the frame sends for flint at the makings of a firepit
    // (`ground_fire_claim`, which turns the placement into a strike). Sent
    // as itself: the harness's `use_aimed` has no copy of that claim, and
    // would set the nodule down instead.
    s.send(ClientMessage::UseBlock { global_x: under_the_sticks.0, global_y: under_the_sticks.1, global_z: under_the_sticks.2 });
    let lit = s.until(3.0, |s| s.block(over).map(t::block_kind) == Some(t::BLOCK_FIREPIT_LIT));
    assert!(
        lit,
        "flint struck where the sticks and the log lay lit nothing: {:?}",
        s.heard
            .iter()
            .filter(|m| matches!(m, ServerMessage::Said { .. } | ServerMessage::Notice { .. } | ServerMessage::Error(_)))
            .collect::<Vec<_>>(),
    );
    assert!(s.until(3.0, |s| age_of(s) == Some(Age::Fire)), "a lit firepit and not the fire age: {:?}", age_of(&s));

    // ---- clay: the pottery raw, and a kiln ----
    s.give(t::BLOCK_CLAY, 15);
    s.give(t::BLOCK_SAND, 1);
    s.give(t::BLOCK_COBBLESTONE, 4);
    s.seconds(0.3);
    for (name, made) in [("clay vessel", t::BLOCK_VESSEL_RAW), ("ingot mould", t::BLOCK_MOULD_RAW), ("kiln", t::BLOCK_KILN)] {
        craft(&mut s, name);
        assert!(s.until(3.0, |s| s.inventory.count(made) == 1), "{name} was not made");
    }
    assert!(s.until(3.0, |s| age_of(s) == Some(Age::Clay)), "a kiln in the pack and not the clay age: {:?}", age_of(&s));
    let kiln = (x0 + 2, g + 1, z + 2);
    s.select(t::BLOCK_KILN);
    s.look_at_face((kiln.0, g, kiln.2), (0, 1, 0));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.block(kiln).map(t::block_kind) == Some(t::BLOCK_KILN)), "the kiln was not set down");

    // ---- copper: ore, charcoal, a fired pot and mould, in the kiln ----
    s.give(t::BLOCK_COPPER_ORE, 3);
    s.give(t::BLOCK_COAL, 6);
    s.give(t::BLOCK_VESSEL, 1);
    s.give(t::BLOCK_MOULD, 1);
    s.select(t::BLOCK_PEBBLE);
    s.look_at_face(kiln, (-1, 0, 0));
    s.use_aimed();
    assert!(
        s.until(3.0, |s| s.chest_screen.is_open()),
        "the kiln never opened: {:?}",
        s.heard.iter().rev().take(4).collect::<Vec<_>>()
    );
    let mut inputs = INPUT_SLOTS;
    for block in [t::BLOCK_COPPER_ORE, t::BLOCK_VESSEL, t::BLOCK_MOULD, t::BLOCK_COAL] {
        load_hearth(&mut s, block, inputs.next().expect("a free input square"));
    }
    // The recipe takes two of the coal; the fire gets the rest, dragged in
    // again from what is left in the pack if the first drag took it all.
    if s.inventory.count(t::BLOCK_COAL) == 0 {
        s.give(t::BLOCK_COAL, 4);
        s.seconds(0.3);
    }
    load_hearth(&mut s, t::BLOCK_COAL, FUEL_SLOT);
    s.close_screens();
    s.select(t::BLOCK_FLINT);
    s.look_at_face(kiln, (-1, 0, 0));
    s.use_aimed();
    assert!(
        s.until(3.0, |s| s.block(kiln).map(t::block_kind) == Some(t::BLOCK_KILN_LIT)),
        "flint struck on a loaded kiln did not light it: {:?}",
        s.heard.iter().rev().take(4).collect::<Vec<_>>()
    );
    // The hearth's own clock: a heat to climb to copper's and 75 seconds of
    // smelt once it is there. The screen is opened to be told.
    s.select(t::BLOCK_PEBBLE);
    s.look_at_face(kiln, (-1, 0, 0));
    s.use_aimed();
    assert!(s.until(3.0, |s| s.chest_screen.is_open()), "the lit kiln never opened");
    let holds_ingot = |s: &Scenario| {
        OUTPUT_SLOTS.clone().find(|&slot| last_contents(s).block_in(slot).map(t::block_kind) == Some(t::BLOCK_COPPER_INGOT))
    };
    let smelted = s.until(240.0, |s| holds_ingot(s).is_some());
    let state = s.heard.iter().rev().find_map(|m| match m {
        ServerMessage::ChestState { hearth, .. } => Some(*hearth),
        _ => None,
    });
    assert!(smelted, "four minutes of a lit kiln and no ingot: {state:?}, {:?}", last_contents(&s));
    let tray = holds_ingot(&s).expect("the ingot's tray");
    s.chest_shift_click(centre(crate::ui::chest_screen::hearth_slot_rect(tray).expect("a tray square")));
    assert!(s.until(3.0, |s| s.inventory.count(t::BLOCK_COPPER_INGOT) == 1), "the ingot would not come out of the tray");
    assert!(s.until(3.0, |s| age_of(s) == Some(Age::Copper)), "an ingot in the pack and not the copper age: {:?}", age_of(&s));
    s.shot("first_hour_copper");
    no_corrections(&s);
}

// ------------------------------------------------------------------ the night

/// The server's circle a night hunter will not step into round a lit fire
/// (`logic::animals::FIRE_RADIUS`), written out: the scenario asserts what a
/// player would measure, not the constant.
const FIRE_KEEPS: f64 = 5.0;

/// Wolves near `at` on the server, and the nearest one's distance.
fn wolves_near(s: &Scenario, at: DVec3, within: f64) -> (usize, f64) {
    let wolves = s.server().animals_of(primitive_shared::animals::Species::Wolf);
    let apart = |w: &(f32, f32, f32)| (f64::from(w.0) - at.x).hypot(f64::from(w.2) - at.z);
    let near = wolves.iter().filter(|w| apart(w) <= within).count();
    let nearest = wolves.iter().map(apart).fold(f64::INFINITY, f64::min);
    (near, nearest)
}

/// Lies down on the straw at `bed`, and waits to be told the eyes are shut.
fn lie_down_on(s: &mut Scenario, bed: (i32, i32, i32)) {
    s.look_at(DVec3::new(bed.0 as f64 + 0.5, bed.1 as f64 + 0.2, bed.2 as f64 + 0.5));
    s.use_aimed();
    assert!(
        s.until(3.0, |s| s.heard_any(|m| matches!(m, ServerMessage::Asleep { asleep: true }))),
        "the straw could not be slept on: aimed at {:?}",
        s.aimed().map(|(c, b)| (c, t::block_name(b)))
    );
}

/// **A night by a lit firepit.** A pair of wolves put down in the dark sixteen
/// blocks off: the fire is what they see, so they come -- and it is what
/// stops them, so they walk the edge of its light and never step in, and
/// nobody is bitten. Then the player lies down on straw beside it with the
/// night's dice rigged to the worst roll there is, and sleeps to dawn anyway:
/// a fire by the bed is odds of nought, and no roll beats nought.
#[test]
fn a_night_by_a_lit_firepit_keeps_the_wolves_at_the_edge_of_its_light_and_the_sleeper_sleeps_till_dawn() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    s.stand_at(feet_on(x0, z));
    s.server().console_command("/time night");
    let fire = (x0 - 2, GROUND + 1, z);
    s.server().place_block(fire.0, fire.1, fire.2, t::BLOCK_FIREPIT_LIT);
    let fire_at = DVec3::new(fire.0 as f64 + 0.5, fire.1 as f64, fire.2 as f64 + 0.5);
    let ground = (GROUND + 1) as f32;
    for dz in [0.5, 2.5] {
        s.server()
            .spawn_animal(primitive_shared::animals::Species::Wolf, (x0 as f32 + 16.5, ground, z as f32 + dz))
            .expect("no room for a wolf");
    }
    let health = s.health;
    // **Until they come, and then five seconds of them circling**, rather
    // than twenty seconds flat: under a full test run the server's ticks
    // arrive late, and the pair was still eighteen blocks out when the
    // twenty seconds ended -- a red test about a slow machine, not about the
    // fire. What is asserted is the same: that they reach the edge, and that
    // the whole time they are watched none of them steps inside it.
    let (mut nearest, mut came, mut circled) = (f64::INFINITY, false, 0);
    for _ in 0..360 {
        s.seconds(0.25);
        let (_, closest) = wolves_near(&s, fire_at, 64.0);
        nearest = nearest.min(closest);
        came |= closest <= FIRE_KEEPS + 6.0;
        if came {
            circled += 1;
            if circled >= 20 {
                break;
            }
        }
    }
    s.shot("night_firepit");
    assert!(came, "the wolves never came to the edge of the firelight: nearest {nearest:.1}");
    assert!(nearest >= FIRE_KEEPS - 0.5, "a wolf came {nearest:.1} blocks from a lit firepit after dark");
    assert!(s.health >= health, "somebody sitting by a lit firepit was bitten: {health} -> {}", s.health);

    // Down on the straw by the fire, and the worst roll there is.
    let bed = (x0, GROUND + 1, z + 1);
    s.server().place_block(bed.0, bed.1, bed.2, t::BLOCK_STRAW_BED);
    assert!(s.until(2.0, |s| s.block(bed) == Some(t::BLOCK_STRAW_BED)), "the straw never arrived");
    s.server().set_sleeper_dice(Some(0.0));
    lie_down_on(&mut s, bed);
    let dawn = s.until(10.0, |s| {
        s.heard_any(|m| matches!(m, ServerMessage::TimeSync { time_of_day, .. } if (*time_of_day - 0.25).abs() < 1e-3))
    });
    assert!(dawn, "a night asleep by a lit firepit never reached the morning");
    assert!(
        !s.heard_any(|m| matches!(m, ServerMessage::Notice { what: Notice::WokenByWolves })),
        "woken by wolves beside a lit firepit"
    );
    no_corrections(&s);
}

/// **A night in the open.** No fire, straw on the grass, and the same rigged
/// roll: the night finds the sleeper. They are told, in their own language
/// (`Notice::WokenByWolves`), on their feet rather than lying there, with a
/// pair of wolves a couple of seconds' run off -- and with nothing to keep
/// them off, the pair comes in.
#[test]
fn a_night_asleep_in_the_open_without_a_fire_is_woken_by_wolves_that_come_in() {
    let mut s = Scenario::new();
    let (x0, z) = FIELD;
    let (x0, z) = (x0 + 40, z + 20);
    s.stand_at(feet_on(x0, z));
    s.server().console_command("/time night");
    let bed = (x0 + 1, GROUND + 1, z);
    s.server().place_block(bed.0, bed.1, bed.2, t::BLOCK_STRAW_BED);
    assert!(s.until(2.0, |s| s.block(bed) == Some(t::BLOCK_STRAW_BED)), "the straw never arrived");
    s.server().set_sleeper_dice(Some(0.0));
    lie_down_on(&mut s, bed);

    let woken = s.until(10.0, |s| s.heard_any(|m| matches!(m, ServerMessage::Notice { what: Notice::WokenByWolves })));
    assert!(woken, "a night asleep in the open with the worst roll there is passed quietly");
    let name = s.name.clone();
    assert!(s.until(2.0, |s| !s.server().asleep(&name)), "woken by wolves and still asleep");
    // About the bed, not the player: by the time the client has been told,
    // the pair is already running in and may be anywhere between.
    let bed_at = DVec3::new(bed.0 as f64 + 0.5, bed.1 as f64, bed.2 as f64 + 0.5);
    let (pair, nearest) = wolves_near(&s, bed_at, 14.0);
    assert!(pair >= 2, "woken by wolves with {pair} wolves about the bed (nearest {nearest:.1})");
    s.shot("night_open_woken");

    // Stand there: the pack comes in.
    let health = s.health;
    let bitten = s.until(30.0, |s| s.health < health || s.downed.is_some());
    let (_, closest) = wolves_near(&s, s.feet(), 64.0);
    assert!(bitten, "the pack that woke a sleeper in the open never came in: nearest {closest:.1}");
    no_corrections(&s);
}
