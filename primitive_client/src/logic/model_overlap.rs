//! **No two faces of any model fight over one plane.**
//!
//! The report was "модели мерцают из-за наложения текстур". A model here is
//! a handful of boxes, and two boxes whose faces look the same way from the
//! same plane over the same patch are two surfaces the depth buffer cannot
//! tell apart: which one wins is decided by rounding, differently at every
//! pixel and on every frame the camera moves, and where the two wear
//! different pictures -- a hoop over staves, a snout on a face, a blanket
//! over a mattress -- the patch boils between them.
//!
//! `mesh::BITE` and `animal_model::SEAM_BITE` settle two boxes that only
//! *touch*: each is grown a hair, so the face that meets the other ends up
//! buried inside it. Neither can settle two faces that lie flush, because
//! growing both boxes alike leaves flush faces flush. The skeletons were
//! built against that and hold themselves to it
//! (`no_two_bones_of_a_skeleton_fight_over_one_plane`); this is the same
//! rule asked of everything else that is drawn as boxes, on the faces as
//! they are emitted rather than on the tables that describe them, so a
//! model built by arithmetic -- a raft's oars, a turned chair -- is held to
//! it as firmly as one written out by hand.
//!
//! **The rule, in numbers**: two faces pointing the same way, within
//! `CLEARANCE` of one another along that way, may overlap by no more than
//! two bites -- which is what two boxes grown by a bite overlap by when
//! they only touch. `CLEARANCE` is `animal_model`'s, and the arithmetic for
//! it (a `Depth32Float` buffer with a near plane of 0.05 resolves two
//! surfaces 0.005 of a block apart at sixty-four blocks) is written there.
//! Undersides lying on the floor the model stands on are let off: nothing
//! above the floor can see them.

use glam::Vec3;
use primitive_shared::types::{self as t, BlockId};

use crate::engine::mesh::{self, Vertex};
use crate::engine::texture::FaceLayers;

/// A sixteenth of a block, the unit every model is written in.
const SIXTEENTH: f32 = 1.0 / 16.0;

/// How far apart two same-facing faces have to be for the depth buffer to
/// keep them apart: `animal_model::CLEARANCE`, 0.15 of a sixteenth.
const CLEARANCE: f32 = 0.15 * SIXTEENTH;

/// How much two faces that only touch are allowed to overlap: the two
/// bites the boxes they belong to were grown by, and a hair for the
/// arithmetic.
const TOUCH: f32 = 2.0 * mesh::BITE * SIXTEENTH + 1e-5;

/// One face as it is drawn: its outward normal from its winding, its
/// corners in blocks, and which picture it wears.
struct Face {
    normal: Vec3,
    corners: [Vec3; 4],
    picture: u32,
}

fn faces_of(vertices: &[Vertex]) -> Vec<Face> {
    vertices
        .chunks_exact(4)
        .map(|quad| {
            let corners = [0, 1, 2, 3].map(|k| Vec3::from(quad[k].position));
            Face {
                normal: (corners[1] - corners[0]).cross(corners[2] - corners[1]).normalize_or_zero(),
                corners,
                picture: quad[0].tex_layer(),
            }
        })
        .collect()
}

/// How far two quads in parallel planes overlap seen along their normal:
/// the least overlap of their shadows on any edge of either (separating
/// axes -- for rectangles the edges are the axes). Negative when apart.
fn overlap(a: &[Vec3; 4], b: &[Vec3; 4]) -> f32 {
    let mut least = f32::MAX;
    for quad in [a, b] {
        for k in 0..4 {
            let edge = (quad[(k + 1) % 4] - quad[k]).normalize_or_zero();
            let shadow = |q: &[Vec3; 4]| q.iter().map(|p| p.dot(edge)).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            let ((a_lo, a_hi), (b_lo, b_hi)) = (shadow(a), shadow(b));
            least = least.min(a_hi.min(b_hi) - a_lo.max(b_lo));
        }
    }
    least
}

/// Where a face lies, in sixteenths, for a message a person can find in a
/// table of boxes.
fn extent(face: &Face) -> String {
    let lo = face.corners.iter().fold(Vec3::MAX, |m, p| m.min(*p)) * 16.0;
    let hi = face.corners.iter().fold(Vec3::MIN, |m, p| m.max(*p)) * 16.0;
    format!("[{:.2}, {:.2}, {:.2}]..[{:.2}, {:.2}, {:.2}]", lo.x, lo.y, lo.z, hi.x, hi.y, hi.z)
}

