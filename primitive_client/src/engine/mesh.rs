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
    block_height, is_cross, is_cutout, is_flat, is_foliage, is_liquid, is_opaque, is_partial,
    is_translucent, BlockId, Chunk, BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};
use primitive_shared::worldgen::cooled_by_altitude;

use crate::engine::texture::FaceLayers;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    /// Everything that is not the position, in one word.
    ///
    /// ```text
    /// bits 0..3    sky light   0..15
    /// bits 4..7    block light 0..15
    /// bits 8..9    ambient occlusion 0..3
    /// bits 10..12  face index 0..5
    /// bit  13      translucent (drawn blended, see `TRANSLUCENT_BIT`)
    /// bit  14      texture u   (0 or 1)
    /// bit  15      texture v   (0 or 1)
    /// bits 16..23  texture layer
    /// bits 24..31  foliage tint, 0 = none (see `pack_tint`)
    /// ```
    ///
    /// **Why not three separate attributes.** A chunk of real terrain is
    /// around five thousand vertices, and a loaded world is a couple of
    /// hundred chunks: the vertex size is multiplied by something like
    /// a million. Carrying the UV as two f32s and the layer as its own
    /// u32 made the vertex 28 bytes for information that fits in four.
    /// At 16 bytes the same world costs 43% less GPU memory and 43% less
    /// upload bandwidth, which is the part that shows up on a weaker
    /// card or at a longer render distance.
    ///
    /// The UV only needs two bits because block faces are quads mapped
    /// corner-to-corner: `face_uv` returns nothing but zeroes and ones.
    pub packed: u32,
}

/// Set on vertices the fragment shader should give a see-through alpha.
/// Must match `TRANSLUCENT_BIT` in shader.wgsl.
pub const TRANSLUCENT_BIT: u32 = 1 << 13;

/// Stands for "this cell belongs to a chunk we have not loaded".
///
/// Distinct from air, and that distinction is the point: the mesher pads
/// each chunk by one cell so it can cull faces across the seam, and the
/// padding for a neighbour that has not arrived has to answer *something*.
/// Answering air makes the frontier grow a full skin of faces; answering
/// this makes it grow none, and the real faces appear when the neighbour
/// lands and the chunk is remeshed.
///
/// `u16::MAX` is not a real block id, so `is_opaque` reports it opaque --
/// which is also what ambient occlusion and light sampling want, since
/// neither should reach into territory we cannot see.
pub const UNKNOWN_BLOCK: BlockId = BlockId::MAX;

/// Where the texture layer starts in `Vertex::packed`.
const LAYER_SHIFT: u32 = 16;
/// How many layers the vertex can address.
///
/// Eight bits rather than the sixteen this used to spend. The array
/// holds one layer per glyph (95), one per distinct block image, and
/// five for the breaking overlay -- around 120 in a stock install, and
/// only the block ones can ever reach a terrain vertex. Halving the
/// field freed the top byte for the foliage tint without growing the
/// vertex, which was the whole point of packing it in the first place.
///
/// `TextureManager::load` refuses to start with more layers than this,
/// so the truncation cannot happen silently.
pub const MAX_TEXTURE_LAYERS: u32 = 256;
/// Where the foliage tint sits. See `pack_tint`.
const TINT_SHIFT: u32 = 24;
/// Where the two UV bits sit.
const UV_SHIFT: u32 = 14;
/// Everything below this is the light word.
const LIGHT_MASK: u32 = (1 << UV_SHIFT) - 1;

/// Steps per climate axis in a packed tint. See `pack_tint`.
const TINT_LEVELS: u32 = 15;

/// Packs a climate into the byte the shader turns into a colour.
///
/// Zero means "not foliage, do not tint", which is why the two axes get
/// fifteen steps each rather than sixteen: `1 + t*15 + h` runs 1..=225,
/// leaving the code that means *untinted* outside the range instead of
/// spending a whole bit on a flag the vertex had no room for.
///
/// Fifteen steps is coarser than the eye can resolve here. Neighbouring
/// columns nearly always land in the same bucket, and one step is under
/// 7% of a palette that spans straw to swamp-green -- so the terrain
/// shades continuously and a boundary between two buckets is a change no
/// larger than the dithering already in the textures.
#[inline]
pub fn pack_tint(temperature: f32, humidity: f32) -> u32 {
    let quantise = |v: f32| (v.clamp(0.0, 1.0) * (TINT_LEVELS - 1) as f32).round() as u32;
    1 + quantise(temperature) * TINT_LEVELS + quantise(humidity)
}

impl Vertex {
    pub const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Uint32,
    ];

    /// Builds a vertex from the four things the mesher actually knows.
    ///
    /// `uv` components must be 0.0 or 1.0; anything else is rounded,
    /// because there is nowhere to put it. That is a real constraint on
    /// this vertex format rather than an oversight -- see `packed`.
    pub fn new(position: [f32; 3], uv: [f32; 2], tex_layer: u32, light: u32) -> Self {
        Self::tinted(position, uv, tex_layer, light, 0)
    }

    /// The same, with a foliage tint from `pack_tint`.
    pub fn tinted(
        position: [f32; 3],
        uv: [f32; 2],
        tex_layer: u32,
        light: u32,
        tint: u32,
    ) -> Self {
        let u = (uv[0] >= 0.5) as u32;
        let v = (uv[1] >= 0.5) as u32;
        Self {
            position,
            packed: (light & LIGHT_MASK)
                | (u << UV_SHIFT)
                | (v << (UV_SHIFT + 1))
                | ((tex_layer & 0xFF) << LAYER_SHIFT)
                | ((tint & 0xFF) << TINT_SHIFT),
        }
    }

    /// The decoders, mirroring what the shader does. Used by the tests
    /// that check the packing round-trips -- a silent mismatch here
    /// shows up as the whole world wearing the wrong textures.
    #[allow(dead_code)]
    pub fn uv(&self) -> [f32; 2] {
        [
            ((self.packed >> UV_SHIFT) & 1) as f32,
            ((self.packed >> (UV_SHIFT + 1)) & 1) as f32,
        ]
    }

    #[allow(dead_code)]
    pub fn tex_layer(&self) -> u32 {
        (self.packed >> LAYER_SHIFT) & 0xFF
    }

    #[allow(dead_code)]
    pub fn tint(&self) -> u32 {
        self.packed >> TINT_SHIFT
    }

    #[allow(dead_code)]
    pub fn light(&self) -> u32 {
        self.packed & LIGHT_MASK
    }

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

