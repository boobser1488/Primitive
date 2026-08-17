//! Turning a sprite into a solid: the shape a dropped item has.
//!
//! ## Why anything but a cube
//!
//! Everything lying in the world used to be drawn as a small spinning
//! cube of its own texture. For a block that is honest -- a dropped
//! cobblestone *is* a cube. For anything else it is a lie the eye
//! catches immediately: a handful of fibre or a twig is not a box, and a
//! sprite with transparent corners wrapped onto one comes out with those
//! corners drawn as whatever was behind them, because the pass entities
//! are drawn in cannot discard.
//!
//! So an item gets the shape its picture already describes. The texture
//! is read as a stencil: every texel that is not transparent becomes a
//! slab one texel across and `THICKNESS` deep, and the whole thing is
//! one thin plate with a silhouette cut out of it. The point is that
//! the artist draws one 16x16 picture and gets a three-dimensional
//! object out of it -- no second asset, no modelling, and a new item
//! is a new PNG.
//!
//! ## Not one box per texel
//!
//! A 16x16 sprite has 256 texels; a box each is 1,536 quads for one
//! dropped twig, rebuilt every frame because items bob and spin.
//!
//! Two things bring that down to a few dozen. The flat faces -- front
//! and back -- are merged into the largest rectangles that fit inside
//! the silhouette, which for a drawn sprite is a handful. And the rim is
//! only generated where the silhouette actually has an edge: a texel in
//! the middle of a solid area has no side to show, and runs of edge
//! along the same line are merged into one quad.
//!
//! The result is built once, at load, and only transformed per frame.

use glam::{Mat4, Vec3};
use image::RgbaImage;

use crate::engine::mesh::pack_light;

/// How thick a sprite is once it has been given a third dimension, as a
/// fraction of its width.
///
/// One texel: enough to read as an object rather than as a decal,
/// little enough to still read as flat. Thicker and a dropped twig
/// looks like a plank of itself; thinner and it disappears edge-on.
const THICKNESS: f32 = 1.0 / 16.0;

/// A texel counts as part of the shape at or above this alpha. The same
/// cutoff the shader uses, so what is modelled and what is drawn agree.
const ALPHA_CUTOFF: u8 = 128;

/// One vertex of an item model.
///
/// Its own format rather than the terrain's, because the terrain vertex
/// packs its texture coordinates into two bits -- block faces are mapped
/// corner to corner, so they only ever need zero and one. A sprite quad
/// covers some arbitrary rectangle of its texture and needs the real
/// numbers.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ItemVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// Texture layer in the top half, the light word in the bottom --
    /// the same light word the terrain uses, so `shade` in the shader
    /// needs no second version.
    pub packed: u32,
}

impl ItemVertex {
    pub const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Uint32,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ItemVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// One rectangle of the model, in the sprite's own space.
///
/// x and y run -0.5..0.5 across the sprite, z is ±`THICKNESS`/2, and the
/// face index is the terrain's, purely so the shader can pick a normal
/// and light it the same way it lights everything else.
#[derive(Debug, Clone, Copy)]
pub struct Quad {
    pub corners: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub face: u8,
}

/// A sprite with a thickness. Built once per texture at load.
#[derive(Debug, Clone, Default)]
pub struct ItemModel {
    pub quads: Vec<Quad>,
}

impl ItemModel {
    /// How many triangles one of these costs to draw.
    #[cfg(test)]
    pub fn triangles(&self) -> usize {
        self.quads.len() * 2
    }

    /// Reads a texture's alpha as a silhouette and gives it depth.
    pub fn from_image(image: &RgbaImage) -> Self {
        let width = image.width() as usize;
        let height = image.height() as usize;
        let solid: Vec<bool> = image
            .pixels()
            .map(|pixel| pixel.0[3] >= ALPHA_CUTOFF)
            .collect();
        Self::from_mask(&solid, width, height)
    }

