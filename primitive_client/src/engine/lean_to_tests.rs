//! **A lean-to is drawn where it is walked into, aimed at where it is drawn,
//! and lit as the way its faces point** -- in every facing, on the geometry
//! the mesher emits rather than on the numbers that describe it.
//!
//! The hut is one model drawn by its middle cell (`mesh::lean_to_block`) and
//! fifteen cells that collide as a table of slices fitted over that model
//! (`lean_to::boxes`). The two are written in different places by different
//! means -- a Blockbench file and a const table -- and nothing but this
//! keeps them one shape: a course of leaves lengthened in the editor, or a
//! slice's pitch retyped, would otherwise be a roof a body walks through or
//! a wall of air beside the thatch, and every other check would pass.

use glam::Vec3;
use primitive_shared::lean_to;
use primitive_shared::types::{BlockId, Facing, BLOCK_AIR};

use crate::engine::mesh::{self, Vertex};
use crate::engine::texture::FaceLayers;

const FACINGS: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];

/// A sixteenth of a cell.
const T: f32 = 1.0 / 16.0;

/// The hut round an anchor at the origin: every cell and what is in it.
fn hut(facing: Facing) -> Vec<((i32, i32, i32), BlockId)> {
    lean_to::cells((0, 0, 0), facing).to_vec()
}

/// The model as the mesher draws it, from the anchor at the origin.
fn drawn(facing: Facing) -> Vec<Vertex> {
    let layers = FaceLayers::empty_for_test();
    let (mut v, mut i) = (Vec::new(), Vec::new());
    for (at, id) in hut(facing) {
        let corner = [at.0 as f32, at.1 as f32, at.2 as f32];
        mesh::furniture_block(corner, id, true, &layers, 0xFF, &mut v, &mut i);
    }
    assert_eq!(i.len(), v.len() / 4 * 6, "{facing:?}: the hut is not quads");
    v
}

/// Every box the hut's fifteen cells collide as, in the world.
fn collided(facing: Facing) -> Vec<(Vec3, Vec3)> {
    let cells = hut(facing);
    let near = |x: i32, y: i32, z: i32| cells.iter().find(|&&(at, _)| at == (x, y, z)).map_or(BLOCK_AIR, |&(_, id)| id);
    let mut boxes = Vec::new();
    for &((x, y, z), id) in &cells {
        primitive_shared::geometry::for_each_block_box(
            id,
            x,
            y,
            z,
            |dx, dy, dz| near(x + dx, y + dy, z + dz),
            |lo, hi| boxes.push((Vec3::from(lo), Vec3::from(hi))),
        );
    }
    boxes
}

/// Points over a quad a sixteenth or less apart, corners and edges included.
fn samples(quad: &[Vertex]) -> Vec<Vec3> {
    let p = |k: usize| Vec3::from(quad[k].position);
    let (a, e1, e2) = (p(0), p(1) - p(0), p(3) - p(0));
    let n1 = (e1.length() / T).ceil().max(1.0) as usize;
    let n2 = (e2.length() / T).ceil().max(1.0) as usize;
    let mut out = Vec::new();
    for s in 0..=n1 {
        for t in 0..=n2 {
            out.push(a + e1 * (s as f32 / n1 as f32) + e2 * (t as f32 / n2 as f32));
        }
    }
    out
}

/// How far a point is from a quad: its corners are a rectangle (a face of a
/// box, turned or not), so the nearest point is the point's own place along
/// each edge, held to the edge's length.
fn distance_to_quad(point: Vec3, quad: &[Vertex]) -> f32 {
    let p = |k: usize| Vec3::from(quad[k].position);
    let (a, e1, e2) = (p(0), p(1) - p(0), p(3) - p(0));
    let s = ((point - a).dot(e1) / e1.length_squared().max(1e-12)).clamp(0.0, 1.0);
    let t = ((point - a).dot(e2) / e2.length_squared().max(1e-12)).clamp(0.0, 1.0);
    (a + e1 * s + e2 * t - point).length()
}

