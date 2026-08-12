//! Chunk meshing: face culling, ambient occlusion, and baked light.
//!
//! Still naive per-block face culling rather than greedy meshing --
//! correct and simple, not maximally compact.
//!
//! Two things changed with the lighting rewrite:
//!
//! 1. **Meshing no longer computes light.** It reads from the persistent
//!    world-space `LightMap`. Previously every remesh rebuilt an
//!    18x64x18 padded volume and flood-filled it twice -- and since a
//!    remesh cascades to 8 neighbours, one block edit cost nine of those.
//!    Now a remesh is a pure read.
//! 2. **Blocks are read straight from the world** through a
//!    `BlockSource`, so face culling across chunk seams keeps working
//!    without copying anything.
//!
//! Vertices carry a packed light word: sky level, block level, ambient
//! occlusion, and which face they belong to. Sky and block light stay
//! separate so the shader can apply the day/night cycle to sky only --
//! a glowstone stays lit at midnight, and the sun moving costs zero
//! re-meshing.
//!
//! Ambient occlusion is the standard voxel trick: for each vertex, look
//! at the three cells touching that corner from outside; the more are
//! solid, the darker the vertex. It's what makes the base of a wall read
//! as a corner rather than as flat shading.

use bytemuck::{Pod, Zeroable};

use primitive_shared::lighting::{BlockSource, LightMap};
use primitive_shared::types::ChunkPos;
use primitive_shared::types::{
    is_cutout, is_liquid, is_opaque, is_translucent, BlockId, Chunk, BLOCK_AIR, CHUNK_SIZE_X,
    CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};

use crate::texture::FaceLayers;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub tex_layer: u32,
    /// Bit-packed to keep the vertex at 28 bytes: a chunk full of terrain
    /// is tens of thousands of vertices, and four separate f32 attributes
    /// would nearly double the buffer for information that only needs 13
    /// bits.
    ///
    /// ```text
    /// bits 0..3   sky light   0..15
    /// bits 4..7   block light 0..15
    /// bits 8..9   ambient occlusion 0..3
    /// bits 10..12 face index 0..5
    /// bit  13     translucent (drawn blended, see `TRANSLUCENT_BIT`)
    /// ```
    pub light: u32,
}

/// Set on vertices the fragment shader should give a see-through alpha.
/// Must match `TRANSLUCENT_BIT` in shader.wgsl.
pub const TRANSLUCENT_BIT: u32 = 1 << 13;

