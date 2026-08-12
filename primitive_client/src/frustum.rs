//! View-frustum culling.
//!
//! The renderer used to issue a draw call for every loaded chunk, which
//! at render distance 8 is 289 draws per frame -- and the player can see
//! at most a third of them at a 70 degree field of view. The rest were
//! fully processed by the GPU only to be thrown away after clipping.
//!
//! Extracting the six frustum planes straight from the view-projection
//! matrix (Gribb-Hartmann) means culling needs no extra state: whatever
//! the camera is doing this frame, the planes fall out of the matrix the
//! shader is already using.

use glam::{Mat4, Vec3};

use primitive_shared::types::{ChunkPos, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

/// A plane as (normal.xyz, distance), normals pointing *into* the volume.
#[derive(Debug, Clone, Copy)]
struct Plane {
    normal: Vec3,
    d: f32,
}

impl Plane {
    /// Signed distance from the plane to the closest corner of the box
    /// on the plane's positive side. Positive-vertex testing: if even the
    /// most favourable corner is behind the plane, the whole box is.
    #[inline]
    fn positive_vertex_distance(&self, min: Vec3, max: Vec3) -> f32 {
        let p = Vec3::new(
            if self.normal.x >= 0.0 { max.x } else { min.x },
            if self.normal.y >= 0.0 { max.y } else { min.y },
            if self.normal.z >= 0.0 { max.z } else { min.z },
        );
        self.normal.dot(p) + self.d
    }
}

pub struct Frustum {
    planes: [Plane; 6],
}

impl Frustum {
    pub fn from_view_proj(m: Mat4) -> Self {
        // glam is column-major; row i of the matrix is the i-th component
        // of each column.
        let row = |i: usize| {
            glam::Vec4::new(
                m.x_axis[i],
                m.y_axis[i],
                m.z_axis[i],
                m.w_axis[i],
            )
        };
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));

        // Left/right/bottom/top come from w +/- x|y. Near/far use the
        // z row directly, because wgpu's clip space is 0..1 in z (unlike
        // OpenGL's -1..1) -- using w + z here would clip a slab of the
        // world away right in front of the camera.
        let raw = [
            r3 + r0, // left
            r3 - r0, // right
            r3 + r1, // bottom
            r3 - r1, // top
            r2,      // near
            r3 - r2, // far
        ];

        let planes = raw.map(|p| {
            let normal = Vec3::new(p.x, p.y, p.z);
            let length = normal.length();
            if length > 0.0 {
                Plane {
                    normal: normal / length,
                    d: p.w / length,
                }
            } else {
                Plane {
                    normal: Vec3::Y,
                    d: f32::MAX,
                }
            }
        });

        Self { planes }
    }

    /// Conservative: may return true for a box that's actually outside
    /// (a corner case near the edges), never false for one inside.
    pub fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.positive_vertex_distance(min, max) >= 0.0)
    }

    /// A whole chunk column: full world height, so a chunk stays visible
    /// when only its treetops or its cave floor are on screen.
    pub fn contains_chunk(&self, pos: ChunkPos) -> bool {
        let min = Vec3::new(
            (pos.x * CHUNK_SIZE_X as i32) as f32,
            0.0,
            (pos.z * CHUNK_SIZE_Z as i32) as f32,
        );
        let max = min + Vec3::new(CHUNK_SIZE_X as f32, CHUNK_SIZE_Y as f32, CHUNK_SIZE_Z as f32);
        self.intersects_aabb(min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Camera in the middle of chunk (0, 0) looking down -Z, the way
    /// `Camera::new` starts. Centred rather than on the corner: a camera
    /// sitting exactly on a chunk boundary is a genuine edge case, not
    /// the situation this is meant to describe.
    fn looking_down_neg_z() -> Mat4 {
        let view = Mat4::look_to_rh(Vec3::new(8.0, 32.0, 8.0), -Vec3::Z, Vec3::Y);
        let proj = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.05, 1000.0);
        proj * view
    }

    #[test]
    fn a_chunk_straight_ahead_is_visible() {
        let f = Frustum::from_view_proj(looking_down_neg_z());
        assert!(f.contains_chunk(ChunkPos::new(0, -3)), "directly in front");
        assert!(
            f.contains_chunk(ChunkPos::new(0, 0)),
            "the chunk the camera is standing in"
        );
    }

    #[test]
    fn a_chunk_directly_behind_is_culled() {
        let f = Frustum::from_view_proj(looking_down_neg_z());
        assert!(
            !f.contains_chunk(ChunkPos::new(0, 8)),
            "a chunk well behind the camera must be culled"
        );
    }

    #[test]
    fn a_chunk_far_off_to_the_side_is_culled() {
        let f = Frustum::from_view_proj(looking_down_neg_z());
        assert!(
            !f.contains_chunk(ChunkPos::new(40, -3)),
            "way outside the horizontal field of view"
        );
    }

    #[test]
    fn nothing_near_the_camera_is_wrongly_clipped() {
        // Regression guard for the clip-space convention: with the OpenGL
        // style near plane (w + z) instead of wgpu's (z), chunks just in
        // front of the camera vanish.
        let f = Frustum::from_view_proj(looking_down_neg_z());
        for z in -6..=0 {
            assert!(
                f.contains_chunk(ChunkPos::new(0, z)),
                "chunk at z={z} should be visible"
            );
        }
    }

    #[test]
    fn turning_around_changes_what_is_visible() {
        let view = Mat4::look_to_rh(Vec3::new(8.0, 32.0, 8.0), Vec3::Z, Vec3::Y);
        let proj = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.05, 1000.0);
        let f = Frustum::from_view_proj(proj * view);
        assert!(f.contains_chunk(ChunkPos::new(0, 5)), "now in front");
        assert!(!f.contains_chunk(ChunkPos::new(0, -8)), "now behind");
    }
}