/// **Nothing is drawn where nothing stops a body, and nothing stops a body
/// more than seven sixteenths from what is drawn** -- in every facing.
///
/// The first half is every point of every face of the model (over the
/// ground) inside some cell's collider, give or take half a sixteenth: a
/// course of leaves that stood out of the table would be thatch a player
/// walks into and through. The second is every face of every collider box
/// that is not buried in another -- the surfaces a body actually meets --
/// within seven sixteenths of a face of the model. Seven because the
/// colliders are columns two sixteenths across under a roof pitched up to
/// 1.6, and the eaves of the front slice lie in a hollow of the outer line
/// the table's one pitch rides over; any more than that is air a body is
/// stopped by. The same arithmetic is what `lean_to`'s slices were fitted
/// to, so a model edited without them goes red here and nowhere else.
#[test]
fn a_lean_to_is_drawn_where_it_is_walked_into_whichever_way_it_faces() {
    const SLACK: f32 = 0.5 * T;
    const REACH: f32 = 7.0 * T;
    for facing in FACINGS {
        let model = drawn(facing);
        assert!(model.len() > 4 * 6 * 60, "{facing:?}: the hut is {} corners, not a hut", model.len());
        let boxes = collided(facing);
        let inside = |p: Vec3, grow: f32| boxes.iter().any(|&(lo, hi)| (p.cmpge(lo - grow) & p.cmple(hi + grow)).all());
        let mut loose = Vec::new();
        for quad in model.chunks_exact(4) {
            for point in samples(quad) {
                if point.y > SLACK && !inside(point, SLACK) {
                    loose.push(point);
                }
            }
        }
        assert!(loose.is_empty(), "{facing:?}: {} points of the thatch collide as nothing, e.g. {:?}", loose.len(), &loose[..loose.len().min(6)]);

        let buried = |p: Vec3| boxes.iter().any(|&(lo, hi)| (p.cmpgt(lo + Vec3::splat(1e-4)) & p.cmplt(hi - Vec3::splat(1e-4))).all());
        let mut walls = Vec::new();
        for &(lo, hi) in &boxes {
            // Each face of the box that is not its floor on the ground, as a
            // grid of points half a sixteenth in from its edges.
            for axis in 0..3 {
                for high in [false, true] {
                    if axis == 1 && !high && lo.y <= 1e-4 {
                        continue;
                    }
                    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                    let steps = |a: usize| (((hi[a] - lo[a]) / T).ceil() as usize).max(1);
                    for i in 0..=steps(u) {
                        for j in 0..=steps(v) {
                            let mut p = lo;
                            p[axis] = if high { hi[axis] + 1e-3 } else { lo[axis] - 1e-3 };
                            let along = |a: usize, k: usize| {
                                let at = lo[a] + (hi[a] - lo[a]) * k as f32 / steps(a) as f32;
                                if hi[a] - lo[a] > 2.0 * SLACK { at.clamp(lo[a] + SLACK, hi[a] - SLACK) } else { (lo[a] + hi[a]) / 2.0 }
                            };
                            p[u] = along(u, i);
                            p[v] = along(v, j);
                            if buried(p) {
                                continue;
                            }
                            let nearest = model.chunks_exact(4).map(|quad| distance_to_quad(p, quad)).fold(f32::MAX, f32::min);
                            if nearest > REACH {
                                walls.push((p, nearest));
                            }
                        }
                    }
                }
            }
        }
        walls.sort_by(|a, b| b.1.total_cmp(&a.1));
        assert!(
            walls.is_empty(),
            "{facing:?}: {} points of what collides are more than {REACH} from anything drawn, worst {:?}",
            walls.len(),
            &walls[..walls.len().min(6)]
        );
    }
}

