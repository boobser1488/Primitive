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
    let told = s.until(3.0, |s| s.heard_any(|m| matches!(m, ServerMessage::Error(e) if e.contains("still wet"))));
    assert!(told, "a lift went onto a wet one without a word");
    assert_eq!(s.block(wall), Some(first), "the wet lift changed under a refused one");
    assert_eq!(s.inventory.count(t::BLOCK_COB), 1, "the refused lump was spent");
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
            .filter(|m| matches!(m, ServerMessage::Chat { .. } | ServerMessage::Error(_) | ServerMessage::Mounted { horse: None, .. }))
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
            s.horseback.is_some() || s.heard.iter().any(|m| matches!(m, ServerMessage::Chat { text, .. } if text.contains("throws you")))
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
        s.heard.iter().any(|m| matches!(m, ServerMessage::Chat { text, .. } if text.contains("yours"))),
        "breaking it was never said"
    );
}

// ---------------------------------------------------------------- a night out, and the wet

/// The lean-to's two cells, and whether both are standing.
fn lean_to_at(s: &Scenario, cells: [(i32, i32, i32); 2]) -> bool {
    cells.iter().all(|&c| s.block(c).is_some_and(|b| t::block_kind(b) == t::BLOCK_LEAN_TO))
}

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
    // Put down lying away from the placer, like a pallet: the foot where
    // it was aimed, the head a cell further on.
    let cells = [(x0 + 2, GROUND + 1, z), (x0 + 3, GROUND + 1, z)];
    assert!(s.until(3.0, |s| lean_to_at(s, cells)), "the lean-to was not put down: {:?}", cells.map(|c| s.block(c).map(t::block_name)));
    s.shot("lean_to");
    // Caught out in it first: the rain is on the player.
    let name = s.name.clone();
    assert!(s.until(6.0, |s| s.server().wetness_of(&name).unwrap_or(0.0) > 0.1), "standing in the rain wet nobody");

    // In, and down.
    // At the leaves, which are the pallet's two eighths high.
    s.look_at(DVec3::new(cells[0].0 as f64 + 0.5, cells[0].1 as f64 + 0.06, cells[0].2 as f64 + 0.5));
    s.use_aimed();
    let down = s.until(3.0, |s| s.heard.iter().any(|m| matches!(m, ServerMessage::Asleep { asleep: true })));
    let said: Vec<&String> = s.heard.iter().filter_map(|m| match m {
        ServerMessage::Error(text) => Some(text),
        _ => None,
    }).collect();
    assert!(down, "the lean-to could not be slept in: aimed at {:?}, told {said:?}", s.aimed().map(|(c, b)| (c, t::block_name(b))));
    // Every sample while the leaves are still over the sleeper: none of
    // them wetter than the one before. (The sample taken as the player lay
    // down may have been read where they stood, so it is let go.) The night
    // passes on the server's clock, so this watches until the roof is gone
    // rather than for a fixed while.
    s.seconds(0.6);
    let mut under = vec![s.server().wetness_of(&name).unwrap_or(0.0)];
    for _ in 0..40 {
        s.seconds(0.1);
        if !lean_to_at(&s, cells) {
            break;
        }
        under.push(s.server().wetness_of(&name).unwrap_or(1.0));
    }
    assert!(
        under.windows(2).all(|w| w[1] <= w[0] + 1e-3),
        "it rained into the lean-to: {under:?}"
    );

    // The night passes -- everybody is asleep -- and the roof comes down
    // with the morning, and the sleeper is on their feet beside the heap.
    let gone = s.until(10.0, |s| cells.iter().all(|&c| s.block(c) == Some(t::BLOCK_AIR)));
    assert!(gone, "the lean-to stood through the morning: {:?}", cells.map(|c| s.block(c).map(t::block_name)));
    s.seconds(0.5);
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

// ---------------------------------------------------------------- water

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
    let taken = s.until(8.0, |s| s.feet().x - before > 0.5);
    let carried = s.feet().x - before;
    s.shot("carried_to_the_cut");
    assert!(taken, "the pond poured out beside the swimmer and carried them {carried:.2} blocks");
    no_corrections(&s);
}
