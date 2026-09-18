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
///
/// Public because the flame on a held torch is drawn at the *middle* of
/// this slab and then slid toward the eye far enough to clear the front
/// of it -- see `hand::drawn_in_front_of_the_plate`, which reads this
/// through the transform rather than writing a number of its own. A
/// second copy of it there would be a copy that could drift, and the
/// drift would show as the fire and the fibre z-fighting through a
/// swing.
pub const THICKNESS: f32 = 1.0 / 16.0;

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
/// x and y run -0.5..0.5 across the sprite and z is ±`THICKNESS`/2.
///
/// **No face index.** Every quad used to carry one -- the terrain's
/// 0..5 -- and `append_transformed` wrote it into the light word for
/// the shader to turn into a normal. That is correct exactly once, at
/// the identity transform, and a dropped item spins: see the comment in
/// `append_transformed` for what a whole world of items lit by a
/// model-space normal looked like. The direction a rectangle points is
/// a fact about where its corners ended up, so it is worked out there
/// and nowhere else, and the winding below is the only thing that
/// decides it.
#[derive(Debug, Clone, Copy)]
pub struct Quad {
    pub corners: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
}

/// A sprite with a thickness. Built once per texture at load.
#[derive(Debug, Clone, Default)]
pub struct ItemModel {
    pub quads: Vec<Quad>,
}

impl ItemModel {
    /// How much of the picture the shape actually fills, as a fraction
    /// of the sprite's own width and height.
    ///
    /// **The frame is not the object.** A model's coordinates run
    /// -0.5..0.5 across the whole PNG, so scaling a model scales the
    /// *frame* -- and how much of that frame the drawing occupies is a
    /// decision the artist made about margins, not about how big the
    /// thing is. Native copper is a ten-by-seven lump in a sixteen
    /// square; a flint nodule is twelve by eleven; a handful of fibre
    /// fills the width. Held at one scale they come out three different
    /// sizes, and the largest of them was a slab across the corner of
    /// the screen.
    ///
    /// So anything that wants a *thing* of a given size divides by this.
    /// See `logic::hand::held_scale`, which is the caller that named it.
    ///
    /// `[0.0, 0.0]` for a model with no quads, which the texture loader
    /// refuses to keep -- a picture with no opaque texels is drawn as a
    /// cube instead.
    pub fn silhouette(&self) -> [f32; 2] {
        let (min, max) = self.drawn_box();
        [max[0] - min[0], max[1] - min[1]]
    }

    /// The lowest drawn texel, in the sprite's own space.
    ///
    /// Negative, and how negative is again the artist's margin rather
    /// than anything about the object: ash is drawn to the bottom edge
    /// of its tile and a pick head floats in the middle of its own. Any
    /// caller that has to make a thing *stand* on something has to know
    /// this, because standing the frame on the ground buries the one and
    /// hangs the other in mid-air. See `logic::entities`, which is
    /// where both were photographed.
    pub fn foot(&self) -> f32 {
        self.drawn_box().0[1]
    }

    /// The rectangle the drawing occupies inside the frame: minimum and
    /// maximum in x and y.
    ///
    /// Public because a spear is laid along it. `logic::hand` poses a
    /// held spear by two points in the frame and needs the drawing's
    /// own middle and its own length to do it -- `silhouette` gives the
    /// second and threw away the first, and a pose built on the
    /// *picture's* middle slides down its own axis by whatever margin
    /// the artist left in the corner.
    pub fn drawn_box(&self) -> ([f32; 2], [f32; 2]) {
        if self.quads.is_empty() {
            return ([0.0, 0.0], [0.0, 0.0]);
        }
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for quad in &self.quads {
            for corner in &quad.corners {
                for axis in 0..2 {
                    min[axis] = min[axis].min(corner[axis]);
                    max[axis] = max[axis].max(corner[axis]);
                }
            }
        }
        (min, max)
    }

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
    #[cfg_attr(not(test), allow(dead_code))] // `append_tipped` at rest is this
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