/// Turns a face's texture a quarter turn at a time.
///
/// **Why this is free.** A block face is mapped corner to corner, so the
/// four texture coordinates on it are the four corners of the unit
/// square -- and a quarter turn maps that set of four to itself. The
/// rotated coordinate is therefore still nothing but zeroes and ones,
/// which is exactly what the two bits in the vertex can hold. Nothing
/// grows, no second texture is needed, and the mesher was already
/// writing the corners out one at a time: *which* corner gets which
/// coordinate is a choice rather than a computation.
///
/// **Why bother.** A 16x16 texture stamped across a hillside is a grid,
/// and the eye picks a grid out from much further away than it picks out
/// any single texture -- so a cliff face reads as wallpaper long before
/// you can see what the wallpaper is of. Turning each face by a hash of
/// where it is breaks the repeat without touching the art.
///
/// Only for faces whose texture has no up: see `types::texture_turns`,
/// which is where that decision lives, because it is a property of the
/// block rather than of the mesher.
#[inline]
pub fn turned_uv(uv: [f32; 2], turn: u32) -> [f32; 2] {
    let [u, v] = uv;
    match turn & 3 {
        1 => [v, 1.0 - u],
        2 => [1.0 - u, 1.0 - v],
        3 => [1.0 - v, u],
        _ => uv,
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

/// How much of the wall between two cells the neighbour covers, as a
/// fraction of a full block face.
///
/// A whole opaque block covers all of it. A layer covers as much of a
/// side as it is deep, all of the face it stands *on*, and none of the
/// face above it -- it rests on its own cell floor, so there is nothing
/// of it against the ceiling. Everything see-through covers nothing,
/// which is the answer that keeps water, leaves and plants behaving
/// exactly as they did.
///
/// Takes the neighbour's *cover* rather than its id: everything this
/// needs to know is in that one cached byte, and the whole point of
/// caching it is that this runs nine times per face. See `cover_of`.
#[inline]
fn hidden_by(neighbor_cover: u8, face_index: usize) -> u8 {
    if neighbor_cover == FULL_COVER || neighbor_cover == 0 {
        return neighbor_cover;
    }
    match face_index {
        0 => FULL_COVER, // a layer stands on us: our top is under it
        1 => 0,          // it is above its own floor, not against ours
        _ => neighbor_cover,
    }
}

/// Should we draw `current`'s face that looks into `neighbor`?
///
/// `face_index` matters only for cutout blocks -- see the leaf case
/// below, where it decides *which* of two neighbours emits the face they
/// share.
#[inline]
fn face_visible(
    current: BlockId,
    current_cover: u8,
    neighbor: BlockId,
    neighbor_cover: u8,
    face_index: usize,
) -> bool {
    // A neighbour we have not loaded is *unknown*, not air, and the two
    // want opposite answers depending on what is asking.
    //
    // An opaque block may as well draw: the face is hidden the moment
    // the neighbour arrives, and until then a solid wall is how the edge
    // of the loaded world is supposed to read.
    //
    // A see-through block must not. Water is blended and writes no
    // depth, so a face invented along the frontier shows *through* the
    // terrain in front of it as a bright sheet, and it survives until
    // that chunk is remeshed. Since the frontier moves with the player
    // -- and a chunk can also be remeshed by an edit while a neighbour
    // is evicted -- the sheets come and go, which is exactly the
    // "water chunks sometimes render wrong" symptom.
    if neighbor == UNKNOWN_BLOCK {
        return is_opaque(current);
    }

    // A coating lying on us *is* our top surface now.
    //
    // Ash covers its cell corner to corner and its texture has no holes
    // in it, so the face under it cannot be seen -- and drawing it
    // anyway is what forced the coating to be lifted a fiftieth of a
    // block clear of the ground to stop the two z-fighting. That lift
    // is the gap you could see under the ash along the edge of a bank,
    // and, since the quad is drawn from both sides, the grey sheet
    // floating over the ground when you looked up at one.
    //
    // Skipping the face costs nothing and removes the reason for the
    // lift: with one quad instead of two there is nothing left to
    // fight, so the coating sits on the floor of its cell exactly where
    // the surface it replaces used to be. See `types::flat_lift`.
    //
    // This is the same rule `hidden_by` already applies to a *layer*
    // standing on a block -- a drift of snow hides the top of what it
    // is banked against. A coating is that with a depth of zero, and it
    // was falling through the gap between "a layer" and "something you
    // can see through".
    if face_index == 0 && primitive_shared::types::is_covering_flat(neighbor) {
        return false;
    }

    // How much of the shared wall the neighbour actually hides, against
    // how much of it this block has to show.
    //
    // Both numbers used to be one, because every solid block filled its
    // cell, so the whole question was "is the neighbour opaque". Loose
    // material fills its cell in eighths, and that turns one question
    // into three:
    //
    // * the **top** of a layer is inside its own cell, so nothing in
    //   the cell above can cover it -- a block placed over a drift of
    //   snow leaves the gap you would expect to see, and culling that
    //   face opens a hole into the drift;
    // * the **sides** of a layer are only as tall as the layer, so a
    //   neighbour that is at least as deep hides them completely;
    // * a layer hides the **top face of whatever it stands on**
    //   entirely, because its footprint is the whole cell however
    //   shallow it is.
    //
    // Getting the second one wrong is what would put two exactly
    // coplanar quads between two equal drifts -- the z-fighting that
    // leaves a flickering seam across a snowfield.
    if face_index == 0 && current_cover != FULL_COVER && current_cover != 0 {
        return true; // a layer's top is inside its own cell
    }
    let needed = if face_index <= 1 || current_cover == 0 {
        FULL_COVER
    } else {
        current_cover
    };
    if hidden_by(neighbor_cover, face_index) >= needed {
        return false;
    }

    // Water against water. A lake is a volume, and an internal wall
    // inside it shows as a bright sheet the moment you swim under the
    // surface -- so the faces two cells of water share are culled.
    //
    // **Two cells of one lake need not carry the same id** now that
    // water flows: the level rides in the variant field. So this asks
    // whether both are liquid rather than whether they are equal, and
    // it has to come before the "different blocks always draw" rule
    // below.
    //
    // **No exceptions, and that is the point.** There was one: a deep
    // cell beside a shallow one stood above it, so the band between the
    // two surfaces was a real wall and the deeper of the pair drew it.
    // Then every cell that filled after a player broke a block put a
    // wall across the sea until it finished filling, and any cell that
    // finished short of full kept one for ever.
    //
    // Water is drawn at one height everywhere now -- see
    // `fluid::surface_height` -- so between two cells of it there is
    // nothing to draw, on any face, ever.
    if is_liquid(current) && is_liquid(neighbor) {
        return false;
    }

    if neighbor != current {
        return true;
    }

    if is_translucent(current) {
        return false;
    }

    // Leaves against leaves is the awkward one, and it has been got
    // wrong in both directions.
    //
    // Drawing it from both sides -- the original -- puts two exactly
    // coplanar depth-writing quads in the same place. They z-fight, and
    // because each carries its own face index (and so its own lambert
    // term) the canopy shimmers between two brightnesses as the camera
    // moves. That is the reported distortion.
    //
    // Culling it from both sides fixes the shimmer and empties the
    // canopy out: a tree becomes a hollow shell, and through the gaps in
    // the leaf texture you see daylight where the inside of the tree
    // should be.
    //
    // So: draw it exactly once. Each block emits only its three
    // positive-facing sides against an identical neighbour, so of any
    // adjacent pair the lower one draws the shared face and the upper
    // one skips it. One quad, no duplicate to fight with, and the
    // interior of the canopy keeps its geometry. The cutout pass runs
    // with `cull_mode: None`, so that single quad is visible from both
    // sides.
    if is_cutout(current) {
        return face_index.is_multiple_of(2); // +Y, +X, +Z -- see `faces()`
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
/// Reads out of the 3x3 ring the face loop gathers, rather than sampling
/// the world: `ia`/`ib` say which corner of the ring this vertex is, and
/// the centre `[1][1]` is the face's own neighbour cell.
#[inline]
fn corner_light(
    ring: &[[u8; 3]; 3],
    ia: usize,
    ib: usize,
    side1_opaque: bool,
    side2_opaque: bool,
    diagonal_opaque: bool,
) -> (u8, u8) {
    let mut sky_total = 0u32;
    let mut block_total = 0u32;
    let mut samples = 0u32;

    let mut take = |packed: u8| {
        sky_total += (packed & 0x0F) as u32;
        block_total += ((packed >> 4) & 0x0F) as u32;
        samples += 1;
    };

    // The face's own cell always counts -- it's the one we know is open.
    take(ring[1][1]);
    if !side1_opaque {
        take(ring[ia][1]);
    }
    if !side2_opaque {
        take(ring[1][ib]);
    }
    // The diagonal is only visible from this corner if at least one of
    // the two edges beside it is open; otherwise it's tucked behind a
    // wall and sampling it would leak light around the corner.
    if !diagonal_opaque && !(side1_opaque && side2_opaque) {
        take(ring[ia][ib]);
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
    /// Four ranges, back to back: solid, leaves, sprites, translucent.
    /// One buffer rather than four, because a chunk is one allocation
    /// and the boundaries are three numbers.
    pub indices: Vec<u32>,
    /// Collected while meshing and appended at the end. Separate buffers
    /// only during the build.
    leaves: Vec<u32>,
    sprites: Vec<u32>,
    translucent: Vec<u32>,
    /// `indices[..solid_index_count]` is the solid pass,
    /// `[solid_index_count..leaf_end]` the leaves, `[leaf_end..
    /// sprite_end]` the sprites, and the rest the blended one.
    pub solid_index_count: u32,
    pub leaf_end: u32,
    pub sprite_end: u32,
}

impl MeshBuffers {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.leaves.clear();
        self.sprites.clear();
        self.translucent.clear();
        self.solid_index_count = 0;
        self.leaf_end = 0;
        self.sprite_end = 0;
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
    /// One past the highest non-air cell of the chunk itself. See
    /// `ceiling`.
    ceiling: i32,
}

/// A whole cell's worth of cover: this block hides anything behind it.
const FULL_COVER: u8 = 8;


/// How much of its cell a block hides, in eighths, from a table built
/// once for every id there is.
///
/// **Why this is a cached byte rather than a question asked in the
/// loop.** Face culling and ambient occlusion ask about the *nine* cells
/// around each face, six faces per block, and each of those asks used to
/// run `is_opaque` -- which strips the variant field and tests five
/// predicates. That is around fifty predicate evaluations per visible
/// block, on the hottest loop in the client, for an answer that depends
/// on nothing but the block id. Computing it once per cell while `fill`
/// is already walking every cell turns all of them into an array read.
///
/// The encoding does double duty, which is why it is a depth rather than
/// a flag: `FULL_COVER` is exactly the old "opaque", anything between is
/// a layer and says how deep it is, and zero is everything you can see
/// through. That is the whole of what `face_visible` needs to know about
/// a neighbour.
///
/// **A table rather than a third array in the neighbourhood.** It was an
/// array for one version, filled beside the blocks and the light -- and
/// filling it cost 16,384 more writes per chunk *on the main thread*,
/// which measured as the fill stage doubling from 0.042 to 0.091 ms.
/// That is the one stage a player feels directly: it happens in the
/// frame loop while terrain streams in. A block id is sixteen bits, so
/// every answer this function can ever give fits in 64 KB -- computed
/// once, read as an index, and nothing per cell at all.
///
/// The single-block form. The mesher's own loops take the table once and
/// index it directly -- see `cover_table` -- so this is for everything
/// that asks about one block: `face_visible`'s tests, mostly.
#[inline]
#[cfg_attr(not(test), allow(dead_code))]
fn cover_of(id: BlockId) -> u8 {
    cover_table()[id as usize]
}

/// The table itself, so a loop that reads it half a million times can
/// take the reference **once**.
///
/// `OnceLock` costs an atomic load per access -- nothing on its own, and
/// 0.08 ms per chunk when the access is nine per face on every face of
/// every block. Hoisting it out of the loop is free and gives that back.
#[inline]
fn cover_table() -> &'static [u8; 1 << 16] {
    static TABLE: std::sync::OnceLock<Box<[u8; 1 << 16]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = Box::new([0u8; 1 << 16]);
        for (id, slot) in table.iter_mut().enumerate() {
            let id = id as BlockId;
            *slot = if is_opaque(id) {
                FULL_COVER
            } else if is_partial(id) {
                primitive_shared::types::block_layers(id)
            } else {
                0
            };
        }
        table
    })
}

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
    /// One past the highest cell in the chunk that holds anything.
    ///
    /// The face loop has no business walking the sky. A chunk is the
    /// full 64-block height of the world but terrain tops out well below
    /// that, and every cell above it costs a read and a comparison for a
    /// `continue`.
    ///
    /// This replaces an "is the whole chunk empty" scan that could never
    /// succeed: chunks span the entire world height and every one of
    /// them has bedrock in it, so the scan walked 16,384 cells to return
    /// `false` every single time. Tracking the ceiling during `fill` --
    /// which is already touching every cell -- costs nothing and bounds
    /// the expensive loop instead of merely failing to skip it.
    pub fn ceiling(&self) -> i32 {
        self.ceiling
    }

    /// Recomputes the ceiling from the block array.
    ///
    /// `fill` maintains it as it goes, so the only caller is a test that
    /// writes the blocks directly. It exists so that "what the ceiling
    /// means" has one definition: a test that set the field by hand
    /// would silently disagree with `fill` the day the rule changes, and
    /// the symptom -- `build_mesh` returning nothing -- looks like a
    /// meshing bug rather than a stale fixture.
    #[cfg(test)]
    pub fn recompute_ceiling(&mut self) {
        self.ceiling = 0;
        for y in 0..CHUNK_SIZE_Y as i32 {
            for z in 0..CHUNK_SIZE_Z as i32 {
                for x in 0..CHUNK_SIZE_X as i32 {
                    if self.block(x, y, z) != BLOCK_AIR {
                        self.ceiling = y + 1;
                    }
                }
            }
        }
    }
}

impl Default for Neighbourhood {
    fn default() -> Self {
        Self {
            blocks: vec![BLOCK_AIR; PADDED_X * CHUNK_SIZE_Y * PADDED_Z],
            light: vec![0; PADDED_X * CHUNK_SIZE_Y * PADDED_Z],
            ceiling: 0,
        }
    }
}

impl Neighbourhood {
    pub fn fill<S: BlockSource>(&mut self, pos: primitive_shared::types::ChunkPos, blocks: &S, light: &LightMap) {
        let origin_x = pos.x * CHUNK_SIZE_X as i32;
        let origin_z = pos.z * CHUNK_SIZE_Z as i32;
        self.ceiling = 0;

        // The chunk the last column came from, and its two arrays.
        //
        // 324 columns fall in nine chunks, and the inner loop walks them
        // in runs -- sixteen columns of the middle chunk between the two
        // single-column edges. Asking the map afresh for each of them is
        // 648 hash lookups per fill for nine distinct answers, and this
        // is the one meshing stage that runs on the main thread while
        // terrain streams in. Remembering the last one turns almost all
        // of them into a comparison of two integers.
        struct Memo<'a> {
            pos: primitive_shared::types::ChunkPos,
            blocks: Option<&'a [BlockId]>,
            light: Option<&'a [u8]>,
        }
        let mut memo: Option<Memo> = None;

        for pz in 0..PADDED_Z {
            for px in 0..PADDED_X {
                // Only the chunk's own columns raise the ceiling. A tall
                // neighbour is still sampled for culling and AO, but it
                // cannot make us mesh sky we do not own.
                let is_ours = px >= PAD as usize
                    && pz >= PAD as usize
                    && px < PADDED_X - PAD as usize
                    && pz < PADDED_Z - PAD as usize;
                let gx = origin_x + px as i32 - PAD;
                let gz = origin_z + pz as i32 - PAD;
                let (cpos, lx, lz) =
                    primitive_shared::types::ChunkPos::from_global(gx, gz);

                // Both lookups happen once per *run of columns in one
                // chunk*, not once per cell. Calling `block_at` per cell
                // here cost 20,736 chunk lookups per mesh -- about 10 ms
                // of main-thread time for data we could address
                // directly.
                let (block_data, light_data) = match memo {
                    Some(Memo { pos, blocks, light }) if pos == cpos => (blocks, light),
                    _ => {
                        let found = Memo {
                            pos: cpos,
                            blocks: blocks.chunk_data(cpos),
                            light: light.chunk_data(cpos),
                        };
                        let pair = (found.blocks, found.light);
                        memo = Some(found);
                        pair
                    }
                };

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
                        // A cell nobody can answer for is *unknown*, not
                        // air: see `UNKNOWN_BLOCK`.
                        None => blocks
                            .block_at(gx, y as i32, gz)
                            .unwrap_or(UNKNOWN_BLOCK),
                    };
                    if is_ours && self.blocks[idx] != BLOCK_AIR {
                        self.ceiling = self.ceiling.max(y as i32 + 1);
                    }
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
    ///
    /// The face loop uses `block_near`, which is the same lookup without
    /// the bounds checks (see its note on why it can skip them), so this
    /// general form is left for the callers that cannot make that
    /// argument -- currently only `recompute_ceiling`.
    #[allow(dead_code)]
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
    world: &primitive_shared::worldgen::WorldGen,
    out: &mut MeshBuffers,
) {
    out.clear();
    let ceiling = cache.ceiling();
    if ceiling == 0 {
        return; // sky chunk: nothing to emit
    }
    // Destructured rather than accessed through `out` so the vertex list
    // and both index lists can be borrowed at once inside the face loop.
    let MeshBuffers {
        vertices,
        indices,
        leaves,
        sprites,
        translucent,
        solid_index_count,
        leaf_end,
        sprite_end,
    } = out;
    let textures = layers;

    let origin_x = pos.x * CHUNK_SIZE_X as i32;
    let origin_z = pos.z * CHUNK_SIZE_Z as i32;
    let face_defs = faces();
    // Taken once for the whole chunk rather than per sample: see
    // `cover_table`.
    let cover_table = cover_table();

    // Climate per column of the chunk, sampled on first use and reused
    // by every leaf and blade above it.
    //
    // Lazily rather than up front: four noise samples times 256 columns
    // is real time to spend on a chunk of bare stone, and most chunks
    // are bare stone. NaN is the "not yet" marker -- no real climate can
    // be one, and it costs no second array to say so.
    let mut climate = [[f32::NAN; 2]; CHUNK_SIZE_X * CHUNK_SIZE_Z];

    for y in 0..ceiling {
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
                let cover = cover_table[id as usize];
                let gx = origin_x + x;
                let gz = origin_z + z;

                // What climate this block grew in, for anything alive.
                // Zero -- "no tint" -- for everything else, which is
                // most of the world.
                let tint = if is_foliage(id) {
                    let column = z as usize * CHUNK_SIZE_X + x as usize;
                    if climate[column][0].is_nan() {
                        let (temperature, humidity) = world.climate_column(gx, gz);
                        climate[column] = [temperature, humidity];
                    }
                    let [temperature, humidity] = climate[column];
                    pack_tint(cooled_by_altitude(temperature, y), humidity)
                } else {
                    0
                };

                // Plants are not cubes. Two quads on the cell's
                // diagonals, no face culling to do (there are no faces
                // to hide) and no ambient occlusion (nothing to occlude
                // against) -- so they leave the cube loop entirely.
                // A stone lying on the ground: one quad, flat, and
                // that is the whole model. Cheapest thing the mesher
                // emits, which is what lets it appear in every biome.
                if is_flat(id) {
                    flat_block(
                        [gx as f32, y as f32, gz as f32],
                        id,
                        textures.layer_for_face(id, 0),
                        cache.light_near(cell, y, 0, 0, 0),
                        vertices,
                        sprites,
                    );
                    continue;
                }

                if is_cross(id) {
                    cross_block(
                        [gx as f32, y as f32, gz as f32],
                        textures.layer_for_face(id, 0),
                        cache.light_near(cell, y, 0, 0, 0),
                        tint,
                        vertices,
                        sprites,
                    );
                    continue;
                }

                // Everything about this block that does not depend on
                // which of its faces is being drawn, worked out once.
                //
                // All three used to be inside the face loop, which is to
                // say six times per block over the whole chunk, for
                // answers that are a function of the id alone. Cheap
                // each -- but the face loop is the hottest code in the
                // client and there is nothing else in it that is not
                // per-face.
                //
                // **How tall this block is drawn**, as a fraction of its
                // cell. Two reasons for it to be less than one, and they
                // never apply at once:
                //
                // *Liquids* render slightly below a full block, so the
                // surface reads as a surface. With a full-height cube, a
                // one-block-deep pool -- which is most of any shoreline
                // -- looks exactly like solid ground at foot level, and
                // standing in it looks like standing *on* it. Only the
                // top of the column is lowered: water with water above
                // it is drawn to the top of its cell whatever its own
                // level says, or every layer of a deep lake would show a
                // seam and a half-full cell under a full one would open
                // a slot through the middle of a waterfall. That last
                // rule used to live here, in a two-line `if` this file
                // owned, and nothing outside the mesher knew it -- so
                // the drowning check and the anti-cheat both put the top
                // of a submerged cell `SURFACE_DROP` lower than the
                // mesher drew it. It is now
                // `fluid::surface_height_with_above`, in the module that
                // exists so the three of them cannot disagree.
                //
                // *Loose material* is drawn at exactly the depth it is,
                // which is the whole point of layers: what you see is
                // what you walk on, because both come from
                // `block_height`.
                let top = if is_liquid(id) {
                    primitive_shared::fluid::surface_height_with_above(
                        id,
                        cache.block_near(cell, y, 0, 1, 0),
                    )
                } else {
                    block_height(id)
                };

                let translucent_flag = if is_translucent(id) { TRANSLUCENT_BIT } else { 0 };
                // **Leaves are lit flat, and only leaves.**
                //
                // Smooth lighting and ambient occlusion are what make a
                // *wall* read as a wall: they cost eighteen samples per
                // face, and they earn it on flat surfaces with corners
                // in them. A canopy has neither. It is the densest
                // geometry in the world -- a tree is a few hundred faces
                // where a cliff is a dozen -- and every one of those
                // faces was paying for corner darkening that lands on a
                // surface made of holes, where it reads as noise if it
                // reads at all.
                //
                // So a leaf face takes the light of the cell in front of
                // it and no occlusion, which is one sample instead of
                // eighteen. Foliage is the one place where the flat
                // version also looks *better*: a canopy shaded corner by
                // corner has a visible lattice in it.
                let flat_lit = is_cutout(id);
                // Which way this block lies, for the UV turn below. Per
                // block, not per face: it is a function of the id alone.
                let axis = primitive_shared::types::block_axis(id);

                for (face_index, face) in face_defs.iter().enumerate() {
                    let n = face.neighbor;
                    // Once, not twice. The id and its cover come from
                    // the same cell, and looking it up again for the
                    // second of them is a redundant load on the hottest
                    // loop in the client -- six per block, every block.
                    let neighbor_id = cache.block_near(cell, y, n[0], n[1], n[2]);
                    let neighbor_cover = cover_table[neighbor_id as usize];
                    if !face_visible(id, cover, neighbor_id, neighbor_cover, face_index) {
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

                    // The nine cells around the face's neighbour, in the
                    // plane of the face, gathered once.
                    //
                    // Between them the four corners touch exactly these
                    // nine: each corner wants the neighbour cell, the
                    // two beside it and the diagonal. Sampling per
                    // corner re-read the neighbour four times and each
                    // edge cell twice -- twenty-eight lookups where nine
                    // will do, on the hottest loop in the client.
                    let mut ring_light = [[0u8; 3]; 3];
                    let mut ring_opaque = [[false; 3]; 3];
                    if flat_lit {
                        let light = shaded_canopy(
                            cache.light_near(cell, y, n[0], n[1], n[2]),
                        );
                        ring_light = [[light; 3]; 3];
                    } else {
                        for (ia, da) in [-1i32, 0, 1].into_iter().enumerate() {
                            for (ib, db) in [-1i32, 0, 1].into_iter().enumerate() {
                                let mut off = n;
                                off[axis_a] += da;
                                off[axis_b] += db;
                                ring_light[ia][ib] =
                                    cache.light_near(cell, y, off[0], off[1], off[2]);
                                // From the cached cover rather than from
                                // `is_opaque` on the id: this is nine reads
                                // per face on the hottest loop there is.
                                ring_opaque[ia][ib] = cover_table[cache
                                    .block_near(cell, y, off[0], off[1], off[2])
                                    as usize]
                                    == FULL_COVER;
                            }
                        }
                    }

                    let base = vertices.len() as u32;
                    let mut ao_values = [0u8; 4];

                    // A quarter turn per step, from a hash of the cell
                    // and which face of it this is -- so the six faces
                    // of one block disagree with each other as well as
                    // with their neighbours, and the same block comes
                    // out the same way round every time the chunk is
                    // remeshed. See `turned_uv`.
                    let turn = if primitive_shared::types::texture_turns(id, face_index) {
                        cell_hash(gx, y, gz) >> (face_index as u32 * 2)
                    } else {
                        0
                    }
                    // ...plus the quarter turn a lying block imposes.
                    // `layer_for_face` already picks the right *image*
                    // for a turned block; this is the other half of the
                    // same rotation, without which a fallen log's bark
                    // ran across the trunk instead of along it. Quarter
                    // turns compose by addition, so the hash turn above
                    // (never set for wood, but the code should not care)
                    // stacks with it instead of being overwritten.
                    + axis_uv_turn(axis, face_index);

                    for (corner_index, corner) in face.corners.iter().enumerate() {
                        let uv = turned_uv(face_uv(face_index, *corner), turn);
                        // Which side of the ring this corner sits on:
                        // index 0 is the -1 offset, 2 is +1.
                        let ia = if corner[axis_a] > 0.5 { 2 } else { 0 };
                        let ib = if corner[axis_b] > 0.5 { 2 } else { 0 };

                        let side1 = ring_opaque[ia][1];
                        let side2 = ring_opaque[1][ib];
                        let diagonal = ring_opaque[ia][ib];
                        let ao = vertex_ao(side1, side2, diagonal);
                        ao_values[corner_index] = ao;

                        // Smooth lighting: average the four cells that
                        // touch this corner from the outside, skipping
                        // opaque ones (which are dark and would drag the
                        // average down through a wall).
                        let (sky, block_light) =
                            corner_light(&ring_light, ia, ib, side1, side2, diagonal);

                        vertices.push(Vertex::tinted(
                            [
                                gx as f32 + corner[0],
                                // Scaling rather than subtracting keeps
                                // the bottom corners on the cell floor,
                                // so a side face shortens to meet the
                                // lowered top instead of shearing.
                                //
                                y as f32 + corner[1] * top,
                                gz as f32 + corner[2],
                            ],
                            uv,
                            layer,
                            pack_light(sky, block_light, ao, face_index as u8) | translucent_flag,
                            tint,
                        ));
                    }

                    // Flip the quad's diagonal when the AO values are
                    // anisotropic. Without this the interpolation across
                    // the two triangles produces the classic diagonal
                    // seam artefact on shaded corners.
                    // Leaves and sprites are both alpha cutouts, and
                    // they are kept apart anyway: a leaf is mostly
                    // solid and a tuft of grass is mostly empty, and
                    // the renderer can afford them different treatment
                    // at distance only if it can tell them apart. See
                    // `renderer::render`.
                    let target = if translucent_flag != 0 {
                        &mut *translucent
                    } else if is_cutout(id) {
                        &mut *leaves
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
    indices.extend_from_slice(leaves);
    *leaf_end = indices.len() as u32;
    indices.extend_from_slice(sprites);
    *sprite_end = indices.len() as u32;
    indices.extend_from_slice(translucent);
}

/// **The shape of a tuft, as four corners per plane.**
///
/// Shared rather than inlined into the mesher because the mining
/// overlay has to draw its cracks on exactly this, and the two drifting
/// apart is what "the crack texture is a box around the grass" was: the
/// overlay drew the *bounding box* of a tuft, so hitting a blade put a
/// metre cube of cracks in the air around it.
///
/// The jitter comes from the cell, so a tuft is in the same place every
/// time the chunk is remeshed -- and now, in the same place the cracks
/// on it are.
pub(crate) fn cross_planes(origin: [f32; 3]) -> [[[f32; 3]; 4]; 2] {
    /// How far in from the cell's edges the planes sit.
    const INSET: f32 = 0.08;
    /// Plants stop short of the ceiling; a tuft of grass filling the
    /// whole cell looks like a hedge.
    const HEIGHT: f32 = 0.94;
    /// How far a tuft may wander from the middle of its cell.
    const JITTER: f32 = 0.07;
    /// ...and how much of its height it may gain or lose.
    const HEIGHT_JITTER: f32 = 0.12;

    let noise = cell_hash(origin[0] as i32, origin[1] as i32, origin[2] as i32);
    let signed = |shift: u32| (((noise >> shift) & 0xFF) as f32 / 255.0) * 2.0 - 1.0;
    let offset_x = signed(0) * JITTER;
    let offset_z = signed(8) * JITTER;
    // Clamped to the cell. The jitter can add an eighth to a height of
    // 0.94, which is 1.05 -- a tall tuft grew *through* the block above
    // it, and the box you aim at cannot follow it there without
    // becoming clickable from inside that block.
    let height = (HEIGHT * (1.0 + signed(16) * HEIGHT_JITTER)).min(1.0);

    let (lo, hi) = (INSET, 1.0 - INSET);
    [((lo, lo), (hi, hi)), ((lo, hi), (hi, lo))].map(|(a, b)| {
        [(a.0, 0.0, a.1), (b.0, 0.0, b.1), (b.0, height, b.1), (a.0, height, a.1)].map(
            |(x, y, z)| {
                [
                    origin[0] + x + offset_x,
                    origin[1] + y,
                    origin[2] + z + offset_z,
                ]
            },
        )
    })
}

/// **The shape of a stone lying on the ground**, as four corners.
///
/// Shared for the same reason as `cross_planes`: the mining overlay
/// draws on this, and a bounding box would put a six-sided shell around
/// a quad two centimetres thick.
/// Takes the block rather than its measurements: how far in a flat thing
/// lies and how far above the floor are two answers to the same
/// question -- is this an object on a surface, or is it the surface --
/// and reading them from one id is what keeps them from disagreeing.
/// See `types::is_covering_flat`.
pub(crate) fn flat_quad(origin: [f32; 3], block: BlockId) -> [[f32; 3]; 4] {
    let inset = primitive_shared::types::flat_inset(block);
    let lift = primitive_shared::types::flat_lift(block);
    let (lo, hi) = (inset, 1.0 - inset);
    [(lo, lo), (hi, lo), (hi, hi), (lo, hi)]
        .map(|(x, z)| [origin[0] + x, origin[1] + lift, origin[2] + z])
}

/// Two quads crossing at the cell's diagonals: grass, sticks, anything
/// standing *in* a block rather than being one.
///
/// **Two quads, not four.** This used to emit each plane twice, once in
/// each winding, on the argument that the terrain pipeline culls back
/// faces -- but plants go into the *cutout* pass, which has been
/// `cull_mode: None` since leaves needed to be visible from inside a
/// canopy. So both copies were drawn: two exactly coplanar,
/// depth-writing quads in the same place, z-fighting along every blade,
/// which is the shimmer you could see on a field of grass from a few
/// blocks away. It also doubled the geometry of the single densest
/// thing in the world -- a plains chunk is a tuft every three columns.
///
/// Inset from the cell's corners so a tuft does not poke through the
/// wall of the block beside it, and lit from its own cell rather than
/// per corner: there is nothing here for ambient occlusion to darken,
/// and a blade of grass shaded like a wall reads as a mistake.
///
/// Each tuft is nudged off the centre of its cell by a hash of where it
/// stands. Without it a field is a lattice -- every blade on the same
/// grid, at the same height, and the regularity is obvious from any
/// distance where you can see more than a dozen of them at once. The
/// offset is bounded by the inset, so a shifted tuft still cannot reach
/// into the neighbouring cell.
fn cross_block(
    origin: [f32; 3],
    layer: u32,
    light: u8,
    tint: u32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block) = (light & 0x0F, (light >> 4) & 0x0F);
    // Face 0 (up) for the light direction: a plant has no normal worth
    // the name, and shading it as an upward face keeps it the same
    // brightness from every side, which is what a billboard wants.
    let packed = pack_light(sky, block, 3, 0);

    // The corners come from `cross_planes`, which the mining overlay
    // reads too -- see there for why they are not worked out here.
    for plane in cross_planes(origin) {
        let base = vertices.len() as u32;
        for (corner, uv) in plane.into_iter().zip([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
        {
            vertices.push(Vertex::tinted(corner, uv, layer, packed, tint));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn flat_block(
    origin: [f32; 3],
    block: BlockId,
    layer: u32,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    // Face 0 (up), which is the way it faces and the light it should
    // catch.
    let packed = pack_light(sky, block_light, 3, 0);

    // A quarter turn per step, as a rotation of which corner gets which
    // texture coordinate.
    let turn = (cell_hash(origin[0] as i32, origin[1] as i32, origin[2] as i32) & 3) as usize;
    const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

    let base = vertices.len() as u32;
    // The corners come from `flat_quad`, shared with the mining overlay.
    for (corner, position) in flat_quad(origin, block).into_iter().enumerate() {
        vertices.push(Vertex::new(position, UVS[(corner + turn) % 4], layer, packed));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// How many levels of light a leaf gives up to the canopy around it.
///
/// Two of fifteen: about a seventh, which is the difference between a
/// wood you look *into* and a green wall.
const CANOPY_SHADE: u8 = 2;

/// A leaf's light, with the shade of the canopy it is part of taken off.
///
/// **Why leaves need this and nothing else does.** Every other face gets
/// smooth lighting: the light at each corner is averaged over the cells
/// that touch it, so a surface picks up the shadow of whatever stands
/// near it. Leaves opted out of that for speed -- a canopy is the
/// densest geometry in the world and eighteen samples a face is a great
/// deal to spend on a surface made of holes -- and take the light of the
/// single cell in front of them instead.
///
/// That is cheap and it is also *too bright*, in the one place it is
/// used. The cell in front of a leaf on the outside of a tree is open
/// sky, so it reads the full fifteen; a real canopy is a stack of
/// leaves each shading the ones under it, and the flat version has no
/// way to know. So a forest came out the colour of a lawn, and from a
/// distance a wood was a bright green mass with no depth in it at all.
///
/// A flat discount is the honest fix for a flat approximation: it costs
/// the one subtraction the fast path was built to afford, and it gives
/// back exactly what that path threw away. Both channels, because a
/// torch under a tree lights the underside of the canopy no differently
/// than the sun lights the top of it.
#[inline]
fn shaded_canopy(light: u8) -> u8 {
    let sky = (light & 0x0F).saturating_sub(CANOPY_SHADE);
    let block = ((light >> 4) & 0x0F).saturating_sub(CANOPY_SHADE);
    sky | (block << 4)
}

/// The quarter turn that keeps a texture's "up" pointing along a lying
/// block's own top, per world face.
///
/// `FaceLayers::layer_for_face` rotates *which image* each world face of
/// a turned block shows; this is the matching rotation of how that image
/// lies on the face. Without it the side of a fallen log wore the right
/// bark with the grain running across the trunk -- the layer had been
/// turned and the coordinates had not.
///
/// The values are the rotations written out, one per face, exactly like
/// `local_face`'s tables and checked the same way: the test below
/// reconstructs each face's world-space "up" from the UVs this produces
/// and demands it point along the block's axis, with no mirroring. The
/// faces that show the block's own ends (the cut rings of a log) get no
/// turn -- an end has no grain direction for one to preserve.
#[inline]
fn axis_uv_turn(axis: primitive_shared::types::Axis, face_index: usize) -> u32 {
    use primitive_shared::types::Axis;
    match axis {
        Axis::Y => 0,
        Axis::X => match face_index {
            0 | 1 | 4 => 1,
            5 => 3,
            _ => 0, // the ends
        },
        Axis::Z => match face_index {
            0 => 2,
            2 => 3,
            3 => 1,
            _ => 0, // face 1 already reads along +Z; 4 and 5 are the ends
        },
    }
}

pub(crate) fn cell_hash(x: i32, y: i32, z: i32) -> u32 {
    let mut h = (x as u32)
        .wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^ (h >> 13)
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
    fn the_vertex_packing_roundtrips() {
        // The shader decodes this word with the same shifts. A silent
        // mismatch does not fail anything -- it puts the wrong texture
        // on every block in the world, which is a lot of pixels to
        // notice from a unit test's point of view.
        for (uv, layer, light, tint) in [
            ([0.0, 0.0], 0u32, 0u32, 0u32),
            ([1.0, 0.0], 1, pack_light(15, 0, 0, 0), 1),
            ([0.0, 1.0], 108, pack_light(0, 15, 3, 5), pack_tint(0.5, 0.5)),
            (
                [1.0, 1.0],
                MAX_TEXTURE_LAYERS - 1,
                pack_light(9, 4, 2, 3) | TRANSLUCENT_BIT,
                pack_tint(1.0, 1.0),
            ),
        ] {
            let v = Vertex::tinted([1.0, 2.0, 3.0], uv, layer, light, tint);
            assert_eq!(v.uv(), uv, "uv did not survive packing");
            assert_eq!(v.tex_layer(), layer, "layer did not survive packing");
            assert_eq!(v.light(), light, "light did not survive packing");
            assert_eq!(v.tint(), tint, "tint did not survive packing");
            assert_eq!(v.position, [1.0, 2.0, 3.0]);
        }
    }

    #[test]
    fn the_tint_code_never_collides_with_untinted() {
        // Zero is what the shader reads as "leave this alone", so no
        // real climate may pack to it -- and the whole square has to fit
        // in the byte it shares with nothing.
        for t in 0..=20 {
            for h in 0..=20 {
                let code = pack_tint(t as f32 / 20.0, h as f32 / 20.0);
                assert!(code > 0, "climate ({t},{h}) packed to the untinted code");
                assert!(code <= 0xFF, "climate ({t},{h}) packed to {code}, past a byte");
            }
        }
        // ...and out-of-range input saturates rather than wrapping into
        // some other climate.
        assert_eq!(pack_tint(-5.0, -5.0), pack_tint(0.0, 0.0));
        assert_eq!(pack_tint(5.0, 5.0), pack_tint(1.0, 1.0));
    }

    #[test]
    fn the_tint_is_a_monotonic_map_of_the_climate() {
        // Warmer must never come back as a *lower* temperature bucket,
        // or the shader's bilinear blend runs backwards somewhere in the
        // middle of the world.
        let decode = |code: u32| {
            let index = code - 1;
            (index / TINT_LEVELS, index % TINT_LEVELS)
        };
        let mut last = (0, 0);
        for step in 0..=14 {
            let v = step as f32 / 14.0;
            let (t, h) = decode(pack_tint(v, v));
            assert!(t >= last.0 && h >= last.1, "climate {v} went backwards");
            last = (t, h);
        }
        assert_eq!(last, (TINT_LEVELS - 1, TINT_LEVELS - 1));
    }

    #[test]
    fn the_vertex_is_as_small_as_it_claims() {
        // The whole reason for the packing. A regression here is a
        // silent 75% increase in GPU memory for the terrain.
        assert_eq!(std::mem::size_of::<Vertex>(), 16);
    }

    #[test]
    fn every_uv_the_mesher_produces_fits_in_two_bits() {
        // The packing assumes block faces are mapped corner to corner.
        // If `face_uv` ever returned anything between 0 and 1, the
        // texture coordinates would silently snap.
        let corners = faces();
        for (face_index, face) in corners.iter().enumerate() {
            for corner in face.corners.iter() {
                let uv = face_uv(face_index, *corner);
                for component in uv {
                    assert!(
                        component == 0.0 || component == 1.0,
                        "face {face_index} produced uv {uv:?}, which cannot be packed"
                    );
                }
            }
        }
    }

    /// World-space direction of the texture's "up" (decreasing v) and
    /// "right" (increasing u) on one face, from the corner UVs.
    fn image_axes(face_index: usize, turn: u32) -> ([f32; 3], [f32; 3]) {
        let face = &faces()[face_index];
        let mut up = [0.0f32; 3];
        let mut right = [0.0f32; 3];
        for corner in face.corners.iter() {
            let [u, v] = turned_uv(face_uv(face_index, *corner), turn);
            for a in 0..3 {
                // Corners with v = 0 pull "up" toward themselves, v = 1
                // push away; likewise u for "right".
                up[a] += corner[a] * (1.0 - 2.0 * v);
                right[a] += corner[a] * (2.0 * u - 1.0);
            }
        }
        (up, right)
    }

    #[test]
    fn a_lying_blocks_texture_runs_along_the_block() {
        use primitive_shared::types::Axis;
        // The world-space direction each axis turns the block's own top
        // toward -- which is where a side texture's "up" must point.
        for (axis, expected_up, side_faces) in [
            (Axis::X, [1.0, 0.0, 0.0], [0usize, 1, 4, 5]),
            (Axis::Z, [0.0, 0.0, 1.0], [0, 1, 2, 3]),
        ] {
            for face_index in side_faces {
                let turn = axis_uv_turn(axis, face_index);
                let (up, right) = image_axes(face_index, turn);
                let dot: f32 = (0..3).map(|a| up[a] * expected_up[a]).sum();
                assert!(
                    dot > 1.9,
                    "{axis:?} face {face_index}: image up is {up:?}, not along the axis"
                );
                // ...and turned, not mirrored: right x up must still be
                // the outward normal, as it is on every unturned face.
                let normal = &faces()[face_index].neighbor;
                let cross = [
                    right[1] * up[2] - right[2] * up[1],
                    right[2] * up[0] - right[0] * up[2],
                    right[0] * up[1] - right[1] * up[0],
                ];
                let outward: f32 =
                    (0..3).map(|a| cross[a] * normal[a] as f32).sum();
                assert!(
                    outward > 0.0,
                    "{axis:?} face {face_index}: the texture is mirrored"
                );
            }
        }
        // An upright block is left exactly alone.
        for face_index in 0..6 {
            assert_eq!(axis_uv_turn(Axis::Y, face_index), 0);
        }
    }

    #[test]
    fn every_unturned_face_reads_upright_and_unmirrored() {
        // The convention the axis turns are measured against: on the
        // vertical faces of a standing block, "up" is +Y and no face is
        // mirrored. If `face_uv` ever breaks this, the test above keeps
        // passing for the wrong reason.
        for face_index in 2..6 {
            let (up, right) = image_axes(face_index, 0);
            assert_eq!(up, [0.0, 2.0, 0.0], "face {face_index} is not upright");
            let normal = &faces()[face_index].neighbor;
            let cross = [
                right[1] * up[2] - right[2] * up[1],
                right[2] * up[0] - right[0] * up[2],
                right[0] * up[1] - right[1] * up[0],
            ];
            let outward: f32 = (0..3).map(|a| cross[a] * normal[a] as f32).sum();
            assert!(outward > 0.0, "face {face_index} is mirrored");
        }
    }

    #[test]
    fn a_coating_lies_on_the_floor_and_hides_what_it_covers() {
        use primitive_shared::types::{BLOCK_ASH, BLOCK_DIRT, BLOCK_PEBBLE};

        // **The gap under the ash.** A coating used to be lifted a
        // fiftieth of a block clear of the ground so it would not
        // z-fight the face beneath it -- which showed as daylight under
        // the ash along the edge of a bank, and, since the quad is
        // drawn from both sides, as a grey sheet hanging in the air
        // when you looked up at one.
        for corner in flat_quad([4.0, 7.0, 9.0], BLOCK_ASH) {
            assert_eq!(corner[1], 7.0, "the coating is still floating");
        }
        // ...and it covers the cell corner to corner, so there is no
        // rim of bare earth around it.
        let xs: Vec<f32> = flat_quad([4.0, 7.0, 9.0], BLOCK_ASH)
            .iter()
            .map(|c| c[0])
            .collect();
        assert!(xs.contains(&4.0) && xs.contains(&5.0));

        // An *object* keeps its lift, and needs it: the ground under a
        // pebble is still drawn, and two coplanar quads twenty blocks
        // off flicker between each other as the camera moves.
        for corner in flat_quad([4.0, 7.0, 9.0], BLOCK_PEBBLE) {
            assert!(corner[1] > 7.0, "a pebble sunk into the ground");
        }

        // The other half of the fix, and what pays for the lift going
        // away: the face under a coating is not drawn at all, so there
        // is nothing left for it to fight with.
        assert!(
            !shows_face(BLOCK_DIRT, BLOCK_ASH, 0),
            "the ground under a coating is drawn twice over"
        );
        // Only that face, and only under a coating. The sides of the
        // block are still visible past the ash on top of it, and a
        // pebble hides nothing -- earth shows all round one.
        for face in 1..6 {
            assert!(shows_face(BLOCK_DIRT, BLOCK_ASH, face), "face {face} vanished");
        }
        assert!(shows_face(BLOCK_DIRT, BLOCK_PEBBLE, 0), "a pebble hid the ground");
    }

    #[test]
    fn a_canopy_is_darker_than_the_sky_it_stands_under() {
        // Leaves take the light of the cell in front of them, which for
        // the outside of a tree is open sky -- so without a discount a
        // wood is lit like a lawn. See `shaded_canopy`.
        let open = pack_light(15, 0, 3, 0) as u8;
        let shaded = shaded_canopy(open);
        assert!(shaded & 0x0F < 15, "a leaf in full sun is not shaded at all");
        assert_eq!(shaded & 0x0F, 15 - CANOPY_SHADE);

        // Both channels: a torch under a tree lights the underside of
        // the canopy no differently than the sun lights the top.
        let torch = 0x0F << 4;
        assert_eq!((shaded_canopy(torch) >> 4) & 0x0F, 15 - CANOPY_SHADE);

        // ...and the discount never wraps a dark leaf round to a bright
        // one, which is what a plain subtraction on a nibble would do.
        for level in 0..=CANOPY_SHADE {
            let dim = level | (level << 4);
            assert_eq!(shaded_canopy(dim), 0, "{level} wrapped instead of clamping");
        }
        // Nothing leaks between the two nibbles.
        assert_eq!(shaded_canopy(0x0F), 15 - CANOPY_SHADE);
        assert_eq!(shaded_canopy(0xF0), (15 - CANOPY_SHADE) << 4);
    }

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

    /// Every face of a block whose six neighbours are all `neighbor`.
    ///
    /// The cover byte is derived here rather than passed in: it is a
    /// pure function of the block, and a test that could pass a cover
    /// that disagreed with its block would be testing a state the
    /// mesher cannot be in.
    fn faces_drawn(current: BlockId, neighbor: BlockId) -> usize {
        (0..6).filter(|&f| shows_face(current, neighbor, f)).count()
    }

    /// `face_visible` with both cover bytes filled in from the blocks.
    fn shows_face(current: BlockId, neighbor: BlockId, face: usize) -> bool {
        face_visible(current, cover_of(current), neighbor, cover_of(neighbor), face)
    }

    #[test]
    fn hidden_faces_are_not_emitted() {
        assert_eq!(faces_drawn(BLOCK_STONE, BLOCK_STONE), 0);
        assert_eq!(faces_drawn(BLOCK_STONE, BLOCK_AIR), 6);
        // Water surface against air: visible. Water against water: not.
        assert_eq!(faces_drawn(BLOCK_WATER, BLOCK_AIR), 6);
        assert_eq!(faces_drawn(BLOCK_WATER, BLOCK_WATER), 0);
        // Stone against water: visible, so a lake bed still renders.
        assert_eq!(faces_drawn(BLOCK_STONE, BLOCK_WATER), 6);
    }

    #[test]
    fn no_two_cells_of_water_ever_share_a_face() {
        // **Water looks the same everywhere, so between two cells of it
        // there is nothing to draw.**
        //
        // There used to be one exception: a deeper cell beside a
        // shallower one stood above it, and the band between the two
        // surfaces was a real wall. Which meant every cell that filled
        // after a player broke a block put a wall across the sea while
        // it filled, and any cell that finished short of full kept one
        // for ever. A level nobody can see cannot do that -- see
        // `fluid::surface_height`.
        use primitive_shared::types::with_layers;
        for a in [BLOCK_WATER, with_layers(BLOCK_WATER, 1), with_layers(BLOCK_WATER, 5)] {
            for b in [BLOCK_WATER, with_layers(BLOCK_WATER, 2), with_layers(BLOCK_WATER, 7)] {
                assert_eq!(
                    faces_drawn(a, b),
                    0,
                    "a wall inside the water between {a:#x} and {b:#x}"
                );
            }
        }
        // ...and water still shows itself against everything else.
        assert_eq!(faces_drawn(with_layers(BLOCK_WATER, 2), BLOCK_AIR), 6);
        assert_eq!(faces_drawn(BLOCK_STONE, with_layers(BLOCK_WATER, 2)), 6);
    }

    #[test]
    fn the_face_between_two_leaves_is_drawn_exactly_once() {
        // Not twice -- two coplanar depth-writing quads z-fight, and
        // since each carries its own face index they shade differently,
        // so the canopy shimmers between two brightnesses.
        //
        // Not zero either -- that empties the canopy out, and through
        // the gaps in the leaf texture you see daylight where the inside
        // of the tree should be.
        use primitive_shared::types::BLOCK_LEAVES;
        for face in 0..6 {
            let opposite = face ^ 1; // +Y/-Y, +X/-X, +Z/-Z are paired
            let mine = shows_face(BLOCK_LEAVES, BLOCK_LEAVES, face);
            let theirs = shows_face(BLOCK_LEAVES, BLOCK_LEAVES, opposite);
            assert!(
                mine ^ theirs,
                "face {face} and its neighbour's {opposite} both {}",
                if mine { "draw" } else { "skip" }
            );
        }
        assert_eq!(faces_drawn(BLOCK_LEAVES, BLOCK_LEAVES), 3);
        assert_eq!(faces_drawn(BLOCK_LEAVES, BLOCK_AIR), 6);
        // A solid block next to leaves still draws its own face: the
        // leaves' silhouette is full of holes and does not cover it.
        assert_eq!(faces_drawn(BLOCK_STONE, BLOCK_LEAVES), 6);
    }

    #[test]
    fn a_leaf_cluster_keeps_its_interior() {
        // The whole point of drawing the shared face once rather than
        // never: a block buried in the middle of a canopy still
        // contributes geometry, so a tree has depth when you look into
        // it instead of being a hollow shell.
        use primitive_shared::types::BLOCK_LEAVES;
        assert!(
            faces_drawn(BLOCK_LEAVES, BLOCK_LEAVES) > 0,
            "an enclosed leaf block emits nothing -- the canopy is hollow"
        );
    }

    #[test]
    fn nothing_see_through_is_drawn_against_an_unloaded_chunk() {
        // Guessing "air" for a chunk that has not arrived invents a face
        // along the streaming frontier. For water -- blended, no depth
        // write -- that face shows straight through the terrain in front
        // of it, and it moves with the player as chunks load.
        use primitive_shared::types::BLOCK_LEAVES;
        assert_eq!(faces_drawn(BLOCK_WATER, UNKNOWN_BLOCK), 0);
        assert_eq!(faces_drawn(BLOCK_LEAVES, UNKNOWN_BLOCK), 0);
        // Opaque terrain still does: the face is hidden as soon as the
        // neighbour lands, and until then a solid wall is how the edge
        // of the loaded world should read.
        assert_eq!(faces_drawn(BLOCK_STONE, UNKNOWN_BLOCK), 6);
    }

    #[test]
    fn unknown_is_not_a_block_anyone_can_place() {
        // The sentinel has to stay outside the real id space, or a chunk
        // could legitimately contain it and vanish.
        assert!(!primitive_shared::types::is_known_block(UNKNOWN_BLOCK));
        assert!(is_opaque(UNKNOWN_BLOCK), "unknown must not leak light or AO");
    }
}

/// The reported bug, at the level of a finished mesh rather than a
/// culling rule: a lake at the edge of the loaded world used to grow a
/// skin of water faces along the seam.
#[cfg(test)]
mod frontier_tests {
    use super::*;
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::types::{ChunkPos, BLOCK_WATER, CHUNK_VOLUME};

    /// One chunk of water up to `surface`, with nothing around it.
    fn lone_water_chunk(surface: usize) -> (ChunkManager, LightMap, ChunkPos) {
        let pos = ChunkPos::new(0, 0);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..=surface {
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, y, z)] = BLOCK_WATER;
                }
            }
        }
        let mut chunks = ChunkManager::new(4);
        chunks.insert(Chunk { pos, blocks });
        let mut light = LightMap::new();
        light.load_chunk(&chunks, pos);
        (chunks, light, pos)
    }

    fn mesh_it(chunks: &ChunkManager, light: &LightMap, pos: ChunkPos) -> MeshBuffers {
        let mut cache = Neighbourhood::default();
        cache.fill(pos, chunks, light);
        let mut out = MeshBuffers::default();
        build_mesh(pos, &cache, &crate::engine::texture::FaceLayers::empty_for_test(), &primitive_shared::worldgen::WorldGen::new(0), &mut out);
        out
    }

    #[test]
    fn a_lake_at_the_edge_of_the_world_grows_no_walls() {
        const SURFACE: usize = 19;
        let (chunks, light, pos) = lone_water_chunk(SURFACE);
        let mesh = mesh_it(&chunks, &light, pos);

        let water_indices = mesh.indices.len() as u32 - mesh.sprite_end;
        // Only the surface should be drawn: one quad per column, six
        // indices each. Water against water is culled, the bottom is
        // against the world floor, and all four sides face chunks we do
        // not have -- which is the case this test exists for. Before the
        // fix those sides added 4 x 16 x 20 quads of bright blue sheet.
        let expected = (CHUNK_SIZE_X * CHUNK_SIZE_Z * 6) as u32;
        assert_eq!(
            water_indices, expected,
            "expected only the lake surface, got {} quads",
            water_indices / 6
        );
    }

    #[test]
    fn the_walls_appear_once_the_neighbour_actually_arrives_and_is_dry() {
        // The other half of the contract: suppressing the faces must not
        // mean they are gone for good. Give the lake a dry neighbour and
        // the shoreline has to be drawn.
        const SURFACE: usize = 19;
        let (mut chunks, mut light, pos) = lone_water_chunk(SURFACE);
        let before = {
            let mesh = mesh_it(&chunks, &light, pos);
            mesh.indices.len() as u32 - mesh.sprite_end
        };

        let dry = ChunkPos::new(1, 0);
        chunks.insert(Chunk {
            pos: dry,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        });
        light.load_chunk(&chunks, dry);

        let after = {
            let mesh = mesh_it(&chunks, &light, pos);
            mesh.indices.len() as u32 - mesh.sprite_end
        };
        assert!(
            after > before,
            "the shoreline facing the new chunk was never drawn ({before} then {after})"
        );
    }

    #[test]
    fn opaque_terrain_still_closes_itself_off_at_the_frontier() {
        // Solid blocks keep drawing into the unknown: the face is hidden
        // the moment the neighbour lands, and until then a wall is how
        // the edge of the world should look.
        let pos = ChunkPos::new(0, 0);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                blocks[Chunk::index(x, 5, z)] = primitive_shared::types::BLOCK_STONE;
            }
        }
        let mut chunks = ChunkManager::new(4);
        chunks.insert(Chunk { pos, blocks });
        let mut light = LightMap::new();
        light.load_chunk(&chunks, pos);

        let mesh = mesh_it(&chunks, &light, pos);
        // Top and bottom are 256 quads each; the four edges add 16 more
        // apiece, and those are the ones that would vanish if opaque
        // blocks were culled against unknown territory too.
        let solid_quads = mesh.solid_index_count / 6;
        assert!(
            solid_quads > (CHUNK_SIZE_X * CHUNK_SIZE_Z * 2) as u32,
            "the slab's edges were culled away: {solid_quads} quads"
        );
    }
}

#[cfg(test)]
mod uv_and_light_tests {
    use super::*;
    use crate::engine::texture::FaceLayers;
    use primitive_shared::types::BLOCK_STONE;

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

    /// The 3x3 ring the face loop gathers, with every cell the same.
    fn lit_ring(sky: u8, block: u8) -> [[u8; 3]; 3] {
        [[(sky & 0x0F) | ((block & 0x0F) << 4); 3]; 3]
    }

    #[test]
    fn corner_light_averages_open_neighbours() {
        let ring = lit_ring(8, 4);
        let (sky, block) = corner_light(&ring, 2, 2, false, false, false);
        assert_eq!((sky, block), (8, 4), "a uniformly lit area averages to itself");
    }

    #[test]
    fn corner_light_ignores_opaque_neighbours_instead_of_counting_them_as_dark() {
        // With one side walled off, the average must stay at the open
        // cells' level. Counting the wall as 0 would smear a shadow
        // along every wall base.
        let mut ring = lit_ring(12, 0);
        // Darken the cell we're about to call opaque, to prove it is
        // skipped rather than averaged in.
        ring[2][1] = 0;
        let (sky, _) = corner_light(&ring, 2, 2, true, false, false);
        assert_eq!(sky, 12, "opaque neighbours must not drag the average down");
    }

    #[test]
    fn corner_light_varies_across_a_gradient() {
        // The whole point of smooth lighting: two corners of the same
        // face sitting in different light must come out different.
        let mut ring = lit_ring(0, 0);
        for (ia, row) in ring.iter_mut().enumerate() {
            for cell in row.iter_mut() {
                *cell = ia as u8 * 6;
            }
        }
        let left = corner_light(&ring, 0, 1, false, false, false);
        let right = corner_light(&ring, 2, 1, false, false, false);
        assert!(
            left.0 < right.0,
            "smooth lighting should follow the gradient ({} vs {})",
            left.0,
            right.0
        );
    }

    #[test]
    fn the_ring_a_face_gathers_is_the_one_its_corners_read() {
        // The refactor this guards: AO and smooth light used to sample
        // the world per corner, and now read a ring gathered once per
        // face. A ring indexed the wrong way round would put each
        // corner's shading on the opposite corner -- which looks like
        // lighting, just wrong, and no other test would notice.
        //
        // A wall on one side of an otherwise open face: the two corners
        // against the wall must come out darker than the two away from
        // it.
        let cache = cache_with_wall();
        let layers = FaceLayers::empty_for_test();
        let mut out = MeshBuffers::default();
        build_mesh(ChunkPos::new(0, 0), &cache, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);

        // The top face of the block at (5, 5, 5): its corners at x = 6
        // touch the wall at x = 6, the ones at x = 5 do not.
        let top_face = faces()
            .iter()
            .position(|f| f.neighbor == [0, 1, 0])
            .expect("no upward face") as u32;
        let ao_of = |v: &Vertex| (v.light() >> 8) & 0x3;
        let top: Vec<&Vertex> = out
            .vertices
            .iter()
            .filter(|v| (v.light() >> 10) & 0x7 == top_face)
            .filter(|v| v.position[1] == 6.0)
            .filter(|v| v.position[0] >= 5.0 && v.position[0] <= 6.0)
            .filter(|v| v.position[2] >= 5.0 && v.position[2] <= 6.0)
            .collect();
        assert_eq!(top.len(), 4, "expected one quad on top of the block");
        let against_wall: Vec<u32> = top
            .iter()
            .filter(|v| v.position[0] == 6.0)
            .map(|v| ao_of(v))
            .collect();
        let away: Vec<u32> = top
            .iter()
            .filter(|v| v.position[0] == 5.0)
            .map(|v| ao_of(v))
            .collect();
        assert_eq!(against_wall.len(), 2);
        assert_eq!(away.len(), 2);
        assert!(
            against_wall.iter().max() < away.iter().min(),
            "the corners beside the wall are not the darkened ones: {against_wall:?} vs {away:?}"
        );
    }

    /// One block to mesh, with a wall of stone one cell to its +x side.
    fn cache_with_wall() -> Neighbourhood {
        let mut cache = Neighbourhood::default();
        for cell in cache.light.iter_mut() {
            *cell = 0x0F;
        }
        cache.blocks[padded_index(5 + PAD as usize, 5, 5 + PAD as usize)] = BLOCK_STONE;
        for y in 5..8 {
            cache.blocks[padded_index(6 + PAD as usize, y, 5 + PAD as usize)] = BLOCK_STONE;
        }
        cache.recompute_ceiling();
        cache
    }
}

#[cfg(test)]
mod turned_texture_tests {
    use super::*;
    use primitive_shared::types::{texture_turns, BLOCK_GRASS, BLOCK_LOG, BLOCK_STONE};

    /// The four corners a face is mapped with.
    fn corners_of(face: usize, turn: u32) -> Vec<[i32; 2]> {
        let face_defs = faces();
        let mut out: Vec<[i32; 2]> = face_defs[face]
            .corners
            .iter()
            .map(|c| {
                let uv = turned_uv(face_uv(face, *c), turn);
                [uv[0] as i32, uv[1] as i32]
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn a_turn_still_fits_the_two_bits_the_vertex_has() {
        // The whole reason rotation is free. If a turn ever produced a
        // coordinate that was not 0 or 1, the packing would silently
        // round it and the texture would come out mapped to a corner.
        for face in 0..6 {
            for turn in 0..8 {
                for corner in faces()[face].corners.iter() {
                    let uv = turned_uv(face_uv(face, *corner), turn);
                    for component in uv {
                        assert!(
                            component == 0.0 || component == 1.0,
                            "face {face} turn {turn} produced {uv:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_turn_still_covers_the_whole_texture_exactly_once() {
        // A rotation is a permutation of the four corners, so it has to
        // stay one: a turn that mapped two corners to the same place
        // would fold the texture over itself.
        for face in 0..6 {
            let straight = corners_of(face, 0);
            for turn in 1..4 {
                let turned = corners_of(face, turn);
                assert_eq!(turned.len(), 4);
                assert_eq!(
                    turned, straight,
                    "face {face} turn {turn} does not cover the same four corners"
                );
            }
        }
    }

    #[test]
    fn four_quarter_turns_come_back_to_where_they_started() {
        let uv = [1.0, 0.0];
        let once = turned_uv(uv, 1);
        assert_ne!(once, uv, "a quarter turn did nothing");
        assert_eq!(turned_uv(turned_uv(turned_uv(once, 1), 1), 1), uv);
        // ...and the count wraps rather than running off the end.
        assert_eq!(turned_uv(uv, 4), turned_uv(uv, 0));
        assert_eq!(turned_uv(uv, 7), turned_uv(uv, 3));
    }

    #[test]
    fn a_wall_a_player_squared_off_is_laid_all_one_way() {
        // The rule that replaced the one this module was written for.
        // Turning each face by a hash breaks up a hillside, and it
        // breaks up a wall exactly as thoroughly -- so a floor of stone
        // the player laid by hand came out as a patchwork of the same
        // texture at four different angles. What the game scatters may
        // turn; what a player builds may not.
        let out = super::transparency_tests::mesh_of(&super::transparency_tests::cache_of(
            |_, y, _| if y < 4 { BLOCK_STONE } else { BLOCK_AIR },
        ));
        let laid: std::collections::HashSet<[i32; 2]> = out
            .vertices
            .iter()
            .filter(|v| v.position[1] == 4.0)
            .map(|v| {
                let uv = v.uv();
                [uv[0] as i32, uv[1] as i32]
            })
            .collect();
        // Four corners, one orientation: exactly the four UVs of the
        // unit square, not eight or twelve of them.
        assert_eq!(laid.len(), 4, "a field of stone was laid every which way");
    }

    #[test]
    fn what_has_an_up_is_left_alone_and_so_is_what_gets_built_with() {
        // A plank turned sideways or a log lying across its own grain
        // was always worse than the repetition it hid. Building blocks
        // are now on the same list, and for the same kind of reason:
        // two of them side by side have to match.
        for id in [BLOCK_LOG, BLOCK_GRASS, BLOCK_STONE, primitive_shared::types::BLOCK_DIRT] {
            for face in 0..6 {
                assert!(!texture_turns(id, face), "{id} turned on face {face}");
            }
        }
        // What is scattered rather than built still turns: you cannot
        // build a wall out of pebbles, and a hundred of them all facing
        // the same way is a lattice.
        assert!(texture_turns(primitive_shared::types::BLOCK_PEBBLE, 0));
        assert!(texture_turns(primitive_shared::types::BLOCK_FLINT, 0));
    }
}

#[cfg(test)]
mod transparency_tests {
    use super::*;
    use crate::engine::texture::FaceLayers;
    use primitive_shared::types::{
        BLOCK_LEAVES, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME,
    };

    /// Fills a neighbourhood directly, bypassing the world, so these
    /// tests exercise the geometry split rather than chunk loading.
    pub(super) fn cache_of(fill: impl Fn(i32, i32, i32) -> BlockId) -> Neighbourhood {
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
        cache.recompute_ceiling();
        cache
    }

    pub(super) fn mesh_of(cache: &Neighbourhood) -> MeshBuffers {
        let mut out = MeshBuffers::default();
        build_mesh(ChunkPos::new(0, 0), cache, &FaceLayers::empty_for_test(), &primitive_shared::worldgen::WorldGen::new(0), &mut out);
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
            (out.indices.len() as u32) > out.sprite_end,
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
        assert_eq!(out.sprite_end, out.solid_index_count);
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
            out.sprite_end > out.solid_index_count,
            "the leaves should have their own range"
        );
        assert_eq!(
            out.sprite_end as usize,
            out.indices.len(),
            "there is no water here, so nothing follows the cutout range"
        );
    }

    #[test]
    fn the_three_ranges_are_ordered_and_cover_every_index() {
        // The renderer draws `0..solid`, `solid..sprite_end` and
        // `cutout_end..len`. If those ever stopped being ordered and
        // contiguous, triangles would be drawn twice or not at all.
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_LEAVES,
            5 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }));
        assert!(out.solid_index_count <= out.sprite_end);
        assert!(out.sprite_end as usize <= out.indices.len());
        assert!(out.solid_index_count > 0);
        assert!(out.sprite_end > out.solid_index_count, "leaves missing");
        assert!(
            (out.indices.len() as u32) > out.sprite_end,
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
            .filter(|v| v.light() & TRANSLUCENT_BIT != 0)
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
        assert_eq!(out.sprite_end % 3, 0);
        let _ = CHUNK_VOLUME;
    }

    #[test]
    fn clearing_resets_the_split_too() {
        // The buffers are pooled and reused; a stale opaque count would
        // draw the previous chunk's water as this chunk's stone.
        let mut out = mesh_of(&cache_of(|_, y, _| if y == 4 { BLOCK_WATER } else { BLOCK_AIR }));
        assert!(!out.indices.is_empty());
        out.clear();
        assert_eq!(out.solid_index_count, 0);
        assert_eq!(out.sprite_end, 0);
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
                primitive_shared::fluid::SURFACE_DROP
            } else {
                0.0
            };
            for corner in face.corners.iter() {
                out.push(Vertex::new(
                    [
                        8.0 + corner[0],
                        water_top as f32 + corner[1] - drop * corner[1],
                        8.0 + corner[2],
                    ],
                    face_uv(face_index, *corner),
                    0,
                    0,
                ));
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

    /// The highest vertex the real mesher emitted, from a real build.
    fn highest_vertex(fill: impl Fn(i32, i32, i32) -> BlockId) -> f32 {
        let mesh = super::transparency_tests::mesh_of(&super::transparency_tests::cache_of(fill));
        mesh.vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MIN, f32::max)
    }

    #[test]
    fn a_partly_filled_cell_is_drawn_at_its_own_level() {
        // The whole point of a level: what the mesher draws is what
        // `fluid::surface_height` says, so the collider and the fog --
        // which read the same function -- agree with the picture.
        use primitive_shared::types::with_layers;
        for level in 1..8u8 {
            let block = with_layers(BLOCK_WATER, level);
            let top = highest_vertex(move |_, y, _| match y {
                0..=3 => BLOCK_STONE,
                4 => block,
                _ => BLOCK_AIR,
            });
            let expected = 4.0 + primitive_shared::fluid::surface_height(block);
            assert!(
                (top - expected).abs() < 1e-4,
                "level {level} drew its surface at {top}, not {expected}"
            );
        }
    }

    #[test]
    fn a_partly_filled_cell_under_a_full_one_is_drawn_full() {
        // A half-full cell of *flowing* water under a full one is part
        // of the volume. Drawing it at its own level would open a slot
        // through the middle of a waterfall.
        use primitive_shared::types::with_layers;
        let top = highest_vertex(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => with_layers(BLOCK_WATER, 3),
            5 => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        // The surface is the top of the *upper* cell, and the lower one
        // fills its own cell completely -- which is only visible as the
        // absence of a seam, so what this really checks is that the
        // build produced the one surface and nothing above it.
        let expected = 5.0 + primitive_shared::fluid::surface_height(BLOCK_WATER);
        assert!(
            (top - expected).abs() < 1e-4,
            "the surface landed at {top}, not {expected}"
        );
    }

    /// Every vertex the mesher put in the blended range, which for these
    /// fixtures is exactly the water.
    fn water_vertices(fill: impl Fn(i32, i32, i32) -> BlockId) -> Vec<[f32; 3]> {
        let mesh = super::transparency_tests::mesh_of(&super::transparency_tests::cache_of(fill));
        let mut seen = std::collections::BTreeSet::new();
        for index in &mesh.indices[mesh.sprite_end as usize..] {
            let p = mesh.vertices[*index as usize].position;
            seen.insert((p[0].to_bits(), p[1].to_bits(), p[2].to_bits()));
        }
        seen.into_iter()
            .map(|(x, y, z)| {
                [
                    f32::from_bits(x),
                    f32::from_bits(y),
                    f32::from_bits(z),
                ]
            })
            .collect()
    }

    #[test]
    fn a_level_lake_is_one_flat_plane() {
        // The averaging must not put a ripple into water that is level
        // everywhere: every corner sees the same four cells, so every
        // corner gets the same number.
        let surface = 4.0 + primitive_shared::fluid::surface_height(BLOCK_WATER);
        for point in water_vertices(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }) {
            assert!(
                point[1] == 4.0 || (point[1] - surface).abs() < 1e-5,
                "a vertex at {} is neither on the bed nor on the surface",
                point[1]
            );
        }
    }

    #[test]
    fn every_cell_of_water_is_drawn_dead_level() {
        // **No slope, anywhere.** A cell of water is a flat lid at its
        // own depth, and two cells that hold the same amount are at the
        // same height whatever is around them -- including at a shore,
        // where half the neighbours are land.
        //
        // The surface *was* interpolated across the corners, to turn the
        // step between two depths into a ramp. It also tilted every cell
        // that had a different neighbour, which is most of them at a
        // shoreline, and a lake with a tilt in it reads as a bug however
        // gentle the tilt is.
        let surface = 4.0 + primitive_shared::fluid::surface_height(BLOCK_WATER);

        // A pool with a ragged edge, so most cells have a land
        // neighbour on at least one side.
        for point in water_vertices(|x, y, z| match y {
            0..=3 => BLOCK_STONE,
            4 => {
                if (x * 5 + z * 3) % 7 < 4 {
                    BLOCK_WATER
                } else {
                    BLOCK_STONE
                }
            }
            _ => BLOCK_AIR,
        }) {
            assert!(
                point[1] == 4.0 || (point[1] - surface).abs() < 1e-5,
                "a water vertex at {} is neither on the bed nor on the surface",
                point[1]
            );
        }
    }

    #[test]
    fn a_solid_block_is_not_lowered() {
        // The drop must apply to liquids only -- shaving stone would put
        // a visible step under the player's feet everywhere.
        let mut cache = Neighbourhood::default();
        for cell in cache.blocks.iter_mut() {
            *cell = BLOCK_STONE;
        }
        let drop = if is_liquid(cache.block(0, 5, 0)) { primitive_shared::fluid::SURFACE_DROP } else { 0.0 };
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
            primitive_shared::fluid::SURFACE_DROP
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

        // The fastest batch, not the average of all of them.
        //
        // A desktop measuring itself is interrupted constantly, and
        // interruptions only ever make a batch *slower* -- so the mean
        // drifts with whatever else the machine is doing, and two runs
        // of the same code differ by a fifth. The minimum is the batch
        // that got the fewest interruptions, which is the closest thing
        // to the cost of the code itself.
        const BATCHES: usize = 7;
        let time = |rounds: usize, f: &mut dyn FnMut()| {
            let mut best = f64::MAX;
            for _ in 0..BATCHES {
                let started = Instant::now();
                for _ in 0..rounds {
                    f();
                }
                let per_round = started.elapsed().as_secs_f64() * 1000.0 / rounds as f64;
                best = best.min(per_round);
            }
            best
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
            build_mesh(pos, &cache, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
            std::hint::black_box(&out);
        });

        let sky = World(vec![BLOCK_AIR; CHUNK_VOLUME]);
        let mut sky_light = LightMap::new();
        sky_light.load_chunk(&sky, pos);
        let mut sky_cache = Neighbourhood::default();
        sky_cache.fill(pos, &sky, &sky_light);
        let per_sky = time(ROUNDS, &mut || {
            build_mesh(pos, &sky_cache, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
            std::hint::black_box(&out);
        });

        build_mesh(pos, &cache, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
        let bytes: &[u8] = bytemuck::cast_slice(&out.vertices);
        let mut checksum: u64 = 0xcbf29ce484222325;
        for b in bytes {
            checksum ^= *b as u64;
            checksum = checksum.wrapping_mul(0x100000001b3);
        }
        println!("checksum {checksum:016x}");
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

#[cfg(test)]
mod perf_probe {
    use super::*;
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::types::ChunkPos;
    use primitive_shared::worldgen::WorldGen;

    #[test]
    #[ignore]
    fn measure_real_terrain() {
        // Several patches, far apart, because one patch is one kind of
        // country: a sample taken in an ocean says terrain is free, and
        // one taken in a mountain range says it is ruinous.
        const PATCHES: [(i32, i32); 4] = [(0, 0), (40, -25), (-60, 70), (120, 120)];
        let seed: u32 = std::env::var("PRIMITIVE_TERRAIN_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1337);
        let gen = WorldGen::new(seed);
        let r = 3;

        let (mut gen_ms, mut light_ms, mut mesh_ms) = (0.0f32, 0.0f32, 0.0f32);
        let (mut verts, mut tris, mut n, mut worst) = (0usize, 0usize, 0usize, 0usize);
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let mut cache = Neighbourhood::default();
        let mut out = MeshBuffers::default();

        for (ox, oz) in PATCHES {
            let mut chunks = ChunkManager::new(8);
            let t0 = std::time::Instant::now();
            for cx in -r..=r {
                for cz in -r..=r {
                    chunks.insert(gen.generate_chunk(ChunkPos::new(ox + cx, oz + cz)));
                }
            }
            gen_ms += t0.elapsed().as_secs_f32() * 1000.0;

            let mut light = LightMap::new();
            let t1 = std::time::Instant::now();
            for cx in -r..=r {
                for cz in -r..=r {
                    light.load_chunk(&chunks, ChunkPos::new(ox + cx, oz + cz));
                }
            }
            light_ms += t1.elapsed().as_secs_f32() * 1000.0;

            let t2 = std::time::Instant::now();
            for cx in -(r - 1)..=(r - 1) {
                for cz in -(r - 1)..=(r - 1) {
                    let pos = ChunkPos::new(ox + cx, oz + cz);
                    cache.fill(pos, &chunks, &light);
                    build_mesh(pos, &cache, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
                    verts += out.vertices.len();
                    tris += out.indices.len() / 3;
                    worst = worst.max(out.vertices.len());
                    n += 1;
                }
            }
            mesh_ms += t2.elapsed().as_secs_f32() * 1000.0;
        }

        let chunks_generated = PATCHES.len() * ((2 * r + 1) * (2 * r + 1)) as usize;
        println!(
            "seed {seed}: gen {:.2} ms/chunk | light {:.2} ms/chunk | mesh {:.2} ms/chunk |              {} verts/chunk (worst {worst}) | {} tris/chunk",
            gen_ms / chunks_generated as f32,
            light_ms / chunks_generated as f32,
            mesh_ms / n as f32,
            verts / n,
            tris / n
        );
    }
}

/// Loose material, now that it fills its cell like everything else.
///
/// What is left of the old layer suite is the half that was never about
/// depth: a block of sand has to mesh exactly like a block of stone,
/// and two of them side by side have to share their wall. The tests
/// that measured surfaces at eighths of a block went with the feature.
#[cfg(test)]
mod loose_material_tests {
    use super::plant_tests::{cache_of, mesh_of};
    use super::*;
    use primitive_shared::types::{with_layers, BLOCK_SAND, BLOCK_SNOW, BLOCK_STONE};

    /// How many quads the mesh holds. Everything the cube path emits is
    /// four vertices, so this is exact.
    fn faces_of(mesh: &MeshBuffers) -> usize {
        assert_eq!(mesh.vertices.len() % 4, 0, "something emitted a partial quad");
        mesh.vertices.len() / 4
    }

    /// A world holding nothing but the cells listed.
    fn just(blocks: &[((i32, i32, i32), BlockId)]) -> Neighbourhood {
        cache_of(|x, y, z| {
            blocks
                .iter()
                .find(|((bx, by, bz), _)| (*bx, *by, *bz) == (x, y, z))
                .map_or(BLOCK_AIR, |(_, id)| *id)
        })
    }

    #[test]
    fn asking_for_a_layer_gives_back_the_whole_block() {
        // The removal, stated as an equation. Every caller that used to
        // build a drift now builds a block, including the ones in old
        // saves -- which is what makes the change need no migration.
        for depth in 1..8u8 {
            assert_eq!(with_layers(BLOCK_SNOW, depth), BLOCK_SNOW, "depth {depth}");
            assert_eq!(block_height(with_layers(BLOCK_SAND, depth)), 1.0);
        }
    }

    #[test]
    fn a_block_of_sand_is_meshed_exactly_like_a_block_of_stone() {
        // Loose material must cost nothing to anything that is not
        // loose: face for face and corner for corner.
        let sand = mesh_of(&cache_of(|_, y, _| if y == 0 { BLOCK_SAND } else { BLOCK_AIR }));
        let stone = mesh_of(&cache_of(|_, y, _| if y == 0 { BLOCK_STONE } else { BLOCK_AIR }));
        assert_eq!(sand.vertices.len(), stone.vertices.len());
        assert_eq!(sand.indices.len(), stone.indices.len());
        // ...and standing at the same height, which is what a layer
        // used to change.
        let top = |mesh: &MeshBuffers| {
            mesh.vertices
                .iter()
                .fold(f32::MIN, |hi, v| hi.max(v.position[1]))
        };
        assert_eq!(top(&sand), top(&stone));
    }

    #[test]
    fn two_blocks_of_snow_share_no_wall() {
        // Two coplanar depth-writing quads in the same place z-fight,
        // and a snowfield made of them flickers along every seam.
        let apart = faces_of(&mesh_of(&just(&[
            ((0, 1, 0), BLOCK_SNOW),
            ((2, 1, 0), BLOCK_SNOW),
        ])));
        let touching = faces_of(&mesh_of(&just(&[
            ((0, 1, 0), BLOCK_SNOW),
            ((1, 1, 0), BLOCK_SNOW),
        ])));
        assert_eq!(apart, 12, "two lone blocks should be two closed boxes");
        assert_eq!(touching, 10, "the wall between two touching blocks was drawn");
    }
}

/// The plants: how much geometry a tuft of grass costs, and what colour
/// it comes out.
#[cfg(test)]
mod plant_tests {
    use super::*;
    use crate::engine::texture::FaceLayers;
    use primitive_shared::types::{
        BLOCK_DIRT, BLOCK_LEAVES, BLOCK_STONE, BLOCK_TALL_GRASS,
    };
    use primitive_shared::worldgen::WorldGen;

    pub(super) fn cache_of(fill: impl Fn(i32, i32, i32) -> BlockId) -> Neighbourhood {
        let mut cache = Neighbourhood::default();
        for pz in 0..PADDED_Z {
            for px in 0..PADDED_X {
                for y in 0..CHUNK_SIZE_Y {
                    cache.blocks[padded_index(px, y, pz)] =
                        fill(px as i32 - PAD, y as i32, pz as i32 - PAD);
                    cache.light[padded_index(px, y, pz)] = 0x0F;
                }
            }
        }
        cache.recompute_ceiling();
        cache
    }

    pub(super) fn mesh_of(cache: &Neighbourhood) -> MeshBuffers {
        let mut out = MeshBuffers::default();
        build_mesh(
            ChunkPos::new(0, 0),
            cache,
            &FaceLayers::empty_for_test(),
            &WorldGen::new(4321),
            &mut out,
        );
        out
    }

    /// One tuft standing on dirt in the corner of an otherwise empty
    /// chunk.
    fn one_tuft() -> Neighbourhood {
        cache_of(|x, y, z| match (x, y, z) {
            (0, 0, 0) => BLOCK_DIRT,
            (0, 1, 0) => BLOCK_TALL_GRASS,
            _ => BLOCK_AIR,
        })
    }

    #[test]
    fn a_tuft_of_grass_is_two_quads_rather_than_four() {
        // The bug this guards: the cutout pass runs with culling off, so
        // emitting both windings put two coplanar depth-writing quads in
        // the same place. They z-fought -- the shimmer on a field seen
        // from a few blocks away -- and doubled the geometry of the
        // densest thing in the world.
        let out = mesh_of(&one_tuft());
        let plant_indices = out.sprite_end - out.solid_index_count;
        assert_eq!(
            plant_indices, 12,
            "a tuft should be two quads (12 indices), not {}",
            plant_indices / 6
        );
        // ...and exactly the four corners of each, shared between its
        // two triangles.
        assert_eq!(out.vertices.len(), 8 + 5 * 4, "8 plant + 5 dirt faces");
    }

    #[test]
    fn a_tuft_stays_inside_its_own_cell() {
        // It is nudged off centre so a field is not a lattice. The
        // nudge is bounded by the inset, or a shifted tuft would poke
        // through the block beside it.
        let out = mesh_of(&one_tuft());
        // The plant's own vertices, found through the cutout range
        // rather than by height: the dirt cube's top corners sit at
        // exactly y = 1 too.
        let plant: std::collections::BTreeSet<u32> = out.indices
            [out.solid_index_count as usize..out.sprite_end as usize]
            .iter()
            .copied()
            .collect();
        assert_eq!(plant.len(), 8, "the tuft is two quads");
        for vertex in plant.iter().map(|&i| &out.vertices[i as usize]) {
            let [x, y, z] = vertex.position;
            assert!((0.0..=1.0).contains(&x), "x = {x} left the cell");
            assert!((0.0..=1.0).contains(&z), "z = {z} left the cell");
            assert!((1.0..=2.1).contains(&y), "y = {y} left the cell");
        }
    }

    #[test]
    fn the_same_cell_is_nudged_the_same_way_every_time() {
        // A plant that jumped whenever its chunk was remeshed -- which
        // is every time a block near it changes -- would be worse than
        // the grid this replaces.
        let first = mesh_of(&one_tuft());
        let second = mesh_of(&one_tuft());
        let positions = |m: &MeshBuffers| {
            m.vertices.iter().map(|v| v.position).collect::<Vec<_>>()
        };
        assert_eq!(positions(&first), positions(&second));
    }

    #[test]
    fn neighbouring_tufts_do_not_all_stand_in_the_same_place() {
        let out = mesh_of(&cache_of(|x, y, z| match y {
            0 => BLOCK_DIRT,
            1 if (0..4).contains(&x) && (0..4).contains(&z) => BLOCK_TALL_GRASS,
            _ => BLOCK_AIR,
        }));
        // Each plant's first vertex, relative to its own cell. If the
        // jitter were not there they would all be identical.
        let offsets: Vec<i32> = out
            .vertices
            .iter()
            .filter(|v| v.position[1] > 1.0 && v.position[1] < 2.0)
            .map(|v| (v.position[0].fract() * 1000.0) as i32)
            .collect();
        let distinct: std::collections::HashSet<i32> = offsets.iter().copied().collect();
        assert!(
            distinct.len() > 4,
            "sixteen tufts stand on {} distinct offsets -- the field is a lattice",
            distinct.len()
        );
    }

    #[test]
    fn a_loose_stone_is_a_single_quad() {
        // The cheapest thing the mesher emits, and it has to stay that
        // way: this is the one piece of decoration in every biome, so
        // its cost is multiplied by the whole world. A tuft is two
        // quads; a cube is six.
        use primitive_shared::types::{BLOCK_DIRT, BLOCK_PEBBLE};
        let out = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            (0, 0, 0) => BLOCK_DIRT,
            (0, 1, 0) => BLOCK_PEBBLE,
            _ => BLOCK_AIR,
        }));
        let stone = out.sprite_end - out.leaf_end;
        assert_eq!(stone, 6, "a stone should be one quad, not {}", stone / 6);
    }

    #[test]
    fn a_stone_lies_flat_just_above_the_ground() {
        // Exactly on the surface the two are coplanar and z-fight, which
        // over a field of these is a carpet of flickering.
        use primitive_shared::types::{BLOCK_DIRT, BLOCK_PEBBLE};
        let out = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            (0, 0, 0) => BLOCK_DIRT,
            (0, 1, 0) => BLOCK_PEBBLE,
            _ => BLOCK_AIR,
        }));
        let stone: Vec<&Vertex> = out.indices[out.leaf_end as usize..out.sprite_end as usize]
            .iter()
            .map(|&i| &out.vertices[i as usize])
            .collect();
        for vertex in &stone {
            assert!(vertex.position[1] > 1.0, "the stone sank into the ground");
            // Far enough off the surface that the depth buffer can tell
            // the two apart at range -- a thousandth of a block could
            // not, and the stones flickered. Not so far that it reads as
            // hovering: a fiftieth is under a pixel.
            assert!(vertex.position[1] < 1.05, "the stone is hovering");
            assert!((0.0..=1.0).contains(&vertex.position[0]));
            assert!((0.0..=1.0).contains(&vertex.position[2]));
        }
        // Flat: every corner at the same height.
        let first = stone[0].position[1];
        assert!(stone.iter().all(|v| v.position[1] == first), "the stone is tilted");
    }

    #[test]
    fn stones_do_not_all_face_the_same_way() {
        // Turned a quarter at a time by a hash of where they lie, so a
        // scattering does not read as one stone stamped over and over.
        use primitive_shared::types::{BLOCK_DIRT, BLOCK_PEBBLE};
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0 => BLOCK_DIRT,
            1 => BLOCK_PEBBLE,
            _ => BLOCK_AIR,
        }));
        // The first vertex of each stone: its UV says which way round it
        // was laid.
        let uvs: std::collections::HashSet<[i32; 2]> = out.indices
            [out.leaf_end as usize..out.sprite_end as usize]
            .chunks(6)
            .map(|quad| {
                let uv = out.vertices[quad[0] as usize].uv();
                [uv[0] as i32, uv[1] as i32]
            })
            .collect();
        assert!(uvs.len() > 1, "every stone was laid the same way round");
    }

    #[test]
    fn only_living_things_carry_a_tint() {
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0 => BLOCK_STONE,
            1 => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        }));
        let tinted = out.vertices.iter().filter(|v| v.tint() != 0).count();
        let plain = out.vertices.iter().filter(|v| v.tint() == 0).count();
        assert!(tinted > 0, "the leaves were left the colour of the texture");
        assert!(plain > 0, "the stone was tinted as if it were alive");
        // Every tinted vertex must be one of the leaves, which start
        // above y = 1.
        for vertex in out.vertices.iter().filter(|v| v.tint() != 0) {
            assert!(vertex.position[1] >= 1.0, "something below the canopy was tinted");
        }
    }

    #[test]
    fn every_face_of_a_grass_block_carries_the_climate() {
        // Including the sides. They used to be left alone, because their
        // texture is turf over dirt in one image and tinting all of it
        // turned the exposed earth savanna-yellow -- but a top that
        // changed colour while its own sides did not was worse. The
        // shader tints by how green each texel is instead, so the rule
        // here is simply "everything alive".
        use primitive_shared::types::BLOCK_GRASS;
        let out = mesh_of(&cache_of(|_, y, _| if y == 0 { BLOCK_GRASS } else { BLOCK_AIR }));
        assert!(!out.vertices.is_empty());
        for vertex in &out.vertices {
            assert_ne!(
                vertex.tint(),
                0,
                "a face of a grass block was left without a climate"
            );
        }
    }

    #[test]
    fn the_tint_follows_the_climate_rather_than_being_one_colour() {
        // Two columns far enough apart to be in different weather must
        // not come out the same, or the whole thing is an expensive way
        // to multiply by a constant.
        let world = WorldGen::new(99);
        let tint_at = |gx: i32, gz: i32| {
            let (t, h) = world.climate_column(gx, gz);
            pack_tint(cooled_by_altitude(t, 30), h)
        };
        let samples: std::collections::HashSet<u32> = (-20..20)
            .map(|step| tint_at(step * 400, step * 260))
            .collect();
        assert!(
            samples.len() > 3,
            "forty far-apart columns produced {} distinct tints",
            samples.len()
        );
    }

    #[test]
    fn a_canopy_on_a_peak_is_colder_than_the_same_canopy_at_sea_level() {
        // The lapse rate, which is what stops a mountain wearing the
        // colours of the plain it rises out of.
        let world = WorldGen::new(5);
        let (temperature, _) = world.climate_column(0, 0);
        assert!(
            cooled_by_altitude(temperature, 60) <= cooled_by_altitude(temperature, 20),
            "altitude made the world warmer"
        );
    }
}

/// Leaves and sprites are both alpha cutouts and are still kept apart.
#[cfg(test)]
mod cutout_split_tests {
    use super::*;
    use crate::engine::texture::FaceLayers;
    use primitive_shared::types::{BLOCK_DIRT, BLOCK_LEAVES, BLOCK_TALL_GRASS};
    use primitive_shared::worldgen::WorldGen;

    fn mesh_of(fill: impl Fn(i32, i32, i32) -> BlockId) -> MeshBuffers {
        let mut cache = Neighbourhood::default();
        for pz in 0..PADDED_Z {
            for px in 0..PADDED_X {
                for y in 0..CHUNK_SIZE_Y {
                    cache.blocks[padded_index(px, y, pz)] =
                        fill(px as i32 - PAD, y as i32, pz as i32 - PAD);
                    cache.light[padded_index(px, y, pz)] = 0x0F;
                }
            }
        }
        cache.recompute_ceiling();
        let mut out = MeshBuffers::default();
        build_mesh(
            ChunkPos::new(0, 0),
            &cache,
            &FaceLayers::empty_for_test(),
            &WorldGen::new(1),
            &mut out,
        );
        out
    }

    #[test]
    fn a_leaf_and_a_tuft_end_up_in_different_ranges() {
        // The renderer treats them differently at distance -- filling a
        // leaf's holes is invisible, filling a tuft's would put a green
        // square in the air -- and it can only do that if the mesher
        // has told them apart.
        let out = mesh_of(|_, y, _| match y {
            0 => BLOCK_DIRT,
            1 => BLOCK_TALL_GRASS,
            4 => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        });
        assert!(out.solid_index_count > 0, "no solid ground");
        assert!(out.leaf_end > out.solid_index_count, "the leaves went missing");
        assert!(out.sprite_end > out.leaf_end, "the grass went missing");
    }

    #[test]
    fn the_four_ranges_stay_ordered_and_cover_every_index() {
        // The renderer draws four slices back to back. If they ever
        // stopped being ordered and contiguous, triangles would be drawn
        // twice or not at all.
        let out = mesh_of(|_, y, _| match y {
            0 => BLOCK_DIRT,
            1 => BLOCK_TALL_GRASS,
            4 => BLOCK_LEAVES,
            6 => primitive_shared::types::BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        assert!(out.solid_index_count <= out.leaf_end);
        assert!(out.leaf_end <= out.sprite_end);
        assert!(out.sprite_end as usize <= out.indices.len());
        assert!(
            (out.indices.len() as u32) > out.sprite_end,
            "the water went missing"
        );
        let vertices = out.vertices.len() as u32;
        assert!(out.indices.iter().all(|i| *i < vertices));
    }

    #[test]
    fn a_chunk_of_bare_stone_pays_for_none_of_the_other_three() {
        let out = mesh_of(|_, y, _| if y < 3 { primitive_shared::types::BLOCK_STONE } else { BLOCK_AIR });
        assert!(out.solid_index_count > 0);
        assert_eq!(out.solid_index_count, out.leaf_end);
        assert_eq!(out.leaf_end, out.sprite_end);
        assert_eq!(out.sprite_end as usize, out.indices.len());
    }
}

/// **How much greedy meshing would actually buy, on this mesher.**
///
/// The received wisdom is that merging coplanar faces removes half the
/// geometry, and on a mesher whose faces carry a position and a texture
/// and nothing else it does. These faces carry per-corner ambient
/// occlusion, per-corner sky light and per-corner block light, and two
/// faces can only become one rectangle if a single set of corner values
/// describes both -- which near an edge, an overhang, a tree or a cave
/// mouth they do not.
///
/// So the number is worth having before the vertex format, the shader
/// and `build_mesh` are rewritten around it. This counts what a merge
/// would find on real generated terrain: how many faces are even the
/// right *shape* (a layer of snow is not a unit square), how many of
/// those are lit uniformly enough for one rectangle to describe them,
/// and what the merge then does with what is left.
///
/// ```text
/// cargo test --release -p primitive_client --bin primitive_client \
///     -- --ignored --nocapture greedy
/// ```
#[cfg(test)]
mod greedy_potential {
    use super::*;
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::types::ChunkPos;
    use primitive_shared::worldgen::WorldGen;
    use std::collections::{HashMap, HashSet};

    /// Everything a merged rectangle would have to agree on: which of
    /// the six directions, the plane it lies in, the texture layer, the
    /// foliage tint, and the corner lighting with the translucent flag.
    type Key = (u8, i32, u32, u32, u32);

    /// What a quad turned out to be.
    enum Shape {
        /// A unit square on the grid, lit by one value at all four
        /// corners: the only thing a merge can touch. Carries its key
        /// and where in the plane it sits, in whole cells.
        Mergeable(Key, (i32, i32)),
        /// A unit square whose corners disagree. Ambient occlusion and
        /// a light gradient both do this, and both happen exactly where
        /// the geometry is densest.
        LitPerCorner,
        /// Not a unit square at all: a layer, a slab, a tuft of grass.
        NotASquare,
    }

    fn classify(quad: &[Vertex]) -> Shape {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for vertex in quad {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(vertex.position[axis]);
                hi[axis] = hi[axis].max(vertex.position[axis]);
            }
        }
        // Flat on exactly one axis, one cell across on the other two,
        // and sitting on the integer grid. Anything else -- a sprite's
        // diagonal, a slab's shortened side -- cannot become a
        // grid-aligned rectangle however it is lit.
        let flat: Vec<usize> = (0..3).filter(|&axis| hi[axis] - lo[axis] < 1e-4).collect();
        let [normal] = flat[..] else {
            return Shape::NotASquare;
        };
        let (u, v) = other_axes(normal);
        let square = (hi[u] - lo[u] - 1.0).abs() < 1e-4 && (hi[v] - lo[v] - 1.0).abs() < 1e-4;
        let on_grid = [normal, u, v]
            .iter()
            .all(|&axis| (lo[axis] - lo[axis].round()).abs() < 1e-4);
        if !square || !on_grid {
            return Shape::NotASquare;
        }

        // Sky, block and ambient occlusion live in the bottom ten bits.
        // The face index above them is the same for all four corners of
        // one quad by construction, so it needs no comparing.
        let lighting = quad[0].light() & 0x3ff;
        if quad.iter().any(|vertex| vertex.light() & 0x3ff != lighting) {
            return Shape::LitPerCorner;
        }
        let face = ((quad[0].light() >> 10) & 7) as u8;
        let translucent = quad[0].light() & TRANSLUCENT_BIT;
        Shape::Mergeable(
            (
                face,
                lo[normal].round() as i32,
                quad[0].tex_layer(),
                quad[0].tint(),
                lighting | translucent,
            ),
            (lo[u].round() as i32, lo[v].round() as i32),
        )
    }

    /// The standard greedy pass over one plane: take the lowest cell
    /// left, run it as far as it goes along u, then grow that whole run
    /// along v while every cell of the next row is there. Returns how
    /// many rectangles it took to cover them all.
    fn merge(cells: &HashSet<(i32, i32)>) -> usize {
        let mut left = cells.clone();
        let mut order: Vec<(i32, i32)> = cells.iter().copied().collect();
        order.sort_unstable_by_key(|&(u, v)| (v, u));
        let mut rectangles = 0;
        for start in order {
            if !left.contains(&start) {
                continue;
            }
            let (u0, v0) = start;
            let mut width = 1;
            while left.contains(&(u0 + width, v0)) {
                width += 1;
            }
            let mut height = 1;
            while (0..width).all(|d| left.contains(&(u0 + d, v0 + height))) {
                height += 1;
            }
            for dv in 0..height {
                for du in 0..width {
                    left.remove(&(u0 + du, v0 + dv));
                }
            }
            rectangles += 1;
        }
        rectangles
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn greedy_meshing_potential() {
        // Real generated terrain rather than a hand-built fixture. The
        // shape is the entire question: a fixture with a deliberately
        // jagged surface has no runs to find and would report merging
        // as worthless, and a flat plain would report it as halving
        // everything. Several seeds, because a seed is a landscape.
        for seed in [1337u32, 7, 2024] {
            let generator = WorldGen::new(seed);
            let mut chunks = ChunkManager::new(4);
            let mut light = LightMap::new();
            for cx in -1..=1 {
                for cz in -1..=1 {
                    chunks.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
                }
            }
            for cx in -1..=1 {
                for cz in -1..=1 {
                    light.load_chunk(&chunks, ChunkPos::new(cx, cz));
                }
            }
            let pos = ChunkPos::new(0, 0);
            let mut cache = Neighbourhood::default();
            cache.fill(pos, &chunks, &light);
            let mut mesh = MeshBuffers::default();
            build_mesh(
                pos,
                &cache,
                &crate::engine::texture::FaceLayers::empty_for_test(),
                &generator,
                &mut mesh,
            );

            // Only the passes a merge could touch. Sprites are two
            // crossed planes standing in a cell and merge with nothing.
            let solid = &mesh.indices[..mesh.solid_index_count as usize];
            let leaves = &mesh.indices[mesh.solid_index_count as usize..mesh.leaf_end as usize];

            let mut planes: HashMap<Key, HashSet<(i32, i32)>> = HashMap::new();
            let (mut total, mut per_corner, mut not_square) = (0usize, 0usize, 0usize);
            for pass in [solid, leaves] {
                for quad in pass.chunks_exact(6) {
                    let base = *quad.iter().min().expect("six indices") as usize;
                    total += 1;
                    match classify(&mesh.vertices[base..base + 4]) {
                        Shape::Mergeable(key, cell) => {
                            planes.entry(key).or_default().insert(cell);
                        }
                        Shape::LitPerCorner => per_corner += 1,
                        Shape::NotASquare => not_square += 1,
                    }
                }
            }

            let eligible: usize = planes.values().map(HashSet::len).sum();
            let merged: usize = planes.values().map(merge).sum();
            // What the chunk would cost afterwards: the merged
            // rectangles plus every face the merge could not touch.
            let after = merged + per_corner + not_square;
            let sprites = (mesh.indices.len() - mesh.leaf_end as usize) / 6;
            let percent = |n: usize| n as f32 / total.max(1) as f32 * 100.0;

            println!("\nseed {seed}");
            println!("  faces (solid + leaves)  {total}");
            println!(
                "    lit per corner        {per_corner} ({:.0}%) -- AO or a light gradient",
                percent(per_corner)
            );
            println!(
                "    not a unit square     {not_square} ({:.0}%) -- layers, slabs",
                percent(not_square)
            );
            println!("    mergeable             {eligible} ({:.0}%)", percent(eligible));
            println!("  those merge to          {merged} rectangles");
            println!(
                "  chunk faces after       {after} ({:+.0}% overall)",
                (after as f32 / total.max(1) as f32 - 1.0) * 100.0
            );
            println!("  (plus {sprites} sprite quads, which never merge)");
        }
    }
}
