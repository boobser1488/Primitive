//! What a raft looks like: five logs lashed with rawhide under a floor of
//! planks, a mast, a sail on a yard, and a pair of oars.
//!
//! ## Built in the deck's own frame
//!
//! Every box is written in `[along, up, across]` measured from the waterline
//! under the middle of the raft -- the frame `raft::Body::world_of` turns into
//! the world -- so the planks' top is `raft::FREEBOARD` *by construction*, and
//! the floor a player's feet rest on (`physics::Player::decks`) is the floor
//! that is drawn. A model measured in its own units and scaled to fit would be
//! a deck that is a finger's width above or below the collider, which reads as
//! floating feet or sunk shins on every frame anybody stands on it.
//!
//! ## Three rotations and one rule for the light
//!
//! A box is turned twice before it reaches the world: by its own turn (an oar
//! swung out and dipped, the yard swung to the wind) and by the raft's
//! heading. The light word holds one of six directions (`mesh::pack_light`),
//! so every face is lit as whichever axis its *world* normal is nearest --
//! the rule `animal_model` arrived at the hard way, written once here as a
//! nearest-axis test on the turned normal rather than as a table of quarter
//! turns, because nothing on a raft turns by quarters.
//!
//! ## What was not drawn
//!
//! No rigging lines, no rowlocks, no knots. At the distance a raft is seen
//! from, a line a sixtieth of a block thick is a shimmer, and every box added
//! is twelve triangles a frame for every raft in sight. The lashings are the
//! exception, because they are what says "lashed" rather than "a floor of
//! logs", and they are wide enough to read.

use glam::{Mat3, Vec3};
use primitive_shared::raft::{self, Body, Wind};
use primitive_shared::types::{BLOCK_HIDE, BLOCK_LOG, BLOCK_PLANKS, BLOCK_STRIPPED_LOG};

use crate::engine::mesh::{face_uv, faces, pack_light, Vertex};
use crate::engine::texture::FaceLayers;

/// How far forward of the middle the mast stands, in blocks.
///
/// Forward rather than central, so that a passenger standing in the middle of
/// the deck is not standing in the mast -- which is not solid (a pole a tenth
/// of a block wide that stopped a walking player would be a deck with an
/// invisible pillar in it) and so would otherwise be drawn through a body.
///
/// **Shared now**: the server has to answer "is this player standing at the
/// sail" (`raft::at_the_sail`), and a mast drawn here and reached from there
/// would be a rope hanging where nothing is.
const MAST_ALONG: f32 = raft::MAST_ALONG;
/// How high the yard hangs above the deck.
const YARD_UP: f32 = 2.05;
/// The furthest the yard swings off square, in radians. Past this a square
/// sail wraps round its own mast.
///
/// **The same number the rules clamp the angle to**, and it has to be: what
/// the yard is drawn at is what the wind is measured against
/// (`raft::Body::sail_normal`). Two copies of this is a sail that points one
/// way and pulls another, which is a dial that lies.
const YARD_SWING: f32 = raft::SAIL_MAX_ANGLE;
/// Half the yard's length.
const YARD_HALF: f32 = 0.95;
/// How far under the yard the middle of the sail hangs, and half its height.
const SAIL_DROP: f32 = 0.66;
const SAIL_HALF_HEIGHT: f32 = 0.62;
/// The most the sail stands off the yard in a full wind.
const SAIL_BELLY: f32 = 0.18;
/// Where the oars pivot, along the deck and out from its middle.
const OARLOCK_ALONG: f32 = -0.65;
const OARLOCK_ACROSS: f32 = 0.95;

/// Everything the model needs that is not where the raft is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rigging {
    /// Whether the sail is up.
    pub sail: bool,
    /// How far the yard is braced round from square: `Body::sail_angle`,
    /// which is what a hand on the sheets has set it to.
    pub angle: f32,
    /// The wind in the world, which the sail bellies with.
    pub wind: Wind,
    /// How far through a stroke the oars are, in radians, or `None` when
    /// nobody is rowing and they lie shipped along the sides.
    pub stroke: Option<f32>,
    /// A blow's flash, 0..1, as an animal's.
    pub hurt: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Material {
    /// Bark along a log, and the cut end at its ends.
    Log,
    Planks,
    /// A peeled pole: the mast, the yard, the oar shafts.
    Pole,
    /// Rawhide and the sail: the stretched hide.
    Hide,
    /// An oar's blade: a board.
    Board,
}