impl Vertex {
    pub const ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Uint32,
        3 => Uint32,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[inline]
pub fn pack_light(sky: u8, block: u8, ao: u8, face: u8) -> u32 {
    (sky.min(15) as u32)
        | ((block.min(15) as u32) << 4)
        | ((ao.min(3) as u32) << 8)
        | ((face.min(5) as u32) << 10)
}

pub struct Face {
    pub corners: [[f32; 3]; 4],
    /// Offset to the cell this face looks into.
    neighbor: [i32; 3],
    /// Index of the axis the normal runs along (0 = x, 1 = y, 2 = z).
    normal_axis: usize,
}

/// Texture coordinates for one corner of one face.
///
/// **This used to be a single shared `FACE_UVS` array applied to every
/// face.** Because each face lists its corners in its own winding order,
/// the same four UVs landed on different corners per face -- so side
/// textures came out rotated 90 degrees and some were mirrored. With a
/// uniform stone texture nobody notices; with a grass side texture the
/// green strip ends up running vertically.
///
/// The rule: `v = 0` is the top of the image, so on any vertical face
/// `v` must follow `1 - y`. `u` must increase to the right *as seen from
/// outside the block*, which is a different world axis for each face:
///
/// | face | outside observer looks along | their right | u    | v     |
/// |------|------------------------------|-------------|------|-------|
/// | +Y   | -Y (down)                    | +X          | x    | z     |
/// | -Y   | +Y (up)                      | +X          | x    | 1 - z |
/// | +X   | -X                           | -Z          | 1 - z| 1 - y |
/// | -X   | +X                           | +Z          | z    | 1 - y |
/// | +Z   | -Z                           | +X          | x    | 1 - y |
/// | -Z   | +Z                           | -X          | 1 - x| 1 - y |
#[inline]
pub fn face_uv(face_index: usize, corner: [f32; 3]) -> [f32; 2] {
    let [x, y, z] = corner;
    match face_index {
        0 => [x, z],             // +Y top
        1 => [x, 1.0 - z],       // -Y bottom
        2 => [1.0 - z, 1.0 - y], // +X east
        3 => [z, 1.0 - y],       // -X west
        4 => [x, 1.0 - y],       // +Z south
        _ => [1.0 - x, 1.0 - y], // -Z north
    }
}

/// Face order must match `FACE_NORMALS` in shader.wgsl.
pub fn faces() -> [Face; 6] {
    [
        // 0: +Y top
        Face {
            corners: [
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 0.0],
            ],
            neighbor: [0, 1, 0],
            normal_axis: 1,
        },
        // 1: -Y bottom
        Face {
            corners: [
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
            ],
            neighbor: [0, -1, 0],
            normal_axis: 1,
        },
        // 2: +X east
        Face {
            corners: [
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 1.0, 1.0],
                [1.0, 0.0, 1.0],
            ],
            neighbor: [1, 0, 0],
            normal_axis: 0,
        },
        // 3: -X west
        Face {
            corners: [
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 1.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
            neighbor: [-1, 0, 0],
            normal_axis: 0,
        },
        // 4: +Z south
        Face {
            corners: [
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
            neighbor: [0, 0, 1],
            normal_axis: 2,
        },
        // 5: -Z north
        Face {
            corners: [
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
            ],
            neighbor: [0, 0, -1],
            normal_axis: 2,
        },
    ]
}

/// Should we draw `current`'s face that looks into `neighbor`?
#[inline]
fn face_visible(current: BlockId, neighbor: BlockId) -> bool {
    if is_opaque(neighbor) {
        return false;
    }
    // Two adjacent cells of the same *blended* block -- water against
    // water -- would otherwise render an internal wall, visible as a
    // bright sheet every time you looked through a lake from inside it.
    //
    // This deliberately does **not** apply to cutout blocks. A leaf
    // block's face is mostly holes, so the face between two of them is
    // not hidden by anything: culling it made a tree one layer of leaves
    // thick, and you could see straight through the canopy into empty
    // space where the rest of it should have been. Leaves therefore draw
    // every face, exactly as they do in the game this borrows the trick
    // from.
    if neighbor == current && is_translucent(current) {
        return false;
    }
    true
}

/// Average light over the four cells meeting at one vertex of a face.
///
/// **Why not just use the face's own cell.** That's what this did
/// before, and it gives every corner of a quad the same value -- so a
/// glowstone lights a hard square of blocks with visible steps between
/// them, and a cave wall lit from one side changes brightness in whole
/// block units. Averaging the corner's four cells is the standard voxel
/// smooth-lighting trick: the value now varies across each quad, and
/// the GPU interpolates it into a gradient for free.
///
/// Opaque cells are skipped rather than counted as dark. Counting them
/// would bleed shadow around every corner -- the block behind a wall has
/// no business dimming the lit face in front of it. The AO term already
/// handles corner darkening, and it's computed from exactly these same
/// three neighbours, so the two agree by construction.
#[allow(clippy::too_many_arguments)]
#[inline]
fn corner_light(
    cache: &Neighbourhood,
    base: usize,
    y: i32,
    // The face's neighbour cell, as an offset from the meshed cell.
    n: [i32; 3],
    offset_a: [i32; 3],
    offset_b: [i32; 3],
    side1_opaque: bool,
    side2_opaque: bool,
    diagonal_opaque: bool,
) -> (u8, u8) {
    let mut sky_total = 0u32;
    let mut block_total = 0u32;
    let mut samples = 0u32;

    let mut take = |dx: i32, dy: i32, dz: i32| {
        let packed = cache.light_near(base, y, dx, dy, dz);
        sky_total += (packed & 0x0F) as u32;
        block_total += ((packed >> 4) & 0x0F) as u32;
        samples += 1;
    };

    // The face's own cell always counts -- it's the one we know is open.
    take(n[0], n[1], n[2]);
    if !side1_opaque {
        take(n[0] + offset_a[0], n[1] + offset_a[1], n[2] + offset_a[2]);
    }
    if !side2_opaque {
        take(n[0] + offset_b[0], n[1] + offset_b[1], n[2] + offset_b[2]);
    }
    // The diagonal is only visible from this corner if at least one of
    // the two edges beside it is open; otherwise it's tucked behind a
    // wall and sampling it would leak light around the corner.
    if !diagonal_opaque && !(side1_opaque && side2_opaque) {
        take(
            n[0] + offset_a[0] + offset_b[0],
            n[1] + offset_a[1] + offset_b[1],
            n[2] + offset_a[2] + offset_b[2],
        );
    }

    (
        (sky_total / samples) as u8,
        (block_total / samples) as u8,
    )
}

/// Classic voxel ambient occlusion: 0 = fully occluded corner (darkest),
/// 3 = open (brightest).
#[inline]
fn vertex_ao(side1: bool, side2: bool, corner: bool) -> u8 {
    if side1 && side2 {
        // Both edges blocked: the corner cell can't be seen at all, so
        // there's no point sampling it.
        return 0;
    }
    3 - (side1 as u8 + side2 as u8 + corner as u8)
}

/// Builds a mesh for one chunk, in world-space coordinates already offset
/// by the chunk's position (so the renderer can upload these vertices
/// as-is, no per-draw transform needed).
///
/// `blocks` supplies the world (including the neighbouring chunks, for
/// culling and AO across seams) and `light` the precomputed light.
/// Reusable scratch buffers. Meshing runs on a budget every frame, and
/// growing two fresh `Vec`s to tens of thousands of elements each time
/// is pure allocator churn -- the caller keeps one of these and clears
/// it instead.
/// One chunk's geometry, in two passes.
///
/// `indices` holds the opaque triangles first and the translucent ones
/// after, with `opaque_index_count` marking the boundary. One vertex
/// buffer and one index buffer per chunk, two draw calls into different
/// ranges of them -- rather than two buffers, which would double the
/// per-chunk allocations for the sake of a handful of water faces.
///
/// The split exists because blended geometry cannot be drawn in the
/// middle of the opaque pass: it has to come after everything behind it,
/// and it must not write depth, or the terrain under a lake stops being
/// drawn at all.
#[derive(Default)]
pub struct MeshBuffers {
    pub vertices: Vec<Vertex>,
    /// Three ranges, back to back: solid, then cutout, then translucent.
    /// One buffer rather than three, because a chunk is one allocation
    /// and the boundaries are two numbers.
    pub indices: Vec<u32>,
    /// Collected while meshing and appended at the end. Separate buffers
    /// only during the build.
    cutout: Vec<u32>,
    translucent: Vec<u32>,
    /// `indices[..solid_index_count]` is the solid pass,
    /// `[solid_index_count..cutout_end]` the cutout pass, and the rest
    /// the blended one.
    pub solid_index_count: u32,
    pub cutout_end: u32,
}

impl MeshBuffers {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.cutout.clear();
        self.translucent.clear();
        self.solid_index_count = 0;
        self.cutout_end = 0;
    }
}

/// A chunk plus one block of horizontal padding, with blocks and both
/// light channels copied in up front.
///
/// This exists purely for speed. Sampling the world through a
/// `BlockSource` means a chunk-position hash lookup per cell, and the
/// mesher samples roughly 500,000 cells per chunk (6 faces x 4 corners x
/// 3 AO neighbours, plus culling). Measured at ~20 ms per chunk, which
/// blew straight through the frame's meshing budget.
///
/// Filling this cache costs 18x18 = 324 lookups -- one per column, not
/// one per cell -- after which every sample is an array index.
pub struct Neighbourhood {
    blocks: Vec<BlockId>,
    /// Nibble-packed, same layout as `LightMap`: sky low, block high.
    light: Vec<u8>,
}