    /// The same, from a bare mask. Split out because the interesting
    /// half has nothing to do with images and everything to do with
    /// rectangles, and that half is what wants testing.
    pub fn from_mask(solid: &[bool], width: usize, height: usize) -> Self {
        let mut quads = Vec::new();
        if width == 0 || height == 0 {
            return Self { quads };
        }
        let at = |x: usize, y: usize| solid[y * width + x];

        // ---- the two flat faces ----
        //
        // Greedy rectangles: take the first cell not yet covered, run
        // right while the row is still solid and uncovered, then down
        // while whole rows of that width are. The standard voxel greedy
        // sweep, in two dimensions.
        let mut covered = vec![false; solid.len()];
        for y0 in 0..height {
            for x0 in 0..width {
                if !at(x0, y0) || covered[y0 * width + x0] {
                    continue;
                }
                let mut x1 = x0;
                while x1 + 1 < width && at(x1 + 1, y0) && !covered[y0 * width + x1 + 1] {
                    x1 += 1;
                }
                let mut y1 = y0;
                'rows: while y1 + 1 < height {
                    for x in x0..=x1 {
                        if !at(x, y1 + 1) || covered[(y1 + 1) * width + x] {
                            break 'rows;
                        }
                    }
                    y1 += 1;
                }
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        covered[y * width + x] = true;
                    }
                }
                let rect = Rect { x0, y0, x1, y1 };
                quads.push(flat_face(rect, width, height, true));
                quads.push(flat_face(rect, width, height, false));
            }
        }

        // ---- the rim ----
        //
        // A side is needed exactly where a solid texel meets something
        // that is not one, edge of the picture included. Runs along the
        // same line become one quad, which is most of what keeps a
        // drawn sprite (all long smooth strokes) cheap.
        for (side, along_y) in [(Side::Left, true), (Side::Right, true), (Side::Top, false), (Side::Bottom, false)] {
            let (outer, inner) = if along_y { (width, height) } else { (height, width) };
            for a in 0..outer {
                let mut run: Option<usize> = None;
                for b in 0..=inner {
                    let exposed = b < inner && {
                        let (x, y) = if along_y { (a, b) } else { (b, a) };
                        at(x, y) && !neighbour_solid(&at, x, y, width, height, side)
                    };
                    match (exposed, run) {
                        (true, None) => run = Some(b),
                        (false, Some(start)) => {
                            let rect = if along_y {
                                Rect { x0: a, y0: start, x1: a, y1: b - 1 }
                            } else {
                                Rect { x0: start, y0: a, x1: b - 1, y1: a }
                            };
                            quads.push(rim_face(rect, width, height, side));
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
        }

        Self { quads }
    }

    /// Appends the model to a vertex list, moved, spun about the
    /// vertical axis and lit.
    ///
    /// The spin is what makes a flat plate readable: seen exactly
    /// edge-on it is a line, and an item that vanishes once a second
    /// would be worse than a cube.
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &self,
        vertices: &mut Vec<ItemVertex>,
        indices: &mut Vec<u32>,
        centre: [f32; 3],
        scale: f32,
        yaw: f32,
        layer: u32,
        sky: u8,
        block_light: u8,
    ) {
        // The world's convention is the one that was here first: a
        // positive yaw turns the model the way `x cos - z sin` turns it,
        // which is a rotation about -Y in glam's reckoning. Spelt out
        // rather than left to be rediscovered, because getting it
        // backwards makes every dropped item spin the wrong way round
        // and nothing else at all.
        let transform = Mat4::from_translation(Vec3::from_array(centre))
            * Mat4::from_rotation_y(-yaw)
            * Mat4::from_scale(Vec3::splat(scale));
        self.append_transformed(vertices, indices, transform, layer, sky, block_light);
    }

    /// The same, under any transform at all.
    ///
    /// Split out for the view model. A tool held in the hand is pitched,
    /// rolled, pushed forward and swung, and none of that is expressible
    /// as "a scale and a yaw" -- while everything else about the two
    /// cases is identical: the same quads, the same texture, the same
    /// winding. A second copy of this loop next to the hand would be a
    /// second place for the next winding bug to be fixed in.
    pub fn append_transformed(
        &self,
        vertices: &mut Vec<ItemVertex>,
        indices: &mut Vec<u32>,
        transform: Mat4,
        layer: u32,
        sky: u8,
        block_light: u8,
    ) {
        for quad in &self.quads {
            let base = vertices.len() as u32;
            // Ambient occlusion 3 -- unoccluded. An item lying in the
            // world is surrounded by air by definition.
            let light = pack_light(sky, block_light, 3, quad.face);
            for (corner, uv) in quad.corners.iter().zip(quad.uv.iter()) {
                let position = transform.transform_point3(Vec3::from_array(*corner));
                vertices.push(ItemVertex {
                    position: position.to_array(),
                    uv: *uv,
                    packed: (layer << 16) | light,
                });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

/// A run of texels, inclusive at both ends.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// Is the texel on the far side of this edge part of the shape?
///
/// Off the picture counts as empty, which is what puts a rim around a
/// sprite that runs to the edge of its tile.
fn neighbour_solid(
    at: &impl Fn(usize, usize) -> bool,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    side: Side,
) -> bool {
    match side {
        Side::Left => x > 0 && at(x - 1, y),
        Side::Right => x + 1 < width && at(x + 1, y),
        Side::Top => y > 0 && at(x, y - 1),
        Side::Bottom => y + 1 < height && at(x, y + 1),
    }
}

/// Texel coordinates to the sprite's own space.
///
/// Image rows run downwards and the model's y runs up, so the vertical
/// axis is flipped here and nowhere else.
fn to_local(x: f32, y: f32, width: usize, height: usize) -> (f32, f32) {
    (x / width as f32 - 0.5, 0.5 - y / height as f32)
}

fn flat_face(rect: Rect, width: usize, height: usize, front: bool) -> Quad {
    let (u0, v0) = (
        rect.x0 as f32 / width as f32,
        rect.y0 as f32 / height as f32,
    );
    let (u1, v1) = (
        (rect.x1 + 1) as f32 / width as f32,
        (rect.y1 + 1) as f32 / height as f32,
    );
    let (x0, y0) = to_local(rect.x0 as f32, rect.y0 as f32, width, height);
    let (x1, y1) = to_local((rect.x1 + 1) as f32, (rect.y1 + 1) as f32, width, height);
    let z = if front { THICKNESS * 0.5 } else { -THICKNESS * 0.5 };

    // Wound so the visible side faces outwards on both plates.
    if front {
        Quad {
            corners: [[x0, y1, z], [x1, y1, z], [x1, y0, z], [x0, y0, z]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            face: 4, // +Z
        }
    } else {
        Quad {
            corners: [[x1, y1, z], [x0, y1, z], [x0, y0, z], [x1, y0, z]],
            uv: [[u1, v1], [u0, v1], [u0, v0], [u1, v0]],
            face: 5, // -Z
        }
    }
}

/// One strip of the rim, standing between the front and back plates.
fn rim_face(rect: Rect, width: usize, height: usize, side: Side) -> Quad {
    let (u0, v0) = (
        rect.x0 as f32 / width as f32,
        rect.y0 as f32 / height as f32,
    );
    let (u1, v1) = (
        (rect.x1 + 1) as f32 / width as f32,
        (rect.y1 + 1) as f32 / height as f32,
    );
    let (x0, y0) = to_local(rect.x0 as f32, rect.y0 as f32, width, height);
    let (x1, y1) = to_local((rect.x1 + 1) as f32, (rect.y1 + 1) as f32, width, height);
    let (front, back) = (THICKNESS * 0.5, -THICKNESS * 0.5);

    // The texture on a rim strip is the one texel it stands on, stretched
    // across the thickness. There is nothing else it could be: the
    // artist drew a picture, not a solid, and the edge of a stroke is
    // the colour of that stroke.
    match side {
        Side::Left => Quad {
            corners: [[x0, y1, back], [x0, y1, front], [x0, y0, front], [x0, y0, back]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            face: 3, // -X
        },
        Side::Right => Quad {
            corners: [[x1, y1, front], [x1, y1, back], [x1, y0, back], [x1, y0, front]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            face: 2, // +X
        },
        // **Wound the other way round than they read.** Listing the top
        // strip from back to front puts its geometric normal at -Y while
        // the quad calls itself face 0, which is +Y -- and the face
        // index is what `face_normal` in the shader turns into a
        // lambert term. So the top edge of every dropped sprite was lit
        // as though it faced the ground and the bottom edge as though it
        // faced the sky, which is upside down twice over. Nothing
        // vanished, because the item pipeline does not cull, so the
        // mistake could only ever be seen as shading -- which is exactly
        // the kind that survives.
        Side::Top => Quad {
            corners: [[x0, y0, front], [x1, y0, front], [x1, y0, back], [x0, y0, back]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            face: 0, // +Y
        },
        Side::Bottom => Quad {
            corners: [[x0, y1, back], [x1, y1, back], [x1, y1, front], [x0, y1, front]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            face: 1, // -Y
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(rows: &[&str]) -> (Vec<bool>, usize, usize) {
        let height = rows.len();
        let width = rows[0].len();
        let mut solid = Vec::with_capacity(width * height);
        for row in rows {
            assert_eq!(row.len(), width, "ragged mask");
            solid.extend(row.chars().map(|c| c == '#'));
        }
        (solid, width, height)
    }

    fn model(rows: &[&str]) -> ItemModel {
        let (solid, width, height) = mask(rows);
        ItemModel::from_mask(&solid, width, height)
    }

    #[test]
    fn an_empty_sprite_has_no_shape() {
        let m = model(&["....", "....", "....", "...."]);
        assert!(m.quads.is_empty(), "empty texture produced geometry");
    }

    #[test]
    fn a_full_square_is_a_box() {
        // Six faces and not one more: the two plates each merge to a
        // single rectangle, and each of the four edges is one run.
        let m = model(&["####", "####", "####", "####"]);
        assert_eq!(m.quads.len(), 6, "a solid sprite should be a plain box");
    }

    #[test]
    fn merging_beats_one_box_per_texel() {
        // The whole reason the greedy sweep is here. Sixteen texels as
        // separate boxes would be 96 quads.
        let m = model(&["####", "####", "####", "####"]);
        assert!(m.quads.len() < 16, "no merging happened at all");
    }

    #[test]
    fn a_hole_gets_a_rim_of_its_own() {
        // Interior edges matter as much as the silhouette: without them
        // you see through the plate into nothing.
        let solid = model(&["####", "#..#", "#..#", "####"]);
        let full = model(&["####", "####", "####", "####"]);
        assert!(
            solid.quads.len() > full.quads.len(),
            "a ring produced no more geometry than a solid square"
        );
    }

    #[test]
    fn every_quad_stays_inside_the_sprite() {
        let m = model(&[".##.", "####", ".##.", "..#."]);
        assert!(!m.quads.is_empty());
        for quad in &m.quads {
            for corner in &quad.corners {
                assert!((-0.5..=0.5).contains(&corner[0]), "x {} left the sprite", corner[0]);
                assert!((-0.5..=0.5).contains(&corner[1]), "y {} left the sprite", corner[1]);
                assert!(corner[2].abs() <= THICKNESS, "z {} is thicker than the plate", corner[2]);
            }
            for uv in &quad.uv {
                assert!((0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]));
            }
        }
    }

    #[test]
    fn every_quad_faces_the_way_it_says_it_does() {
        // The face index is not decoration: the shader turns it into a
        // normal and lights the quad by it. A strip wound against its
        // own label is lit as its opposite, and because the item
        // pipeline does not cull, nothing disappears to give it away --
        // the top and bottom rims were lit upside down for exactly that
        // reason.
        let m = model(&[".##.", "####", "#..#", "..#."]);
        assert!(!m.quads.is_empty());
        // Face order is the mesher's: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
        const NORMALS: [[f32; 3]; 6] = [
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        for quad in &m.quads {
            let [a, b, c, _] = quad.corners;
            let edge1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let edge2 = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
            let cross = [
                edge1[1] * edge2[2] - edge1[2] * edge2[1],
                edge1[2] * edge2[0] - edge1[0] * edge2[2],
                edge1[0] * edge2[1] - edge1[1] * edge2[0],
            ];
            let claimed = NORMALS[quad.face as usize];
            let dot: f32 = (0..3).map(|i| cross[i] * claimed[i]).sum();
            assert!(
                dot > 0.0,
                "a quad calling itself face {} is wound facing the other way",
                quad.face
            );
        }
    }

    #[test]
    fn a_sprite_running_to_the_edge_still_has_sides() {
        // Off the picture counts as empty, or a stroke touching the
        // border would have an open end.
        let m = model(&["##", "##"]);
        // Two plates plus four rims.
        assert_eq!(m.quads.len(), 6);
    }

    #[test]
    fn one_lone_texel_is_a_complete_little_box() {
        let m = model(&["....", ".#..", "....", "...."]);
        assert_eq!(m.quads.len(), 6, "a single texel should be closed on all sides");
    }

    #[test]
    fn a_drawn_sprite_costs_tens_of_quads_rather_than_hundreds() {
        // A diagonal stroke across a 16x16 tile: the shape most item
        // textures actually are. Per-texel boxes would be ~200 quads.
        let mut rows = Vec::new();
        for y in 0..16u32 {
            let mut row = String::new();
            for x in 0..16u32 {
                row.push(if x.abs_diff(y) <= 1 { '#' } else { '.' });
            }
            rows.push(row);
        }
        let refs: Vec<&str> = rows.iter().map(|r| r.as_str()).collect();
        let m = model(&refs);
        assert!(
            m.quads.len() < 120,
            "a simple stroke came to {} quads",
            m.quads.len()
        );
        assert!(m.quads.len() > 8, "a stroke needs more than a plate");
    }

    #[test]
    fn the_two_plates_face_opposite_ways() {
        let m = model(&["##", "##"]);
        assert!(m.quads.iter().any(|q| q.face == 4), "no front");
        assert!(m.quads.iter().any(|q| q.face == 5), "no back");
    }

    #[test]
    fn appending_places_the_model_where_it_is_told() {
        let m = model(&["##", "##"]);
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        m.append(&mut vertices, &mut indices, [10.0, 20.0, 30.0], 0.5, 0.0, 7, 15, 0);
        assert_eq!(indices.len(), m.triangles() * 3);
        for vertex in &vertices {
            assert!((vertex.position[0] - 10.0).abs() <= 0.3);
            assert!((vertex.position[1] - 20.0).abs() <= 0.3);
            assert!((vertex.position[2] - 30.0).abs() <= 0.3);
            assert_eq!(vertex.packed >> 16, 7, "the texture layer was lost");
        }
    }

    #[test]
    fn a_quarter_turn_of_yaw_sends_x_to_z() {
        // `append` is a wrapper around `append_transformed` now, and the
        // one thing that wrapper can get wrong is the sign of the
        // rotation -- glam turns about +Y where the hand-written loop
        // this replaced turned about -Y. Nothing crashes if it is
        // flipped; every dropped item simply spins backwards, which is
        // exactly the kind of thing nobody notices for a month.
        let m = model(&["##", "##"]);
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        m.append(
            &mut vertices,
            &mut indices,
            [0.0, 0.0, 0.0],
            2.0,
            std::f32::consts::FRAC_PI_2,
            0,
            15,
            0,
        );
        // The corner that sat at +x before the turn is now at +z.
        let far_z = vertices
            .iter()
            .map(|v| v.position[2])
            .fold(f32::MIN, f32::max);
        assert!(far_z > 0.9, "a quarter turn did not carry +x round to +z");
        let far_x = vertices
            .iter()
            .map(|v| v.position[0])
            .fold(f32::MIN, f32::max);
        assert!(far_x < 0.2, "the model is still facing the way it started");
    }

    #[test]
    fn the_vertex_is_the_size_the_pipeline_expects() {
        assert_eq!(std::mem::size_of::<ItemVertex>(), 24);
    }
}