/// One box of the model, in the deck's frame.
#[derive(Debug, Clone, Copy)]
struct Piece {
    centre: Vec3,
    half: Vec3,
    /// Its own turn, applied about its centre before the raft's heading.
    turn: Mat3,
    material: Material,
}

impl Piece {
    fn square(centre: Vec3, half: Vec3, material: Material) -> Self {
        Self { centre, half, turn: Mat3::IDENTITY, material }
    }
}

/// The turn that makes a box's own +Z run along `along`, keeping its +Y as
/// near to up as it will go. Always a proper rotation, so a box stays wound
/// the way `faces` winds it -- a reflection here would turn every face of an
/// oar inside out, the fault `every_face_of_the_raft_is_wound_outward`
/// exists to catch.
fn facing(along: Vec3) -> Mat3 {
    let z = along.normalize();
    let x = Vec3::Y.cross(z).normalize();
    let y = z.cross(x);
    Mat3::from_cols(x, y, z)
}

/// The angle a relative direction makes, wrapped into -PI..PI.
fn wrap(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let a = angle.rem_euclid(TAU);
    if a > PI {
        a - TAU
    } else {
        a
    }
}

/// Every box of a raft with this rigging, pointing its bow along `yaw`.
fn pieces(yaw: f32, rigging: &Rigging) -> Vec<Piece> {
    let top = raft::FREEBOARD;
    let mut out = Vec::with_capacity(24);

    // Five logs, fore and aft, from a little under the waterline to just under
    // the planks. A hair of air between the log tops and the planks' bottoms,
    // so no two faces share a plane (see `animal_model::SEAM_BITE`).
    let log_half = Vec3::new(raft::HALF_LENGTH, 0.17, raft::HALF_WIDTH / 5.0 - 0.01);
    for i in 0..5 {
        let across = -raft::HALF_WIDTH + raft::HALF_WIDTH / 5.0 * (2 * i + 1) as f32;
        out.push(Piece::square(Vec3::new(0.0, top - 0.11 - log_half.y, across), log_half, Material::Log));
    }
    // Six planks across them, their tops exactly the deck.
    let plank_half = Vec3::new(raft::HALF_LENGTH / 6.0 - 0.01, 0.05, raft::HALF_WIDTH);
    for i in 0..6 {
        let along = -raft::HALF_LENGTH + raft::HALF_LENGTH / 6.0 * (2 * i + 1) as f32;
        out.push(Piece::square(Vec3::new(along, top - plank_half.y, 0.0), plank_half, Material::Planks));
    }
    // Two rawhide lashings round the logs, standing a little proud of them.
    for along in [-1.0f32, 1.0] {
        out.push(Piece::square(
            Vec3::new(along, top - 0.11 - log_half.y, 0.0),
            Vec3::new(0.06, log_half.y + 0.02, raft::HALF_WIDTH + 0.02),
            Material::Hide,
        ));
    }

    // The mast.
    let mast_foot = Vec3::new(MAST_ALONG, top, 0.0);
    out.push(Piece::square(mast_foot + Vec3::Y * 1.1, Vec3::new(0.06, 1.1, 0.06), Material::Pole));

    // The yard and the sail, braced where the hand on the sheets put them.
    //
    // **It used to square itself to the wind.** The yard swung on its own to
    // whatever angle caught the most, and the sail was therefore a weather
    // vane: it showed the wind, which was the only thing about the wind a
    // player could see, and it showed nothing about the raft. Now the angle
    // is the player's (`raft::Body::sail_angle`) and this draws it, which is
    // the whole of what makes the trim a decision -- a control you cannot see
    // the position of is a control nobody learns. What shows the wind instead
    // is the belly below, and the dial on the screen (`ui::hud::sail_gauge`).
    let swing = raft::trim_clamped(rigging.angle);
    // The yard's own +X is the way the sail's face looks: turned from along.
    // The same turn `Body::sail_normal` works the wind against.
    let yard_turn = Mat3::from_rotation_y(-swing);
    let yard_at = mast_foot + Vec3::Y * YARD_UP;
    out.push(Piece { centre: yard_at, half: Vec3::new(0.04, 0.04, YARD_HALF), turn: yard_turn, material: Material::Pole });
    if rigging.sail {
        // Which side the wind is on, and how hard it leans on the face it
        // can see: the sail bellies away from the wind, so a sail drawing
        // bellies forward and one taken aback bellies against its own mast.
        // The same dot product the rules drive the raft with, so what a
        // player sees filling is what is pulling.
        let relative = wrap(rigging.wind.toward - yaw);
        let press = (relative - swing).cos() * rigging.wind.strength.clamp(0.0, 1.0);
        let belly = press * SAIL_BELLY;
        let sheet = yard_at + yard_turn * Vec3::new(belly + 0.06, -SAIL_DROP, 0.0);
        out.push(Piece { centre: sheet, half: Vec3::new(0.02, SAIL_HALF_HEIGHT, 0.88), turn: yard_turn, material: Material::Hide });
    } else {
        // Furled: rolled up under the yard.
        let roll = yard_at + yard_turn * Vec3::new(0.0, -0.13, 0.0);
        out.push(Piece { centre: roll, half: Vec3::new(0.08, 0.08, 0.86), turn: yard_turn, material: Material::Hide });
    }

    // The oars.
    for side in [-1.0f32, 1.0] {
        let (lock, shaft_dir) = match rigging.stroke {
            // Out over the water and swept through the stroke: back along the
            // side and dipped at the catch, forward and lifted at the finish.
            Some(phase) => {
                let sweep = 0.25 + 0.45 * phase.sin();
                let dip = 0.36 + 0.14 * phase.cos();
                let out_flat = Vec3::new(-sweep.sin(), 0.0, side * sweep.cos());
                (
                    Vec3::new(OARLOCK_ALONG, top + 0.05, side * OARLOCK_ACROSS),
                    Vec3::new(out_flat.x * dip.cos(), -dip.sin(), out_flat.z * dip.cos()),
                )
            }
            // Shipped: laid along the side of the deck, blades aft.
            None => (Vec3::new(0.2, top + 0.04, side * (raft::HALF_WIDTH - 0.2)), Vec3::new(-1.0, 0.0, 0.0)),
        };
        let turn = facing(shaft_dir);
        out.push(Piece { centre: lock + turn * Vec3::new(0.0, 0.0, 0.35), half: Vec3::new(0.035, 0.035, 1.05), turn, material: Material::Pole });
        out.push(Piece { centre: lock + turn * Vec3::new(0.0, 0.0, 1.35), half: Vec3::new(0.022, 0.11, 0.3), turn, material: Material::Board });
    }
    out
}