/// How far below a full block a liquid's surface sits.
const LIQUID_SURFACE_DROP: f32 = 0.12;

const PAD: i32 = 1;
const PADDED_X: usize = CHUNK_SIZE_X + 2;
const PADDED_Z: usize = CHUNK_SIZE_Z + 2;

#[inline]
fn padded_index(px: usize, y: usize, pz: usize) -> usize {
    (y * PADDED_Z + pz) * PADDED_X + px
}

/// How far one step along each axis moves through the padded arrays.
///
/// The mesher samples the neighbourhood a few hundred thousand times per
/// chunk, and doing it by recomputing `padded_index` from three
/// coordinates means a multiply, an add and four range comparisons every
/// time. Stepping by a constant offset from the current cell's index is
/// an add.
const STRIDE_X: isize = 1;
const STRIDE_Z: isize = PADDED_X as isize;
const STRIDE_Y: isize = (PADDED_X * PADDED_Z) as isize;

impl Neighbourhood {
    /// True if the chunk itself (ignoring the padding) has no blocks.
    ///
    /// Above the terrain most chunks are pure sky. Scanning 16k cells to
    /// discover that is much cheaper than the ~500k-cell face loop that
    /// would find nothing to emit.
    pub fn chunk_is_empty(&self) -> bool {
        for y in 0..CHUNK_SIZE_Y as i32 {
            for z in 0..CHUNK_SIZE_Z as i32 {
                for x in 0..CHUNK_SIZE_X as i32 {
                    if self.block(x, y, z) != BLOCK_AIR {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl Default for Neighbourhood {
    fn default() -> Self {
        Self {
            blocks: vec![BLOCK_AIR; PADDED_X * CHUNK_SIZE_Y * PADDED_Z],
            light: vec![0; PADDED_X * CHUNK_SIZE_Y * PADDED_Z],
        }
    }
}

impl Neighbourhood {
    pub fn fill<S: BlockSource>(&mut self, pos: primitive_shared::types::ChunkPos, blocks: &S, light: &LightMap) {
        let origin_x = pos.x * CHUNK_SIZE_X as i32;
        let origin_z = pos.z * CHUNK_SIZE_Z as i32;

        for pz in 0..PADDED_Z {
            for px in 0..PADDED_X {
                let gx = origin_x + px as i32 - PAD;
                let gz = origin_z + pz as i32 - PAD;
                let (cpos, lx, lz) =
                    primitive_shared::types::ChunkPos::from_global(gx, gz);

                // Both lookups happen once per column, not once per
                // cell. Calling `block_at` per cell here cost 20,736
                // chunk lookups per mesh -- about 10 ms of main-thread
                // time for data we could address directly.
                let light_data = light.chunk_data(cpos);
                let block_data = blocks.chunk_data(cpos);

                for y in 0..CHUNK_SIZE_Y {
                    let idx = padded_index(px, y, pz);
                    let cell = Chunk::index(lx, y, lz);
                    self.blocks[idx] = match block_data {
                        Some(data) => data[cell],
                        // No bulk access: either the chunk isn't loaded,
                        // or this `BlockSource` doesn't implement the
                        // optional fast path. Fall back to the per-cell
                        // query rather than silently reading air --
                        // getting that wrong makes whole chunks vanish.
                        None => blocks.block_at(gx, y as i32, gz).unwrap_or(BLOCK_AIR),
                    };
                    self.light[idx] = match light_data {
                        Some(data) => data[cell],
                        // Unlit (unloaded) neighbour: full sky, so the
                        // frontier reads slightly bright rather than as a
                        // wall of shadow.
                        None => 0x0F,
                    };
                }
            }
        }
    }

    /// Local chunk coordinates, extended by one in X/Z. Outside the
    /// world vertically: below is solid, above is open air.
    #[inline]
    fn block(&self, lx: i32, y: i32, lz: i32) -> BlockId {
        if y < 0 {
            return primitive_shared::types::BLOCK_STONE;
        }
        if y >= CHUNK_SIZE_Y as i32 || lx < -PAD || lz < -PAD
            || lx > CHUNK_SIZE_X as i32 || lz > CHUNK_SIZE_Z as i32
        {
            return BLOCK_AIR;
        }
        self.blocks[padded_index((lx + PAD) as usize, y as usize, (lz + PAD) as usize)]
    }

    /// The block one step away from `base`, which is the padded index
    /// of the cell being meshed.
    ///
    /// **Why this can skip the horizontal bounds checks.** Every sample
    /// the mesher takes is at most one cell away from the cell it is
    /// meshing, on each axis. The face's neighbour moves one step along
    /// the face normal; the ambient-occlusion corners move one step
    /// along each of the two *perpendicular* axes, so the total on any
    /// one axis is never more than one. With x and z running 0..15 and
    /// the cache padded by one, `x + dx` lands in -1..16 -- exactly the
    /// padded range, always. Only y can leave the world, because the
    /// world has a top and a bottom and no padding for them.
    ///
    /// The general `block` remains for the places where that argument
    /// does not hold.
    #[inline(always)]
    fn block_near(&self, base: usize, y: i32, dx: i32, dy: i32, dz: i32) -> BlockId {
        let ny = y + dy;
        if ny < 0 {
            return primitive_shared::types::BLOCK_STONE;
        }
        if ny >= CHUNK_SIZE_Y as i32 {
            return BLOCK_AIR;
        }
        let index = base as isize
            + dx as isize * STRIDE_X
            + dy as isize * STRIDE_Y
            + dz as isize * STRIDE_Z;
        self.blocks[index as usize]
    }

    /// Same stepping, for the light array. See `block_near`.
    #[inline(always)]
    fn light_near(&self, base: usize, y: i32, dx: i32, dy: i32, dz: i32) -> u8 {
        let ny = y + dy;
        if ny >= CHUNK_SIZE_Y as i32 {
            return 0x0F; // open sky
        }
        if ny < 0 {
            return 0;
        }
        let index = base as isize
            + dx as isize * STRIDE_X
            + dy as isize * STRIDE_Y
            + dz as isize * STRIDE_Z;
        self.light[index as usize]
    }

}

/// Builds the mesh for one chunk from an already-filled neighbourhood.
///
/// Takes no world or GPU handles -- only plain data -- so it can run on
/// a worker thread. The main thread fills the `Neighbourhood` (cheap:
/// 324 column lookups) and this does the expensive part off-thread.
pub fn build_mesh(
    pos: ChunkPos,
    cache: &Neighbourhood,
    layers: &FaceLayers,
    out: &mut MeshBuffers,
) {
    out.clear();
    if cache.chunk_is_empty() {
        return; // sky chunk: nothing to emit
    }
    // Destructured rather than accessed through `out` so the vertex list
    // and both index lists can be borrowed at once inside the face loop.
    let MeshBuffers {
        vertices,
        indices,
        cutout,
        translucent,
        solid_index_count,
        cutout_end,
    } = out;
    let textures = layers;

    let origin_x = pos.x * CHUNK_SIZE_X as i32;
    let origin_z = pos.z * CHUNK_SIZE_Z as i32;
    let face_defs = faces();

    for y in 0..CHUNK_SIZE_Y as i32 {
        for z in 0..CHUNK_SIZE_Z as i32 {
            // The padded index of (0, y, z), stepped by one per x rather
            // than recomputed from three coordinates per cell.
            let mut base = padded_index(PAD as usize, y as usize, (z + PAD) as usize);
            for x in 0..CHUNK_SIZE_X as i32 {
                let cell = base;
                base += STRIDE_X as usize;

                let id = cache.blocks[cell];
                if id == BLOCK_AIR {
                    continue;
                }
                let gx = origin_x + x;
                let gz = origin_z + z;

                for (face_index, face) in face_defs.iter().enumerate() {
                    let n = face.neighbor;
                    let neighbor_id = cache.block_near(cell, y, n[0], n[1], n[2]);
                    if !face_visible(id, neighbor_id) {
                        continue;
                    }

                    let layer = textures.layer_for_face(id, face_index);

                    // Light comes from the *air* cell in front of the
                    // face, never from the block itself (which is solid
                    // and therefore dark). It's averaged per vertex below
                    // rather than taken once per face -- see `corner_light`.

                    // The two axes perpendicular to this face; AO samples
                    // move along them from the neighbour cell.
                    let (axis_a, axis_b) = other_axes(face.normal_axis);

                    // Liquids render slightly below a full block, so
                    // the surface reads as a surface. With a full-height
                    // cube, a one-block-deep pool -- which is most of any
                    // shoreline -- looks exactly like solid ground at
                    // foot level, and standing in it looks like standing
                    // *on* it. Only the top of the column is lowered:
                    // water with water above it stays full height, or
                    // every layer of a deep lake would show a seam.
                    let surface_drop = if is_liquid(id)
                        && !is_liquid(cache.block_near(cell, y, 0, 1, 0))
                    {
                        LIQUID_SURFACE_DROP
                    } else {
                        0.0
                    };

                    let base = vertices.len() as u32;
                    let mut ao_values = [0u8; 4];
                    let translucent_flag = if is_translucent(id) {
                        TRANSLUCENT_BIT
                    } else {
                        0
                    };

                    for (corner_index, corner) in face.corners.iter().enumerate() {
                        let uv = face_uv(face_index, *corner);
                        let sign_a = if corner[axis_a] > 0.5 { 1 } else { -1 };
                        let sign_b = if corner[axis_b] > 0.5 { 1 } else { -1 };

                        let mut offset_a = [0i32; 3];
                        offset_a[axis_a] = sign_a;
                        let mut offset_b = [0i32; 3];
                        offset_b[axis_b] = sign_b;

                        let side1 = is_opaque(cache.block_near(
                            cell,
                            y,
                            n[0] + offset_a[0],
                            n[1] + offset_a[1],
                            n[2] + offset_a[2],
                        ));
                        let side2 = is_opaque(cache.block_near(
                            cell,
                            y,
                            n[0] + offset_b[0],
                            n[1] + offset_b[1],
                            n[2] + offset_b[2],
                        ));
                        let diagonal = is_opaque(cache.block_near(
                            cell,
                            y,
                            n[0] + offset_a[0] + offset_b[0],
                            n[1] + offset_a[1] + offset_b[1],
                            n[2] + offset_a[2] + offset_b[2],
                        ));
                        let ao = vertex_ao(side1, side2, diagonal);
                        ao_values[corner_index] = ao;

                        // Smooth lighting: average the four cells that
                        // touch this corner from the outside, skipping
                        // opaque ones (which are dark and would drag the
                        // average down through a wall).
                        let (sky, block_light) = corner_light(
                            cache, cell, y, n, offset_a, offset_b, side1, side2, diagonal,
                        );

                        vertices.push(Vertex {
                            position: [
                                gx as f32 + corner[0],
                                // The drop applies to the top corners
                                // only, so side faces taper to the
                                // lowered surface instead of shearing.
                                y as f32 + corner[1] - surface_drop * corner[1],
                                gz as f32 + corner[2],
                            ],
                            uv,
                            tex_layer: layer,
                            light: pack_light(sky, block_light, ao, face_index as u8)
                                | translucent_flag,
                        });
                    }

                    // Flip the quad's diagonal when the AO values are
                    // anisotropic. Without this the interpolation across
                    // the two triangles produces the classic diagonal
                    // seam artefact on shaded corners.
                    let target = if translucent_flag != 0 {
                        &mut *translucent
                    } else if is_cutout(id) {
                        &mut *cutout
                    } else {
                        &mut *indices
                    };
                    if ao_values[0] + ao_values[2] > ao_values[1] + ao_values[3] {
                        target.extend_from_slice(&[
                            base,
                            base + 1,
                            base + 2,
                            base,
                            base + 2,
                            base + 3,
                        ]);
                    } else {
                        target.extend_from_slice(&[
                            base + 1,
                            base + 2,
                            base + 3,
                            base + 1,
                            base + 3,
                            base,
                        ]);
                    }
                }
            }
        }
    }

    *solid_index_count = indices.len() as u32;
    indices.extend_from_slice(cutout);
    *cutout_end = indices.len() as u32;
    indices.extend_from_slice(translucent);
}

#[inline]
fn other_axes(normal_axis: usize) -> (usize, usize) {
    match normal_axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_STONE, BLOCK_WATER};

    #[test]
    fn light_packing_roundtrips() {
        let packed = pack_light(15, 9, 2, 5);
        assert_eq!(packed & 0xF, 15);
        assert_eq!((packed >> 4) & 0xF, 9);
        assert_eq!((packed >> 8) & 0x3, 2);
        assert_eq!((packed >> 10) & 0x7, 5);
    }

    #[test]
    fn packing_saturates_instead_of_corrupting_neighbouring_fields() {
        // A light level above 15 must not bleed into the block-light bits.
        let packed = pack_light(200, 0, 0, 0);
        assert_eq!(packed & 0xF, 15);
        assert_eq!((packed >> 4) & 0xF, 0);
    }

    #[test]
    fn ambient_occlusion_darkens_corners() {
        assert_eq!(vertex_ao(false, false, false), 3, "open corner is brightest");
        assert_eq!(vertex_ao(true, true, false), 0, "wedged corner is darkest");
        assert_eq!(vertex_ao(true, false, false), 2);
        assert_eq!(vertex_ao(true, false, true), 1);
    }

    #[test]
    fn hidden_faces_are_not_emitted() {
        assert!(!face_visible(BLOCK_STONE, BLOCK_STONE));
        assert!(face_visible(BLOCK_STONE, BLOCK_AIR));
        // Water surface against air: visible. Water against water: not.
        assert!(face_visible(BLOCK_WATER, BLOCK_AIR));
        assert!(!face_visible(BLOCK_WATER, BLOCK_WATER));
        // Stone against water: visible, so a lake bed still renders.
        assert!(face_visible(BLOCK_STONE, BLOCK_WATER));
    }

    #[test]
    fn leaves_do_not_cull_against_other_leaves() {
        // Regression: they used to, on the same rule water uses. With
        // the alpha cutout that made a canopy exactly one block deep --
        // you looked at a tree and saw straight through it, because
        // every leaf face behind the first had been culled away.
        use primitive_shared::types::BLOCK_LEAVES;
        assert!(face_visible(BLOCK_LEAVES, BLOCK_LEAVES));
        assert!(face_visible(BLOCK_LEAVES, BLOCK_AIR));
        // And a solid block next to leaves still hides its own face,
        // because the leaves' silhouette does not cover it.
        assert!(face_visible(BLOCK_STONE, BLOCK_LEAVES));
    }

    #[test]
    fn a_solid_leaf_cluster_draws_every_internal_face() {
        // The count is the point: a 3x3x3 block of leaves must emit the
        // faces inside it too, or the middle of a tree is empty.
        use primitive_shared::types::BLOCK_LEAVES;
        let mut faces = 0;
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            let _ = (dx, dy, dz);
            if face_visible(BLOCK_LEAVES, BLOCK_LEAVES) {
                faces += 1;
            }
        }
        assert_eq!(faces, 6, "a leaf block surrounded by leaves still draws");
    }
}

#[cfg(test)]
mod uv_and_light_tests {
    use super::*;

    #[test]
    fn every_vertical_face_has_the_texture_top_at_the_top() {
        // v = 0 is the top of the image. A corner at y = 1 (the top edge
        // of the block) must therefore map to v = 0 on all four sides.
        // Getting this wrong is what put the grass strip on its side.
        for face in [2usize, 3, 4, 5] {
            let top = face_uv(face, [0.0, 1.0, 0.0]);
            let bottom = face_uv(face, [0.0, 0.0, 0.0]);
            assert_eq!(top[1], 0.0, "face {face}: block top should be image top");
            assert_eq!(bottom[1], 1.0, "face {face}: block bottom should be image bottom");
        }
    }

    #[test]
    fn side_faces_are_not_rotated() {
        // The two bottom corners of a side face must share v, and differ
        // in u. Before the fix, u followed the vertical axis on the
        // east/west faces, which rotated those textures by 90 degrees.
        for (face, a, b) in [
            (2usize, [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]), // +X: varies in z
            (3usize, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]), // -X: varies in z
            (4usize, [0.0, 0.0, 1.0], [1.0, 0.0, 1.0]), // +Z: varies in x
            (5usize, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), // -Z: varies in x
        ] {
            let uv_a = face_uv(face, a);
            let uv_b = face_uv(face, b);
            assert_eq!(uv_a[1], uv_b[1], "face {face}: both corners are on the bottom edge");
            assert_ne!(uv_a[0], uv_b[0], "face {face}: u must run along the face");
        }
    }

    #[test]
    fn each_face_covers_the_whole_texture_exactly_once() {
        // All four corners must map to the four distinct corners of the
        // image -- no duplicates (degenerate mapping), nothing outside.
        let face_defs = faces();
        for (face_index, face) in face_defs.iter().enumerate() {
            let mut seen: Vec<[i32; 2]> = face
                .corners
                .iter()
                .map(|c| {
                    let uv = face_uv(face_index, *c);
                    assert!((0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]));
                    [uv[0] as i32, uv[1] as i32]
                })
                .collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), 4, "face {face_index} maps corners ambiguously");
        }
    }

