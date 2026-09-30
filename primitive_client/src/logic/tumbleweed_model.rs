//! What a tumbleweed looks like rolling: a ball of stems, drawn as three
//! crossed discs of its picture turning as it goes.
//!
//! The ball is the server's (`logic::tumbleweeds` there): an item the wind
//! pushes. The client draws it where the snapshots put it and **turns it by
//! how far it has gone**, which it can work out from the position alone --
//! a ball that rolled one radius has turned one radian. Nothing about the
//! turn is sent, and nothing needs to be: two players see the same ball in
//! the same place, and if they see it turned differently nobody can tell.
//!
//! Three planes at right angles, not a mesh of a sphere: a tumbleweed *is*
//! mostly holes, and three cut-out discs are six faces that read as a ball
//! from every side for the price of a tuft and a half. Rejected: the
//! dropped-item sprite every other drop wears -- a flat picture spinning on
//! its axis reads as a coin, and the thing is a ball bigger than a boot.

use glam::{Quat, Vec3};

use crate::engine::mesh::{pack_light, Vertex};

/// Half the ball, in blocks: the server's, which it collides at -- a ball
/// drawn bigger than it collides sinks into the wall it stops at.
pub const RADIUS: f32 = primitive_server::logic::tumbleweeds::RADIUS;

/// How the ball is turned when its middle is at `at` (world blocks): a roll
/// about z for the way it went along x and about x for the way it went
/// along z, a radian a radius. A ball does not remember which way it came,
/// so a path that doubled back unrolls it -- which a player watching a ball
/// blown one way does not see.
pub fn turned(at: glam::DVec3, id: u64) -> Quat {
    let r = f64::from(RADIUS);
    // The id's own quarter turn, so two balls side by side are not twins.
    let own = (id % 628) as f32 / 100.0;
    Quat::from_rotation_z((-(at.x / r) % std::f64::consts::TAU) as f32)
        * Quat::from_rotation_x(((at.z / r) % std::f64::consts::TAU) as f32)
        * Quat::from_rotation_y(own)
}

/// Appends the ball, centred on `middle` (relative to the frame's origin).
/// Lit as a plant is, all of it as an upward face (`mesh::cross_block`): a
/// bundle of stems has no side that faces the sun.
pub fn append(middle: Vec3, turn: Quat, layer: u32, (sky, block): (u8, u8), vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>) {
    let packed = pack_light(sky, block, 3, 0);
    let r = RADIUS;
    for (u, v) in [(Vec3::X, Vec3::Y), (Vec3::Z, Vec3::Y), (Vec3::X, Vec3::Z)] {
        let base = vertices.len() as u32;
        for (du, dv, uv) in [(-1.0, -1.0, [0.0, 1.0]), (1.0, -1.0, [1.0, 1.0]), (1.0, 1.0, [1.0, 0.0]), (-1.0, 1.0, [0.0, 0.0])] {
            let corner = middle + turn * ((u * du + v * dv) * r);
            vertices.push(Vertex::tinted(corner.to_array(), uv, layer, packed, 0));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ball_is_drawn_no_bigger_than_the_server_stops_it_at() {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        append(Vec3::ZERO, turned(glam::DVec3::new(3.3, 0.0, -7.1), 9), 1, (15, 0), &mut vertices, &mut indices);
        for v in &vertices {
            let reach = Vec3::from_array(v.position).length();
            assert!(reach <= RADIUS * std::f32::consts::SQRT_2 + 1e-4, "a corner {reach} from the middle");
        }
        assert_eq!(indices.len(), 18, "three planes, two triangles each");
    }

    #[test]
    fn a_ball_that_rolled_is_turned_and_one_that_did_not_is_not() {
        let here = turned(glam::DVec3::new(10.0, 64.0, 10.0), 1);
        let there = turned(glam::DVec3::new(10.5, 64.0, 10.0), 1);
        assert!(here.angle_between(there) > 1.0, "half a block of rolling turned it by {}", here.angle_between(there));
        assert!(here.angle_between(turned(glam::DVec3::new(10.0, 64.0, 10.0), 1)) < 1e-2);
    }
}