/// The yard and the sail as one box in the deck's frame, `[along, up from the
/// waterline, across]`, as its centre and half-size: what a click at the
/// rigging meets (`Entities::aimed_raft`).
///
/// **A click at the sail has to reach the raft.** At the oars, using the raft
/// raises or furls the sail (`ClientMessage::UseRaft`), and the one box a
/// click was tested against was the hull, a slab from the keel to the planks.
/// A rower sits at the stern looking forward at the mast; a click at the sail
/// went over the planks and met nothing, and the only way to raise it was to
/// look down at the boards by your own feet, which nothing on the screen says.
/// Measured off the constants the pieces are laid out with, as wide as the yard
/// swings (`YARD_SWING`) and as deep as the sail bellies, so what can be
/// clicked is what is drawn at every wind
/// (`a_click_at_the_rigging_meets_every_corner_of_the_yard_and_the_sail_at_every_wind`).
///
/// Only for using a raft, never for striking one: a passenger standing under
/// the yard is inside this box, and a blow aimed at them must not land on the
/// timber instead.
pub fn rig_box() -> (Vec3, Vec3) {
    let top = raft::FREEBOARD + YARD_UP + 0.04;
    let bottom = raft::FREEBOARD + YARD_UP - SAIL_DROP - SAIL_HALF_HEIGHT;
    let along = YARD_HALF * YARD_SWING.sin() + SAIL_BELLY + 0.08;
    (Vec3::new(MAST_ALONG, (top + bottom) * 0.5, 0.0), Vec3::new(along, (top - bottom) * 0.5, YARD_HALF + 0.06))
}