    #[test]
    fn opposite_side_faces_read_the_same_way_round() {
        // Standing outside the block, the +Z and -Z faces should both
        // show the texture the right way round rather than one being a
        // mirror of the other. Their u axes therefore run opposite ways
        // in world space.
        let south_left = face_uv(4, [0.0, 0.0, 1.0])[0];
        let north_left = face_uv(5, [1.0, 0.0, 0.0])[0];
        assert_eq!(south_left, 0.0);
        assert_eq!(north_left, 0.0);
    }

    fn lit_cache(sky: u8, block: u8) -> Neighbourhood {
        let mut cache = Neighbourhood::default();
        for cell in cache.light.iter_mut() {
            *cell = (sky & 0x0F) | ((block & 0x0F) << 4);
        }
        cache
    }

    #[test]
    fn corner_light_averages_open_neighbours() {
        let cache = lit_cache(8, 4);
        let (sky, block) = corner_light(
            &cache,
            padded_index(5 + PAD as usize, 5, 5 + PAD as usize),
            5,
            [0, 0, 0],
            [1, 0, 0],
            [0, 0, 1],
            false,
            false,
            false,
        );
        assert_eq!((sky, block), (8, 4), "a uniformly lit area averages to itself");
    }

    #[test]
    fn corner_light_ignores_opaque_neighbours_instead_of_counting_them_as_dark() {
        // With one side walled off, the average must stay at the open
        // cells' level. Counting the wall as 0 would smear a shadow
        // along every wall base.
        let mut cache = lit_cache(12, 0);
        // Darken the cell we're about to mark opaque, to prove it's
        // skipped rather than averaged in.
        let idx = padded_index(7, 5, 6);
        cache.light[idx] = 0;
        let (sky, _) = corner_light(
            &cache,
            padded_index(5 + PAD as usize, 5, 5 + PAD as usize),
            5,
            [0, 0, 0],
            [1, 0, 0],
            [0, 0, 1],
            true, // side1 is opaque
            false,
            false,
        );
        assert_eq!(sky, 12, "opaque neighbours must not drag the average down");
    }