/// Every pair of faces in `faces` that fights, described.
fn fights(name: &str, faces: &[Face]) -> Vec<String> {
    let floor = faces.iter().flat_map(|f| f.corners.map(|p| p.y)).fold(f32::MAX, f32::min);
    let mut found = Vec::new();
    for (index, a) in faces.iter().enumerate() {
        for b in &faces[index + 1..] {
            if a.normal.dot(b.normal) < 0.9999 {
                continue;
            }
            let apart = a.normal.dot(b.corners[0] - a.corners[0]).abs();
            if apart >= CLEARANCE {
                continue;
            }
            let on_the_floor =
                a.normal.y < -0.9999 && a.corners.iter().chain(b.corners.iter()).all(|p| (p.y - floor).abs() < 1e-4);
            if on_the_floor {
                continue;
            }
            let covered = overlap(&a.corners, &b.corners);
            if covered > TOUCH {
                found.push(format!(
                    "{name}: faces {:.3}/16 apart facing {:?} overlap by {:.3}/16 -- {} (picture {}) and {} (picture {})",
                    apart * 16.0,
                    a.normal.round(),
                    covered * 16.0,
                    extent(a),
                    a.picture,
                    extent(b),
                    b.picture,
                ));
            }
        }
    }
    found
}

/// Every model that is built of boxes in the terrain's vertex, by name: the
/// blocks drawn as models in the world and in the hand, in each state that
/// changes their shape, a raft in each rigging, and every animal standing,
/// fallen at each stage and as bones.
fn every_terrain_model() -> Vec<(String, Vec<Vertex>)> {
    use primitive_shared::animals::Species;
    use primitive_shared::raft::{Body, Wind};
    let layers = FaceLayers::empty_for_test();
    let mut models = Vec::new();
    let mut build = |name: String, emit: &mut dyn FnMut(&mut Vec<Vertex>, &mut Vec<u32>)| {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        emit(&mut vertices, &mut indices);
        assert_eq!(indices.len(), vertices.len() / 4 * 6, "{name} is not made of quads");
        models.push((name, vertices));
    };

    let mut blocks: Vec<(BlockId, String)> =
        t::ALL_BLOCK_IDS.iter().filter(|(block, _)| mesh::has_carried_model(*block)).map(|&(block, name)| (block, name.to_string())).collect();
    for water in [primitive_shared::body::Water::Fresh, primitive_shared::body::Water::Salt] {
        for jugs in 1..=t::BARREL_JUGS {
            blocks.push((t::barrel_of(water, jugs), format!("barrel of {water:?} x{jugs}")));
        }
    }
    blocks.push((t::rack_with_hide(t::BLOCK_DRYING_RACK, true), "drying rack with a hide".to_string()));
    // A hide frame is carried bare; its skin and the lacing run to it are
    // the boxes most likely to lie on each other, raw and cured.
    blocks.push((t::hide_frame_showing(t::BLOCK_HIDE_FRAME, true, false), "hide frame with a raw skin".to_string()));
    blocks.push((t::hide_frame_showing(t::BLOCK_HIDE_FRAME, false, true), "hide frame with a cured skin".to_string()));
    for (block, name) in blocks {
        build(name, &mut |v, i| {
            mesh::carried_model(block, &layers, v, i);
        });
    }
    // The world's own variants the hand never holds.
    for head in [true, false] {
        build(format!("a lone bed half (head {head})"), &mut |v, i| {
            mesh::furniture_block([0.0; 3], t::bed_half(t::Facing::South, head), false, &layers, 0x0F, v, i)
        });
    }
    // A whole rack with each thing it hangs: three fish to a column are
    // wider than the cords are apart (`mesh::hang_goods`).
    let whole_rack = t::rack_cells((0, 0, 0), t::Facing::North)[0].1;
    for goods in 1..primitive_shared::rack::HANGING.len() as u8 {
        build(format!("a whole rack hung with row {goods}"), &mut |v, i| {
            mesh::rack_block([0.0; 3], whole_rack, mesh::RackColumns::Whole(goods, goods), &layers, 0x0F, v, i)
        });
    }
    // A pile of logs is logs pressed against logs on every side, flank to
    // flank and course on course: the model most likely to lay two faces in
    // one plane.
    for logs in 1..=primitive_shared::pit::PILE_LOGS_MAX {
        build(format!("a pile of {logs} logs"), &mut |v, i| {
            mesh::log_pile_block([0.0; 3], primitive_shared::pit::log_pile(logs), &layers, 0x0F, v, i)
        });
    }
    build("bracket fungus".to_string(), &mut |v, i| {
        mesh::bracket_block([0.0; 3], t::BLOCK_BRACKET_FUNGUS, &layers, 0x0F, v, i)
    });
    for kind in [t::BLOCK_STALAGMITE, t::BLOCK_STALACTITE] {
        for size in 0..primitive_shared::dripstone::SIZES {
            let block = primitive_shared::dripstone::sized(kind, size);
            build(format!("dripstone {block:#x}"), &mut |v, i| mesh::dripstone_block([0.0; 3], block, &layers, 0x0F, v, i));
        }
    }
    for (joins, what) in [
        ([Some(16), Some(16), Some(2), Some(2), Some(2), Some(2)], "a trunk with four twigs"),
        ([None, None, Some(4), Some(4), Some(4), None], "a limb with a fork"),
    ] {
        build(format!("branch: {what}"), &mut |v, i| mesh::branch_block([0.0; 3], t::BLOCK_TWIG, (joins, mesh::ALONE), &layers, 0x0F, v, i));
    }

    let wind = Wind { toward: 0.4, strength: 0.8 };
    // Square, braced round one way and braced round the other: the yard is
    // the one part of a raft that moves without the raft moving, so a pass
    // that only ever saw it square would miss every overlap it can make.
    for (sail, angle, stroke) in [(true, 0.0, None), (false, -1.1, Some(1.0)), (true, 1.1, Some(4.0))] {
        let rigging = crate::logic::raft_model::Rigging { sail, angle, wind, stroke, hurt: None };
        build(format!("raft (sail {sail} at {angle}, stroke {stroke:?})"), &mut |v, i| {
            crate::logic::raft_model::build(&Body::at_rest(0.0, 0.0, 0.0, 0.0), &rigging, Vec3::ZERO, &layers, (15, 0), v, i)
        });
    }

    for &species in Species::ALL {
        let animal = crate::logic::animal_model::build;
        build(format!("{} standing", species.name()), &mut |v, i| {
            animal(species, Vec3::ZERO, 0.0, crate::logic::animal_model::Motion::default(), &layers, (15, 0), v, i)
        });
        for stage in 0..3 {
            build(format!("{} fallen, stage {stage}", species.name()), &mut |v, i| {
                crate::logic::animal_model::build_fallen(species, Vec3::ZERO, 0.0, stage, &layers, (15, 0), v, i)
            });
        }
    }
    models
}