/// The layer a material wears on one of a box's own faces.
fn layer_of(material: Material, face: usize, layers: &FaceLayers) -> u32 {
    match material {
        // A log's cut end on the faces that look along it.
        Material::Log if face == 2 || face == 3 => layers.layer_for_face(BLOCK_LOG, 0),
        Material::Log => layers.layer_for_face(BLOCK_LOG, 2),
        Material::Planks | Material::Board => layers.layer_for_face(BLOCK_PLANKS, 0),
        Material::Pole => layers.layer_for_face(BLOCK_STRIPPED_LOG, 2),
        Material::Hide => layers.layer_for_face(BLOCK_HIDE, 0),
    }
}

/// The index `mesh::faces` gives the axis a normal is nearest.
fn nearest_face(normal: Vec3) -> u8 {
    let a = normal.abs();
    if a.y >= a.x && a.y >= a.z {
        if normal.y >= 0.0 { 0 } else { 1 }
    } else if a.x >= a.z {
        if normal.x >= 0.0 { 2 } else { 3 }
    } else if normal.z >= 0.0 {
        4
    } else {
        5
    }
}

/// The outward normal of one of `mesh::faces`, in the box's own frame.
const FACE_NORMALS: [Vec3; 6] = [Vec3::Y, Vec3::NEG_Y, Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];

/// The turn from the deck's frame into the world's.
///
/// `(along, up, across)` to `(x, y, z)`, the rotation `raft::Body::world_of`
/// writes out by hand: along goes to `(cos, 0, sin)` and across to
/// `(-sin, 0, cos)`. glam turns the other way round the Y axis, hence the
/// minus.
fn heading(yaw: f32) -> Mat3 {
    Mat3::from_rotation_y(-yaw)
}