    #[test]
    fn corner_light_varies_across_a_gradient() {
        // The whole point of smooth lighting: two corners of the same
        // face sitting in different light must come out different.
        let mut cache = Neighbourhood::default();
        for pz in 0..PADDED_Z {
            for y in 0..CHUNK_SIZE_Y {
                for px in 0..PADDED_X {
                    // Brightness ramps along x.
                    let level = (px as u8).min(15);
                    cache.light[padded_index(px, y, pz)] = level;
                }
            }
        }
        let cell = |x: usize| padded_index(x + PAD as usize, 5, 5 + PAD as usize);
        let left = corner_light(
            &cache, cell(2), 5, [0, 0, 0], [1, 0, 0], [0, 0, 1], false, false, false,
        );
        let right = corner_light(
            &cache, cell(9), 5, [0, 0, 0], [1, 0, 0], [0, 0, 1], false, false, false,
        );
        assert!(
            left.0 < right.0,
            "smooth lighting should follow the gradient ({} vs {})",
            left.0,
            right.0
        );
    }
}

#[cfg(test)]
mod transparency_tests {
    use super::*;
    use crate::texture::FaceLayers;
    use primitive_shared::types::{
        BLOCK_LEAVES, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME,
    };

    /// Fills a neighbourhood directly, bypassing the world, so these
    /// tests exercise the geometry split rather than chunk loading.
    fn cache_of(fill: impl Fn(i32, i32, i32) -> BlockId) -> Neighbourhood {
        let mut cache = Neighbourhood::default();
        for pz in 0..PADDED_Z {
            for px in 0..PADDED_X {
                for y in 0..CHUNK_SIZE_Y {
                    cache.blocks[padded_index(px, y, pz)] = fill(
                        px as i32 - PAD,
                        y as i32,
                        pz as i32 - PAD,
                    );
                    cache.light[padded_index(px, y, pz)] = 0x0F;
                }
            }
        }
        cache
    }