/// **A cell of a hut is aimed at round the thatch it holds, and no further**:
/// the box a ray stops at is the box round the cell's colliders, so a click
/// on the hollow over the bed reaches the bed and a click beside the eaves
/// reaches the grass.
#[test]
fn a_cell_of_a_lean_to_is_aimed_at_round_what_it_collides_as() {
    for facing in FACINGS {
        let cells = hut(facing);
        for &((x, y, z), id) in &cells {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            primitive_shared::geometry::for_each_block_box(id, x, y, z, |_, _, _| BLOCK_AIR, |a, b| {
                lo = lo.min(Vec3::from(a));
                hi = hi.max(Vec3::from(b));
            });
            let (a, b) = primitive_shared::geometry::block_box_for_aim(id, x, y, z, false).expect("a hut is aimed at");
            assert!(
                (Vec3::from(a) - lo).abs().max_element() < 1e-4 && (Vec3::from(b) - hi).abs().max_element() < 1e-4,
                "{facing:?}: part {} is aimed at {a:?}..{b:?} and collides {lo}..{hi}",
                lean_to::part_of(id)
            );
        }
    }
}

/// **Every face of the hut is lit as the way it points**, turned or not.
///
/// A leaf course is a box turned about its length, and its face index is
/// the nearest of the six to where the turned face looks (`Swing::face_after`),
/// then turned again with the hut (`push_box_moved`). The shader makes a
/// normal out of that index; if it disagreed with the winding, a hut facing
/// east would be lit as one facing west and the shade on its thatch would
/// stay put when it was built the other way round. Asked of the winding,
/// never of the arithmetic that made the index.
#[test]
fn every_face_of_a_lean_to_is_wound_and_lit_the_way_it_points() {
    const AXES: [Vec3; 6] = [Vec3::Y, Vec3::NEG_Y, Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];
    for facing in FACINGS {
        for quad in drawn(facing).chunks_exact(4) {
            let p = |k: usize| Vec3::from(quad[k].position);
            let wound = (p(1) - p(0)).cross(p(2) - p(1)).normalize();
            let face = ((quad[0].light() >> 10) & 7) as usize;
            let best = AXES.iter().map(|axis| axis.dot(wound)).fold(f32::MIN, f32::max);
            assert!(
                AXES[face].dot(wound) > 0.0 && AXES[face].dot(wound) >= best - 1e-3,
                "{facing:?}: a face wound toward {wound} is lit as {:?}",
                AXES[face]
            );
        }
    }
}

/// **The hut is fifteen cells and stands in them**: the model's extent is
/// the three by three by two the rules hold (`lean_to::cells`), less the
/// back row's top, with its mouth toward the player who put it down -- on
/// the side of the anchor `Facing::step` points to.
#[test]
fn a_lean_to_stands_in_its_fifteen_cells_with_its_mouth_toward_its_builder() {
    for facing in FACINGS {
        let model = drawn(facing);
        let (low, high) = mesh::extent(&model);
        let cells = hut(facing);
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for &((x, y, z), _) in &cells {
            lo = lo.min(Vec3::new(x as f32, y as f32, z as f32));
            hi = hi.max(Vec3::new(x as f32 + 1.0, y as f32 + 1.0, z as f32 + 1.0));
        }
        // The feet of the fork and the ribs are driven into the ground, and
        // what is under it is nobody's to see.
        let give = mesh::BITE * T + 1e-4;
        assert!(
            (low.cmpge(lo - Vec3::new(give, 0.25, give)) & high.cmple(hi + Vec3::splat(give))).all(),
            "{facing:?}: drawn {low}..{high} out of the cells {lo}..{hi}"
        );
        // The mouth's end is the tall one: what is drawn over a cell high is
        // all on the mouth's side of the anchor or over the anchor.
        let (dx, dz) = facing.step();
        let toward = Vec3::new(dx as f32, 0.0, dz as f32);
        let middle = Vec3::new(0.5, 0.0, 0.5);
        let tall: Vec<f32> = model.iter().map(|v| Vec3::from(v.position)).filter(|p| p.y > 1.2).map(|p| (p - middle).dot(toward)).collect();
        assert!(!tall.is_empty() && tall.iter().all(|&along| along > -0.6), "{facing:?}: the tall end is not at the mouth");
    }
}
