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