    fn mesh_of(cache: &Neighbourhood) -> MeshBuffers {
        let mut out = MeshBuffers::default();
        build_mesh(ChunkPos::new(0, 0), cache, &FaceLayers::empty_for_test(), &mut out);
        out
    }

    #[test]
    fn water_faces_go_to_the_transparent_half_and_stone_to_the_opaque_one() {
        // A stone floor with a layer of water on it. The water's top
        // face must not be drawn in the opaque pass -- that is exactly
        // what made lakes look like solid slabs.
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }));

        assert!(out.solid_index_count > 0, "the stone should be solid");
        assert!(
            (out.indices.len() as u32) > out.cutout_end,
            "the water surface should have landed in the blended range"
        );
    }

    #[test]
    fn a_chunk_with_no_water_and_no_leaves_is_all_one_range() {
        let out = mesh_of(&cache_of(|_, y, _| if y < 4 { BLOCK_STONE } else { BLOCK_AIR }));
        assert!(out.solid_index_count > 0);
        assert_eq!(
            out.solid_index_count as usize,
            out.indices.len(),
            "solid terrain must not pay for the other two passes"
        );
        assert_eq!(out.cutout_end, out.solid_index_count);
    }

    #[test]
    fn leaves_go_to_the_cutout_range_and_nothing_else_does() {
        // They are a cutout, not a blend: they keep writing depth and
        // need no sorting. But their shader discards, and a shader that
        // can discard costs the GPU early depth rejection for every draw
        // using it -- so they are kept out of the solid range, where the
        // bulk of the triangles are.
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        }));
        assert!(out.solid_index_count > 0, "the stone should be solid");
        assert!(
            out.cutout_end > out.solid_index_count,
            "the leaves should have their own range"
        );
        assert_eq!(
            out.cutout_end as usize,
            out.indices.len(),
            "there is no water here, so nothing follows the cutout range"
        );
    }

    #[test]
    fn the_three_ranges_are_ordered_and_cover_every_index() {
        // The renderer draws `0..solid`, `solid..cutout_end` and
        // `cutout_end..len`. If those ever stopped being ordered and
        // contiguous, triangles would be drawn twice or not at all.
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_LEAVES,
            5 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }));
        assert!(out.solid_index_count <= out.cutout_end);
        assert!(out.cutout_end as usize <= out.indices.len());
        assert!(out.solid_index_count > 0);
        assert!(out.cutout_end > out.solid_index_count, "leaves missing");
        assert!(
            (out.indices.len() as u32) > out.cutout_end,
            "water missing"
        );
        // Every index is a real vertex, whichever range it is in.
        let vertices = out.vertices.len() as u32;
        assert!(out.indices.iter().all(|i| *i < vertices));
    }

    #[test]
    fn only_water_vertices_carry_the_translucent_flag() {
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }));
        let flagged = out
            .vertices
            .iter()
            .filter(|v| v.light & TRANSLUCENT_BIT != 0)
            .count();
        assert!(flagged > 0, "water should be flagged");
        assert!(flagged < out.vertices.len(), "stone should not be");
    }

    #[test]
    fn the_flag_does_not_corrupt_the_light_fields_it_sits_next_to() {
        // It shares a word with sky/block light, AO and the face index.
        let packed = pack_light(15, 15, 3, 5) | TRANSLUCENT_BIT;
        assert_eq!(packed & 0xF, 15);
        assert_eq!((packed >> 4) & 0xF, 15);
        assert_eq!((packed >> 8) & 0x3, 3);
        assert_eq!((packed >> 10) & 0x7, 5);
        assert_ne!(packed & TRANSLUCENT_BIT, 0);
    }

    #[test]
    fn every_index_addresses_a_real_vertex() {
        // The two index lists are built against one shared vertex buffer
        // and concatenated; an off-by-one there would be a GPU crash,
        // not a wrong pixel.
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }));
        let count = out.vertices.len() as u32;
        assert!(out.indices.iter().all(|&i| i < count));
        assert_eq!(out.indices.len() % 3, 0, "triangles come in threes");
        assert_eq!(out.solid_index_count % 3, 0, "each split must be on a triangle boundary");
        assert_eq!(out.cutout_end % 3, 0);
        let _ = CHUNK_VOLUME;
    }

    #[test]
    fn clearing_resets_the_split_too() {
        // The buffers are pooled and reused; a stale opaque count would
        // draw the previous chunk's water as this chunk's stone.
        let mut out = mesh_of(&cache_of(|_, y, _| if y == 4 { BLOCK_WATER } else { BLOCK_AIR }));
        assert!(out.indices.len() > 0);
        out.clear();
        assert_eq!(out.solid_index_count, 0);
        assert_eq!(out.cutout_end, 0);
        assert!(out.indices.is_empty() && out.vertices.is_empty());
    }
}