/// The test's whole message: every fight in every model, or nothing.
fn report(found: Vec<String>) {
    assert!(found.is_empty(), "{} pairs of faces fight over one plane:\n{}", found.len(), found.join("\n"));
}

#[test]
fn no_two_faces_of_a_block_model_a_raft_or_an_animal_fight_over_one_plane() {
    let mut found = Vec::new();
    for (name, vertices) in every_terrain_model() {
        found.extend(fights(&name, &faces_of(&vertices)));
    }
    report(found);
}

/// Another player, as the figure is built without anything on.
///
/// A separate test because a figure is built in its own vertex. **Bare
/// only, and the reason is a finding, not a pass**: dressed, the same scan
/// finds the two legs' garments overlapping each other by 0.63 of a
/// sixteenth between the knees, and a shirt's hem and a sole a fiftieth of a
/// sixteenth off the skin they cover -- garments are padded outward
/// (`player_model::PADDING`) and two padded legs side by side meet in the
/// middle. That is `player_model`'s arrangement to change, and it is written
/// down in the changelog rather than fixed from here.
#[test]
fn no_two_faces_of_a_bare_player_fight_over_one_plane() {
    use crate::logic::player_model::{append, Pose};
    let pose = Pose { outfit: primitive_shared::protocol::Outfit::BARE, ..Pose::default() };
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    append(&pose, Vec3::ZERO, [1.0; 3], &mut vertices, &mut indices);
    let faces: Vec<Face> = vertices
        .chunks_exact(4)
        .map(|quad| {
            let corners = [0, 1, 2, 3].map(|k| Vec3::from(quad[k].position));
            Face {
                normal: (corners[1] - corners[0]).cross(corners[2] - corners[1]).normalize_or_zero(),
                corners,
                picture: 0,
            }
        })
        .collect();
    report(fights("a bare player", &faces));
}