/// Appends a raft to a mesh in the terrain vertex format.
///
/// `origin` is the render origin (see `entities::build_meshes_into`); the
/// light is sampled by the caller once, at the deck, as an animal's is.
#[allow(clippy::too_many_arguments)]
pub fn build(
    body: &Body,
    rigging: &Rigging,
    origin: Vec3,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = light;
    let block_light = match rigging.hurt {
        Some(flash) => block_light.max((primitive_shared::types::MAX_LIGHT as f32 * flash.clamp(0.0, 1.0)) as u8),
        None => block_light,
    };
    let heading = heading(body.yaw);
    let at = Vec3::new(body.x as f32, body.y, body.z as f32) - origin;
    for piece in pieces(body.yaw, rigging) {
        let size = piece.half * 2.0;
        let turn = heading * piece.turn;
        for (face_index, face) in faces().iter().enumerate() {
            let layer = layer_of(piece.material, face_index, layers);
            let lit = nearest_face(turn * FACE_NORMALS[face_index]);
            // How long the face is along its picture's two axes, in blocks:
            // `face_uv` reads x and z off the top, z and y off the sides
            // looking along x, and x and y off the others.
            let (u_len, v_len) = match face_index {
                0 | 1 => (size.x, size.z),
                2 | 3 => (size.z, size.y),
                _ => (size.x, size.y),
            };
            let base = vertices.len() as u32;
            for corner in face.corners.iter() {
                let local = piece.centre + piece.turn * ((Vec3::from(*corner) - Vec3::splat(0.5)) * size);
                let world = at + heading * local;
                let uv = face_uv(face_index, *corner);
                vertices.push(
                    Vertex::tinted(world.to_array(), uv, layer, pack_light(sky, block_light, 3, lit), 0)
                        // One texel to a sixteenth of a block, so a plank is
                        // planks and not the whole picture squeezed onto a
                        // board. See `mesh::FINE_UV_BIT`.
                        .with_fine_uv([uv[0] * u_len, uv[1] * v_len]),
                );
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::raft::Body;

    fn riggings() -> Vec<Rigging> {
        let wind = |toward: f32| Wind { toward, strength: 0.8 };
        vec![
            Rigging { sail: true, angle: 0.0, wind: wind(0.0), stroke: None, hurt: None },
            Rigging { sail: true, angle: 0.9, wind: wind(2.5), stroke: Some(1.0), hurt: None },
            Rigging { sail: false, angle: -1.1, wind: wind(-1.0), stroke: Some(4.0), hurt: Some(0.5) },
        ]
    }

    /// The sail's face, in the world, as the model draws it.
    fn drawn_normal(yaw: f32, rigging: &Rigging) -> Vec3 {
        let sail = pieces(yaw, rigging)
            .into_iter()
            .find(|p| p.material == Material::Hide && p.half.y > 0.5)
            .expect("a sail");
        (heading(yaw) * sail.turn * Vec3::X).normalize()
    }

    #[test]
    fn every_face_of_the_raft_is_wound_outward_and_lit_as_the_way_it_faces() {
        // The two faults `flatcraft-model-winding` records for the drying
        // rack, asked of every box of a raft at headings that are not quarter
        // turns: a face wound inside out (the near side culled, the far side
        // drawn through it), and a face lit as the way it faced before it was
        // turned.
        let layers = FaceLayers::empty_for_test();
        for rigging in riggings() {
            for yaw in [0.0f32, 0.7, 2.0, 4.4] {
                let body = Body::at_rest(10.0, 5.0, -3.0, yaw);
                let (mut vertices, mut indices) = (Vec::new(), Vec::new());
                build(&body, &rigging, Vec3::ZERO, &layers, (15, 0), &mut vertices, &mut indices);
                let boxes = pieces(yaw, &rigging);
                assert_eq!(vertices.len(), boxes.len() * 24);
                let heading = heading(yaw);
                for (b, piece) in boxes.iter().enumerate() {
                    let centre = Vec3::new(body.x as f32, body.y, body.z as f32) + heading * piece.centre;
                    for face in 0..6 {
                        let quad = &vertices[b * 24 + face * 4..b * 24 + face * 4 + 4];
                        let p: Vec<Vec3> = quad.iter().map(|v| Vec3::from(v.position)).collect();
                        let wound = (p[1] - p[0]).cross(p[2] - p[0]);
                        let middle = (p[0] + p[1] + p[2] + p[3]) * 0.25;
                        assert!(
                            wound.dot(middle - centre) > 0.0,
                            "face {face} of box {b} ({:?}) is wound inward at yaw {yaw}",
                            piece.material
                        );
                        assert_eq!(
                            nearest_face(wound.normalize()),
                            nearest_face(heading * piece.turn * FACE_NORMALS[face]),
                            "face {face} of box {b} is lit as another direction at yaw {yaw}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_deck_the_model_draws_is_the_deck_a_player_stands_on() {
        let rigging = riggings()[0];
        let floor: Vec<Piece> = pieces(0.0, &rigging)
            .into_iter()
            .filter(|p| matches!(p.material, Material::Planks | Material::Log))
            .collect();
        let highest = floor.iter().map(|p| p.centre.y + p.half.y).fold(f32::MIN, f32::max);
        assert!((highest - raft::FREEBOARD).abs() < 1e-5, "the planks' top is {highest}, not the deck");
        for piece in &floor {
            assert!(piece.centre.x.abs() + piece.half.x <= raft::HALF_LENGTH + 1e-4, "{piece:?} overhangs the bow");
            assert!(piece.centre.z.abs() + piece.half.z <= raft::HALF_WIDTH + 1e-4, "{piece:?} overhangs the side");
            assert!(piece.centre.y - piece.half.y >= -raft::DRAFT, "{piece:?} is deeper than the hull draws");
        }
    }

    #[test]
    fn the_sail_is_drawn_facing_the_way_the_rules_push_the_raft() {
        // **The dial must not lie.** The angle the player braces the yard to
        // is the angle the wind is measured against (`Body::sail_normal`),
        // and it is also the angle this model turns the sail by. They were
        // two numbers once -- the yard squared itself to the wind while the
        // rules knew nothing about a yard -- and the whole of the trim being
        // a decision is that what you see is what is pulling.
        for yaw in [0.0f32, 0.7, 2.0, 4.4] {
            for step in -6..=6 {
                let angle = step as f32 / 6.0 * raft::SAIL_MAX_ANGLE;
                let rigging = Rigging { sail: true, angle, ..riggings()[0] };
                let mut body = Body::at_rest(0.0, 0.0, 0.0, yaw);
                body.sail_angle = angle;
                let (nx, nz) = body.sail_normal();
                let drawn = drawn_normal(yaw, &rigging);
                assert!(
                    (drawn.x - nx).abs() < 1e-4 && (drawn.z - nz).abs() < 1e-4,
                    "at yaw {yaw} and angle {angle} the sail is drawn facing {drawn} and pushes along ({nx}, {nz})"
                );
            }
        }
    }

    #[test]
    fn the_sail_bellies_downwind_and_is_taken_aback_by_a_head_wind() {
        let sheet = |wind_toward: f32| {
            let rigging = Rigging { sail: true, angle: 0.0, wind: Wind { toward: wind_toward, strength: 1.0 }, stroke: None, hurt: None };
            let boxes = pieces(0.0, &rigging);
            let yard = boxes.iter().find(|p| p.material == Material::Pole && p.half.z > 0.9).expect("a yard");
            let sail = boxes.iter().find(|p| p.material == Material::Hide && p.half.y > 0.5).expect("a sail");
            // How far the sail stands off the yard, along the raft.
            (sail.centre - yard.centre).x
        };
        assert!(sheet(0.0) > 0.1, "a wind from astern did not belly the sail forward");
        assert!(sheet(std::f32::consts::PI) < 0.0, "a head wind bellied the sail forward");
        let furled = pieces(0.0, &Rigging { sail: false, ..riggings()[0] });
        assert!(!furled.iter().any(|p| p.material == Material::Hide && p.half.y > 0.5), "a furled sail is still hoisted");
    }

    #[test]
    fn a_click_at_the_rigging_meets_every_corner_of_the_yard_and_the_sail_at_every_trim() {
        let (centre, half) = rig_box();
        for sail in [true, false] {
            // Every angle the yard can be braced to, and every wind that
            // could be leaning on it there: what can be clicked has to be
            // what is drawn, and both now move it.
            for step in 0..24 {
                let toward = step as f32 / 24.0 * std::f32::consts::TAU;
                let angle = (step as f32 / 23.0 * 2.0 - 1.0) * raft::SAIL_MAX_ANGLE;
                let rigging = Rigging { sail, angle, wind: Wind { toward, strength: 1.0 }, stroke: None, hurt: None };
                let rig: Vec<Piece> = pieces(0.0, &rigging)
                    .into_iter()
                    .filter(|p| p.centre.y > raft::FREEBOARD + 0.5)
                    .filter(|p| p.material == Material::Hide || (p.material == Material::Pole && p.half.z > 0.9))
                    .collect();
                assert_eq!(rig.len(), 2, "not a yard and a sail at wind {toward}, trim {angle}");
                for piece in rig {
                    for corner in 0..8 {
                        let sign = |bit: usize| if (corner >> bit) & 1 == 1 { 1.0 } else { -1.0 };
                        let local = piece.centre + piece.turn * (piece.half * Vec3::new(sign(0), sign(1), sign(2)));
                        assert!(
                            (local - centre).abs().cmple(half + Vec3::splat(1e-4)).all(),
                            "a corner of the {:?} at {local} is outside what a click meets, wind {toward}, trim {angle}",
                            piece.material
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn oars_nobody_is_pulling_lie_aboard_and_pulled_oars_put_their_blades_in_the_water() {
        let blades = |stroke: Option<f32>| -> Vec<f32> {
            let rigging = Rigging { stroke, ..riggings()[0] };
            pieces(0.0, &rigging)
                .iter()
                .filter(|p| p.material == Material::Board)
                .map(|p| p.centre.y)
                .collect()
        };
        for y in blades(None) {
            assert!(y > 0.0, "a shipped oar's blade is under the water at {y}");
        }
        for phase in [0.0f32, 1.5, 3.0, 4.5] {
            for y in blades(Some(phase)) {
                assert!(y < 0.1, "a pulled oar's blade is in the air at {y}, phase {phase}");
            }
        }
    }
}