#[cfg(test)]
mod liquid_surface_tests {
    use super::*;
    use primitive_shared::types::{BLOCK_STONE, BLOCK_WATER};

    /// Builds a mesh for a chunk whose column at (8, *, 8) is water up to
    /// `water_top`, over stone.
    fn water_column_mesh(water_top: usize) -> Vec<Vertex> {
        use primitive_shared::types::CHUNK_VOLUME;

        struct World(Chunk);
        impl BlockSource for World {
            fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
                if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
                    return Some(BLOCK_AIR);
                }
                if !(0..16).contains(&gx) || !(0..16).contains(&gz) {
                    return None;
                }
                Some(self.0.get(gx as usize, gy as usize, gz as usize))
            }
        }

        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..10 {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        for y in 10..=water_top {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_WATER;
                }
            }
        }
        let chunk = Chunk {
            pos: ChunkPos::new(0, 0),
            blocks,
        };
        let world = World(chunk.clone());
        let mut light = LightMap::new();
        light.load_chunk(&world, chunk.pos);

        // A texture manager can't be built without a GPU, so exercise the
        // geometry through `Neighbourhood` + the same helper the mesher
        // uses, rather than the full build.
        let mut cache = Neighbourhood::default();
        cache.fill(chunk.pos, &world, &light);

        // Reproduce the mesher's vertex placement for the water column's
        // top face.
        let mut out = Vec::new();
        let face_defs = faces();
        for (face_index, face) in face_defs.iter().enumerate() {
            if face_index != 0 {
                continue; // top face only
            }
            let id = cache.block(8, water_top as i32, 8);
            let drop = if is_liquid(id) && !is_liquid(cache.block(8, water_top as i32 + 1, 8)) {
                LIQUID_SURFACE_DROP
            } else {
                0.0
            };
            for corner in face.corners.iter() {
                out.push(Vertex {
                    position: [
                        8.0 + corner[0],
                        water_top as f32 + corner[1] - drop * corner[1],
                        8.0 + corner[2],
                    ],
                    uv: face_uv(face_index, *corner),
                    tex_layer: 0,
                    light: 0,
                });
            }
        }
        out
    }

    #[test]
    fn the_water_surface_sits_below_a_full_block() {
        let verts = water_column_mesh(14);
        let top = verts
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MIN, f32::max);
        assert!(
            top < 15.0,
            "water surface should be below the full block height, got {top}"
        );
        assert!(
            top > 14.0,
            "water surface should still be in the top block, got {top}"
        );
    }

    #[test]
    fn a_solid_block_is_not_lowered() {
        // The drop must apply to liquids only -- shaving stone would put
        // a visible step under the player's feet everywhere.
        let mut cache = Neighbourhood::default();
        for cell in cache.blocks.iter_mut() {
            *cell = BLOCK_STONE;
        }
        let drop = if is_liquid(cache.block(0, 5, 0)) { LIQUID_SURFACE_DROP } else { 0.0 };
        assert_eq!(drop, 0.0);
    }

    #[test]
    fn submerged_water_keeps_full_height() {
        // Water with water above it must not be shortened, or a deep
        // lake would show a seam at every layer.
        let mut cache = Neighbourhood::default();
        for cell in cache.blocks.iter_mut() {
            *cell = BLOCK_WATER;
        }
        let id = cache.block(8, 5, 8);
        let drop = if is_liquid(id) && !is_liquid(cache.block(8, 6, 8)) {
            LIQUID_SURFACE_DROP
        } else {
            0.0
        };
        assert_eq!(drop, 0.0, "only the topmost liquid layer is lowered");
    }
}