    /// Part way between standing and lying, so a dropped thing falls
    /// over rather than switching.
    ///
    /// `tip` is 0 for upright and 1 for flat; everything between is a
    /// quarter turn scaled, which is what the eye reads as the object
    /// toppling. See `entities::tipped_over` for why it is not a
    /// boolean any more.
    #[allow(clippy::too_many_arguments)]
    pub fn append_tipped(
        &self,
        vertices: &mut Vec<ItemVertex>,
        indices: &mut Vec<u32>,
        centre: [f32; 3],
        scale: f32,
        yaw: f32,
        tip: f32,
        layer: u32,
        sky: u8,
        block_light: u8,
    ) {
        let transform = Mat4::from_translation(Vec3::from_array(centre))
            * Mat4::from_rotation_y(-yaw)
            * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2 * tip.clamp(0.0, 1.0))
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
            let corners = quad.corners.map(|corner| {
                transform.transform_point3(Vec3::from_array(corner))
            });
            // **The direction the rectangle points after the transform,
            // not the one it was cut with.** The shader turns whatever
            // goes in the light word into a normal and a lambert term,
            // and every quad used to carry the face it had in the
            // sprite's own space -- so the shading was welded to the
            // model. A dropped item spins, and every dropped item in the
            // world was therefore lit by *which side of the plate you
            // happened to be looking at* rather than by where it
            // pointed: the front plate a fixed 0.55 of the sun and the
            // back a fixed 0.35, for ever, however the thing turned. Two
            // identical nodules a pace apart came out different
            // brightnesses for no reason a player could see, and the one
            // facing the sun was as often as not the dark one. The same
            // fault as an animal whose shading does not change as it
            // turns, and it survived for the same reason: nothing
            // disappears, because this pass does not cull.
            //
            // Taken from the emitted geometry rather than by rotating an
            // index, for the reason the rack's winding bug taught:
            // arithmetic checked against its own arithmetic proves
            // nothing, and the corners are what the rasteriser reads.
            let face = nearest_face((corners[1] - corners[0]).cross(corners[2] - corners[1]));
            // Ambient occlusion 3 -- unoccluded. An item lying in the
            // world is surrounded by air by definition.
            let light = pack_light(sky, block_light, 3, face);
            for (position, uv) in corners.iter().zip(quad.uv.iter()) {
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

/// The face index whose normal is closest to `normal`.
///
/// Six directions is a coarse basis for a rotated quad, but it is the
/// one the light word can hold -- see `mesh::pack_light` -- and the
/// error it costs is a few percent of a lambert term on an object with
/// no shadow to compare against. Widening the vertex to carry a real
/// normal would cost twelve bytes per vertex to fix something nobody
/// can see.
///
/// Face order is the mesher's: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
///
/// Lives here rather than in `logic::hand`, where it was written, so
/// that the two places a sprite model is turned into vertices snap to
/// the axes by the same rule. Two copies of this is how the last
/// face-normal bug in this file survived.
pub fn nearest_face(normal: Vec3) -> u8 {
    let [x, y, z] = normal.to_array();
    if y.abs() >= x.abs() && y.abs() >= z.abs() {
        if y >= 0.0 {
            0
        } else {
            1
        }
    } else if x.abs() >= z.abs() {
        if x >= 0.0 {
            2
        } else {
            3
        }
    } else if z >= 0.0 {
        4
    } else {
        5
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

    // Wound so the visible side faces outwards on both plates: the
    // front one anticlockwise seen from +Z, the back one from -Z. The
    // winding is now the only statement either of them makes about
    // which way it points, so getting it backwards is a plate lit as
    // its own reverse.
    if front {
        Quad {
            corners: [[x0, y1, z], [x1, y1, z], [x1, y0, z], [x0, y0, z]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        }
    } else {
        Quad {
            corners: [[x1, y1, z], [x0, y1, z], [x0, y0, z], [x1, y0, z]],
            uv: [[u1, v1], [u0, v1], [u0, v0], [u1, v0]],
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
    //
    // **Each strip is wound to point out of the edge it stands on**:
    // left at -X, right at +X, top at +Y, bottom at -Y. The top and
    // bottom pair once read the other way round -- listed from back to
    // front, which puts the geometric normal at -Y on the strip that
    // stands on the top edge -- and every dropped sprite was lit with
    // its top edge facing the ground and its bottom edge facing the
    // sky. Nothing vanished, because the item pipeline does not cull,
    // so the mistake could only ever be seen as shading, which is
    // exactly the kind that survives. `turning_an_item_a_quarter_turn_    // changes_which_way_its_faces_point` pins all four by emission
    // order.
    match side {
        Side::Left => Quad {
            corners: [[x0, y1, back], [x0, y1, front], [x0, y0, front], [x0, y0, back]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        },
        Side::Right => Quad {
            corners: [[x1, y1, front], [x1, y1, back], [x1, y0, back], [x1, y0, front]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        },
        Side::Top => Quad {
            corners: [[x0, y0, front], [x1, y0, front], [x1, y0, back], [x0, y0, back]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        },
        Side::Bottom => Quad {
            corners: [[x0, y1, back], [x1, y1, back], [x1, y1, front], [x0, y1, front]],
            uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
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
    fn every_quad_of_a_model_is_wound_to_face_out_of_the_shape() {
        // The winding is the only thing that says which way a rectangle
        // points -- `append_transformed` reads it and nothing else -- so
        // a strip wound inwards is lit as its own opposite. Because the
        // item pipeline does not cull, nothing disappears to give that
        // away; the top and bottom rims were upside down for exactly
        // that reason, and only the shading showed it.
        //
        // Checked against the mask the model was cut from rather than
        // against a table of expected faces: step half a texel out of a
        // rim along its own normal and you must leave the shape, step
        // half a texel in and you must still be inside it. That holds
        // for a hole in the middle of a sprite as well as for its
        // outline, which a "points away from the centre" test does not.
        let rows = [".##.", "####", "#..#", "..#."];
        let (solid, width, height) = mask(&rows);
        let m = ItemModel::from_mask(&solid, width, height);
        assert!(!m.quads.is_empty());
        let at = |x: f32, y: f32| {
            // Local space back to texels: x runs right, y runs *up*,
            // and image rows run down. See `to_local`.
            let tx = ((x + 0.5) * width as f32).floor();
            let ty = ((0.5 - y) * height as f32).floor();
            if tx < 0.0 || ty < 0.0 || tx >= width as f32 || ty >= height as f32 {
                return false; // off the picture counts as empty
            }
            solid[ty as usize * width + tx as usize]
        };
        for (index, quad) in m.quads.iter().enumerate() {
            let [a, b, c, d] = quad.corners.map(Vec3::from_array);
            let n = (b - a).cross(c - b).normalize();
            let middle = (a + b + c + d) * 0.25;
            if n.z.abs() > 0.5 {
                // A plate: the one at the front of the slab faces
                // front, the one at the back faces back.
                assert_eq!(
                    n.z > 0.0,
                    middle.z > 0.0,
                    "quad {index} is a plate at z {} wound towards {n}",
                    middle.z
                );
                continue;
            }
            // A rim, half a texel either side of the edge it stands on.
            let step = Vec3::new(n.x * 0.5 / width as f32, n.y * 0.5 / height as f32, 0.0);
            let inner = middle - step;
            let outer = middle + step;
            assert!(
                at(inner.x, inner.y),
                "quad {index} at {middle} is wound towards {n}, and behind it is empty"
            );
            assert!(
                !at(outer.x, outer.y),
                "quad {index} at {middle} is wound towards {n}, which is into the shape"
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
        // Read back out of the light word, which is where the shader
        // reads it: the plates are the first two quads emitted, and
        // untransformed they must come out +Z and -Z. A plate lit as
        // its own reverse is the sprite equivalent of a box drawn
        // inside out.
        let m = model(&["##", "##"]);
        let (mut v, mut i) = (Vec::new(), Vec::new());
        m.append(&mut v, &mut i, [0.0, 0.0, 0.0], 1.0, 0.0, 0, 15, 0);
        assert_eq!(faces_of(&v)[0..2], [4, 5], "the plates are not back to back");
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

    /// Face order is the mesher's: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
    const NORMALS: [Vec3; 6] = [
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
    ];

    /// The face index every emitted quad is carrying, read back out of
    /// the light word the way the shader reads it.
    fn faces_of(vertices: &[ItemVertex]) -> Vec<u8> {
        vertices
            .chunks_exact(4)
            .map(|quad| ((quad[0].packed >> 10) & 7) as u8)
            .collect()
    }

    #[test]
    fn every_face_of_a_turned_item_carries_the_direction_it_actually_points() {
        // **The bug this is here for.** The face index is what the
        // shader turns into a normal and a lambert term, and it used to
        // be written straight out of the model -- before the transform
        // that turns the thing. A dropped item spins, so every one of
        // them was lit by which side of the plate you were looking at
        // rather than by where it pointed: front plate a fixed 0.55 of
        // the sun, back plate a fixed 0.35, and no change at all as the
        // item came round. The same fault as the animal whose shading
        // was welded to its body.
        //
        // Checked against the *emitted geometry*, not against a table of
        // what the rotation ought to do -- the winding is what the GPU
        // reads, and arithmetic verified by its own arithmetic proves
        // nothing.
        let m = model(&[".##.", "####", "#..#", "..#."]);
        for eighth in 0..8 {
            let yaw = eighth as f32 * std::f32::consts::TAU / 8.0;
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            m.append(&mut vertices, &mut indices, [3.0, 4.0, 5.0], 0.4, yaw, 0, 15, 0);
            for quad in vertices.chunks_exact(4) {
                let corners: Vec<Vec3> =
                    quad.iter().map(|v| Vec3::from_array(v.position)).collect();
                let wound = (corners[1] - corners[0]).cross(corners[2] - corners[1]);
                let claimed = NORMALS[((quad[0].packed >> 10) & 7) as usize];
                assert!(
                    wound.normalize().dot(claimed) > 0.7,
                    "at yaw {yaw} a quad wound towards {wound} says it faces {claimed}"
                );
            }
        }
    }

    #[test]
    fn turning_an_item_a_quarter_turn_changes_which_way_its_faces_point() {
        // The other half of the same property, and the one a table of
        // model-space indices would still pass: the faces have to
        // actually *move*. An item whose shading never changes as it
        // spins is the plastic look the whole renderer is written
        // against, and it is invisible in a single frame.
        let m = model(&["##", "##"]);
        let build = |yaw: f32| {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            m.append(&mut v, &mut i, [0.0, 0.0, 0.0], 1.0, yaw, 0, 15, 0);
            faces_of(&v)
        };
        let straight = build(0.0);
        let quarter = build(std::f32::consts::FRAC_PI_2);
        assert_ne!(
            straight, quarter,
            "a quarter turn left every face pointing where it did"
        );
        // By emission order, which is exact: the two plates first, then
        // the left, right, top and bottom rims. The plates start out
        // facing ±Z and the side rims ±X; a quarter turn swaps those
        // two pairs over, and the top and bottom rims do not move at
        // all, because the spin is about Y.
        assert_eq!(straight, vec![4, 5, 3, 2, 0, 1]);
        assert_eq!(quarter[4..], [0, 1], "the spin moved the top or the bottom");
        assert!(
            matches!(quarter[0..2], [2, 3] | [3, 2]),
            "the plates ended up facing {:?} rather than along x",
            &quarter[0..2]
        );
        assert!(
            matches!(quarter[2..4], [4, 5] | [5, 4]),
            "the side rims ended up facing {:?} rather than along z",
            &quarter[2..4]
        );
    }

    #[test]
    fn the_foot_of_a_model_is_the_lowest_texel_the_artist_drew() {
        // What a caller needs to stand a sprite on the ground. A picture
        // drawn to the bottom edge of its tile and one drawn in the
        // middle of it are different objects to place, and the
        // difference is the margin -- not anything about the thing.
        let to_the_edge = model(&["....", "....", ".##.", ".##."]);
        let floating = model(&["....", ".##.", ".##.", "...."]);
        assert!((to_the_edge.foot() - -0.5).abs() < 1e-6, "{}", to_the_edge.foot());
        assert!((floating.foot() - -0.25).abs() < 1e-6, "{}", floating.foot());
        // ...and they are the same size, which is the other half of why
        // one scale for both is wrong.
        assert_eq!(to_the_edge.silhouette(), floating.silhouette());
    }

    #[test]
    fn the_vertex_is_the_size_the_pipeline_expects() {
        assert_eq!(std::mem::size_of::<ItemVertex>(), 24);
    }
}