/// A wall-clock measurement of the two things the client does most.
///
/// An ignored test rather than a `#[bench]` (nightly) or a criterion
/// dependency (a whole crate to measure code that already has a
/// millisecond budget in the frame loop). Run it explicitly:
///
/// ```text
/// cargo test --release -p primitive_client --bin primitive_client \
///     -- --ignored --nocapture bench
/// ```
///
/// Release matters: a debug build is ten to twenty times slower here and
/// the ratios between the stages shift, so debug numbers say nothing
/// about what a player experiences.
#[cfg(test)]
mod bench {
    use super::*;
    use primitive_shared::lighting::compute_isolated;
    use primitive_shared::types::{
        BLOCK_DIRT, BLOCK_GRASS, BLOCK_LEAVES, BLOCK_LOG, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME,
    };
    use std::time::Instant;

    /// A chunk that looks like somewhere you would actually stand.
    ///
    /// The shape is the whole point: a solid cube emits almost no faces
    /// and a checkerboard emits the maximum, and neither number means
    /// anything. This has a surface, a pond, caves and two trees, so it
    /// exercises face culling, the liquid pass and cutout leaves.
    fn terrain() -> Vec<BlockId> {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                let h = 20 + ((x * 7 + z * 13) % 9) + ((x + z) % 3);
                for y in 0..h {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
                blocks[Chunk::index(x, h - 1, z)] = BLOCK_DIRT;
                blocks[Chunk::index(x, h, z)] = BLOCK_GRASS;
                if x < 5 && z < 5 {
                    for y in h..=(h + 1) {
                        blocks[Chunk::index(x, y, z)] = BLOCK_WATER;
                    }
                }
                for y in 6..10 {
                    if (x * 3 + y * 5 + z * 7) % 11 < 4 {
                        blocks[Chunk::index(x, y, z)] = BLOCK_AIR;
                    }
                }
            }
        }
        for (tx, tz) in [(4usize, 11usize), (11usize, 4usize)] {
            for y in 30..35 {
                blocks[Chunk::index(tx, y, tz)] = BLOCK_LOG;
            }
            for dy in 0..3 {
                for dz in 0..3 {
                    for dx in 0..3 {
                        let (x, z) = (tx + dx, tz + dz);
                        if x >= CHUNK_SIZE_X || z >= CHUNK_SIZE_Z {
                            continue;
                        }
                        let y = 33 + dy;
                        if blocks[Chunk::index(x, y, z)] == BLOCK_AIR {
                            blocks[Chunk::index(x, y, z)] = BLOCK_LEAVES;
                        }
                    }
                }
            }
        }
        blocks
    }

    struct World(Vec<BlockId>);

    impl BlockSource for World {
        fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
                return Some(BLOCK_AIR);
            }
            let lx = gx.rem_euclid(CHUNK_SIZE_X as i32) as usize;
            let lz = gz.rem_euclid(CHUNK_SIZE_Z as i32) as usize;
            Some(self.0[Chunk::index(lx, gy as usize, lz)])
        }

        // Every chunk answers with the same terrain, so the mesher's
        // cross-chunk sampling reads real blocks instead of the
        // "unloaded" fallback -- which culls far more faces than a
        // loaded world does and would flatter the numbers.
        fn chunk_data(&self, _pos: ChunkPos) -> Option<&[BlockId]> {
            Some(&self.0)
        }
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn bench_meshing() {
        const ROUNDS: usize = 300;

        let blocks = terrain();
        let solid = blocks.iter().filter(|b| **b != BLOCK_AIR).count();
        let world = World(blocks.clone());
        let pos = ChunkPos::new(0, 0);
        let mut light = LightMap::new();
        light.load_chunk(&world, pos);

        let time = |rounds: usize, f: &mut dyn FnMut()| {
            let started = Instant::now();
            for _ in 0..rounds {
                f();
            }
            started.elapsed().as_secs_f64() * 1000.0 / rounds as f64
        };

        let per_light = time(ROUNDS, &mut || {
            std::hint::black_box(compute_isolated(std::hint::black_box(&blocks)));
        });

        let mut cache = Neighbourhood::default();
        let per_fill = time(ROUNDS, &mut || {
            cache.fill(pos, &world, &light);
            std::hint::black_box(&cache);
        });

        let layers = FaceLayers::empty_for_test();
        let mut out = MeshBuffers::default();
        let per_mesh = time(ROUNDS, &mut || {
            build_mesh(pos, &cache, &layers, &mut out);
            std::hint::black_box(&out);
        });

        let sky = World(vec![BLOCK_AIR; CHUNK_VOLUME]);
        let mut sky_light = LightMap::new();
        sky_light.load_chunk(&sky, pos);
        let mut sky_cache = Neighbourhood::default();
        sky_cache.fill(pos, &sky, &sky_light);
        let per_sky = time(ROUNDS, &mut || {
            build_mesh(pos, &sky_cache, &layers, &mut out);
            std::hint::black_box(&out);
        });

        build_mesh(pos, &cache, &layers, &mut out);
        println!();
        println!(
            "chunk: {solid} solid of {CHUNK_VOLUME} cells, {} vertices, {} indices",
            out.vertices.len(),
            out.indices.len()
        );
        println!("light  {per_light:7.3} ms/chunk   (worker)");
        println!("fill   {per_fill:7.3} ms/chunk   (MAIN THREAD)");
        println!("mesh   {per_mesh:7.3} ms/chunk   (worker)");
        println!("sky    {per_sky:7.3} ms/chunk   (all air)");
        println!("total  {:7.3} ms/chunk", per_light + per_fill + per_mesh);
        println!();
    }
}
