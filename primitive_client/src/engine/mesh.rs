//! Chunk meshing: face culling, ambient occlusion, and baked light.
//!
//! Faces are culled one block at a time and then the coplanar ones that
//! happen to be lit identically are covered with rectangles -- see
//! `MERGE_COPLANAR_FACES`, which is also the line that turns that half
//! off. Anything a rectangle cannot describe is still emitted a face at
//! a time, and the two paths are kept byte-identical for a run of one.
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
    block_height, is_cross, is_cutout, is_flat, is_foliage, is_liquid, is_opaque, is_partial, rest_drop,
    is_translucent, BlockId, BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};
/// Only the tests build chunks a cell at a time now. `fill` addresses
/// whole rows of the array instead, so the mesher itself no longer needs
/// `Chunk::index` -- see `Neighbourhood::fill`.
#[cfg(test)]
use primitive_shared::types::Chunk;
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
    /// bit  14      this face is a block face (see `MOTTLED_BIT`)
    /// bit  15      the layer's ninth bit (see `LAYER_HIGH_SHIFT`)
    /// bits 16..23  texture layer, low eight bits; the tenth and eleventh
    ///              are in `uv` (see `LAYER_TOP_SHIFT`)
    /// bits 24..31  foliage tint, 0 = none (see `pack_tint`); 226 and up
    ///              once meant a texture crop and mean nothing now (see
    ///              `FINE_UV_BIT`, which took its place in `uv`)
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
    /// **Why the UV is no longer in here.** It was two bits, because a
    /// block face was always one cell and `face_uv` returned nothing but
    /// zeroes and ones. Merging coplanar faces (see
    /// `MERGE_COPLANAR_FACES`) breaks that: a rectangle sixteen cells
    /// wide has to tile its texture sixteen times, so the coordinate
    /// needs to count cells rather than name a corner. There was nowhere
    /// in this word to put the other eight bits -- every one of the
    /// thirty-two is spoken for -- so the coordinate moved out to
    /// `uv` and the vertex grew by four bytes.
    ///
    /// The two bits it left are deliberately not reclaimed. Repacking
    /// would move `layer` and `tint`, and every shift in this word is
    /// also written down in `shader.wgsl`; two spare bits are cheaper
    /// than two files that have to agree about five new constants.
    pub packed: u32,
    /// The texture coordinate, counted in **cells rather than corners**.
    ///
    /// ```text
    /// bits 0..4    u, 0..=31
    /// bits 5..9    v, 0..=31
    /// bits 10..28  spare (a fine coordinate uses 0..27, see `FINE_UV_BIT`)
    /// bits 29..30  the layer's tenth and eleventh bits (`LAYER_TOP_SHIFT`)
    /// bit  31      `FINE_UV_BIT`
    /// ```
    ///
    /// A single face still spans 0..1 and reads exactly as it always
    /// did; a merged rectangle spans 0..w, and the sampler tiles it --
    /// which is the whole of why `texture::BLOCK_ADDRESS_MODE` is
    /// `Repeat` and not the clamp it used to be.
    ///
    /// **Five bits is a ceiling and not a description**, which is what
    /// `MAX_RUN` exists to keep honest. A chunk is sixteen cells across
    /// but *sixty-four tall*, and the two vertical face directions grow
    /// their rectangles along y: a shaft cut through bedrock offers a
    /// run of sixty cells, whose far edge is the number 60 and does not
    /// fit. What that looked like was the texture on a deep wall tiling
    /// thirty-one times over sixty blocks -- stretched, and out of step
    /// with the unmerged faces beside it.
    ///
    /// **This is what cost four bytes a vertex**, and the trade was made
    /// on a measurement: the solid pass is bound by chunks and triangles,
    /// not by bandwidth. Doubling the resolution from 1600x900 to
    /// 1920x1080 moved it by 0.16 ms while going from 245 to 521 drawn
    /// chunks moved it by a full millisecond -- so paying 25% more
    /// vertex bandwidth to remove a third of the triangles is a trade in
    /// the direction the hardware cares about. The alternative, packing
    /// the position into one word to stay at sixteen bytes, would have
    /// put the plant jitter and the model boxes' fractional coordinates
    /// into a fixed-point cage to buy back four bytes that nothing is
    /// short of.
    pub uv: u32,
}

/// Set on vertices the fragment shader should give a see-through alpha.
/// Must match `TRANSLUCENT_BIT` in shader.wgsl.
pub const TRANSLUCENT_BIT: u32 = 1 << 13;

/// **Set on the faces of blocks, and on nothing else.**
///
/// The shader gives every block its own faint shade so that a wall of
/// one texture is not a hundred identical copies of it (see
/// `BLOCK_VARIATION` in shader.wgsl). That is a fact about a *cell of
/// the world*, and this bit is how the fragment shader is told that
/// asking which cell it is standing on will get a sensible answer.
///
/// A boar's flank, a dropped axe and a sheet of rain are one object
/// each; hashing the cell they happen to be crossing paints a hard
/// edge across them that slides as they move. They are emitted without
/// this bit and come out unmottled, which is the same reasoning
/// `vs_item` already carried in prose.
///
/// Must match `MOTTLED_BIT` in shader.wgsl.
pub const MOTTLED_BIT: u32 = 1 << 14;

/// Everything the `light` argument of `Vertex::tinted` is allowed to
/// carry: the light word and the flag bits that ride above it.
///
/// It used to be `LIGHT_MASK` alone, and that is exactly the sort of
/// mistake this file is written to prevent -- a bit set at every call
/// site and then quietly masked off in the constructor. Nothing reports
/// it; the world simply stops being mottled and the picture looks
/// almost right.
const PACKED_LIGHT_MASK: u32 = LIGHT_MASK | MOTTLED_BIT;

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
/// Where the ninth bit of the layer sits, on its own below the other
/// eight.
///
/// **Eight bits was the smaller half of a ceiling with two halves**, and
/// this is the half that was ours. The other was `Limits::default()`
/// capping a texture array at 256 layers on every machine -- a
/// portability floor, not a card's refusal, and lifted the same way the
/// buffer size already is (see `renderer::terrain_limits`). With that
/// gone the vertex was the only thing left saying 256, and the atlas was
/// full to the layer: 241 pictures a block face can name plus fifteen of
/// font, in an array of exactly 256.
///
/// A ninth bit doubles what a face can name and costs nothing: bit 15
/// was empty -- the texture coordinate used to live there and moved out
/// to a word of its own -- so the vertex neither grew nor moved a field
/// anything else reads.
///
/// Why not sixteen bits, which is what the *item* vertex carries: the
/// tint byte sits at 24 and the light word runs to 14, so bits 16..23
/// are all the room there is between them. Nine is what fits without
/// making the vertex bigger, and a vertex is multiplied by a million in
/// a loaded world.
const LAYER_HIGH_SHIFT: u32 = 15;
/// Where the layer's tenth and eleventh bits sit -- in `Vertex::uv`, not
/// in `packed`.
///
/// **Nine bits was 512, and the atlas reached 522.** `packed` has no bit
/// left at all, so the room came from the coordinate word, which had one
/// bit neither of its two readings used (30) and one a fine coordinate
/// could give up: fifteen bits an axis counted to 127 pictures, and no
/// model face or part-height side is wider than `MAX_RUN`, thirty-one.
/// Fourteen bits count to 63, and bit 28 is left spare beside these two.
///
/// Eleven bits is 2048, which is what Vulkan and D3D12 put in one array
/// and what eight arrays of GLES's 256 hold (`texture::AtlasSplit`) -- so
/// the vertex, the desktop array and the phone's split all end at the
/// same number instead of three that happen to be near each other.
///
/// Rejected: a byte more a vertex. It is multiplied by a million in a
/// loaded world (see `Vertex::packed`) to carry two bits the word already
/// had spare.
pub(crate) const LAYER_TOP_SHIFT: u32 = 29;
/// The two bits at `LAYER_TOP_SHIFT`, as a mask of the `uv` word.
const LAYER_TOP_MASK: u32 = 0b11 << LAYER_TOP_SHIFT;
/// How many layers the vertex can address: eleven bits' worth.
///
/// It was 256, and it was the number two different ceilings happened to
/// share -- the eight bits here and `Limits::default()`'s cap on a
/// texture array. Sharing a number made them look like one ceiling, and
/// the atlas hit it in 1.5 with no room left at all: 241 pictures a
/// block face can name plus fifteen of packed font, in an array of
/// exactly 256. Both were lifted at once, because lifting either alone
/// buys nothing -- see `LAYER_HIGH_SHIFT` for the vertex and
/// `renderer::terrain_limits` for the array.
///
/// It was then 512, nine bits, and the atlas reached that too; it is
/// eleven bits now (see `LAYER_TOP_SHIFT`), and what a device cannot put
/// in one array it puts in several (`texture::AtlasSplit`).
///
/// This is what a *vertex* can say, and it stays a number in this file so
/// that the mesher, the shader and the atlas loader all read the same one.
///
/// `TextureManager::load` refuses to start with more layers than this,
/// so the truncation cannot happen silently.
pub const MAX_TEXTURE_LAYERS: u32 = 2048;
/// Where the foliage tint sits. See `pack_tint`.
const TINT_SHIFT: u32 = 24;
/// Everything below this is the light word. The bit above it is the
/// block-face flag, and the one above that is the layer's ninth --
/// see `Vertex::packed`.
const LIGHT_MASK: u32 = (1 << 14) - 1;

/// How far a model's box is grown past its stated size, in sixteenths.
/// See the note in `push_box`.
pub(crate) const BITE: f32 = 0.02;

/// Where `v` sits in `Vertex::uv`; `u` is below it.
const V_SHIFT: u32 = 5;
/// How much of one coordinate fits: 0..=31. Must match `UV_MASK` in
/// shader.wgsl.
const UV_MASK: u32 = 31;

/// Set in `Vertex::uv` when the coordinate is a place in the picture
/// rather than a count of cells. Must match `FINE_UV_BIT` in shader.wgsl.
///
/// **This replaced the crop codes, and the reason was a bed.** A model's
/// small faces used to say how big they were in the tint byte -- one of
/// five sizes a side, 1, 2, 4, 8 or 16 sixteenths -- and the shader showed
/// that corner of the picture. Five sizes was what fitted in the byte, so
/// it was a snap: a bed three eighths tall has sides six sixteenths high,
/// nearer eight than four, so every bed wore eight rows of board squeezed
/// into six, and a table's twelve-sixteenth sides wore all sixteen. The
/// player asked for textures that are never stretched or squeezed, only
/// cut, and a snap cannot say that.
///
/// The `uv` word had twenty-two bits nobody used. A vertex with this bit
/// set carries `u` in the fourteen bits below `FINE_V_SHIFT` and `v` in the
/// fourteen above them, in 256ths of a picture -- a sixteenth of a texel on the sixteen
/// texel grid everything is drawn on -- so a face of any size wears
/// exactly the piece of the picture under it, starting wherever it
/// starts. That is the other thing a crop could not do: every crop was the
/// picture's top-left corner, which is why two plank textures and the
/// stripped log were once ruled out for the drying rack.
///
/// Rejected: a bigger crop table in the tint byte. Eight sizes a side is
/// sixty-four codes where thirty were free, and it would still have been a
/// snap and still always the corner.
pub const FINE_UV_BIT: u32 = 1 << 31;

/// "This face is fresh-cut stone": the one face of a part-dug block that the
/// pick has opened (`dig::cut_face`). Must match `CHIPPED_BIT` in
/// shader.wgsl, which darkens the grooves and lifts the chipped edges of
/// [`CHIP_MARKS`] over whatever picture the face already wears.
///
/// **Bit 28 of the `uv` word, the one the fine coordinate left spare**, and
/// only ever set beside `FINE_UV_BIT`: a bite's faces are always cropped, so
/// the two readings of the word cannot meet on a chipped face.
///
/// **Why a mark laid over the rock's own picture, and not the two other
/// ways a cut face could look different:**
///
/// * **A second picture per material** -- a "worked" granite, a "worked"
///   loam. Sixty-odd kinds dig in quarters, and a darker copy of each is
///   sixty layers in an atlas the phones already split across arrays
///   (`texture::AtlasSplit`), for a face that exists for the eight seconds
///   a block takes to dig. Rejected on the budget.
/// * **An overlay quad** with a cut-out chip picture, like the cracks. The
///   cut-out bucket is the one the far leaves are drawn *solid* in
///   (`MeshBuffers::leaves_solid`), so past the see-through line every
///   chipped face would have turned into a square of the overlay's hole
///   colour; and it is a second quad a hair in front of the first, which is
///   the z-fight the whole mesh is written to avoid.
///
/// Chosen: **one bit and a sixteen-by-sixteen mask in the shader**, which
/// keeps each rock's colour (a cut in granite is granite) and costs a branch
/// on fragments that nearly never take it.
pub const CHIPPED_BIT: u32 = 1 << 28;

/// **The chip picture**, drawn on the texel grid every picture in the game is
/// drawn on: the strokes of a pick down the face, each a dark groove with a
/// pale fresh edge on its lit side, and a few pits where a point went in.
/// `#` is a groove, `+` a fresh edge, `o` a pit and `.` the stone as it was.
///
/// The shader carries the same picture as sixteen numbers (`CHIP_ROWS`, two
/// bits a texel); `the_chip_picture_in_the_shader_is_the_one_drawn_here`
/// holds the two together, so this drawing is the one to edit -- and then
/// the numbers in the shader, which the test prints the right ones for.
/// Compiled for the tests only: nothing at run time reads it, the shader
/// does.
#[cfg(test)]
pub const CHIP_MARKS: [&str; 16] = [
    "...+.........+..",
    "..+#....o...+#..",
    "...#+.......#+..",
    "....#........#..",
    "....+#.......+#.",
    ".o...#...+....#.",
    ".....#+.+#....+.",
    "......#.#.......",
    "..+...+#+...o...",
    ".+#....#........",
    "..#+.........+..",
    "...#.....o..+#..",
    "...+#.......#+..",
    "....#...+...#...",
    "....#+.+#...+...",
    ".....#.#........",
];

/// [`CHIP_MARKS`] as the shader reads it: one number a row, two bits a texel
/// from the left, 0 for untouched, 1 for a groove, 2 for a fresh edge and 3
/// for a pit.
#[cfg(test)]
pub fn chip_rows() -> [u32; 16] {
    std::array::from_fn(|row| {
        CHIP_MARKS[row].bytes().enumerate().fold(0u32, |word, (column, texel)| {
            let code = match texel {
                b'#' => 1,
                b'+' => 2,
                b'o' => 3,
                _ => 0,
            };
            word | (code << (column * 2))
        })
    })
}
/// Where `v` starts in a fine coordinate; `u` is the fourteen bits below.
/// Must match `FINE_V_SHIFT` in shader.wgsl.
///
/// It was fifteen, and gave a bit to the texture layer (`LAYER_TOP_SHIFT`).
const FINE_V_SHIFT: u32 = 14;
/// One fine coordinate: 0..=16383 256ths, which is 63 pictures -- more
/// than any model face will ever be wide, and twice `MAX_RUN`, the widest
/// a part-height side merges to.
const FINE_MASK: u32 = (1 << FINE_V_SHIFT) - 1;
/// Steps of a fine coordinate per picture. Must match `FINE_UNITS` in
/// shader.wgsl.
pub(crate) const FINE_UNITS: f32 = 256.0;

/// The longest run a merged rectangle may cover, in cells.
///
/// The far corner of a rectangle carries its own length as a texture
/// coordinate, and `UV_MASK` is the largest number that fits -- so this
/// is not a tuning knob, it is the field's size said out loud. A longer
/// run does not fail: it is silently clamped by `Vertex::tinted` and the
/// picture on it stops lining up with the blocks it covers, which is
/// exactly the sort of fault nothing reports.
///
/// Sixteen cells is the most a horizontal plane can offer anyway; only
/// the four sideways face directions, which grow along the chunk's
/// sixty-four blocks of height, ever meet this.
const MAX_RUN: usize = UV_MASK as usize;

// **Merged rectangles are not grown past their edges, and the attempt
// to grow them is written down here because it very nearly worked.**
//
// Greedy merging makes T-junctions by construction: a rectangle four
// cells wide meets, along one edge, two rectangles two cells wide, and
// the point where those two meet lies in the middle of the long one's
// edge without being a corner of it. The two edges are one line in exact
// arithmetic and two lines in `f32`, and where they differ the
// rasteriser's fill rule can award the pixel to neither triangle. What
// comes through the hole is the sky -- the face under a lawn is culled,
// so there is nothing else behind it. Single white dots at block joins,
// coming and going as the camera turns.
//
// The cure was to let neighbours overlap instead of meeting: three bits
// here saying which corner of the rectangle a vertex is, and a
// three-quarter-pixel growth applied after the projection in
// `shader.wgsl`, with the texture coordinate growing to match. It closed
// the holes -- 12 leaked pixels to 0 in `the_meadow_that_leaks_sky`.
//
// **And it drew a dark line along every rectangle boundary.** A grown
// quad carries its coordinate past the edge of its own tile, and the
// sampler's `AddressMode::Repeat` -- which is what tiles a picture
// across a merged rectangle in the first place -- wraps that to the
// *opposite* edge of the picture. The sliver of overlap is therefore
// painted with the wrong part of the texture. On ice, a smooth bright
// material, that is a grid of dark lines across a frozen lake; it went
// unnoticed at first only because the per-block shade variation was
// laying a chequerboard over the same surface and hiding it (see
// `types::is_one_sheet`, which is the other half of that report).
//
// There is no cheap version of the cure that does not do this. Clamping
// the coordinate inside the tile stretches the picture instead, which is
// the fault `a_block_wears_its_own_shade_however_many_blocks_the_quad_
// covers` catches; growing the position without the coordinate stretches
// it too. The textbook cure -- splitting the long rectangle at every
// point a neighbour's corner touches it -- undoes most of what merging
// is for, and merging is worth 10% to 61% of the quads in a chunk.
//
// So the dots stay, and they are rare: eleven in thirty frames of a
// sweep, every one of them within ten pixels of the horizon. A line
// across every sheet of ice is worse than that.

/// **Whether coplanar faces are merged into rectangles.**
///
/// The one line to turn off. Set it to `false` and every face is emitted
/// on its own again, exactly as before merging existed: the emit path
/// for a single face was kept rather than expressed as a merge of one,
/// so "off" is genuinely the old behaviour and not a degenerate case of
/// the new one. A merged mesh is a large change to the thing every frame
/// draws, and the way to answer "is the merge doing this?" a fortnight
/// from now should be a rebuild, not a revert.
///
/// Off costs nothing at the other end: an unmerged face spans one cell,
/// so its coordinate is a zero or a one and the shader's five-bit read
/// returns what the two-bit read used to.
// **On, and it cracks the terrain. Both halves of that are measured,
// and the second half is why the first one cannot simply be flipped.**
//
// The player's report is white specks on the edges of blocks, appearing
// and vanishing as the camera turns. Three attempts to photograph it
// failed and every one failed the same way: they asked "what is not
// terrain", and most of what is not terrain is *supposed* not to be.
// An offscreen repro counted the sky. A camera sweep counted gaps in a
// leaf canopy, and the text of the debug panel. A magenta backdrop on
// stepped ground counted the background around the staircase.
//
// What worked is `FrameParams::speck_hunt`: the game itself, every
// terrain fragment forced to flat black, the alpha cut-out switched off
// so a canopy is a solid silhouette, water opaque, and no sky, hand,
// particles or interface drawn at all, on a white clear. A white pixel
// is then a hole and there is nothing else it can be. Sixty frames a
// fifth of a degree apart, looking down at plain ground:
//
// * merged, uncut:  **15 white pixels**, in 13 of the 60 frames;
// * cut:            **0**.
//
// That was looking down, where the fault is rare. Looking *along* the
// ground -- which is how a player looks -- is where the count lives.
// Painted by face direction, one seat, thirty frames, the near field,
// counted against the same sweep with the merge switched off so that a
// ledge seen edge-on is not blamed on the merge: cracks that exist
// *only* with the merge on --
//
// * no cutting:                                     34
// * cut against the rectangles of one plane:        26
// * cut against the rectangles of the whole chunk:  27
// * ...and a grid of every fourth cell on the seam: 27
// * ...and the neighbour's block and light changes: 27
// * cut against the unevenly lit faces as well:     **0**
//
// The last line is `direct_marks` in `build_mesh`. Every crease, step
// and wall-foot is a face lit unevenly at its corners, drawn as itself
// and never in the rectangle list; its corners were the ones landing on
// the long rectangles beside it. Everything about the chunk seam was
// measured, changed nothing, and is written up at `mark_corners`.
//
// What is left -- eighteen pixels in those thirty frames -- is there
// with the merge off too, at the same pixels: geometry a pixel wide
// seen edge-on, the floor of any voxel picture drawn without
// anti-aliasing. Not a crack.
//
// **The cost, from one binary in one sitting**, at a render distance of
// twenty-four. Closing the junctions by *cutting* rectangles into
// smaller ones cost about a fifth of the frame: 306 fps against
// 336-390, the GPU at 2.38 ms against 1.90-1.98, because every cut was
// two more triangles and put a new corner on the far edge for the next
// rectangle to be cut by. Closing them by *inserting the vertex into
// the edge* (`triangulate_edged_rect`) costs a third of that: on the
// same seat, 637 000 triangles against 538 000 with the junctions left
// open and 750 000 with the cutting, the solid pass at 0.84 ms against
// 0.76 and 0.98, and the same eighteen near-field pixels as the
// no-merge control -- none of them the merge's. The terrain arena is
// 167 MB either way. On the three benchmark seeds the merge keeps 3%,
// 38% and 0% of what it could save: natural terrain is mostly crease,
// and a crease cannot be one rectangle without a crack. A flat roof is
// still one quad, and `every_face_the_world_shows_is_drawn_exactly_once`
// says no face is lost on the way.
//
// **Why the switch cannot simply be turned off instead:** it was tried,
// and the terrain arena asked the driver for 335544320 bytes against a
// maximum buffer size of 268435456 --
//
// ```text
// In Device::create_buffer, label = `terrain vertex arena`
// Buffer size 335544320 is greater than the maximum buffer size (268435456)
// ```
//
// -- and the game did not start. The merge is what keeps a chunk's
// geometry inside one buffer at all.
//
// The cure that was tried and backed out -- growing quads by a hair in
// screen space -- is written up at `Vertex::tinted`: it stretched the
// texture, and past a tile edge `AddressMode::Repeat` wrapped and drew
// a dark line along every merged rectangle.
pub const MERGE_COPLANAR_FACES: bool = true;

/// Steps per climate axis in a packed tint. See `pack_tint`.
const TINT_LEVELS: u32 = 15;

/// Where the tint byte stops meaning a climate and starts meaning what is
/// *on* a surface: soot, three stages of it, at `SURFACE_TINT_BASE + 1..=3`,
/// ash dug into a furrow at `SURFACE_TINT_BASE + 4`, and a board's four
/// stages of weather at `SURFACE_TINT_BASE + 5..=8`.
///
/// **The byte and not a new vertex field**, because the byte had room: a
/// climate takes codes 1..=225, and 226 and up have meant nothing since the
/// texture crops moved to `FINE_UV_BIT`. And **not a picture per stage**:
/// three stages of soot on six materials is eighteen layers of an atlas that
/// counts every one (`texture::terrain_limits`), for what is one colour laid
/// over the picture the block already has. The shader applies a surface
/// tint to the whole texel, where a climate tint is weighed by how green the
/// texel is -- soot is on the stone as much as on the moss.
pub const SURFACE_TINT_BASE: u32 = 226;

/// The surface tint of moss on a stone or a trunk: `SURFACE_TINT_BASE + 9`,
/// the next code after a board's weather. **A tint, not a picture per mossy
/// block**, for soot's reason: moss is a green laid over the stone the block
/// already is, and the stone showing through is what says which rock it is.
pub const MOSS_TINT: u32 = SURFACE_TINT_BASE + 9;

/// The surface tint a block wears, or `None`: soot on a ceiling over a
/// hearth (`wildfire::soot`), ash dug into tilled earth
/// (`wildfire::is_dressed`), or a board's years in the rain
/// (`weathering::weathering`) at `SURFACE_TINT_BASE + 5..=8`.
///
/// **Weather is a tint for soot's reason**: four stages on eight boards is
/// thirty-two layers of an atlas for what is one colour laid over the
/// board's own grain -- and the grain showing through the grey is what
/// says it is still the same board.
#[inline]
pub fn surface_tint(id: BlockId) -> Option<u32> {
    let soot = primitive_shared::wildfire::soot(id);
    if soot > 0 {
        return Some(SURFACE_TINT_BASE + u32::from(soot));
    }
    let weather = primitive_shared::weathering::weathering(id);
    if weather > 0 {
        return Some(SURFACE_TINT_BASE + 4 + u32::from(weather));
    }
    // A tired furrow (`wildfire::is_tired`) is pale and dry: the colour a
    // cropped-out field goes, and the only way a player can tell which
    // furrows want a rest without hoeing them to find out.
    if primitive_shared::wildfire::is_tired(id) {
        return Some(TIRED_FURROW_TINT);
    }
    primitive_shared::wildfire::is_dressed(id).then_some(SURFACE_TINT_BASE + 4)
}

/// The surface tint of a cropped-out furrow: `SURFACE_TINT_BASE + 10`, the
/// next code after moss. The shader's arm for it comes before the weather's,
/// which would otherwise take every code from five up.
pub const TIRED_FURROW_TINT: u32 = SURFACE_TINT_BASE + 10;
// The code travels in a byte of the vertex; one past it would draw as a
// climate tint on some other block.
const _: () = assert!(TIRED_FURROW_TINT <= 255);

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

// **The tint byte used to carry texture crops above 225**, a five-by-five
// grid of power-of-two sizes that a small face showed the top-left corner
// of its picture by. It could not say six sixteenths, so a bed's sides
// wore eight rows squeezed into six, and it could only ever take a
// picture's corner. A small face carries a real place in the picture now
// (`FINE_UV_BIT`), and the byte is a foliage tint and nothing else. What
// that crop taught still stands in `with_fine_uv`'s callers: a material is
// cut and a picture with a feature in it is not (`animal_model::Skin::tiles`).

impl Vertex {
    /// Location 2 is the per-draw chunk offset and belongs to the
    /// instance buffer, which is why the coordinate is location 3 rather
    /// than filling the gap. Renumbering would have meant editing every
    /// shader that reads the instance attribute for no gain.
    pub const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Uint32,
        3 => Uint32,
    ];

    /// Builds a vertex from the four things the mesher actually knows.
    ///
    /// `uv` components are whole numbers of cells: 0.0 or 1.0 for a
    /// single face, up to `MAX_RUN` for a merged rectangle. Anything
    /// between is rounded, because a coordinate that is not a cell
    /// boundary cannot be expressed -- and nothing produces one, since
    /// every quad this format carries is mapped corner to corner.
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
        // The clamp is the last line of defence and is meant never to
        // fire: silently shortening a coordinate leaves a quad whose
        // picture no longer covers the blocks it is drawn over, and
        // nothing downstream can tell. `MAX_RUN` keeps the mesher on the
        // right side of it; the assertion is what says so in a test run.
        debug_assert!(
            uv[0] <= UV_MASK as f32 && uv[1] <= UV_MASK as f32,
            "a quad asked for {uv:?} cells of texture, which does not fit in five bits"
        );
        debug_assert!(
            tex_layer < MAX_TEXTURE_LAYERS,
            "layer {tex_layer} does not fit in the vertex's eleven bits"
        );
        let u = (uv[0].round().max(0.0) as u32).min(UV_MASK);
        let v = (uv[1].round().max(0.0) as u32).min(UV_MASK);
        Self {
            position,
            packed: (light & PACKED_LIGHT_MASK)
                | ((tex_layer & 0xFF) << LAYER_SHIFT)
                | (((tex_layer >> 8) & 1) << LAYER_HIGH_SHIFT)
                | ((tint & 0xFF) << TINT_SHIFT),
            uv: u | (v << V_SHIFT) | (((tex_layer >> 9) & 0b11) << LAYER_TOP_SHIFT),
        }
    }

    /// The same vertex, wearing a place in the picture instead of a count
    /// of cells: `uv` in pictures, so 0.5 is half way across.
    ///
    /// For every face that is not a whole cell -- a model's box, the side of
    /// a part-height block -- so that it shows the piece of the picture
    /// under it at one texel to a sixteenth. See `FINE_UV_BIT`. Below zero
    /// is clamped: nothing asks for it but a box grown past its cell by
    /// `BITE`, a three-hundredth of a texel.
    pub fn with_fine_uv(mut self, uv: [f32; 2]) -> Self {
        let fine = |c: f32| ((c * FINE_UNITS).round().max(0.0) as u32).min(FINE_MASK);
        // The layer's top bits share this word and are kept: overwriting it
        // whole would draw layer 1500 as layer 476 on every model face.
        self.uv = (self.uv & LAYER_TOP_MASK) | FINE_UV_BIT | fine(uv[0]) | (fine(uv[1]) << FINE_V_SHIFT);
        self
    }

    /// Marks a cropped face as the cut face of a part-dug block. See
    /// [`CHIPPED_BIT`]; after `with_fine_uv`, which writes the whole word.
    pub fn chipped(mut self) -> Self {
        debug_assert!(self.uv & FINE_UV_BIT != 0, "a chip mark on a face with no fine coordinate");
        self.uv |= CHIPPED_BIT;
        self
    }

    /// The decoders, mirroring what the shader does. Used by the tests
    /// that check the packing round-trips -- a silent mismatch here
    /// shows up as the whole world wearing the wrong textures.
    #[allow(dead_code)]
    pub fn uv(&self) -> [f32; 2] {
        if self.uv & FINE_UV_BIT != 0 {
            return [
                (self.uv & FINE_MASK) as f32 / FINE_UNITS,
                ((self.uv >> FINE_V_SHIFT) & FINE_MASK) as f32 / FINE_UNITS,
            ];
        }
        [
            (self.uv & UV_MASK) as f32,
            ((self.uv >> V_SHIFT) & UV_MASK) as f32,
        ]
    }

    #[allow(dead_code)]
    pub fn tex_layer(&self) -> u32 {
        ((self.packed >> LAYER_SHIFT) & 0xFF)
            | (((self.packed >> LAYER_HIGH_SHIFT) & 1) << 8)
            | (((self.uv >> LAYER_TOP_SHIFT) & 0b11) << 9)
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

    /// Where the chunk this vertex belongs to actually is, as one value
    /// per draw rather than one per vertex.
    ///
    /// **This is what makes a vertex position a small number.** A vertex
    /// used to carry its absolute place in the world, and an `f32` a
    /// million blocks from the origin has about six centimetres between
    /// one representable value and the next -- so out where players
    /// actually go the mesh itself was quantised, block faces no longer
    /// met, and the whole world shimmered as the camera moved. Now a
    /// vertex says where it is *inside its chunk*, which is a number
    /// between -1 and 17, and this says where the chunk is relative to
    /// the camera. Both stay small, and small is exact.
    ///
    /// An instance attribute rather than a uniform because the terrain
    /// is already one `draw_indexed` per chunk out of shared buffers:
    /// the draw call selects the instance, so the offset costs one
    /// buffer bind for the whole world and no extra state per chunk.
    pub const INSTANCE_ATTRS: [wgpu::VertexAttribute; 1] =
        wgpu::vertex_attr_array![2 => Float32x4];

    pub fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::INSTANCE_ATTRS,
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

/// Which way each of the six faces of `faces()` looks: the axis and the
/// sign of its outward normal, in the same order. A face is a back face
/// from any eye on the other side of its plane along that axis, which is
/// all the renderer needs to leave a whole group of them out -- see
/// `renderer::solid_ranges_facing`. Kept as a table rather than read
/// out of `faces()` because it is consulted for every chunk of every
/// frame, and a test holds the two together.
pub const FACE_OUTWARD: [(usize, i32); 6] = [(1, 1), (1, -1), (0, 1), (0, -1), (2, 1), (2, -1)];

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

/// Which axis of the cube each half of `face_uv`'s answer reads, and
/// whether it reads it backwards.
///
/// `face_uv` is written as six literal expressions because that is the
/// clearest way to say what a block face's texture does. A *merged*
/// rectangle needs the same six answers scaled -- tile `w` times along
/// one axis and `h` along the other -- and scaling a literal is not
/// something a literal supports. This is the same table said in a form
/// arithmetic can use, and `face_uv_agrees_with_its_own_axis_table`
/// holds the two to each other.
const UV_SOURCE: [[(usize, bool); 2]; 6] = [
    [(0, false), (2, false)], // 0 +Y: [x, z]
    [(0, false), (2, true)],  // 1 -Y: [x, 1 - z]
    [(2, true), (1, true)],   // 2 +X: [1 - z, 1 - y]
    [(2, false), (1, true)],  // 3 -X: [z, 1 - y]
    [(0, false), (1, true)],  // 4 +Z: [x, 1 - y]
    [(0, true), (1, true)],   // 5 -Z: [1 - x, 1 - y]
];

/// `face_uv` for a quad that spans more than one cell.
///
/// `extent` is how many cells the quad covers along each axis; the one
/// along the face normal is never read, because a face's texture never
/// depends on the direction it faces. With every extent at one this
/// returns exactly what `face_uv` returns, which is the property the
/// tests pin.
#[inline]
fn spanning_uv(face_index: usize, corner: [f32; 3], extent: [f32; 3]) -> [f32; 2] {
    UV_SOURCE[face_index].map(|(axis, flipped)| {
        let along = if flipped { 1.0 - corner[axis] } else { corner[axis] };
        along * extent[axis]
    })
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
    // Whether this chunk's crowns are shells: a chunk `lod::coarsen`
    // rewrote, or one past the see-through canopy line
    // (`lod::leaves_see_through_at`). See the canopy rule below, which is
    // the one answer that differs at range.
    shell_crowns: bool,
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
    if face_index == 0 && primitive_shared::types::hides_the_floor(neighbor) {
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
    // **And so is a liquid's**, for exactly the same reason and by a
    // different route: water stops `fluid::SURFACE_DROP` short of the
    // top of its cell, but its *cover* is zero -- it is something you
    // see through -- so the rule above never reaches it and the cover
    // test below asked for a full block's worth of hiding.
    //
    // A full neighbour above gives that, and the answer was wrong: the
    // neighbour's underside is at 1.0 and the water's surface is at
    // 0.88, so culling the water's top face left a twelfth of a block
    // with nothing drawn in it at all. Both faces that could have
    // closed it were culled, each correctly on its own terms. What a
    // player saw was a bright slot round every block they set into a
    // lake -- water is something you build through, so this is a
    // placement the game invites -- and through the slot, the world
    // behind the water.
    //
    // Costs nothing anywhere else: air and water above are already
    // "does not hide", so this only ever fires under a solid lid,
    // which is a cell a natural world barely has.
    //
    // **Except under ice, and against ice on every side.** Ice is cutout
    // and writes depth; water is blended and does not. A water face
    // against ice lies in the very plane the ice's own face does, so the
    // pair z-fought -- the water's pane shimmering on and off the ice at
    // every edge of a frozen bay -- and the top face under the lid sat a
    // `SURFACE_DROP` below the ice, a false surface hiding its underside
    // with a slot of nothing round the edge. So water draws no face into
    // ice at all: the ice draws the boundary, once, from both sides (the
    // cutout pass does not cull backs), and the water under it is drawn
    // up to the ice rather than short of it (`fluid::is_lid`).
    //
    // Rejected: keeping the water's faces and nudging them off the ice's
    // plane. The shimmer goes, the doubled pane stays -- blue glass laid
    // over the ice from below, which is the thing the player saw.
    if is_liquid(current) && primitive_shared::fluid::is_lid(neighbor) {
        return false;
    }
    if face_index == 0 && is_liquid(current) && !is_liquid(neighbor) {
        return true;
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

    // **Two different leaves are still leaves against leaves.** An apple
    // tree is plain apple leaves with five to seven fruiting cells in it,
    // and a picked cell carries a variant: each is a different id from the
    // leaf beside it, so "different blocks always draw" took them, and both
    // cells drew the face between them -- the coplanar pair the note below
    // describes, shimmering round every apple on the tree. An oak's crown
    // grown into a maple's had the same seam. Leafy against leafy of any
    // kind goes the leaves' way now: drawn once, by the lower of the two.
    let leaves_against_leaves = primitive_shared::types::is_leafy(current)
        && primitive_shared::types::is_leafy(neighbor);

    // **At range a crown is a shell, and inside it there is nothing.**
    //
    // Leaves are the one pass `lod::coarsen` leaves at full resolution on
    // purpose (see its note: cubed leaves emit *more* faces than they
    // save), and the bill for that is a canopy that does not get cheaper
    // however far away it is -- measured over the whole arena at 932k leaf
    // triangles at full detail and 897k at *both* coarse levels, while the
    // ground it stands on came down from 1.2M to 637k. A quarter of the
    // world's triangles, none of them touched by the setting whose whole
    // job is triangles at range.
    //
    // Three ways to make a distant wood cheaper were weighed:
    //
    // * *Coarsen the leaves like everything else.* The blob's insides are
    //   drawn, which is the measurement `lod` already recorded.
    // * *Drop crowns past some distance.* A wood is its canopy; what is
    //   left is a field of poles, and the horizon changes shape as you
    //   walk toward it.
    // * **Keep the outside and drop the inside (chosen).** The faces
    //   between two leaf cells are the *interior* of the crown. Near, they
    //   matter -- a player stands under a tree and looks up through it,
    //   and a hollow crown shows daylight through the holes in the leaf
    //   texture. At the range these chunks are drawn at, a whole tree is a
    //   few dozen pixels and no line of sight reaches inside one: what is
    //   left after this is exactly the surface the silhouette is cut from.
    //
    // Only in a coarse chunk or past the see-through canopy line -- the
    // two places the canopy is drawn solid, where there is no hole in a
    // leaf to look into a crown through. With both settings at their
    // "everywhere" ends nothing here changes at all.
    if shell_crowns && leaves_against_leaves {
        return false;
    }

    if neighbor != current && !leaves_against_leaves {
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

/// How bright one corner of a face will come out, for choosing which
/// diagonal the face's two triangles meet along.
///
/// **The light is in it as well as the occlusion.** A quad is two
/// triangles, and whatever its corners carry is interpolated inside each
/// on its own, so a face whose corners disagree shows a crease along the
/// shared diagonal. The flip chose that diagonal by occlusion alone, which
/// is right while the light is the same at all four corners -- noon on open
/// ground -- and blind wherever smooth lighting makes them differ: a stone
/// at the edge of a fire's reach, a wall beside a darker cell, anything at
/// night that one corner sees less of. With the occlusion equal it never
/// fired, and a player photographed a stone face at night in two triangles
/// of different brightness.
///
/// Weighted the way the shader weighs them: the brighter of sky and block
/// light, darkened by occlusion at about the strength it ships with --
/// `1 - 0.45 * (1 - ao / 3)^2`, squared since the seams stopped drawing the
/// grid on the ground (`shade_lit`), in twentieths so it stays whole. One is
/// added to the light so a corner in the dark still orders by its
/// occlusion. A face whose four corners agree compares equal and keeps the
/// diagonal it always had.
#[inline]
fn corner_brightness(ao: u8, (sky, block): (u8, u8)) -> u32 {
    const OCCLUDED: [u32; 4] = [11, 16, 19, 20];
    (sky.max(block) as u32 + 1) * OCCLUDED[ao.min(3) as usize]
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
/// `Default` is written out rather than derived, because two of these
/// fields have a meaning at zero and it is the wrong one: an extent of
/// zero says "this chunk's floor faces are all at y = 0", which is a
/// claim rather than an absence. See `up_faces_from`.
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
    /// Where each of the seven groups inside the solid range ends:
    /// first the faces that belong to no one direction -- racks, models,
    /// anything that is not a cube face -- then the cube faces looking
    /// +Y, -Y, +X, -X, +Z, -Z in the order of `faces()`, so that the
    /// last entry equals `solid_index_count`.
    ///
    /// **Grouped so the renderer can leave out what looks away.** A
    /// chunk east of the camera cannot show a single east-facing quad,
    /// and the GPU knew that too -- after it had run the vertex shader
    /// on all four corners of every one of them. The split lets `render`
    /// skip the group instead, before any vertex is fetched. See
    /// `renderer::solid_ranges_facing` for the measurement.
    pub solid_groups: [u32; 7],
    /// The lowest plane any upward-looking face of this chunk sits on,
    /// and the highest plane any downward-looking one does.
    ///
    /// **Because a chunk is sixty-four blocks tall and its terrain is
    /// not.** `renderer::solid_ranges_facing` drops a direction group
    /// when the eye is on the blind side of the slab the group lives
    /// in, and for x and z that slab is the chunk's real sixteen
    /// blocks. For y it was the whole column of the world, so the two
    /// vertical groups were sent from everywhere -- and every
    /// downward-looking face in the world, eleven per cent of the
    /// terrain by index count, was fetched, transformed and thrown away
    /// by the rasteriser on every frame of a game played on the
    /// surface. These two numbers are that slab, measured.
    ///
    /// Empty groups get an extent that fails both tests, so a chunk
    /// with no floor faces at all costs nothing to skip.
    pub up_faces_from: f32,
    pub down_faces_to: f32,
    /// The highest point of any vertex in the chunk, or `f32::MIN` for
    /// an empty mesh.
    ///
    /// For the shadow pass (`engine::shadow`), which has to decide which
    /// chunks can throw a shadow into the ground around the player. A
    /// chunk is a column the whole height of the world and mostly air,
    /// and asked as that full column every chunk along the sun's line
    /// for two hundred blocks "might" cast -- a streak of drawing that
    /// the GPU then clips away. Worked out here, on the worker, where every
    /// vertex has just been written, rather than on the main thread
    /// during the upload, which is the part of the frame the streaming
    /// budget rations.
    pub top: f32,
    /// Whether this chunk's leaves are to be drawn in the opaque pass: its
    /// crowns were built as shells (`Neighbourhood::draw_leaves_solid`, or a
    /// coarse chunk), and **a shell must never be cut out** -- through the
    /// holes in its leaves is the hollow inside of a tree and the sky beyond
    /// it. The mesh says so rather than the renderer measuring a distance,
    /// so what is drawn can never disagree with what was built while a
    /// rebuilt chunk is still on its way.
    pub leaves_solid: bool,
    by_face: [Vec<u32>; 6],
    /// Faces held back for merging, and the scratch the greedy pass
    /// covers them with. Both live here rather than in `build_mesh` so
    /// that a chunk costs no allocation: the buffers travel to a worker
    /// and back and are reused, which is the whole arrangement `mesher`
    /// exists to keep. See `MERGE_COPLANAR_FACES`.
    mergeable: Vec<Mergeable>,
    covered: Vec<u64>,
}

impl Default for MeshBuffers {
    fn default() -> Self {
        let mut fresh = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            leaves: Vec::new(),
            sprites: Vec::new(),
            translucent: Vec::new(),
            solid_index_count: 0,
            leaf_end: 0,
            sprite_end: 0,
            solid_groups: [0; 7],
            up_faces_from: 0.0,
            down_faces_to: 0.0,
            top: 0.0,
            leaves_solid: false,
            by_face: Default::default(),
            mergeable: Vec::new(),
            covered: Vec::new(),
        };
        // One statement of what empty means, rather than two that can
        // drift apart.
        fresh.clear();
        fresh
    }
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
        self.solid_groups = [0; 7];
        // Extents that no eye can be inside, so an untouched mesh sends
        // neither vertical group rather than both.
        self.up_faces_from = f32::MAX;
        self.down_faces_to = f32::MIN;
        // Nothing, so nothing casts.
        self.top = f32::MIN;
        self.leaves_solid = false;
        for bucket in &mut self.by_face {
            bucket.clear();
        }
        self.mergeable.clear();
        self.covered.clear();
    }
}

/// One face that is a candidate for being swallowed by a rectangle.
///
/// **What has to match before two faces can become one quad**, and this
/// is the whole of the merge rule:
///
/// * the same one of the six directions, and the same plane along it --
///   otherwise they are not coplanar and there is no rectangle;
/// * the same texture layer and the same tint byte;
/// * the same *single* light word, which means the face was lit flat:
///   all four of its corners agreed about sky, block light and ambient
///   occlusion. This is the constraint that decides the whole feature's
///   value, and it is why a third of the chunk merges rather than all of
///   it -- a face at the foot of a wall, under an overhang or at a cave
///   mouth has a gradient across it, and a rectangle has no way to carry
///   four different corners.
/// * the same target list, so a leaf never merges into the solid pass.
///
/// **Nothing is interpolated and nothing is averaged.** The merge only
/// ever replaces faces that were already pixel-for-pixel identical apart
/// from where they sat, so a merged chunk and an unmerged one are the
/// same picture -- which is what makes `MERGE_COPLANAR_FACES` a switch
/// rather than a setting. The other rule that could have been written
/// here -- merge anything close enough and interpolate across it --
/// would change how the world looks, and is deliberately not what this
/// does.
///
/// Four things are excluded before lighting is even looked at, because
/// none of them is a unit square in the plane: part-height blocks and
/// anything wearing a cropped side texture, liquids (whose top follows
/// the cell above), anything with a UV turn (a quarter turn swaps the
/// axes a run would tile along), and the blended pass, which is a
/// handful of faces and not worth the risk.
#[derive(Clone, Copy)]
struct Mergeable {
    /// Which of the six face directions.
    face: u8,
    /// The cell coordinate along the face normal.
    plane: u8,
    /// ...and along the two axes the rectangle grows in. See
    /// `other_axes`: `u` runs along the first, `v` along the second.
    u: u8,
    v: u8,
    /// Everything the four corners have to agree on, in one word so the
    /// grouping is a sort rather than a comparison of five fields.
    ///
    /// ```text
    /// bits 0..13   the light word: sky, block, ambient occlusion, face
    /// bits 14..24  texture layer -- eleven bits, as in the vertex
    /// bits 25..32  tint
    /// bit  33      goes in the cutout pass rather than the solid one
    /// bit  34      one sheet, so no per-face shade (see `KEY_UNMOTTLED`)
    /// ```
    ///
    /// **Sixty-four bits because the layer grew to nine and this word
    /// was full to the bit.** Every field in it decides whether two
    /// faces may become one rectangle, so dropping one to make room
    /// would let two faces that differ merge -- and the cheapest one to
    /// drop, the one-sheet flag, is exactly the case where the mistake
    /// is invisible in a test and obvious in a wall. Widening the word
    /// instead costs eight bytes on a few hundred faces per chunk, which
    /// is the sort and nothing else.
    key: u64,
}

/// Where the light word ends and the layer begins in `Mergeable::key`.
const KEY_LAYER_SHIFT: u32 = 14;
/// The layer's width in the key: all of it, eleven bits. A key that kept
/// nine would let a face of layer 1500 merge into one of layer 476 -- and
/// draw the whole rectangle in whichever came first.
const KEY_LAYER_MASK: u64 = MAX_TEXTURE_LAYERS as u64 - 1;
/// ...and where the tint begins, above the layer's eleven bits. See
/// `MAX_TEXTURE_LAYERS`.
const KEY_TINT_SHIFT: u32 = 25;
/// Set when the face belongs to the cutout pass.
const KEY_CUTOUT: u64 = 1 << 33;
/// ...and the top bit says the material is one sheet, so the faces of
/// this rectangle must **not** be given a shade of their own. See
/// `types::is_one_sheet` and `MOTTLED_BIT`. It rides in the key rather
/// than being looked up again in `emit_merged`, which has the light and
/// the layer but no longer has the block.
const KEY_UNMOTTLED: u64 = 1 << 34;

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
    /// Whether these cells are the world or a simplification of it.
    ///
    /// Set by `lod::coarsen` and cleared by `fill`, and read in exactly
    /// one place: the choice between smooth lighting and flat. It rides
    /// on the snapshot rather than being a parameter of `build_mesh`
    /// because it is a fact about *these blocks* -- they are not the
    /// world, they are a stand-in for a piece of it three hundred
    /// blocks away -- and every caller that has one of these already
    /// knows which it is holding.
    coarse: bool,
    /// What is in each pit kiln of the chunk, as the server last said
    /// (`ServerMessage::PitPottery`), by global cell. Empty for a chunk
    /// with no pit, which is nearly all of them -- a list rather than a map
    /// for that reason. Set after `fill` by the one caller that has it
    /// (`set_pottery`); a snapshot nobody told draws plain pots.
    pottery: Vec<((i32, i32, i32), Vec<BlockId>)>,
    /// The chests of this chunk whose lid the frame is swinging rather than
    /// the mesh holding still (`ChunkManager::lids_in`, `chest_lid_block`).
    /// A list for `pottery`'s reason, and usually an empty one: a chest
    /// somebody has open is one cell in a world of them.
    swung_lids: Vec<(i32, i32, i32)>,
    /// Whether the stones and sticks of this chunk are laid flat rather
    /// than given their thickness (`relief`). Set after `fill` by the
    /// dispatch, which knows how far the chunk is from the player
    /// (`lod::relief_at`); cleared by `fill`, so a snapshot nobody told --
    /// every test and every photograph -- draws them solid.
    stones_lie_flat: bool,
    /// Whether this chunk's crowns are built as solid shells rather than
    /// see-through with their insides (`lod::leaves_see_through_at`). Set
    /// after `fill` by the dispatch, cleared by `fill` -- so, like the
    /// stones, a snapshot nobody told draws the canopy see-through.
    leaves_solid: bool,
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
/// Whether the mesher draws this block as something other than the cube
/// its table row describes -- a model of boxes, a pair of crossed quads, a
/// flat quad -- and so covers nothing of the cell under it, whatever that
/// row says about its height.
///
/// **A list that was grown one sky hole at a time, and is one question
/// now.** A block whose row is a part-height cube hides the top face of
/// the block it stands on (`face_visible`: a layer's footprint is the whole
/// cell), which is right for a drift of snow and wrong for anything drawn
/// narrower than its cell. It was found for the carcass, then the barrel
/// and the jug, then the nest, each from a photograph of a pale square of
/// sky round the thing -- and the straw bed, the bed, the stool and the
/// table had it all along, because they were drawn as the cube and the
/// cube did cover its floor until they became models. The rest of the
/// table rows that describe one shape and are drawn as another are here
/// too, so the next model added to `build_mesh` is one line in this list
/// and not the next photograph; `no_block_leaves_a_hole_in_the_floor_under_it`
/// meshes every block there is on a floor and holds this to it.
pub(crate) fn drawn_as_model(id: BlockId) -> bool {
    use primitive_shared::types as t;
    is_cross(id)
        || is_flat(id)
        || t::is_carcass(id)
        || t::is_bones(id)
        // A body and the bones it becomes are the player's own figure
        // lying down (`player_model::build_fallen`), and a figure on its
        // side covers no more of the floor than an animal on its side
        // does.
        || t::is_corpse(id)
        || t::is_barrel(id)
        || t::is_branch(id)
        || is_furniture(id)
        // A pit kiln is pots, fibre or logs in a hole (`pit_kiln_block`), and
        // the floor of the pit shows between the pots.
        || primitive_shared::pit::is_pit_kiln(id)
        // ...and an unlit pile of logs is its logs (`log_pile_block`): one
        // log lies in the middle of its cell with the grass round it.
        || primitive_shared::pit::pile_extent(id).is_some()
        // A standing torch is a pole a sixteenth and a half across in the
        // middle of its cell (`standing_torch_block`), and hides nothing.
        || primitive_shared::wildfire::is_standing_torch(id)
        // A wild hive is a comb against a trunk (`hive_block`), half a cell
        // deep: the cells beside it show past it, so it covers nothing.
        || primitive_shared::bees::is_hive(id)
        // A window lattice is a panel across the middle of its cell.
        || primitive_shared::types::is_lattice(id)
        // ...and a pit prop is a post in the middle of its cell.
        || primitive_shared::types::is_prop(id)
        // A door is a slab of boards along one side of its cell
        // (`door_block`), and the floor of the doorway shows past it.
        || t::is_door(id)
        // A step is a tread and a riser (`step_block`), and the back half of
        // its cell above the tread is air a face beside it shows into.
        || t::is_step(id)
        // A spike of dripstone is half a cell across at its widest
        // (`dripstone_block`), and a stalactite covers nothing under it at
        // all.
        || primitive_shared::dripstone::is_dripstone(id)
        // A thing set down is drawn as nothing here and as an item lying on
        // the floor by the frame (`entities::build_set_down_into`): its
        // eighth of a cube hid the grass under every knife, a square of sky.
        || t::is_set_down(id)
        || matches!(
            t::block_kind(id),
            t::BLOCK_NEST
                | t::BLOCK_NEST_EGGS
                // A cairn is three stones and the air between them.
                | t::BLOCK_CAIRN
                | t::BLOCK_JUG
                | t::BLOCK_DRYING_RACK
                | t::BLOCK_HIDE_FRAME
                | t::BLOCK_BRACKET_FUNGUS
        )
}

/// What a quarter turn about the vertical does to how a block is drawn.
/// See [`what_a_quarter_turn_does`].
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuarterTurn {
    /// The same drawing: a stone, a table, a barrel.
    Unchanged,
    /// A different one: a jug's handle moves, a chair's back.
    Changed,
    /// Drawn differently in two cells before anything is turned: a tuft
    /// jittered by where it grows, a carcass lying the way its cell
    /// hashes. The world has chosen its direction already.
    ChosenByTheCell,
}

/// A drawing reduced to what an eye can tell apart: the block seen
/// straight on from each of its six sides, as a picture of how near the
/// nearest surface is and what it is painted with.
///
/// **Depth pictures, and not the triangles or even the planes.** Two ways
/// of cutting one shape into boxes are one thing to look at and two things
/// to compare. A barrel's walls are two long boxes and two short ones
/// fitted between them, so a quarter turn swaps which pair is long and
/// moves every face buried in a corner; compared plane by plane, the
/// barrel -- the same from every side -- came out as a block that turns.
/// What is nearest along a line of sight does not care how it was cut.
///
/// Faces that are not square to an axis (the crossed quads of a tuft or a
/// flame) are left out: nothing that turns is built from them.
#[cfg(test)]
pub(crate) type Look = [Vec<Option<(i64, u32)>>; 6];

/// Cells across each depth picture: a sixty-fourth of a block, which is
/// the quarter sixteenth every model is written to.
#[cfg(test)]
const LOOK_GRID: usize = 64;

/// How `block` looks standing alone in the air at `at`, from each of its
/// six sides, turned `quarters` quarter turns about the cell's upright
/// middle. Depths are in 256ths of a block, in the cell's own frame.
#[cfg(test)]
pub(crate) fn look_of(block: BlockId, at: (i32, i32, i32), quarters: u32) -> Look {
    let cache = plant_tests::cache_of(|x, y, z| if (x, y, z) == at { block } else { BLOCK_AIR });
    let mesh = plant_tests::mesh_of(&cache);
    let mut look: Look = std::array::from_fn(|_| vec![None; LOOK_GRID * LOOK_GRID]);
    let cell = 256 / LOOK_GRID as i64;
    for tri in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|k| {
            let p = mesh.vertices[k as usize].position;
            let mut q = [
                ((p[0] - at.0 as f32) * 256.0).round() as i64,
                ((p[1] - at.1 as f32) * 256.0).round() as i64,
                ((p[2] - at.2 as f32) * 256.0).round() as i64,
            ];
            for _ in 0..quarters % 4 {
                q = [q[2], q[1], 256 - q[0]];
            }
            q
        });
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        // Square to one axis, and which way along it the face looks.
        let (axis, outward) = match (n[0] != 0, n[1] != 0, n[2] != 0) {
            (true, false, false) => (0, n[0] > 0),
            (false, true, false) => (1, n[1] > 0),
            (false, false, true) => (2, n[2] > 0),
            _ => continue,
        };
        let side = axis * 2 + usize::from(!outward);
        let (s, t) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let depth = a[axis];
        let layer = mesh.vertices[tri[0] as usize].tex_layer();
        let flat = [a, b, c].map(|p| (p[s], p[t]));
        let edge = |p: (i64, i64), q: (i64, i64), r: (i64, i64)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
        let range = |values: [i64; 3]| {
            let low = values.iter().min().copied().unwrap_or(0).div_euclid(cell).max(0);
            let high = values.iter().max().copied().unwrap_or(0).div_euclid(cell).min(LOOK_GRID as i64 - 1);
            low..=high
        };
        for i in range(flat.map(|p| p.0)) {
            for j in range(flat.map(|p| p.1)) {
                let centre = (i * cell + cell / 2, j * cell + cell / 2);
                let d = [
                    edge(flat[0], flat[1], centre),
                    edge(flat[1], flat[2], centre),
                    edge(flat[2], flat[0], centre),
                ];
                if !(d.iter().all(|&e| e >= 0) || d.iter().all(|&e| e <= 0)) {
                    continue;
                }
                let slot = &mut look[side][i as usize * LOOK_GRID + j as usize];
                let nearer = match *slot {
                    None => true,
                    Some((was, was_layer)) => {
                        if depth == was {
                            layer > was_layer
                        } else {
                            (depth > was) == outward
                        }
                    }
                };
                if nearer {
                    *slot = Some((depth, layer));
                }
            }
        }
    }
    look
}

/// Whether a quarter turn of `block`, as it would be put down with no
/// facing, would show.
///
/// Asked of the mesher's real output, so a new model is covered the day
/// it is drawn -- see the client's
/// `a_block_turns_when_it_is_placed_exactly_when_turning_it_would_show`,
/// which is what holds the table's `faces` to this.
#[cfg(test)]
pub(crate) fn what_a_quarter_turn_does(block: BlockId) -> QuarterTurn {
    // A lean-to is drawn whole by its middle cell and by no other
    // (`lean_to_block`), so the cell a player puts down -- its mouth, the
    // bare id -- draws nothing alone, and nothing turned is nothing. Its
    // middle, turned the same way, is the hut.
    let block = if primitive_shared::lean_to::is_lean_to(block) {
        primitive_shared::lean_to::cell(primitive_shared::types::block_facing(block), 8)
    } else {
        block
    };
    let here = look_of(block, (8, 4, 8), 0);
    // **Several other cells, not one.** A cell's turn is two bits of a hash,
    // so any one other cell has a quarter chance of being laid exactly the
    // same way -- which is what (3, 4, 11) is for a stone lying on the ground
    // (`relief::Relief::turn_of`), and a scattering the world turns read here
    // as a block a placer should turn.
    if [(3, 4, 11), (5, 4, 2), (12, 4, 6), (1, 4, 14)].into_iter().any(|cell| look_of(block, cell, 0) != here) {
        return QuarterTurn::ChosenByTheCell;
    }
    if look_of(block, (8, 4, 8), 1) == here {
        QuarterTurn::Unchanged
    } else {
        QuarterTurn::Changed
    }
}

/// **A block light passes through and sight does not: ice.**
///
/// `is_opaque` is a question about light, and ice answers it no -- two
/// levels taken out, so the sea under a frozen bay is not a cave (see its
/// row in `blocks`). Cover is a question about *sight*, and the picture is
/// solid in every texel, so ice hides its whole cell exactly as stone does.
/// Reading the light answer for both is what drew ice as a leaf:
///
/// * every face between two cells of a frozen lake was drawn, by the leaf
///   rule (`face_visible`: the lower of two draws the face they share) --
///   a wall of quads under every sheet of ice, buried where nothing can
///   see it, a whole lake's worth per chunk;
/// * a leaf, a tuft of turf or a stone beside ice drew its face into the
///   ice as well as the ice drawing its own -- two depth-writing quads in
///   one plane, which is the shimmer every other coplanar pair in this file
///   was written to stop.
///
/// Rejected: making ice opaque in its row. That is the lighting answer,
/// and a frozen bay with a roof that stops the sun is the thing the row
/// was written to prevent.
#[inline]
fn hides_its_cell_from_sight(id: BlockId) -> bool {
    primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_ICE
}

#[inline]
fn cover_table() -> &'static [u8; 1 << 16] {
    static TABLE: std::sync::OnceLock<Box<[u8; 1 << 16]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = Box::new([0u8; 1 << 16]);
        for (id, slot) in table.iter_mut().enumerate() {
            let id = id as BlockId;
            *slot = if is_opaque(id) || hides_its_cell_from_sight(id) {
                FULL_COVER
            } else if primitive_shared::types::is_step(id) {
                // **A step covers what its tread covers**: the whole top of
                // the block under it, and the lower half of every side --
                // the tread is the whole lower half of the cell in every
                // shape a step takes (`geometry::step_pose_boxes`), so a
                // slab beside a stair is hidden where the two meet and
                // nothing taller is. It was `FULL_COVER` by way of
                // `is_opaque`, which culled the upper half of every face
                // a stair stood against and left a hole there.
                //
                // Rejected: crediting the riser too, per facing. The back
                // of a straight step is whole, but an outside corner cuts
                // its riser to a post (`geometry::step_shape`), and the
                // cover is a question about the pair of cells alone; a
                // buried face behind a riser costs two triangles, a
                // missing one is a hole.
                FULL_COVER / 2
            } else if drawn_as_model(id) {
                // **A carcass covers nothing, whatever its table row
                // says.** Its row is a low cube -- that is what the
                // collider and the step height read -- but it is drawn
                // as the animal's model (`animal_model::build_fallen`),
                // which does not fill the cell's floor. Read as a layer
                // it hid the top face of the block under it, and a
                // player looking down at a dead boar saw the sky
                // through the ground around its belly.
                0
            } else if is_partial(id) {
                primitive_shared::types::block_layers(id)
            } else {
                0
            };
        }
        table
    })
}

#[cfg(test)]
mod cover_claim_tests {
    use super::*;

    /// Which way a face of the cube looks, as (axis, toward +).
    fn face_dir(face: usize) -> (usize, bool) {
        let n = faces()[face].neighbor;
        let axis = (0..3).find(|&a| n[a] != 0).unwrap_or(1);
        (axis, n[axis] > 0)
    }

    /// What `id`, standing alone, fails to draw of the side a neighbour's
    /// `face` looks at, up to the height the cover table says it hides:
    /// the number of `LOOK_GRID` cells of that side with nothing drawn in
    /// the plane of the wall the two share.
    fn undrawn_of_what_it_hides(id: BlockId, look: &Look, face: usize) -> usize {
        let hidden = hidden_by(cover_of(id), face);
        if hidden == 0 {
            return 0;
        }
        // The neighbour's face looks along `dir`; what it looks at is this
        // block's side facing back along it.
        let (axis, toward_plus) = face_dir(face);
        let side = axis * 2 + usize::from(toward_plus);
        let plane = if toward_plus { 0 } else { 256 };
        let (s, t) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let reach = LOOK_GRID * hidden as usize / FULL_COVER as usize;
        let mut undrawn = 0;
        for i in 0..LOOK_GRID {
            for j in 0..LOOK_GRID {
                // How far up this cell of a side wall is: only as much of
                // the wall as the cover reaches is claimed.
                let up = if s == 1 { i } else if t == 1 { j } else { 0 };
                if axis != 1 && up >= reach {
                    continue;
                }
                if look[side][i * LOOK_GRID + j].map(|(depth, _)| depth) != Some(plane) {
                    undrawn += 1;
                }
            }
        }
        undrawn
    }

    #[test]
    fn nothing_hides_a_neighbours_face_it_does_not_draw_over_itself() {
        // **"если поставить с блоком, то грань блока будет пустая".** A
        // step's row is a cube with a wall's opacity, so `is_opaque` called
        // it a whole block and the cover table said it hid every face beside
        // it -- and the block a stair was set against lost the whole face,
        // of which the stair draws only the tread's half: the upper half
        // was a hole straight through into the block.
        //
        // Asked of every id that claims any cover at all, against what the
        // mesher really draws of it standing alone: every face the table
        // says it hides for a neighbour, it draws itself, in the plane of
        // the wall the two share, as high as it claims.
        let mut wrong = Vec::new();
        for id in 0..=u16::MAX {
            let id = id as BlockId;
            if !primitive_shared::types::is_known_block(id) || cover_of(id) == 0 {
                continue;
            }
            let look = look_of(id, (8, 4, 8), 0);
            for face in 0..6 {
                let undrawn = undrawn_of_what_it_hides(id, &look, face);
                if undrawn > 0 {
                    wrong.push(format!(
                        "{} ({id}, cover {}): hides a neighbour's face {face} and leaves {undrawn} of {} of it undrawn",
                        primitive_shared::types::block_name(id),
                        cover_of(id),
                        LOOK_GRID * LOOK_GRID
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "a hole where a face was culled:\n  {}", wrong.join("\n  "));
    }
}

#[cfg(test)]
mod model_cover_tests {
    use super::*;

    /// A piece of branch with the given joins, as quads.
    fn branch_quads(width: u8, joins: [Option<u8>; 6]) -> Vec<[[f32; 3]; 4]> {
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        branch_block(
            [0.0; 3],
            primitive_shared::types::branch(width),
            (joins, ALONE),
            &layers,
            0xFF,
            &mut vertices,
            &mut indices,
        );
        vertices
            .chunks_exact(4)
            .map(|q| [q[0].position, q[1].position, q[2].position, q[3].position])
            .collect()
    }

    #[test]
    fn a_piece_of_branch_is_as_wide_as_it_says_and_stays_in_its_cell() {
        // A trunk joined above and below: one post, floor to ceiling, as
        // wide as the variant and centred -- the property the taper is
        // drawn with. Everything inside the cell, give or take the bite.
        let slack = 2.0 * BITE / 16.0;
        for width in (2..=16).step_by(2) {
            let quads = branch_quads(width, [Some(width), Some(width), None, None, None, None]);
            let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
            for corner in quads.iter().flatten() {
                for v in corner {
                    assert!((-slack..=1.0 + slack).contains(v), "a {width}-wide piece leaves its cell");
                }
                min_x = min_x.min(corner[0]);
                max_x = max_x.max(corner[0]);
            }
            let drawn = (max_x - min_x) * 16.0;
            assert!(
                (drawn - width as f32).abs() < 0.1,
                "a piece {width} sixteenths wide is drawn {drawn} wide"
            );
        }
    }

    #[test]
    fn a_piece_of_branch_reaches_the_face_it_is_joined_through_and_no_other() {
        // An arm to +X reaches the cell's +X face; nothing reaches the
        // faces with no wood behind them, or a lone twig would be joined
        // to the air.
        let quads = branch_quads(8, [Some(8), Some(8), Some(4), None, None, None]);
        let reach = |axis: usize, f: fn(f32, f32) -> f32, start: f32| {
            quads.iter().flatten().map(|c| c[axis]).fold(start, f)
        };
        assert!((reach(0, f32::max, f32::MIN) - 1.0).abs() < 0.01, "the arm to +X stops short");
        assert!(reach(0, f32::min, f32::MAX) > 0.2, "a piece with nothing at -X reaches it");
        assert!(reach(2, f32::max, f32::MIN) < 0.8, "a piece with nothing at +Z reaches it");
        // ...and the arm is the thinner of the two, so the halves meet.
        let arm_top = quads
            .iter()
            .flatten()
            .filter(|c| c[0] > 0.9)
            .map(|c| c[1])
            .fold(f32::MIN, f32::max);
        assert!((arm_top * 16.0 - 10.0).abs() < 0.1, "a join to a 4-wide piece is {} tall", arm_top * 16.0 - 6.0);
    }

    #[test]
    fn a_trunk_draws_no_faces_inside_the_wood_it_runs_into() {
        // A straight run of trunk joined above and below by pieces as
        // thick: four sides and no ends. The ends were two of every six
        // quads of a trunk and could never be seen.
        let quads = branch_quads(12, [Some(12), Some(16), None, None, None, None]);
        assert_eq!(quads.len(), 4, "a trunk inside a trunk drew {} quads", quads.len());
        // ...and the top of a trunk under a thinner piece keeps its ring.
        let quads = branch_quads(12, [Some(6), Some(12), None, None, None, None]);
        assert_eq!(quads.len(), 5, "the step from twelve to six has no ring of cut wood");
    }

    /// Whether a line straight along z through `(x, y)` meets a solid
    /// triangle of `mesh` between `z0` and `z1`.
    fn solid_along_z(mesh: &MeshBuffers, (x, y): (f32, f32), (z0, z1): (f32, f32)) -> bool {
        mesh.indices[..mesh.solid_index_count as usize].chunks_exact(3).any(|triangle| {
            let p: Vec<[f32; 3]> = triangle.iter().map(|&i| mesh.vertices[i as usize].position).collect();
            if p.iter().any(|c| !(z0..=z1).contains(&c[2])) {
                return false;
            }
            let side = |a: [f32; 3], b: [f32; 3]| (x - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (y - b[1]);
            let d = [side(p[0], p[1]), side(p[1], p[2]), side(p[2], p[0])];
            let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
            area.abs() > 1e-6 && (d.iter().all(|v| *v >= 0.0) || d.iter().all(|v| *v <= 0.0))
        })
    }

    #[test]
    fn two_columns_of_trunk_leave_no_hole_in_the_seam_between_them() {
        // **"деревья странные"**: a trunk two cells wide, each column a post
        // twelve sixteenths across, had a slit of sky at every block where
        // the arms between the columns stopped short of their cells' floors
        // and ceilings. Looked through along z at the seam, just under and
        // just over each boundary between two pieces, the wood has to be
        // there.
        use super::transparency_tests::{cache_of, mesh_of};
        use primitive_shared::types::{branch, BLOCK_AIR, BLOCK_STONE};
        let mesh = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            _ if y < 10 => BLOCK_STONE,
            (5 | 6, 10..=13, 5) => branch(12),
            _ => BLOCK_AIR,
        }));
        for seam in [5.97, 6.03] {
            for y in [10.03, 10.97, 11.03, 11.97, 12.03, 12.97, 13.03] {
                assert!(solid_along_z(&mesh, (seam, y), (5.0, 6.0)), "the seam between the columns is open at x {seam}, y {y}");
            }
        }
    }

    #[test]
    fn a_stem_under_leaves_runs_up_into_them() {
        // The other picture: a sapling's stem ending a third of a block under
        // the cube of leaves it holds up.
        use super::transparency_tests::{cache_of, mesh_of};
        use primitive_shared::types::{branch, BLOCK_AIR, BLOCK_LEAVES, BLOCK_STONE};
        let mesh = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            _ if y < 10 => BLOCK_STONE,
            (5, 10, 5) => branch(4),
            (5, 11, 5) => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        }));
        let top = mesh.indices[..mesh.solid_index_count as usize]
            .iter()
            .map(|&i| mesh.vertices[i as usize].position)
            .filter(|p| p[0] > 5.0 && p[0] < 6.0 && p[2] > 5.0 && p[2] < 6.0)
            .map(|p| p[1])
            .fold(f32::MIN, f32::max);
        assert!(top > 10.99, "the stem stops at {top}, under the leaves it holds");
    }

    #[test]
    fn a_piece_of_branch_hides_no_face_of_its_neighbours() {
        // A bough is a post in a cell of air: covered as a cube, the leaves
        // round a trunk lost the faces towards it and the grass under a
        // sapling was a hole to the sky.
        for width in (2..=16).step_by(2) {
            assert_eq!(cover_of(primitive_shared::types::branch(width)), 0, "a {width}-wide piece covers");
        }
    }

    #[test]
    fn a_set_down_jug_has_an_opening_and_stays_inside_its_row() {
        // **The first jug model had no mouth**: its comment promised a lip
        // and it drew a solid neck, so looking down at a jug showed a flat
        // clay top. Asked as geometry: whatever horizontal surface covers
        // the middle of the cell has to be well below the rim.
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        jug_block(
            [0.0; 3],
            primitive_shared::types::BLOCK_JUG,
            &layers,
            0xFF,
            &mut vertices,
            &mut indices,
        );
        let centre = (0.5f32, 0.5f32);
        let mut highest_over_centre = f32::MIN;
        for triangle in indices.chunks_exact(3) {
            let p: Vec<[f32; 3]> = triangle.iter().map(|&i| vertices[i as usize].position).collect();
            let flat = (p[0][1] - p[1][1]).abs() < 1e-5 && (p[0][1] - p[2][1]).abs() < 1e-5;
            if !flat {
                continue;
            }
            let side = |a: [f32; 3], b: [f32; 3]| {
                (centre.0 - b[0]) * (a[2] - b[2]) - (a[0] - b[0]) * (centre.1 - b[2])
            };
            let d = [side(p[0], p[1]), side(p[1], p[2]), side(p[2], p[0])];
            let outside = d.iter().any(|v| *v < -1e-6) && d.iter().any(|v| *v > 1e-6);
            if !outside {
                highest_over_centre = highest_over_centre.max(p[0][1]);
            }
        }
        assert!(
            highest_over_centre < 9.0 / 16.0,
            "the middle of the jug is covered at {} sixteenths -- it has no mouth",
            highest_over_centre * 16.0
        );
        // ...and the whole model is inside its cell and no taller than the
        // five eighths its table row tells the collider.
        let slack = 2.0 * BITE / 16.0;
        for vertex in &vertices {
            let [x, y, z] = vertex.position;
            assert!((-slack..=1.0 + slack).contains(&x) && (-slack..=1.0 + slack).contains(&z));
            assert!(
                (-slack..=10.0 / 16.0 + slack).contains(&y),
                "the jug rises to {} sixteenths",
                y * 16.0
            );
        }
    }

    #[test]
    fn a_model_standing_in_a_cell_never_hides_the_ground_under_it() {
        // A model that draws less than its cell's floor must not be read
        // as a layer, or the top of the block under it is culled and the
        // sky shows through the ground round its feet.
        use primitive_shared::body::Water;
        use primitive_shared::types::{barrel_of, BARREL_JUGS, BLOCK_JUG};
        assert_eq!(cover_of(BLOCK_JUG), 0, "a jug hides the ground it stands on");
        for kind in [Water::Fresh, Water::Standing, Water::Salt] {
            for jugs in 0..=BARREL_JUGS {
                assert_eq!(
                    cover_of(barrel_of(kind, jugs)),
                    0,
                    "a barrel of {kind:?} x{jugs} hides the ground it stands on"
                );
            }
        }
    }
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

    /// Lays this chunk's stones and sticks flat, for a chunk too far away
    /// for their thickness to be a pixel. See `lod::relief_at`.
    pub fn lay_stones_flat(&mut self, flat: bool) {
        self.stones_lie_flat = flat;
    }

    /// Builds this chunk's crowns as solid shells, for a chunk past the
    /// see-through canopy line. See `lod::leaves_see_through_at`.
    pub fn draw_leaves_solid(&mut self, solid: bool) {
        self.leaves_solid = solid;
    }

    /// What is in the pit kilns of this chunk. See `pottery`.
    pub fn set_pottery(&mut self, pottery: Vec<((i32, i32, i32), Vec<BlockId>)>) {
        self.pottery = pottery;
    }

    /// The pieces in the pit kiln at a global cell, if the server has said.
    fn pottery_at(&self, cell: (i32, i32, i32)) -> Option<&[BlockId]> {
        self.pottery.iter().find(|(at, _)| *at == cell).map(|(_, pieces)| pieces.as_slice())
    }

    /// The chests of this chunk the frame is drawing a lid for. See
    /// `swung_lids`.
    pub fn set_swung_lids(&mut self, lids: Vec<(i32, i32, i32)>) {
        self.swung_lids = lids;
    }

    /// The lids this neighbourhood's mesh leaves out, for the frame to take
    /// over when the mesh lands (`ChunkManager::note_meshed_lids`).
    pub fn swung_lids(&self) -> &[(i32, i32, i32)] {
        &self.swung_lids
    }

    /// Whether the chest at a global cell has its lid drawn by the frame.
    pub(crate) fn lid_swings_at(&self, cell: (i32, i32, i32)) -> bool {
        self.swung_lids.contains(&cell)
    }

    /// Whether `mark_coarse` was called on this snapshot.
    pub(crate) fn is_coarse(&self) -> bool {
        self.coarse
    }

    /// Says these cells are `lod::coarsen`'s work rather than the
    /// world's, which changes exactly one thing about how they are
    /// meshed: see `flat_lit` in `build_mesh`.
    pub(crate) fn mark_coarse(&mut self) {
        self.coarse = true;
    }

    /// One of the chunk's own cells: the block, and its packed light.
    ///
    /// The mesher itself never uses this -- it steps through the padded
    /// array by constant offsets, which is the whole reason that array
    /// is shaped the way it is. This is for `lod::coarsen`, which walks
    /// the chunk once by coordinate before the meshing starts and would
    /// otherwise need the padding layout made public.
    pub(crate) fn own_cell(&self, x: usize, y: usize, z: usize) -> (BlockId, u8) {
        let index = padded_index(x + PAD as usize, y, z + PAD as usize);
        (self.blocks[index], self.light[index])
    }

    /// ...and writing one back. Only the chunk's own cells: the ring of
    /// padding around them has to stay as the world really is, which is
    /// what makes a seam between two detail levels hole-free. See
    /// `lod`.
    pub(crate) fn set_own_cell(&mut self, x: usize, y: usize, z: usize, id: BlockId, light: u8) {
        let index = padded_index(x + PAD as usize, y, z + PAD as usize);
        self.blocks[index] = id;
        self.light[index] = light;
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
            coarse: false,
            pottery: Vec::new(),
            swung_lids: Vec::new(),
            stones_lie_flat: false,
            leaves_solid: false,
        }
    }
}

/// One source plane -- a chunk's cells at a single y.
const SOURCE_PLANE: usize = CHUNK_SIZE_X * CHUNK_SIZE_Z;
/// ...and one padded plane, which is what a step in y costs here.
const PADDED_PLANE: usize = PADDED_X * PADDED_Z;

/// Which padded columns one of the nine chunks owns, and where they sit
/// inside it, along one axis.
///
/// The padded box is one chunk with a single column of each neighbour
/// glued round it, so along either axis the three chunks contribute a
/// column, sixteen columns and a column. Returned as
/// (first padded index, first local index, how many) so the copy below
/// can address a whole run at once.
#[inline]
fn span(step: usize, size: usize) -> (usize, usize, usize) {
    match step {
        0 => (0, size - 1, 1),         // the neighbour's last column
        1 => (PAD as usize, 0, size),  // the chunk itself
        _ => (size + PAD as usize, 0, 1), // the neighbour's first column
    }
}

/// A chunk's blocks, but only if there are as many of them as there are
/// cells in a chunk.
///
/// A `Chunk` carries its block array in a `Vec` -- serde's derive stops
/// at arrays of thirty-two -- so a chunk that arrived over a socket
/// carries whatever length the sender put in it. The copy below moves
/// whole rows with `copy_from_slice`, which would panic on a short one;
/// treating it as absent instead lands on the per-cell fallback, which
/// is the same answer the old per-cell indexing would have given for a
/// chunk that was merely missing.
#[inline]
fn chunk_slice(data: Option<&[BlockId]>) -> Option<&[BlockId]> {
    data.filter(|d| d.len() >= CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z)
}

impl Neighbourhood {
    pub fn fill<S: BlockSource>(&mut self, pos: primitive_shared::types::ChunkPos, blocks: &S, light: &LightMap) {
        let origin_x = pos.x * CHUNK_SIZE_X as i32;
        let origin_z = pos.z * CHUNK_SIZE_Z as i32;
        // These buffers go round a pool, so a snapshot that was coarse
        // last time has to stop being coarse the moment it holds real
        // blocks again -- otherwise a chunk the player walked *toward*
        // would be rebuilt at full detail and still lit flat.
        self.coarse = false;
        // ...and the pottery goes with the blocks it was told about: a
        // pooled snapshot must not draw last chunk's bricks in this one.
        self.pottery.clear();
        // ...nor last chunk's open chest, which would take the lid off a
        // shut one here and leave it off until something edited the chunk.
        self.swung_lids.clear();
        // ...and a far chunk's flat stones do not follow the snapshot to a
        // near one.
        self.stones_lie_flat = false;
        // ...nor its solid crowns.
        self.leaves_solid = false;

        // **The skyline first, so the copy below can stop under it.**
        //
        // The ceiling is a function of the chunk's own cells alone (a
        // tall neighbour is sampled for culling and ambient occlusion but
        // cannot make us mesh sky we do not own), and the chunk's own
        // cells arrive as one contiguous array -- so finding it is a
        // scan of whole planes from the roof of the world down, stopping
        // at the first that holds anything. On ordinary terrain that is
        // a few kilobytes read in order and the answer is around y 45,
        // which then takes a third of the padded box out of the copy
        // entirely. It used to be tracked per cell *during* the fill,
        // which meant the fill had to walk the sky to discover there was
        // nothing in it.
        self.ceiling = match (blocks.packed_chunk(pos), chunk_slice(blocks.chunk_data(pos))) {
            // The client's own chunks. A section of sky is one comparison
            // here rather than 4,096, so two thirds of a chunk is
            // dismissed in a dozen branches. See `PackedChunk::skyline`.
            (Some(packed), _) => packed.skyline(),
            (None, Some(own)) => (0..CHUNK_SIZE_Y)
                .rev()
                .find(|y| own[y * SOURCE_PLANE..(y + 1) * SOURCE_PLANE]
                    .iter()
                    .any(|&id| id != BLOCK_AIR))
                .map_or(0, |y| y as i32 + 1),
            // A `BlockSource` with no bulk access cannot be asked for its
            // skyline cheaply -- finding it would be the 65,536 per-cell
            // queries this exists to avoid. Meshing the whole column
            // instead is slower and produces exactly the same mesh: the
            // face loop emits nothing for an air cell either way. Nothing
            // in the client takes this path; it is here so a test source
            // still gets a correct mesh.
            (None, None) => CHUNK_SIZE_Y as i32,
        };

        // One past the last plane the face loop can read. It walks
        // `0..ceiling` and samples one cell above, so the plane *at* the
        // ceiling has to be filled and nothing above it ever is. Planes
        // left untouched keep whatever the previous chunk to use this
        // pooled neighbourhood put there, which is exactly why the bound
        // has to be one *more* than the loop's own.
        let planes = (self.ceiling as usize + 1).min(CHUNK_SIZE_Y);

        // Nine chunks, one rectangle of columns each, copied a row at a
        // time.
        //
        // **A row is contiguous in both arrays**, and that is the whole
        // point. Addressing this per cell -- which is what walking a
        // column at a time forced -- steps through the source in strides
        // of a plane and the destination in strides of a padded plane,
        // so all sixty-four cells of a column land on sixty-four
        // different cache lines and every one of the 324 columns pays
        // that again. Sixteen adjacent cells of one row are thirty-two
        // adjacent bytes; the middle chunk, which is 256 of the 324
        // columns, therefore moves as one `copy_from_slice` per row
        // rather than as sixteen scattered loads and stores.
        for sz in 0..3usize {
            for sx in 0..3usize {
                let cpos = primitive_shared::types::ChunkPos::new(
                    pos.x + sx as i32 - PAD,
                    pos.z + sz as i32 - PAD,
                );
                // Nine lookups per fill, once each, rather than one per
                // column: that is what the memo this replaced was for.
                //
                // Packed first, because that is what the client keeps;
                // the flat slice is for sources that build their world
                // out of plain arrays. The light is always packed, and
                // packed to the right length by construction
                // (`PackedLight::pack` refuses any other), which is why
                // the length check this used to make on it is gone.
                let packed = blocks.packed_chunk(cpos);
                let block_data = match packed {
                    Some(_) => None,
                    None => chunk_slice(blocks.chunk_data(cpos)),
                };
                let light_data = light.chunk_light(cpos);

                let (dst_px, src_lx, width) = span(sx, CHUNK_SIZE_X);
                let (dst_pz, src_lz, depth) = span(sz, CHUNK_SIZE_Z);

                for y in 0..planes {
                    let dst_plane = y * PADDED_PLANE;
                    let src_plane = y * SOURCE_PLANE;
                    for row in 0..depth {
                        let dst = dst_plane + (dst_pz + row) * PADDED_X + dst_px;
                        let src = src_plane + (src_lz + row) * CHUNK_SIZE_X + src_lx;

                        match (packed, block_data) {
                            // A row is always inside one section, which
                            // is the contract `copy_run` asks for.
                            (Some(chunk), _) => {
                                chunk.copy_run(src, &mut self.blocks[dst..dst + width]);
                            }
                            (None, Some(data)) => {
                                self.blocks[dst..dst + width]
                                    .copy_from_slice(&data[src..src + width]);
                            }
                            // No bulk access: either the chunk isn't
                            // loaded, or this `BlockSource` doesn't
                            // implement the optional fast path. Fall back
                            // to the per-cell query rather than silently
                            // reading air -- getting that wrong makes
                            // whole chunks vanish. A cell nobody can
                            // answer for is *unknown*, not air: see
                            // `UNKNOWN_BLOCK`.
                            (None, None) => {
                                let gz = origin_z + (dst_pz + row) as i32 - PAD;
                                for step in 0..width {
                                    let gx = origin_x + (dst_px + step) as i32 - PAD;
                                    self.blocks[dst + step] = blocks
                                        .block_at(gx, y as i32, gz)
                                        .unwrap_or(UNKNOWN_BLOCK);
                                }
                            }
                        }

                        match light_data {
                            Some(data) => {
                                data.copy_run(src, &mut self.light[dst..dst + width]);
                            }
                            // Unlit (unloaded) neighbour: full sky, so the
                            // frontier reads slightly bright rather than a
                            // wall of shadow.
                            None => self.light[dst..dst + width].fill(0x0F),
                        }
                    }
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
    /// argument: `recompute_ceiling`, and `liquid_depth_below`, which
    /// walks a whole column downward and so is the one sample in the
    /// mesher that is not one step from the cell being meshed.
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

    /// Where the neighbouring chunks' meshers may have put a corner on
    /// our seam, as a bitset over the chunk's corner lattice.
    ///
    /// A face in the chunk next door cannot be one rectangle with the
    /// face beside it if the two differ in block or light, so every ring
    /// cell whose signature differs from the cell beside it along the
    /// seam, or the cell below it, has all four of its seam-plane corners
    /// marked. That is a superset of where the neighbour's quads actually
    /// end -- its ambient occlusion can differ where these do not, and
    /// its greedy scan can stop for reasons a row further in -- and a
    /// superset is the right side to err on: a mark that was not needed
    /// costs one extra vertex, a mark that was needed and missing costs a
    /// crack.
    ///
    /// **Taken out once, and put back on a different world.** On natural
    /// terrain this changed nothing measurable -- eighteen near-field
    /// pixels with it and without, the floor of the instrument -- and it
    /// went, because a mark that buys nothing is a cost. The test world
    /// is not natural terrain: every plot is one material and the lanes
    /// between them lie exactly on the chunk seams, so a thirteen-long
    /// rectangle of cobble ends at a seam against gravel rectangles that
    /// end somewhere else, and the player photographed the result as
    /// dotted lines along every seam. With the closing done by inserting
    /// vertices rather than cutting, these marks add vertices along the
    /// seam and nothing cascades from them.
    ///
    /// Indexed exactly as the lattice in `mark_corners` is:
    /// `lattice_index(x, y, z)`.
    pub fn seam_marks(&self) -> Vec<u64> {
        let mut marks = vec![0u64; (LATTICE_X * LATTICE_Y * LATTICE_Z).div_ceil(64)];
        let mut mark = |x: usize, y: usize, z: usize| {
            let i = lattice_index(x, y, z);
            marks[i / 64] |= 1 << (i % 64);
        };
        let signature = |lx: i32, y: i32, lz: i32| -> (BlockId, u8) {
            let block = self.block(lx, y, lz);
            let light = if (0..CHUNK_SIZE_Y as i32).contains(&y) {
                self.light[padded_index((lx + PAD) as usize, y as usize, (lz + PAD) as usize)]
            } else {
                0
            };
            (block, light)
        };
        let ceiling = self.ceiling().clamp(0, CHUNK_SIZE_Y as i32) as usize;
        for y in 0..ceiling {
            for t in 0..CHUNK_SIZE_X.max(CHUNK_SIZE_Z) {
                // West and east seams run along z; south and north along x.
                for (lx, lz, seam_x, seam_z) in [
                    (-1, t as i32, Some(0usize), None),
                    (CHUNK_SIZE_X as i32, t as i32, Some(LATTICE_X - 1), None),
                    (t as i32, -1, None, Some(0usize)),
                    (t as i32, CHUNK_SIZE_Z as i32, None, Some(LATTICE_Z - 1)),
                ] {
                    let along = if seam_x.is_some() { CHUNK_SIZE_Z } else { CHUNK_SIZE_X };
                    if t >= along {
                        continue;
                    }
                    let here = signature(lx, y as i32, lz);
                    let (before_x, before_z) =
                        if seam_x.is_some() { (lx, lz - 1) } else { (lx - 1, lz) };
                    let changes = (t > 0 && signature(before_x, y as i32, before_z) != here)
                        || (y > 0 && signature(lx, y as i32 - 1, lz) != here);
                    if !changes {
                        continue;
                    }
                    let (x0, z0) = match (seam_x, seam_z) {
                        (Some(x), None) => (x, t),
                        (None, Some(z)) => (t, z),
                        _ => unreachable!(),
                    };
                    let (x1, z1) = match (seam_x, seam_z) {
                        (Some(_), None) => (x0, z0 + 1),
                        _ => (x0 + 1, z0),
                    };
                    for (x, z) in [(x0, z0), (x1, z1)] {
                        mark(x, y, z);
                        mark(x, y + 1, z);
                    }
                }
            }
        }
        marks
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
/// **Which ids go through every model in `build_mesh` to the cube**,
/// learned as the mesher meets them.
///
/// Between the water hand-overs and the cube loop, `build_mesh` asks each
/// cell some thirty questions -- loose item? hive? prop? stake? door? step?
/// dripstone? rack? set-down thing? kiln? pile? carcass? body? palm? branch?
/// barrel? bones? furniture? plant? -- and every one that says yes draws its
/// model and takes the next cell. Nearly every cell of a world is stone,
/// soil or sand, which says no to all thirty and goes on to be a cube, and
/// 1.5 made the list longer by a model at a time (racks, piles, stakes,
/// frames, set-down things, carcasses, bones). Measured on the benchmark's
/// own ground (`arena::what_the_benchmark_scene_is_made_of`, world `night`,
/// 441 fine chunks, release): skipping the list for stone, dirt, grass and
/// sand alone took 12.20 → 11.04 ms a chunk, and this table, for every id
/// the list turns down, **6.05 → 5.02 ms** (the shipped release profile,
/// the two alternated in one binary, best of three; the same A/B is the
/// last line of that tool). The synthetic chunk of `bench_meshing` barely
/// moves (5.15 → 5.10): its time is faces, not cells, and faces are what
/// this does not touch.
///
/// **The answer is a property of the id, so it is kept.** Every question
/// is about the id alone -- `is_door(id)`, `Species::of_carcass(id)`, the
/// kind in a `matches!` -- and every yes ends in `continue`. So a cell that
/// comes out of the bottom of the list has an id that says no to all of
/// it, and so will every other cell of that id, in any chunk, on any
/// thread. The two flames that draw *and* fall through (a lit campfire, a
/// burning block) sit outside the skipped part for that reason.
///
/// Considered:
///
/// * **A table written by hand** -- "stone, dirt, grass, sand are cubes".
///   The fastest and the one that rots: the next model added to the list
///   for an id in that table would be drawn as a cube, silently. Rejected.
/// * **A table computed from the same predicates at startup.** The list
///   written twice; the same rot, a line further away. Rejected.
/// * **Learned from the list itself (chosen).** The list stays the only
///   place the question is asked; the table can only ever hold what it
///   answered. A new model in the list is a new `continue`, and an id it
///   takes never reaches the line that learns. The one rule this puts on
///   the list: **a question in it must be about the id alone and end in
///   `continue`** -- a model that falls through, or asks the neighbours
///   whether to draw, belongs with the flames above it.
///
/// Sixty-five thousand ids a bit each, eight kilobytes, shared by every
/// mesher thread: a bit only ever goes from 0 to 1, and a thread that has
/// not seen another's write yet just asks the thirty questions once more.
pub(crate) mod plain_cube {
    use primitive_shared::types::BlockId;
    use std::sync::atomic::{AtomicU64, Ordering};

    static KNOWN: [AtomicU64; 1 << 10] = [const { AtomicU64::new(0) }; 1 << 10];

    #[inline]
    pub fn known(id: BlockId) -> bool {
        #[cfg(test)]
        if ASK_EVERYTHING.with(std::cell::Cell::get) {
            return false;
        }
        KNOWN[usize::from(id) >> 6].load(Ordering::Relaxed) & (1 << (id & 63)) != 0
    }

    #[inline]
    pub fn learn(id: BlockId) {
        let word = &KNOWN[usize::from(id) >> 6];
        let bit = 1 << (id & 63);
        // A load first: nearly every call is for a bit already set, and a
        // read-modify-write on a word every thread reads would bounce the
        // cache line between them.
        if word.load(Ordering::Relaxed) & bit == 0 {
            word.fetch_or(bit, Ordering::Relaxed);
        }
    }

    #[cfg(test)]
    thread_local! {
        /// Set by a test to mesh on this thread as if nothing were known,
        /// whatever the other tests have taught the table meanwhile.
        pub static ASK_EVERYTHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
}

pub fn build_mesh(
    pos: ChunkPos,
    cache: &Neighbourhood,
    layers: &FaceLayers,
    world: &primitive_shared::worldgen::WorldGen,
    out: &mut MeshBuffers,
) {
    out.clear();
    // Before the empty-chunk return below: an empty chunk's flag is read
    // like any other's.
    out.leaves_solid = cache.is_coarse() || cache.leaves_solid;
    // **The corners of every face that does not go through the merge.**
    // A face lit unevenly at its corners -- a crease, a step, the foot
    // of a wall -- is drawn as itself, one quad, and never enters the
    // rectangle list. Its corners are corners all the same, and a merged
    // rectangle beside it has to take a vertex where they land on its
    // edge; the lattice `mark_corners` builds is made from the
    // rectangles alone, so these are collected here and handed in. See
    // the note there for the forty-five holes that were this.
    let mut direct_marks: Vec<u64> =
        vec![0u64; ((CHUNK_SIZE_X + 1) * (CHUNK_SIZE_Y + 1) * (CHUNK_SIZE_Z + 1)).div_ceil(64)];
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
        solid_groups,
        up_faces_from,
        down_faces_to,
        top,
        // Written before the destructure, above.
        leaves_solid: _,
        by_face,
        mergeable,
        covered,
    } = out;
    let textures = layers;

    let origin_x = pos.x * CHUNK_SIZE_X as i32;
    let origin_z = pos.z * CHUNK_SIZE_Z as i32;
    let face_defs = faces();
    // Taken once for the whole chunk rather than per sample: see
    // `cover_table`.
    let cover_table = cover_table();
    // Asked once for the chunk rather than six times per cell: it is one
    // flag on the snapshot, and the face loop is the hottest code here.
    let coarse = cache.is_coarse();
    // A stone's thickness, or the flat quad: near, the relief; far, and in
    // any chunk `lod::coarsen` rewrote, the quad. See `lod::relief_at` for
    // where near ends and what it measured.
    let reliefs_here = !coarse && !cache.stones_lie_flat;
    // A crown's inside, or only its outside: see `face_visible` and
    // `MeshBuffers::leaves_solid`.
    let shell_crowns = coarse || cache.leaves_solid;

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
                    // ...or what is on its surface, for the few blocks that
                    // carry that in their variant. See `surface_tint`.
                    surface_tint(id).unwrap_or(0)
                };

                // Plants are not cubes. Two quads on the cell's
                // diagonals, no face culling to do (there are no faces
                // to hide) and no ambient occlusion (nothing to occlude
                // against) -- so they leave the cube loop entirely.
                // A stone lying on the ground: one quad, flat, and
                // that is the whole model. Cheapest thing the mesher
                // emits, which is what lets it appear in every biome.
                // **Something standing in the sea is drawn twice: the
                // thing, and then the sea.** Kelp, seagrass, a coral that
                // grows as a sprite, a shell -- all liquid by their rows
                // (`types::BLOCK_KELP`), so every neighbour of one already
                // treats the cell as water: the faces two cells of sea share
                // are culled against it, the surface averages its corners
                // with it, the depth under a face counts it. What the cell
                // itself still owes is the sprite, which goes in the cutout
                // pass as any tuft does, and its own share of the water,
                // which is the cube path below with the plant taken out of
                // the id. Drawing only the sprite left a hole in the sea the
                // shape of every stem; drawing only the water hid the forest.
                // **Drowned wood is the same bargain with a post for a
                // sprite** (`types::BLOCK_DROWNED_BOUGH`): the snag's bark,
                // joined to the dry piece over the surface, and the pool.
                let id = if primitive_shared::types::stands_in_water(id) {
                    if primitive_shared::types::is_branch(id) {
                        branch_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            branch_joins(cache, cell, y),
                            textures,
                            cache.light_near(cell, y, 0, 0, 0),
                            vertices,
                            indices,
                        );
                    } else if is_flat(id) {
                        flat_block(
                            [gx, y, gz],
                            [x as f32, y as f32 - rest_drop(id, cache.block_near(cell, y, 0, -1, 0)), z as f32],
                            id,
                            textures.layer_for_face(id, 0),
                            if reliefs_here { textures.relief(id) } else { None },
                            cache.light_near(cell, y, 0, 0, 0),
                            vertices,
                            sprites,
                        );
                    } else {
                        cross_block(
                            [gx, y, gz],
                            [x as f32, y as f32 - cross_drop(cache, cell, y, id), z as f32],
                            id,
                            textures.layer_for_face(id, 0),
                            cache.light_near(cell, y, 0, 0, 0),
                            tint,
                            vertices,
                            sprites,
                        );
                    }
                    primitive_shared::types::BLOCK_WATER
                } else if trap_in_water(cache, cell, y, id) {
                    // **A fish trap set in water is the same bargain from the
                    // other side**: solid to the rules on purpose (see
                    // `types::BLOCK_FISH_TRAP` -- the flow must not rewrite
                    // it), and wicker to the eye, so the eye has to find
                    // water in it. Drawn as the cube path drew it -- the
                    // cell's own picture on six faces, in the cutout pass,
                    // lit flat -- and then its water through the cube path.
                    // See `trap_in_water` for what the water round it does.
                    let open = (0..6u8).fold(0u8, |open, face| {
                        let n = face_defs[face as usize].neighbor;
                        let beside = cache.block_near(cell, y, n[0], n[1], n[2]);
                        // Buried in a full neighbour, or the second of two
                        // traps' shared face: drawn once, by the lower, as
                        // `face_visible` draws a cutout against its own kind.
                        let hidden = cover_table[beside as usize] == FULL_COVER
                            || (face % 2 == 1
                                && primitive_shared::types::block_kind(beside)
                                    == primitive_shared::types::BLOCK_FISH_TRAP);
                        open | (u8::from(hidden) << face)
                    });
                    let light = cache.light_near(cell, y, 0, 0, 0);
                    push_box_faces(
                        [x as f32, y as f32, z as f32],
                        [0.0; 3],
                        [16.0; 3],
                        0,
                        std::array::from_fn(|face| textures.layer_for_face(id, face)),
                        true,
                        light & 0x0F,
                        (light >> 4) & 0x0F,
                        open,
                        vertices,
                        leaves,
                    );
                    primitive_shared::types::BLOCK_WATER
                } else if crown_in_water(cache, cell, [x, y, z], id) {
                    // **A bush standing in a pond holds the pond**, and it is
                    // the trap's bargain again (`trap_in_water`): a crown is
                    // solid to the rules, so the flow stops at its faces, and
                    // what that drew was a cell of air in the water walled by
                    // the surface of every cell beside it -- a bubble round
                    // the bush, and from above a dry square hole in the pond
                    // with leaves in it. "сделай затопление листвы".
                    //
                    // So the cell draws both: the crown, here, exactly as the
                    // cube path would have drawn it, and then its own share of
                    // the water through the cube path below. The water round
                    // it draws no wall into it (`seen_by_water`) and counts it
                    // as depth (`liquid_depth_below`), so a submerged bush is
                    // inside one body of water rather than beside one.
                    //
                    // **The faces are `face_visible`'s answer and not a rule
                    // of their own.** The trap works out its own hidden mask
                    // because a trap meets a trap and nothing else; a crown
                    // meets the crown above it, the crown of another tree
                    // grown into it, and a picked apple cell -- all of which
                    // `face_visible` already has an answer for, and an answer
                    // written here beside it would be the coplanar pair that
                    // shimmers round every apple the moment the two drift
                    // apart.
                    //
                    // Lit flat, as the trap is: a submerged crown gives up
                    // the smooth corner light the cube path would have given
                    // it, which is a quarter of a shade under water that is
                    // itself tinted by depth.
                    let cover = cover_table[id as usize];
                    let hidden = (0..6u8).fold(0u8, |hidden, face| {
                        let n = face_defs[face as usize].neighbor;
                        let beside = cache.block_near(cell, y, n[0], n[1], n[2]);
                        let cover_there = cover_table[beside as usize];
                        let drawn = face_visible(id, cover, beside, cover_there, face as usize, shell_crowns);
                        hidden | (u8::from(!drawn) << face)
                    });
                    let light = cache.light_near(cell, y, 0, 0, 0);
                    push_box_moved(
                        [x as f32, y as f32, z as f32],
                        [0.0; 3],
                        [16.0; 3],
                        0,
                        None,
                        std::array::from_fn(|face| textures.layer_for_face(id, face)),
                        true,
                        light & 0x0F,
                        (light >> 4) & 0x0F,
                        tint,
                        hidden,
                        vertices,
                        leaves,
                    );
                    primitive_shared::types::BLOCK_WATER
                } else {
                    id
                };

                // **Every model this id could be, asked once per id rather
                // than once per cell.** A cell of stone went through thirty
                // questions below before reaching the cube it always is; see
                // `plain_cube` for what that cost and why the answer can be
                // kept. The two flames between the parts stay unasked-for
                // by it -- they draw *and* fall through to the cube.
                let plain = plain_cube::known(id);
                if !plain && is_flat(id) {
                    // **On the real top of what it lies on**: a lip on a
                    // slope, a floor dug down (`types::rest_drop`). From the
                    // floor of its own cell the snow on every lip of a
                    // winter hillside hung a quarter of a block over the
                    // grass, with the turf showing through the gap.
                    flat_block(
                        [gx, y, gz],
                        [x as f32, y as f32 - rest_drop(id, cache.block_near(cell, y, 0, -1, 0)), z as f32],
                        id,
                        textures.layer_for_face(id, 0),
                        if reliefs_here { textures.relief(id) } else { None },
                        cache.light_near(cell, y, 0, 0, 0),
                        vertices,
                        sprites,
                    );
                    continue;
                }

                // **A fire burns upward, so it is drawn upward.**
                //
                // The stones and the charred wood are the cube this
                // block already was; the flame is two quads crossing on
                // the cell's diagonals, exactly the way a tuft of grass
                // is drawn -- and for the same reason, which is that a
                // flame is a shape with no thickness that has to look
                // like something from every side.
                //
                // It used to be a picture on the *top face* of the cube:
                // fire seen from directly above, on a block four pixels
                // tall, which from standing height is a bright smear on
                // the ground.
                //
                // Emitted before the cube rather than instead of it --
                // both happen, and the flame goes in the sprite pass
                // where the alpha is cut out.
                if matches!(
                    primitive_shared::types::block_kind(id),
                    primitive_shared::types::BLOCK_CAMPFIRE_LIT | primitive_shared::types::BLOCK_FIREPIT_LIT
                ) {
                    flame_block(
                        [gx, y, gz],
                        [x as f32, y as f32, z as f32],
                        textures,
                        cache.light_near(cell, y, 0, 0, 0),
                        vertices,
                        sprites,
                    );
                }

                // **Wood that has caught burns on its top**: the hearth's own
                // two sheets of flame, stood on the block rather than in it,
                // and only where the cell over it is open -- a burning wall
                // under a roof burns against the roof, and a flame drawn
                // into a solid block is a flame nobody could see anyway.
                // Emitted before the cube, which is drawn as ever with the
                // char picture its row names (`wildfire::is_blazing`).
                if primitive_shared::wildfire::is_blazing(id)
                    && cover_table[cache.block_near(cell, y, 0, 1, 0) as usize] != FULL_COVER
                {
                    flame_block(
                        [gx, y, gz],
                        [x as f32, y as f32 + 1.0 - FLAME_FOOT, z as f32],
                        textures,
                        cache.light_near(cell, y, 0, 1, 0),
                        vertices,
                        sprites,
                    );
                }

                if !plain {
                    // **A standing torch is a pole**, drawn as one and instead of
                    // the cube, with its flame on the top cell. See
                    // `standing_torch_block`.
                    if primitive_shared::wildfire::is_standing_torch(id) {
                        standing_torch_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        if primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_STANDING_TORCH_LIT {
                            flame_block(
                                [gx, y, gz],
                                [x as f32, y as f32 + TORCH_FLAME_RISE, z as f32],
                                textures,
                                cache.light_near(cell, y, 0, 0, 0),
                                vertices,
                                sprites,
                            );
                        }
                        continue;
                    }

                    // **A wild hive is a comb on a trunk**, drawn against the
                    // wall its bits name and not as a cube of its cell. See
                    // `types::hive_side`.
                    if primitive_shared::bees::is_hive(id) {
                        let light = model_light(cache, cell, y, cover_table);
                        let quarters = turned_from_north(primitive_shared::types::hive_side(id));
                        push_box_faces(
                            [x as f32, y as f32, z as f32],
                            [2.0, 0.0, 7.0],
                            [14.0, 16.0, 16.0],
                            quarters,
                            std::array::from_fn(|face| textures.layer_for_face(id, face)),
                            true,
                            light & 0x0F,
                            (light >> 4) & 0x0F,
                            0,
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // **A pit prop is a post**, drawn where it is collided
                    // (`geometry::block_box`, `types::BLOCK_PROP`).
                    // **A window lattice is a panel of slats** across the middle
                    // of its cell (`types::lattice_box`), into the sprites'
                    // range: its picture is mostly holes, and the solid pass
                    // fills a hole with shade rather than letting it through.
                    if primitive_shared::types::is_lattice(id) {
                        let light = model_light(cache, cell, y, cover_table);
                        let (min, max) = primitive_shared::types::lattice_box(id);
                        push_box(
                            [x as f32, y as f32, z as f32],
                            min.map(|v| v * 16.0),
                            max.map(|v| v * 16.0),
                            0,
                            textures.layer_for_face(id, 2),
                            true,
                            light & 0x0F,
                            (light >> 4) & 0x0F,
                            vertices,
                            sprites,
                        );
                        continue;
                    }

                    if primitive_shared::types::is_prop(id) {
                        let light = model_light(cache, cell, y, cover_table);
                        // In the bark of the wood it was cut from (`types::carries_wood`):
                        // one picture for every prop was an oak post in a birch
                        // mine.
                        let layer = textures.layer_for_face(
                            primitive_shared::wood::WOODS[primitive_shared::types::furniture_wood(id)].log,
                            2,
                        );
                        // The post is where the collider has it, by the one
                        // function both ask (`types::prop_box`); unturned,
                        // because the box is already where it stands.
                        let (min, max) = primitive_shared::types::prop_box(id);
                        push_box(
                            [x as f32, y as f32, z as f32],
                            min.map(|v| v * 16.0),
                            max.map(|v| v * 16.0),
                            0,
                            layer,
                            true,
                            light & 0x0F,
                            (light >> 4) & 0x0F,
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // **A stake is a pole**, stood up or driven into the wall it
                    // is turned toward. See `stake_block`.
                    if primitive_shared::types::is_stake(id) {
                        stake_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // **A door is its slab of boards**, instead of the cube its
                    // row describes. See `door_block`.
                    if primitive_shared::types::is_door(id) {
                        // Each broad face lit from the room it faces: see
                        // `door_block`. A side that looks into a wall falls back
                        // to the brighter of the neighbours, as the edges do.
                        let edges = model_light(cache, cell, y, cover_table);
                        let facing_room = |toward_back: bool| {
                            let (dx, dz) = door_face_offset(id, toward_back);
                            if cover_table[cache.block_near(cell, y, dx, 0, dz) as usize] == FULL_COVER {
                                edges
                            } else {
                                cache.light_near(cell, y, dx, 0, dz)
                            }
                        };
                        door_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            [edges, facing_room(false), facing_room(true)],
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // **A step is its two boxes**, instead of the cube its row
                    // describes. See `step_block`.
                    if primitive_shared::types::is_step(id) {
                        step_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            |dx, dy, dz| cache.block_near(cell, y, dx, dy, dz),
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // **A spike of dripstone is its tiers**, instead of the cube
                    // its row describes. See `dripstone_block`.
                    if primitive_shared::dripstone::is_dripstone(id) {
                        dripstone_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // A rack is a frame of poles with a skin in it, which
                    // is not a cube however it is textured. Drawn here and
                    // *instead of* the cube, unlike the flame above it. The hide
                    // frame is never whole, so always `RackColumns::Lone`, and
                    // `rack_block` draws it as the laced frame it is.
                    if primitive_shared::rack::is_rack(id) {
                        let columns = rack_columns(cache, cell, y, id);
                        rack_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            columns,
                            textures,
                            // Not its own cell's: see `model_light`.
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // **A thing set down is not drawn here at all**: it is an
                    // item model, lying, and the frame draws it with the dropped
                    // stacks (`entities::build_set_down_into`). A cube an eighth
                    // tall under it would be a paving slab under every knife.
                    if primitive_shared::types::is_set_down(id) {
                        continue;
                    }

                    // A pit kiln is pots, fibre and logs stacked in a hole -- see
                    // `pit_kiln_block` -- and drawn instead of the cube, like the
                    // rack. Alight, it wears the hearth's flame, stood on the
                    // logs rather than buried in them.
                    if primitive_shared::pit::is_pit_kiln(id) {
                        pit_kiln_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            cache.pottery_at((gx, y, gz)),
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        if primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_PIT_KILN_LIT {
                            flame_block(
                                [gx, y, gz],
                                [x as f32, y as f32 + PIT_FLAME_RISE, z as f32],
                                textures,
                                cache.light_near(cell, y, 0, 0, 0),
                                vertices,
                                sprites,
                            );
                        }
                        continue;
                    }

                    // An unlit pile of logs is the logs, lying -- see
                    // `log_pile_block`. Alight it stays the cube of fire its
                    // row describes, and goes down the path below.
                    if primitive_shared::pit::pile_extent(id).is_some() {
                        log_pile_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // A carcass is the animal lying where it fell -- the
                    // animal's own model, rolled on its side and baked into
                    // the chunk -- and not the low cube its table row
                    // collides as. See `animal_model::build_fallen`. The yaw
                    // is hashed from the world cell so that two kills in one
                    // meadow do not lie parallel, and so the same carcass
                    // faces the same way after every remesh.
                    if let Some(species) = primitive_shared::animals::Species::of_carcass(id) {
                        let packed = model_light(cache, cell, y, cover_table);
                        let yaw =
                            crate::logic::animal_model::carcass_yaw(origin_x + x, y, origin_z + z);
                        crate::logic::animal_model::build_fallen(
                            species,
                            glam::Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5),
                            yaw,
                            primitive_shared::animals::butchering_stage(id),
                            textures,
                            (packed & 15, (packed >> 4) & 15),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // A dead player is the player, lying down -- the figure
                    // everybody else walks around in, rolled onto its side
                    // and baked into the chunk exactly as a carcass is (see
                    // `player_model::build_fallen`, which argues the whole of
                    // it). Same yaw hash as a carcass, for the same three
                    // reasons: two bodies in one clearing do not lie
                    // parallel, a body faces the same way after every remesh,
                    // and the mining cracks land on it rather than beside it.
                    //
                    // **Only the bones are baked now.** A body wears the player's
                    // own skin and clothes, which the terrain vertex cannot reach,
                    // and is drawn with the other figures
                    // (`player_model::append_lying`); `build_fallen` emits nothing
                    // for one, and the cell keeps its floor (`drawn_as_model`).
                    if let Some(stage) = crate::logic::player_model::Dead::of(id) {
                        let packed = model_light(cache, cell, y, cover_table);
                        let yaw =
                            crate::logic::animal_model::carcass_yaw(origin_x + x, y, origin_z + z);
                        crate::logic::player_model::build_fallen(
                            stage,
                            glam::Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5),
                            yaw,
                            textures,
                            (packed & 15, (packed >> 4) & 15),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // A jug is a vessel, not a cube of clay: see `jug_block`.
                    if primitive_shared::types::block_kind(id)
                        == primitive_shared::types::BLOCK_JUG
                    {
                        jug_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // **A palm's crown is fronds and fruit, not cubes of leaf.**
                    // What each cell is -- the heart, a length of a frond, a bunch
                    // of coconuts -- is read off the cells round it: see
                    // `CrownPart`.
                    if matches!(
                        primitive_shared::types::block_kind(id),
                        primitive_shared::types::BLOCK_PALM_FRONDS | primitive_shared::types::BLOCK_PALM_COCONUTS
                    ) {
                        let near = |dx: i32, dy: i32, dz: i32| cache.block_near(cell, y, dx, dy, dz);
                        palm_crown_block(
                            [gx, y, gz],
                            [x as f32, y as f32, z as f32],
                            id,
                            (crown_part(near), near),
                            textures,
                            (cache.light_near(cell, y, 0, 0, 0), model_light(cache, cell, y, cover_table)),
                            tint,
                            vertices,
                            indices,
                            sprites,
                        );
                        continue;
                    }
                    // A piece of an experimental tree is a post of bark joined
                    // to the pieces beside it: see `branch_block` and
                    // `branch_joins`. A palm is one leaning curve rather than
                    // posts with arms: see `palm::PalmCourse`. Asked before the
                    // branch path below, which would draw the palm's steps as
                    // right angles.
                    if primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_PALM_TRUNK {
                        palm_trunk_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            |dx, dy, dz| cache.block_near(cell, y, dx, dy, dz),
                            textures,
                            cache.light_near(cell, y, 0, 0, 0),
                            vertices,
                            indices,
                        );
                        continue;
                    }
                    if primitive_shared::types::is_branch(id) {
                        let joins = branch_joins(cache, cell, y);
                        branch_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            joins,
                            textures,
                            cache.light_near(cell, y, 0, 0, 0),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // A barrel is staves round water, not a cube of wood:
                    // see `barrel_block`.
                    if primitive_shared::types::is_barrel(id) {
                        barrel_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // A bracket fungus is a shelf on the side of the cell,
                    // not a cross standing in it: see `bracket_block`.
                    if primitive_shared::types::block_kind(id)
                        == primitive_shared::types::BLOCK_BRACKET_FUNGUS
                    {
                        bracket_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // A cairn is three stones stacked: see `cairn_block`.

                    if primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_CAIRN {

                        cairn_block(

                            [x as f32, y as f32, z as f32],

                            id,

                            textures,

                            model_light(cache, cell, y, cover_table),

                            vertices,

                            indices,

                        );

                        continue;

                    }


                    // A nest is a bowl of twigs with eggs in it, and both
                    // halves are model rather than tile: see `nest_block`.
                    if matches!(
                        primitive_shared::types::block_kind(id),
                        primitive_shared::types::BLOCK_NEST
                            | primitive_shared::types::BLOCK_NEST_EGGS
                    ) {
                        nest_block(
                            [x as f32, y as f32, z as f32],
                            id,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // ...and a skeleton is what is left of a carcass nobody
                    // came back for: not the animal's model in bone but its
                    // bones, built from that model's proportions (see
                    // `animal_model::skeleton_parts`). Same yaw hash, so the
                    // skull lies at the end the head did rather than the
                    // whole thing turning as it rots.
                    if let Some(species) = primitive_shared::types::species_in_bones(id) {
                        let packed = model_light(cache, cell, y, cover_table);
                        let yaw =
                            crate::logic::animal_model::carcass_yaw(origin_x + x, y, origin_z + z);
                        crate::logic::animal_model::build_bones(
                            species,
                            glam::Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5),
                            yaw,
                            textures,
                            (packed & 15, (packed >> 4) & 15),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    // Furniture is boxes -- boards, poles, straw, a blanket --
                    // and not the part-height cube its row collides as: see
                    // `furniture_block`. The other half of a bed is looked for
                    // here because only here are the neighbours in reach, and
                    // the seam between the two is only left open where it is.
                    if is_furniture(id) {
                        let partnered = primitive_shared::types::bed_partner((gx, y, gz), id)
                            .is_some_and(|(other, expected)| {
                                cache.block_near(cell, y, other.0 - gx, 0, other.2 - gz) == expected
                            });
                        // **A chest somebody has open leaves its lid out**: the
                        // frame is swinging that lid this very frame
                        // (`chest_lid_block`), and a lid drawn here as well would
                        // be a second one lying shut through it.
                        let hinged = if cache.lid_swings_at((gx, y, gz)) {
                            Hinged::Bodied
                        } else {
                            Hinged::Whole
                        };
                        furniture_block_hinged(
                            [x as f32, y as f32, z as f32],
                            id,
                            partnered,
                            hinged,
                            textures,
                            model_light(cache, cell, y, cover_table),
                            vertices,
                            indices,
                        );
                        continue;
                    }

                    if is_cross(id) {
                        // **On the real top of what it grows on**
                        // (`types::stand_drop`): a flower on a turf lip was
                        // drawn from its own cell's floor, a quarter of a
                        // block over the grass. The light is still its own
                        // cell's, which is the air the plant stands in, and
                        // the shadow pass draws these same vertices.
                        cross_block(
                            [gx, y, gz],
                            [x as f32, y as f32 - cross_drop(cache, cell, y, id), z as f32],
                            id,
                            textures.layer_for_face(id, cross_face(id)),
                            cache.light_near(cell, y, 0, 0, 0),
                            tint,
                            vertices,
                            sprites,
                        );
                        continue;
                    }
                    // Through every model above without one taking the cell:
                    // nothing there will take this id either. See `plain_cube`.
                    plain_cube::learn(id);
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
                // standing in it looks like standing *on* it.
                //
                // *Loose material* is drawn at exactly the depth it is,
                // which is the whole point of layers: what you see is
                // what you walk on, because both come from
                // `block_height`.
                //
                // **And a liquid with a liquid under it hangs below its
                // own floor**, by exactly the drop, so that a column of
                // water is one unbroken box from the bed to the surface.
                // Something has to close that seam or a deep lake shows
                // one across every layer of it; the other way round --
                // the *lower* cell reaching up to the top of its cell --
                // is what this used to do, and it is the bug a player
                // photographed. See `fluid::underhang`, which carries
                // the whole argument.
                // What is left of a block a digger has been working at, or
                // `None` for the whole block every other cell is. Read once
                // per block: the face loop below is the hottest code in the
                // client and this is a bit test for all but a handful of
                // cells in a world.
                // ...or of a wall built part of the way up (`build`), which
                // is the same box the other way up and is drawn the same way.
                let bite = primitive_shared::dig::part_box(id);
                // **A stage wears the picture of what it looks like**: three
                // courses in mortar are brickwork, a wet lift of cob is mud
                // (`build::drawn_as`). The id itself for everything else.
                let pictured = primitive_shared::build::drawn_as(id);
                let above = cache.block_near(cell, y, 0, 1, 0);
                // A trap standing in the water is water to the water round
                // it (`seen_by_water`), so a column through one is still one
                // box.
                let above = if is_liquid(id) { seen_by_water(cache, cell, [x, y, z], [0, 1, 0], above) } else { above };
                // Only water asks, and this is the hottest loop there is:
                // a dry block does not pay a read for it.
                let below = if is_liquid(id) {
                    seen_by_water(cache, cell, [x, y, z], [0, -1, 0], cache.block_near(cell, y, 0, -1, 0))
                } else {
                    BLOCK_AIR
                };
                let (bottom, top) = if is_liquid(id) {
                    (
                        -primitive_shared::fluid::underhang(id, below),
                        // A slice of a column reaches the cell above's
                        // underhang whatever its own depth, and so does
                        // a cell with air under it, which is falling; a
                        // surface is drawn at its depth. See
                        // `fluid::drawn_top`.
                        primitive_shared::fluid::drawn_top(id, above, below),
                    )
                } else if let Some((min, max)) = bite {
                    // **A block being quarried is the box of what is left
                    // of it** (`dig::bite_box`), and the box is the one
                    // `geometry::block_box` hands the collider -- which is
                    // the whole of "a partial block is drawn where it is
                    // collided". A bite out of the underside is the one
                    // shape here whose floor is not the cell's floor.
                    (min[1], max[1])
                } else {
                    (0.0, block_height(id))
                };

                // **A spill is a slope, not a staircase.** Every cell of
                // water is drawn at its own depth (`fluid::surface_height`),
                // and two cells a few eighths apart would meet in a step
                // with a wall down it -- the picture that once made every
                // depth draw at one height. So a surface cell's four top
                // corners are each the average of the surface heights of
                // the liquid cells around that corner, at this level.
                // Both cells either side of a shared corner average the
                // same four cells, so they meet exactly and no wall is
                // needed: between two cells of water no face is drawn at
                // all (see `face_visible`), and the mesh is still
                // watertight. A corner with air on three sides is the
                // cell's own height, so a lone puddle keeps its edge.
                //
                // Only for a cell with no water above it. A cell inside
                // a column is full, its top is never drawn, and its sides
                // meet the cell above at the plain height -- averaging
                // them would open a sliver against that cell's underhang.
                // A neighbour that *does* have water above counts at its
                // own height rather than the top of its cell for the same
                // reason: the cell above it hangs down to exactly there.
                //
                // Nor for a cell with air under it. That is water falling,
                // drawn whole (`fluid::is_falling`), and sloping its top to
                // the lake it is dropping past would tilt the front of a
                // fall into a wedge -- a plate again, at an angle.
                let liquid_corners: Option<[f32; 4]> =
                    if is_liquid(id)
                        && !is_liquid(above)
                        && !primitive_shared::fluid::is_lid(above)
                        && !primitive_shared::fluid::is_falling(id, below)
                    {
                        Some(std::array::from_fn(|k| {
                            let (cx, cz) = ((k / 2) as i32, (k % 2) as i32);
                            let (mut sum, mut count) = (0.0f32, 0.0f32);
                            for dx in cx - 1..=cx {
                                for dz in cz - 1..=cz {
                                    let near = cache.block_near(cell, y, dx, 0, dz);
                                    // A trap counts as the full water it is
                                    // drawn holding, whether this cell can
                                    // see that it holds any or not: one
                                    // diagonal away its far sides are past
                                    // the padding, and both cells at a corner
                                    // have to average the same four or the
                                    // surface opens along the seam. Only a
                                    // spill short of full can tell.
                                    // **...and so does a crown**, for the same
                                    // reason and one more: the flooded crown
                                    // is the cell asking (`crown_in_water`),
                                    // and it is not water by its id. Where
                                    // four cells of a sunk bush met at a
                                    // corner, none of the four counted, the
                                    // corner came out 0/0 -- NaN -- and the
                                    // GPU dropped every triangle of the
                                    // surface that touched it: the pond open
                                    // over the middle of every bush in it.
                                    let near = if primitive_shared::types::block_kind(near) == primitive_shared::types::BLOCK_FISH_TRAP
                                        || cubed_crown(near)
                                    {
                                        primitive_shared::types::BLOCK_WATER
                                    } else {
                                        near
                                    };
                                    if is_liquid(near) {
                                        sum += primitive_shared::fluid::surface_height(near);
                                        count += 1.0;
                                    }
                                }
                            }
                            sum / count
                        }))
                    } else {
                        None
                    };

                let translucent_flag = if is_translucent(id) { TRANSLUCENT_BIT } else { 0 };
                // **How much water is under this face**, in cells.
                //
                // The byte a vertex carries is a foliage tint; on a
                // *translucent* face it is not, because water is not
                // foliage. So this is the byte's second reading, and
                // the two cannot meet: the translucent bit says which
                // one is in force.
                //
                // What it is for: the sea used to be a sheet of
                // constant transparency, so ten blocks of water hid its
                // bed exactly as poorly as a puddle. Skylight fell
                // three levels per block of water then (`light_opacity`
                // of water was 2, and a step costs one more; it is two a
                // block now -- see water's row), so a gently
                // shelving sea bed comes out in hard brightness
                // terraces -- and through a 28% window those terraces
                // read as light dots and short horizontal dashes
                // scattered over dark blue, thicker toward the horizon
                // where perspective packs more of them into a pixel,
                // and twinkling whenever the camera turns. A player
                // photographed exactly that and called it white noise.
                // See `WATER_DEPTH_FADE` in shader.wgsl for what the
                // number is used for.
                // Worked out at the first face that needs it rather
                // than up here for every liquid cell -- and, since the
                // fade belongs to the surface alone (see the emission
                // below), the only face that ever needs it is the top
                // one.
                //
                // **Measured**, on the worst chunk there is for it:
                // sixteen blocks of ocean in every column, four
                // thousand water cells of which two hundred and
                // fifty-six draw anything -- all the rest are enclosed
                // by more water and emit no face at all. Minimum of
                // four hundred meshes: 0.365 ms with no counting,
                // 0.555 ms counting for every liquid cell, 0.407 ms
                // counting only for the cells that draw. So the feature
                // costs a ninth of a chunk that is nothing but sea, and
                // nothing whatever for a chunk with no water in it. See
                // `what_the_water_depth_scan_costs`.
                let mut water_depth: Option<u32> = None;
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
                // **A coarse chunk is lit flat, and this is what makes
                // the whole detail level worth having.**
                //
                // Smooth lighting and ambient occlusion are computed per
                // *cell*, and the greedy merge will only join faces
                // whose four corners agree exactly (see `Mergeable`). On
                // natural terrain almost every top face has a step
                // somewhere along its edge, so almost every one of them
                // has a corner darker than the other three -- which is
                // why a chunk of meadow merges 256 top faces into about
                // 240 quads rather than into one.
                //
                // Coarsening the blocks alone therefore bought nothing:
                // measured at 7% off the solid pass, because the
                // simplified world was still shaded cell by cell and
                // still could not merge. Lit flat, the same coarse world
                // gives up a third of its triangles, and what is thrown
                // away is a corner gradient across a face four pixels
                // wide at a hundred and sixty blocks, behind the near
                // end of the fog.
                //
                // **A leaf, not anything in the cutout pass.** This asked
                // `is_cutout`, which was the same question while every cube
                // in that pass was a leaf. Ice is in it now -- light passes
                // it, so the table calls it see-through (`types::is_cutout`)
                // -- and it inherited all of the canopy's bargain: lit flat
                // from the cell in front, no corner shade, and two levels
                // taken off for the tree it is not part of. A frozen bay came
                // out a flat grey sheet darker than the snow on its shore, and
                // a torch on the ice lit it in hard squares a block wide. Ice
                // is ground, and is lit as ground.
                let canopy = primitive_shared::types::is_leafy(id);
                let flat_lit = canopy || coarse;
                // Which way this block lies, for the UV turn below. Per
                // block, not per face: it is a function of the id alone.
                let axis = primitive_shared::types::block_axis(id);
                // **Moss, on the north face** (`ground::MOSSY`), and on the top
                // of a stone, which the rain reaches as the north reaches a
                // trunk. Per block the question, per face the answer. Only
                // where the block wears no other surface tint -- a sooted
                // mossy stone is not a thing the world makes.
                let mossy = tint == 0 && primitive_shared::ground::is_mossy(id);
                let moss_on_top = mossy && !primitive_shared::wood::is_log(id);
                // **A hillside of turf is green from its foot**
                // (`ground::turf_wraps_the_side`): a side face whose cell
                // diagonally below is the same turf wears the top's own
                // picture instead of soil with a fringe. Asked per block
                // first, because `is_turf` is a two-way match and this is the
                // hottest loop in the client -- the four reads below are paid
                // only on turf whose top is showing, which is the surface of
                // a meadow and nothing else.
                let turf_sides = primitive_shared::ground::turf_may_wrap(id, above);

                #[cfg(test)]
                let faces_started = phase_clock::started();
                for (face_index, face) in face_defs.iter().enumerate() {
                    // **Moss is a picture, not a wash.** The face wears the
                    // block's own side with moss grown over it
                    // (`texture::mossy`); the tint is what it used to be and
                    // is kept only for a pack whose atlas has no such picture.
                    let moss_here = mossy && (face_index == 5 || (face_index == 0 && moss_on_top));
                    let moss_layer = moss_here.then(|| textures.mossy(id)).flatten();
                    let tint = if moss_here && moss_layer.is_none() { MOSS_TINT } else { tint };
                    let n = face.neighbor;
                    // Once, not twice. The id and its cover come from
                    // the same cell, and looking it up again for the
                    // second of them is a redundant load on the hottest
                    // loop in the client -- six per block, every block.
                    let neighbor_id = cache.block_near(cell, y, n[0], n[1], n[2]);
                    let neighbor_id = if is_liquid(id) {
                        let seen = seen_by_water(cache, cell, [x, y, z], n, neighbor_id);
                        // The water a flooded crown holds, looking at the
                        // crown beside it: see `dry_beside_a_flooded_crown`.
                        if seen != neighbor_id
                            && cubed_crown(cache.blocks[cell])
                            && dry_beside_a_flooded_crown(cache, cell, [x, y, z], n, neighbor_id)
                        {
                            neighbor_id
                        } else {
                            seen
                        }
                    } else {
                        neighbor_id
                    };
                    let neighbor_cover = cover_table[neighbor_id as usize];
                    // **A part-height cube keeps its underside**, and the
                    // reason is the hair it is grown by (see `reach`
                    // below). Grown, a side standing at the brink of a
                    // drop no longer meets the wall under it edge to edge:
                    // it stands a hair in front of it, and a ray from
                    // below, climbing more steeply than the hair is wide,
                    // passes between the two into a cell whose floor this
                    // block culled. The underside is what that ray meets
                    // instead. One quad per hearth and per pack, seen from
                    // nowhere else: from above it is a back face.
                    // ...and a block with a bite out of it is part-height
                    // in the sense this flag means -- a cube that does not
                    // fill its cell -- whichever axis the bite came out of.
                    // Its underside is kept for the same ray that reaches
                    // under a hearth, and its untouched sides are grown by
                    // the same hair.
                    let part_height = (top < 1.0 || bite.is_some()) && !is_liquid(id);
                    if !(part_height && face_index == 1)
                        && !face_visible(id, cover, neighbor_id, neighbor_cover, face_index, shell_crowns)
                    {
                        continue;
                    }

                    // A wrapped turf side wears face 0 -- the top's own
                    // picture -- and nothing else about the face changes: it
                    // keeps its normal, its light, its climate tint and its
                    // crop, because it is still the side of a block.
                    let wrapped = turf_sides
                        && face_index >= 2
                        && primitive_shared::ground::turf_wraps_the_side(
                            id,
                            above,
                            neighbor_id,
                            cache.block_near(cell, y, n[0], -1, n[2]),
                        );
                    let layer = moss_layer.unwrap_or_else(|| {
                        textures.layer_for_face(pictured, if wrapped { 0 } else { face_index })
                    });

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
                    //
                    // **The top of a part-height block under a solid one
                    // is lit from its own cell.** That top is *inside* its
                    // cell -- a campfire's stones stand a quarter of the
                    // way up it -- so the air in front of it is the rest
                    // of its own cell, not the cell above. Read from the
                    // cell above, which is what every other face does and
                    // right for them, a block laid over a campfire handed
                    // the fire's top the nought inside that block: "if a
                    // block stands above a campfire it turns black", and
                    // the ring of the hearth went black under it while the
                    // flame beside it burned at full light. Only when the
                    // cell above is solid, because under open air the cell
                    // above is the brighter of the two -- the own cell
                    // pays the block's opacity -- and a drift of snow in
                    // a field must not dim by a step for this.
                    let n = if part_height && face_index == 0 && neighbor_cover == FULL_COVER {
                        [0, 0, 0]
                    } else {
                        n
                    };
                    let mut ring_light = [[0u8; 3]; 3];
                    let mut ring_opaque = [[false; 3]; 3];
                    if flat_lit {
                        let raw = cache.light_near(cell, y, n[0], n[1], n[2]);
                        // A canopy is taken down a step so that a tree
                        // reads as a mass rather than as a bright cloud
                        // (see `shaded_canopy`). Distant ground is not:
                        // it is the same ground as the chunk in front of
                        // it, and darkening it would draw the ring where
                        // the detail level changes.
                        let light = if canopy { shaded_canopy(raw) } else { raw };
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

                    let mut ao_values = [0u8; 4];

                    // **A part-height block's side wears a strip of its
                    // picture cut to its height**, not the whole picture
                    // squeezed in. The squeeze made the side's texel
                    // density anisotropic -- a campfire's quarter-height
                    // side held all thirty-two rows -- and past a few
                    // paces the squeezed axis dropped into minification,
                    // pushing the whole face onto the filtered path
                    // while the other axis was still magnified. What
                    // that looks like is wide soft smears across the
                    // stones of every fire in the world. It wears the
                    // picture's top rows, as many as it is tall, through
                    // the same fine coordinate the model boxes use (see
                    // `FINE_UV_BIT`) -- exactly as many, where the old crop
                    // could only say a power of two and a bed's six
                    // sixteenths wore eight -- and `texture::resize_to` is
                    // how a strip-shaped picture meets it half way.
                    let side_crop = top < 1.0 && face_index >= 2 && tint == 0 && !is_liquid(id);

                    // A quarter turn per step, from a hash of the cell
                    // and which face of it this is -- so the six faces
                    // of one block disagree with each other as well as
                    // with their neighbours, and the same block comes
                    // out the same way round every time the chunk is
                    // remeshed. See `turned_uv`. Never on a cropped
                    // side: a quarter turn swaps the crop's axes.
                    let turn = if !side_crop
                        && primitive_shared::types::texture_turns(id, face_index)
                    {
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

                    // **What each corner is lit by, before anything is
                    // emitted.** Whether this face can be merged into a
                    // rectangle at all is a question about all four
                    // corners at once -- they have to agree -- and that
                    // cannot be asked from inside a loop that has
                    // already pushed the first of them. See `Mergeable`.
                    let mut corner_lights = [(0u8, 0u8); 4];
                    for (corner_index, corner) in face.corners.iter().enumerate() {
                        // Which side of the ring this corner sits on:
                        // index 0 is the -1 offset, 2 is +1.
                        let ia = if corner[axis_a] > 0.5 { 2 } else { 0 };
                        let ib = if corner[axis_b] > 0.5 { 2 } else { 0 };

                        let side1 = ring_opaque[ia][1];
                        let side2 = ring_opaque[1][ib];
                        let diagonal = ring_opaque[ia][ib];
                        ao_values[corner_index] = vertex_ao(side1, side2, diagonal);

                        // Smooth lighting: average the four cells that
                        // touch this corner from the outside, skipping
                        // opaque ones (which are dark and would drag the
                        // average down through a wall).
                        corner_lights[corner_index] =
                            corner_light(&ring_light, ia, ib, side1, side2, diagonal);
                    }

                    // Lit flat, and therefore describable by one
                    // rectangle however many cells it covers.
                    let uniform = ao_values.iter().all(|a| *a == ao_values[0])
                        && corner_lights.iter().all(|c| *c == corner_lights[0]);
                    // **A block with a bite out of it is never merged.**
                    // The merge emits whole cells -- a rectangle spans
                    // 0..w of them (`emit_merged`) -- and a quarried block
                    // does not fill its cell on the axis the pick came in
                    // from. `top == 1.0` already keeps a part-height cube
                    // out for exactly this reason, and it is not enough on
                    // its own: a wall dug sideways is full height and
                    // three quarters wide, so it passed that test and came
                    // out drawn as the whole cell it no longer is.
                    if MERGE_COPLANAR_FACES
                        && uniform
                        && top == 1.0
                        && bite.is_none()
                        && !side_crop
                        && turn == 0
                        && translucent_flag == 0
                        && !is_liquid(id)
                    {
                        // Chunk coordinates, like every other number
                        // the mesher emits: `plane`, `u` and `v` are
                        // this cell's place along the face normal and
                        // the two axes a rectangle grows in.
                        let here = [x, y, z];
                        let (sky, block_light) = corner_lights[0];
                        mergeable.push(Mergeable {
                            face: face_index as u8,
                            plane: here[face.normal_axis] as u8,
                            u: here[axis_a] as u8,
                            v: here[axis_b] as u8,
                            key: pack_light(sky, block_light, ao_values[0], face_index as u8)
                                as u64
                                | ((layer as u64) << KEY_LAYER_SHIFT)
                                | ((tint as u64 & 0xFF) << KEY_TINT_SHIFT)
                                | if is_cutout(id) { KEY_CUTOUT } else { 0 }
                                | if primitive_shared::types::is_one_sheet(id) {
                                    KEY_UNMOTTLED
                                } else {
                                    0
                                },
                        });
                        continue;
                    }

                    // Drawn as itself: mark its four corners for the
                    // merge's cutting pass. See `direct_marks`. Not a
                    // part-height cube's: its corners are grown off the
                    // lattice (`reach`) and lie on no rectangle's edge.
                    for corner in face.corners.iter().filter(|_| !part_height) {
                        let (cx, cy, cz) = (
                            x as usize + corner[0] as usize,
                            y as usize + corner[1] as usize,
                            z as usize + corner[2] as usize,
                        );
                        let i = (cx * (CHUNK_SIZE_Y + 1) + cy) * (CHUNK_SIZE_Z + 1) + cz;
                        direct_marks[i / 64] |= 1 << (i % 64);
                    }

                    // **A part-height cube is grown past its cell by a
                    // hair, and that hair is the dots on the hearth.** A
                    // campfire is a quarter of a block tall, so the top
                    // corners of its sides sit at y + 0.25 on the line
                    // where it meets the wall beside it -- strictly inside
                    // that wall's vertical edge (a pack, half a block, at
                    // y + 0.5). That is the T-junction the greedy merge
                    // used to make, and the cure for those cannot reach
                    // it: `mark_corners` is a lattice of whole blocks, and
                    // it only ever puts vertices into merged rectangles,
                    // never into the single face of a wall beside a fire.
                    // Where rounding opens the crack, it looks into the
                    // fire's own cell, whose floor the fire culls -- so
                    // what shows is the sky.
                    // See `a_part_height_block_puts_no_corner_inside_
                    // another_faces_edge` for the corners and
                    // `renderer::where_part_height_blocks_let_the_
                    // background_through` for the pixels.
                    //
                    // So the cube stops *meeting* what is beside it and
                    // overlaps it, which is the rule every model box
                    // already lives by (`BITE` in `push_box`, the same
                    // eight-hundredth of a block): its sides stand a hair
                    // outside the cell, its corners leave the lattice
                    // lines, and wherever a crack could open, the cube's
                    // own face is in front of it. The picture stretches
                    // by the same hair, which nothing can see. Only
                    // across -- the top stays at its height and the foot
                    // on its floor; see the underside kept above for the
                    // one gap the growth itself would open.
                    //
                    // Rejected: a vertex at y + 0.25 in every edge the
                    // fire touches. The texture could say it now
                    // (`FINE_UV_BIT`); the light word cannot -- four bits
                    // of light and two of occlusion, so the new vertex
                    // would carry a rounded value and kink the shading of
                    // every wall a fire is built against -- and it would
                    // have to reach single faces and merged rectangles
                    // alike, on both sides of a chunk seam. Furniture needs
                    // none of this: it is drawn as a model now
                    // (`furniture_block`), and a model's boxes already
                    // overlap.
                    //
                    // **An axis a dig has cut into is the cut, exactly.**
                    // The hair is for a face that still stands in the wall
                    // of its cell and could crack against the block beside
                    // it; the face a pick has just opened stands in the
                    // middle of the cell with nothing to crack against,
                    // and growing it would put the rock a hair further out
                    // than the box the player walks into. So an axis the
                    // bite has moved takes the bite's own coordinate and
                    // the other two are grown as a hearth's are.
                    let reach = |axis: usize, c: f32| -> f32 {
                        if let Some((min, max)) = bite {
                            if min[axis] > 0.0 || max[axis] < 1.0 {
                                return if c > 0.5 { max[axis] } else { min[axis] };
                            }
                        }
                        if !part_height {
                            c
                        } else if c > 0.5 {
                            1.0 + BITE * T
                        } else {
                            -BITE * T
                        }
                    };
                    let base = vertices.len() as u32;
                    for (corner_index, corner) in face.corners.iter().enumerate() {
                        let uv = turned_uv(face_uv(face_index, *corner), turn);
                        let ao = ao_values[corner_index];
                        let (sky, block_light) = corner_lights[corner_index];

                        let vertex = Vertex::tinted(
                            [
                                // **Chunk-local, not world.** A vertex
                                // in absolute world coordinates is an
                                // f32 holding a number up to seven
                                // digits long, and its last bit is worth
                                // several centimetres out where players
                                // actually go -- so the mesh itself
                                // quantises and the world visibly
                                // trembles. What crosses to the GPU now
                                // is a number between -1 and 17, and
                                // where the chunk *is* rides along as a
                                // per-draw offset. See
                                // `renderer::ChunkOffset`.
                                x as f32 + reach(0, corner[0]),
                                // Interpolating between the box's
                                // floor and its top keeps the corners
                                // *on* them, so a side face shortens to
                                // meet a lowered top -- or lengthens to
                                // meet a floor hung below the cell --
                                // instead of shearing. `bottom` is
                                // zero for everything but submerged
                                // water.
                                // ...and a water surface's top is the
                                // sloped one for this corner; see
                                // `liquid_corners`.
                                y as f32
                                    + bottom
                                    + corner[1]
                                        * (liquid_corners.map_or(top, |tops| {
                                            tops[(corner[0] as usize) * 2 + corner[2] as usize]
                                        }) - bottom),
                                z as f32 + reach(2, corner[2]),
                            ],
                            uv,
                            layer,
                            pack_light(sky, block_light, ao, face_index as u8)
                                | translucent_flag
                                // A sheet is not mottled -- see
                                // `types::is_one_sheet`.
                                | if primitive_shared::types::is_one_sheet(id) {
                                    0
                                } else {
                                    MOTTLED_BIT
                                },
                            // One byte, three readings that cannot
                            // meet: a translucent face is neither
                            // foliage nor cropped, and a cropped side
                            // is never foliage.
                            if translucent_flag != 0 {
                                // **Only the surface carries a depth**,
                                // and the reason is what the number
                                // means: `liquid_depth_below` counts the
                                // water *under* this cell, which is what
                                // a ray crosses when it goes down
                                // through the top of a lake and has
                                // nothing to do with what it crosses
                                // through a *wall*. A waterfall is one
                                // cell thick and stands over a pool, so
                                // its sides were handed the pool's depth
                                // -- four and five -- and drawn at an
                                // alpha of 0.95 and 0.97 where a sheet
                                // of falling water is 0.72. Two things
                                // came of that, and a player
                                // photographed the second: the fall
                                // stopped being water and became a slab
                                // of blue paint, and the far wall of the
                                // column -- which the blended pass
                                // composites over the near one, because
                                // it draws with no back-face culling and
                                // in emission order inside a chunk --
                                // stopped blending into it and became a
                                // hard-edged patch of somebody else's
                                // transparency hanging in mid-fall.
                                //
                                // 1 is `WATER_ALPHA` exactly (the
                                // shader fades from the first cell), so
                                // every vertical face of water is
                                // drawn exactly as it was before the
                                // fade existed, and the fade keeps the
                                // face it was measured on. See
                                // `WATER_DEPTH_FADE` in shader.wgsl.
                                if face_index == 0 {
                                    *water_depth.get_or_insert_with(|| {
                                        liquid_depth_below(cache, x, y, z)
                                    })
                                } else {
                                    1
                                }
                            } else {
                                tint
                            },
                        );
                        // `face_uv` puts v 0 at a side's top edge and 1 at
                        // its foot, so scaling v by the height keeps the
                        // top rows and cuts the rest.
                        // **A bite wears a piece of the picture its own size.**
                        // The face of a quarried block is smaller than its
                        // cell along whichever axes the pick has eaten, and
                        // the whole picture squeezed onto it is stone at
                        // four times the density of the wall it is cut into
                        // -- the same fault a leg of hide had (see
                        // `animal_model::material_cut`). The two axes are
                        // the face's own (`UV_SOURCE`), so a side bitten
                        // from the left crops across and a floor bitten
                        // downward crops down.
                        let bite_crop = bite.map(|(min, max)| {
                            let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
                            let [(ua, _), (va, _)] = UV_SOURCE[face_index];
                            [size[ua], size[va]]
                        });
                        // ...and the face the pick opened wears the chip
                        // marks as well (`CHIPPED_BIT`): the others round a
                        // bite were rock a moment ago too, but this is the
                        // one the digger is looking into.
                        // Not on a turf lip (`dig::is_turf_lip`): its top is
                        // the meadow the generator laid on a slope, not a
                        // face anybody opened, and chip marks on it would
                        // draw every rise of every meadow as a quarry. Its
                        // sides take the crop above like any bite, which
                        // keeps the turf side's fringe and the soil under it.
                        let cut = bite_crop.is_some()
                            && primitive_shared::dig::cut_face(id) == Some(FACE_OUTWARD[face_index])
                            && !primitive_shared::dig::is_turf_lip(id);
                        // **The side of shallow water is cut, not squeezed.**
                        // Water was left out of `side_crop`, so a side two
                        // eighths deep wore the whole picture pressed into
                        // two eighths -- ripples at four times their density
                        // down every shore and spill. Its corners are not
                        // one height (`liquid_corners` slopes a spill), so
                        // the picture is pinned to the world instead: v is
                        // how far below the cell's top this corner is, and a
                        // slope shows the ripples a level surface would, cut
                        // along the slope.
                        let liquid_crop = is_liquid(id) && face_index >= 2 && (top < 1.0 || liquid_corners.is_some());
                        let liquid_v = || {
                            let corner_top = liquid_corners.map_or(top, |tops| tops[(corner[0] as usize) * 2 + corner[2] as usize]);
                            (1.0 - (bottom + corner[1] * (corner_top - bottom))).clamp(0.0, 1.0)
                        };
                        vertices.push(match (bite_crop, side_crop) {
                            (None, false) if liquid_crop => vertex.with_fine_uv([uv[0], liquid_v()]),
                            (Some([du, dv]), _) if cut => vertex.with_fine_uv([uv[0] * du, uv[1] * dv]).chipped(),
                            (Some([du, dv]), _) => vertex.with_fine_uv([uv[0] * du, uv[1] * dv]),
                            (None, true) => vertex.with_fine_uv([uv[0], uv[1] * top]),
                            (None, false) => vertex,
                        });
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
                        // Into its direction's bucket, not the shared
                        // list: see `MeshBuffers::solid_groups`.
                        &mut by_face[face_index]
                    };
                    // ...and when the *light* is: see `corner_brightness`.
                    let bright = |k: usize| corner_brightness(ao_values[k], corner_lights[k]);
                    if bright(0) + bright(2) > bright(1) + bright(3) {
                        target.extend_from_slice(&[
                            base,
                            base + 1,
                            base + 2,
                            base,
                            base + 2,
                            base + 3,
                        ]);
                    } else {
                        // The other diagonal, written so that the
                        // *first* triangle names the base vertex -- a
                        // rotation of a triangle is the same triangle
                        // to the rasteriser, and the flat outputs are
                        // equal on all four corners, so the provoking
                        // vertex does not matter. What it buys is that
                        // every polygon in the index stream announces
                        // where it starts: see `polygon_runs` in the
                        // tests, which take a mesh apart again.
                        target.extend_from_slice(&[
                            base + 3,
                            base,
                            base + 1,
                            base + 1,
                            base + 2,
                            base + 3,
                        ]);
                    }
                }
                #[cfg(test)]
                phase_clock::add(3, faces_started);
            }
        }
    }

    // Everything held back above, covered with as few rectangles as it
    // takes. After the face loop rather than inside it because a
    // rectangle is a fact about a whole plane, and the loop only ever
    // sees one cell of one.
    if MERGE_COPLANAR_FACES {
        // ...and the seam: corners the chunk next door may have put on
        // our boundary. See `Neighbourhood::seam_marks`.
        for (word, seam) in direct_marks.iter_mut().zip(cache.seam_marks()) {
            *word |= seam;
        }
        emit_merged(mergeable, covered, vertices, by_face, leaves, &direct_marks);
    }

    // The solid range is assembled last, group by group: whatever went
    // straight into `indices` -- racks, models -- and then the six
    // directions. See `MeshBuffers::solid_groups`.
    solid_groups[0] = indices.len() as u32;
    for (face, bucket) in by_face.iter().enumerate() {
        // How high the two vertical groups reach, measured here because
        // this is where their vertices are all in one place. Walked as
        // indices rather than as quads: a quad is four of six indices
        // and telling them apart would mean knowing how each emitter
        // wound its triangles, where a minimum does not care.
        // See `MeshBuffers::up_faces_from`.
        match face {
            0 => {
                for &index in bucket {
                    *up_faces_from = up_faces_from.min(vertices[index as usize].position[1]);
                }
            }
            1 => {
                for &index in bucket {
                    *down_faces_to = down_faces_to.max(vertices[index as usize].position[1]);
                }
            }
            _ => {}
        }
        indices.extend_from_slice(bucket);
        solid_groups[face + 1] = indices.len() as u32;
    }
    *solid_index_count = indices.len() as u32;
    indices.extend_from_slice(leaves);
    *leaf_end = indices.len() as u32;
    indices.extend_from_slice(sprites);
    *sprite_end = indices.len() as u32;
    indices.extend_from_slice(translucent);
    // See `MeshBuffers::top`.
    *top = vertices.iter().fold(f32::MIN, |high, vertex| high.max(vertex.position[1]));
}

/// One merged rectangle, before it is turned into four vertices.
///
/// Held rather than emitted straight away because a rectangle cannot be
/// judged alone: which vertices its edges need depends on the
/// rectangles beside it, and those may belong to another material. See
/// `t_points`.
#[derive(Clone, Copy)]
struct PlaneRect {
    u0: u32,
    v0: u32,
    w: u32,
    h: u32,
    key: u64,
    /// Which of the six directions, and which slice along its normal.
    /// Carried so that rectangles from every plane of the chunk can sit
    /// in one list and be checked against each other -- see
    /// `mark_corners` and `t_points`.
    face: u8,
    plane: u32,
}

/// The lattice points strictly inside a rectangle's four edges: where
/// some other quad's corner lands on it. `u` values along the bottom
/// and top edges, `v` values up the left and right, each ascending.
#[derive(Default, Debug)]
struct EdgePoints {
    bottom: Vec<u32>,
    top: Vec<u32>,
    left: Vec<u32>,
    right: Vec<u32>,
}

impl EdgePoints {
    fn clear(&mut self) {
        self.bottom.clear();
        self.top.clear();
        self.left.clear();
        self.right.clear();
    }

    fn len(&self) -> usize {
        self.bottom.len() + self.top.len() + self.left.len() + self.right.len()
    }
}

/// One bit per cell corner of the chunk: seventeen by sixty-five by
/// seventeen, three hundred words.
const LATTICE_X: usize = CHUNK_SIZE_X + 1;
const LATTICE_Y: usize = CHUNK_SIZE_Y + 1;
const LATTICE_Z: usize = CHUNK_SIZE_Z + 1;

fn lattice_index(x: usize, y: usize, z: usize) -> usize {
    (x * LATTICE_Y + y) * LATTICE_Z + z
}

/// Where a rectangle's (u, v) sits in the chunk, given its face.
fn rect_corner_cell(r: &PlaneRect, face_defs: &[Face; 6], u: usize, v: usize) -> (usize, usize, usize) {
    let face = &face_defs[r.face as usize];
    let n = face.normal_axis;
    let (a, b) = other_axes(n);
    let mut p = [0usize; 3];
    p[n] = r.plane as usize + face.corners[0][n] as usize;
    p[a] = u;
    p[b] = v;
    (p[0], p[1], p[2])
}

/// **`PRIMITIVE_NO_TSPLIT=1` leaves the T-junctions in.** Not a setting
/// -- nobody wants specks -- but the only way to measure what closing
/// them costs *from one binary in one session*: two builds benchmarked
/// minutes apart differ by more than the change does.
fn t_junctions_left_open() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("PRIMITIVE_NO_TSPLIT").is_ok())
}

/// Every corner of every quad in the chunk, as one bitset.
///
/// **Found by lookup, not by comparison.** Setting every rectangle's
/// four corners and then walking each rectangle's perimeter is a few
/// thousand bit tests per chunk and no pairwise anything.
///
/// The `extra_corners` are **the corners the rectangle list does not
/// contain.** A face lit unevenly at its corners -- every crease, every
/// step, the foot of every wall -- never enters the list: it is drawn
/// as itself. Its corners are corners all the same, and they are
/// exactly the ones that land on the edges of the long rectangles
/// beside them, because that is where creases are. Built by
/// `build_mesh` as it emits those faces, and folded in here.
///
/// **Three wrong answers came before that one, and the numbers are
/// kept.** The near-field count -- holes in a flat face, thirty frames,
/// one seat -- was 52 with no cutting, 44 cutting within a plane, 45
/// cutting across the whole chunk, and 4 with the merge switched off
/// entirely. So forty holes were the merge's and none of the cutting
/// reached them. The chunk seam was the obvious suspect: marking every
/// boundary cell corner cured it and cost a flat roof forty-six quads;
/// a grid of every fourth cell along the seam changed nothing (45);
/// marking the seam wherever the neighbouring chunk's block or light
/// changed -- read out of the cache -- changed nothing either (45), at
/// the price of the every-cell version. None of them could have worked,
/// because the corners were never on the seam: they were on the faces
/// the list had skipped. With those in, the count fell to eighteen --
/// and eighteen is what the terrain shows *with the merge switched off
/// entirely*, at the same render distance, at the same pixels: ledges
/// and trenches a pixel wide, seen edge-on, which no mesher can close.
/// Against that control the cracks the merge itself makes are 34 with
/// no cutting, 26 to 27 with any cutting that stops at the rectangle
/// list, and 0 with these corners in.
fn mark_corners(rects: &[PlaneRect], lattice: &mut Vec<u64>, face_defs: &[Face; 6], extra_corners: &[u64]) {
    lattice.clear();
    lattice.resize((LATTICE_X * LATTICE_Y * LATTICE_Z).div_ceil(64), 0);
    for r in rects {
        let (u0, v0, u1, v1) = (r.u0 as usize, r.v0 as usize, (r.u0 + r.w) as usize, (r.v0 + r.h) as usize);
        for (u, v) in [(u0, v0), (u1, v0), (u0, v1), (u1, v1)] {
            let (x, y, z) = rect_corner_cell(r, face_defs, u, v);
            let i = lattice_index(x, y, z);
            lattice[i / 64] |= 1 << (i % 64);
        }
    }
    for (word, marks) in lattice.iter_mut().zip(extra_corners) {
        *word |= *marks;
    }
}

/// The corners of other quads that lie strictly inside this rectangle's
/// edges -- its T-junctions -- whichever plane they came from.
///
/// **This is the whole of the white-speck fix**, and the fault it
/// removes is worth stating exactly. A greedy merge makes rectangles of
/// different lengths meet edge to edge: a long one may run from u=0 to
/// u=8 while the two beside it stop and start at u=4. The long
/// rectangle's edge and the short ones' edges lie on the same line, but
/// the point u=4 is a *vertex of one and the interior of an edge of the
/// other* -- a T-junction. The two triangles either side of it are then
/// rasterised from different equations, floating point does not promise
/// they agree to the last bit, and where they disagree a pixel belongs
/// to neither. Behind it is whatever is behind it: the sky, or the
/// block one cell back.
///
/// **Across planes, not within one -- and that was the second lesson.**
/// The first version looked only at the rectangles in the same plane,
/// and photographed looking *down* it was clean. Photographed looking
/// *along* the ground it was not: a long side face of a bank meets the
/// top faces of the blocks in front of it at a right angle, and every
/// one of those top faces puts a corner on the side face's edge. The
/// instrument that found it paints each face direction a flat colour,
/// and the picture was a red pixel -- a top face -- in the middle of a
/// blue wall. So the lattice holds the corners of every quad in the
/// chunk, in world cells, and a rectangle collects every set bit
/// strictly inside any of its edges.
///
/// A rectangle one cell wide has no interior on its short edges and
/// the range is empty, which is why an unmerged face never collects
/// anything.
fn t_points(rect: &PlaneRect, lattice: &[u64], face_defs: &[Face; 6], out: &mut EdgePoints) {
    out.clear();
    if t_junctions_left_open() {
        return;
    }
    let at = |u: usize, v: usize| {
        let (x, y, z) = rect_corner_cell(rect, face_defs, u, v);
        let i = lattice_index(x, y, z);
        lattice[i / 64] & (1 << (i % 64)) != 0
    };
    let (u0, v0) = (rect.u0 as usize, rect.v0 as usize);
    let (u1, v1) = (u0 + rect.w as usize, v0 + rect.h as usize);
    for u in u0 + 1..u1 {
        if at(u, v0) {
            out.bottom.push(u as u32);
        }
        if at(u, v1) {
            out.top.push(u as u32);
        }
    }
    for v in v0 + 1..v1 {
        if at(u0, v) {
            out.left.push(v as u32);
        }
        if at(u1, v) {
            out.right.push(v as u32);
        }
    }
}

/// Triangulates a rectangle whose edges carry extra vertices, into
/// exactly `points + 2` triangles, none of them degenerate.
///
/// **Why not cut the rectangle into smaller rectangles, which is what
/// this used to do.** A cut through a rectangle costs two triangles per
/// piece, and it puts a *new* corner on the far edge -- which is a new
/// T-junction for whatever lies beyond that edge, so the cutting had to
/// be repeated until nothing moved: sixty-four passes, because eight
/// left thirty-six holes where sixty-four left eighteen. Measured on
/// the benchmark seat at a render distance of twenty-four, the cutting
/// added 212 000 triangles to a 538 000-triangle frame and 0.22 ms to a
/// 0.76 ms solid pass -- and a flat-colour run showed the whole
/// fragment shader costs 0.08 ms, so the pass is paying for triangles,
/// not pixels. Inserting the vertex into the edge instead creates no
/// corner anywhere, needs one pass, and a rectangle with `k` points on
/// its boundary is `k + 2` triangles, which is the least any
/// triangulation of `k + 4` vertices can be.
///
/// **The construction, because a fan from a corner does not work.**
/// A fan from corner A puts A, a point on edge AB and B in one
/// triangle, which has no area: the point is still not a vertex of
/// anything drawn and the T-junction is still there. So: the left
/// column (A, its points, D) is fanned from the first point on the
/// bottom edge -- or B when there is none -- which lies off the column's
/// line; the right column (B, its points, C) from the last point on the
/// top edge, or D; and what is left is a strip between the bottom chain
/// and the top chain, two rows on two different lines, zipped together
/// in order of `u`. Every triangle has two vertices on one line and its
/// third on another, so none is flat.
///
/// `corners` are the vertex indices of A, B, C, D -- the (0,0), (w,0),
/// (w,h), (0,h) corners in (u, v) -- and the points follow from
/// `first_point` in the order `t_points` filled them; `ccw` says whether
/// the face's own winding runs counter-clockwise in (u, v), and the
/// output is flipped to match when it does not. Indices come out in
/// (u, v) counter-clockwise order otherwise.
///
/// The first triangle written always names the polygon's lowest vertex
/// index, like every other quad in the mesh: that is how the tests find
/// where one polygon ends and the next begins. See `polygon_runs`.
fn triangulate_edged_rect(points: &EdgePoints, corners: [u32; 4], first_point: u32, ccw: bool, out: &mut Vec<u32>) {
    let start = out.len();
    // Local numbering of the points: bottom, top, left, right, in the
    // order `t_points` filled them.
    let (a, b, c, d) = (corners[0], corners[1], corners[2], corners[3]);
    let bottom = |i: usize| first_point + i as u32;
    let top = |i: usize| first_point + (points.bottom.len() + i) as u32;
    let left = |i: usize| first_point + (points.bottom.len() + points.top.len() + i) as u32;
    let right =
        |i: usize| first_point + (points.bottom.len() + points.top.len() + points.left.len() + i) as u32;

    let mut emit = |p: u32, q: u32, r: u32| {
        if ccw {
            out.extend_from_slice(&[p, q, r]);
        } else {
            out.extend_from_slice(&[p, r, q]);
        }
    };

    // The apex for the left column: off the line u = u0.
    let p_apex = if points.bottom.is_empty() { b } else { bottom(0) };
    // ...and for the right column: off the line u = u1.
    let q_apex = if points.top.is_empty() { d } else { top(points.top.len() - 1) };

    // Left column, bottom to top: A, its points, D.
    let mut prev = a;
    for i in 0..points.left.len() {
        emit(prev, p_apex, left(i));
        prev = left(i);
    }
    emit(prev, p_apex, d);

    // Right column, bottom to top: B, its points, C.
    let mut prev = b;
    for i in 0..points.right.len() {
        emit(prev, right(i), q_apex);
        prev = right(i);
    }
    emit(prev, c, q_apex);

    // The strip between: the bottom chain from the left apex to B and
    // the top chain from D to the right apex, zipped by u.
    let lower_u = |i: usize| -> u32 {
        if i < points.bottom.len() { points.bottom[i] } else { u32::MAX }
    };
    let lower_id = |i: usize| -> u32 { if i < points.bottom.len() { bottom(i) } else { b } };
    let lower_len = points.bottom.len() + 1;
    let upper_u = |j: usize| -> u32 { if j == 0 { 0 } else { points.top[j - 1] } };
    let upper_id = |j: usize| -> u32 { if j == 0 { d } else { top(j - 1) } };
    let upper_len = points.top.len() + 1;
    let (mut i, mut j) = (0usize, 0usize);
    while i + 1 < lower_len || j + 1 < upper_len {
        let take_lower = j + 1 >= upper_len || (i + 1 < lower_len && lower_u(i + 1) <= upper_u(j + 1));
        if take_lower {
            emit(lower_id(i), lower_id(i + 1), upper_id(j));
            i += 1;
        } else {
            emit(lower_id(i), upper_id(j + 1), upper_id(j));
            j += 1;
        }
    }

    // The triangle that names the lowest index goes first. Triangle
    // order is nothing to the rasteriser; see the note above.
    let base = *corners.iter().min().expect("four corners");
    let found = out[start..]
        .chunks_exact(3)
        .position(|tri| tri.contains(&base))
        .expect("a corner is a vertex of some triangle");
    if found != 0 {
        let (head, rest) = out[start..].split_at_mut(3);
        head.swap_with_slice(&mut rest[(found - 1) * 3..found * 3]);
    }
}

/// How many cells a face direction's plane is, along the two axes a
/// rectangle grows in.
///
/// A chunk is sixteen by sixty-four by sixteen, so the plane a face
/// tiles is square for the two vertical directions and a tall strip for
/// the four sideways ones.
#[inline]
fn plane_extent(face_index: usize) -> (usize, usize) {
    let axis_size = |axis: usize| match axis {
        0 => CHUNK_SIZE_X,
        1 => CHUNK_SIZE_Y,
        _ => CHUNK_SIZE_Z,
    };
    let normal_axis = match face_index {
        0 | 1 => 1,
        2 | 3 => 0,
        _ => 2,
    };
    let (axis_a, axis_b) = other_axes(normal_axis);
    (axis_size(axis_a), axis_size(axis_b))
}


/// **Where the mesher's milliseconds go**, for
/// `what_the_mesher_spends_its_time_on`.
///
/// Test builds only: the shipped mesher has no clocks in it, and the
/// `#[cfg(test)]` on every statement below is what guarantees that rather
/// than a promise in a comment. Thread-local because the mesher runs on
/// several worker threads at once and a shared counter would measure the
/// contention instead of the work.
#[cfg(test)]
pub(crate) mod phase_clock {
    use std::cell::Cell;

    /// What each slot counts, in order.
    pub const NAMES: [&str; 4] =
        ["merge: greedy scan", "merge: corner lattice", "merge: emitting rects", "the face loop itself"];

    thread_local! {
        static NS: [Cell<u64>; NAMES.len()] = const { [const { Cell::new(0) }; NAMES.len()] };
        /// Whether this thread is taking the clock at all.
        ///
        /// **Off unless a test asks for it, and this is not a nicety.**
        /// The face-loop phase is timed *per cell*, so with the clock
        /// always on a test build paid two `Instant::now()` calls on
        /// every one of a chunk's 16,384 cells -- and on Windows that is
        /// a `QueryPerformanceCounter` each, tens of nanoseconds apiece.
        /// Every mesher measurement in this file is taken from a test
        /// binary, so `how_long_meshing_takes` and `measure_real_terrain`
        /// were reporting the mesher *plus* the stopwatch watching it,
        /// and a change that made the mesher faster moved a number that
        /// was part clock. `what_the_mesher_spends_its_time_on` turns it
        /// on for itself, which is the only place the breakdown is read.
        pub static ENABLED: Cell<bool> = const { Cell::new(false) };
    }

    /// Now, if this thread is timing; `None` if it is not. The call sites
    /// keep the `Instant::now()` itself behind this.
    #[inline(always)]
    pub fn started() -> Option<std::time::Instant> {
        ENABLED.with(Cell::get).then(std::time::Instant::now)
    }

    pub fn add(phase: usize, started: Option<std::time::Instant>) {
        if let Some(started) = started {
            let taken = started.elapsed();
            NS.with(|ns| ns[phase].set(ns[phase].get() + taken.as_nanos() as u64));
        }
    }

    /// The counts so far, and zero them for the next round.
    pub fn take() -> [u64; NAMES.len()] {
        NS.with(|ns| std::array::from_fn(|i| ns[i].replace(0)))
    }
}

/// Covers the held-back faces with rectangles and emits those instead.
///
/// **The standard greedy pass, per plane.** Take the lowest cell still
/// uncovered, run it as far as it goes along `u`, then grow that whole
/// run along `v` for as long as every cell of the next row is there.
/// That is not the *minimum* number of rectangles -- finding that is
/// expensive and the shapes here are runs of open ground and cave wall,
/// where the greedy answer and the optimal one are usually the same
/// rectangle.
///
/// Grouping is a sort rather than a map: a chunk hands this a few
/// hundred faces, and sorting a few hundred `u64`s costs less than the
/// hashing would, with no allocation at all.
///
/// The plane bitmap is at most 64 x 16 cells, so it fits in sixteen
/// words on the stack of this function -- one `Vec` reused across
/// chunks, cleared per group.
fn emit_merged(
    faces_left: &mut [Mergeable],
    covered: &mut Vec<u64>,
    vertices: &mut Vec<Vertex>,
    by_face: &mut [Vec<u32>; 6],
    leaves: &mut Vec<u32>,
    extra_corners: &[u64],
) {
    // Face, then plane, then everything the corners have to agree on:
    // that is the group. Within a group, `v` before `u` so the greedy
    // scan below meets cells in the order it wants them.
    faces_left.sort_unstable_by_key(|f| (f.face, f.plane, f.key, f.v, f.u));

    let face_defs = faces();
    let mut rects: Vec<PlaneRect> = Vec::new();
    let mut lattice: Vec<u64> = Vec::new();
    // **The whole chunk first, then the cutting, then the drawing.** The
    // greedy scan below still runs per key -- two cells with different
    // light or a different picture cannot be one rectangle -- but every
    // rectangle of every plane is collected before any is emitted,
    // because the fault they are collected for is *geometric* and
    // crosses planes. A crack does not care that the quad on the other
    // side of it is a different green, or a different direction: see
    // `split_t_junctions`.
    #[cfg(test)]
    let greedy_started = phase_clock::started();
    let mut plane_start = 0usize;
    while plane_start < faces_left.len() {
        let plane_head = faces_left[plane_start];
        let mut plane_end = plane_start + 1;
        while plane_end < faces_left.len()
            && faces_left[plane_end].face == plane_head.face
            && faces_left[plane_end].plane == plane_head.plane
        {
            plane_end += 1;
        }
        let plane = &faces_left[plane_start..plane_end];
        plane_start = plane_end;

        let face_index = plane_head.face as usize;
        let (u_count, v_count) = plane_extent(face_index);

        let mut start = 0usize;
        while start < plane.len() {
        let head = plane[start];
        let mut end = start + 1;
        while end < plane.len() && plane[end].key == head.key {
            end += 1;
        }
        let group = &plane[start..end];
        start = end;

        let words = (u_count * v_count).div_ceil(64);
        covered.clear();
        covered.resize(words, 0);
        for cell in group {
            let bit = cell.v as usize * u_count + cell.u as usize;
            covered[bit / 64] |= 1 << (bit % 64);
        }
        let occupied = |covered: &[u64], u: usize, v: usize| {
            let bit = v * u_count + u;
            covered[bit / 64] & (1 << (bit % 64)) != 0
        };

        for cell in group {
            let (u0, v0) = (cell.u as usize, cell.v as usize);
            if !occupied(covered, u0, v0) {
                continue; // already inside a rectangle emitted earlier
            }
            // Both runs stop at `MAX_RUN` as well as at the plane's
            // edge, because a rectangle longer than that cannot say how
            // long it is. Splitting it in two costs one extra quad and
            // keeps the texture in step with the blocks; see `MAX_RUN`.
            let mut width = 1;
            while u0 + width < u_count && width < MAX_RUN && occupied(covered, u0 + width, v0) {
                width += 1;
            }
            let mut height = 1;
            while v0 + height < v_count
                && height < MAX_RUN
                && (0..width).all(|step| occupied(covered, u0 + step, v0 + height))
            {
                height += 1;
            }
            for row in 0..height {
                for step in 0..width {
                    let bit = (v0 + row) * u_count + u0 + step;
                    covered[bit / 64] &= !(1 << (bit % 64));
                }
            }

            rects.push(PlaneRect {
                u0: u0 as u32,
                v0: v0 as u32,
                w: width as u32,
                h: height as u32,
                key: head.key,
                face: plane_head.face,
                plane: u32::from(plane_head.plane),
            });
        }
        }
    }

    #[cfg(test)]
    phase_clock::add(0, greedy_started);

    // **The T-junctions, closed before anything is drawn.** Every
    // corner in the chunk goes into one lattice; each rectangle then
    // collects the corners lying on its edges and is drawn as a polygon
    // with those as vertices. See `t_points` and
    // `triangulate_edged_rect`.
    #[cfg(test)]
    let corners_started = phase_clock::started();
    mark_corners(&rects, &mut lattice, &face_defs, extra_corners);
    #[cfg(test)]
    phase_clock::add(1, corners_started);
    let mut points = EdgePoints::default();

    // Whether each face's corner order runs counter-clockwise in its own
    // (u, v), which is what the triangulation has to match.
    let winding_ccw: [bool; 6] = std::array::from_fn(|face_index| {
        let face = &face_defs[face_index];
        let (a, b) = other_axes(face.normal_axis);
        let mut twice_area = 0.0f32;
        for i in 0..4 {
            let p = face.corners[i];
            let q = face.corners[(i + 1) % 4];
            twice_area += p[a] * q[b] - q[a] * p[b];
        }
        twice_area > 0.0
    });

    #[cfg(test)]
    let emitting_started = phase_clock::started();
    for rect in &rects {
        let face_index = rect.face as usize;
        let face = &face_defs[face_index];
        let normal_axis = face.normal_axis;
        let (axis_a, axis_b) = other_axes(normal_axis);
        let (u0, v0) = (rect.u0 as usize, rect.v0 as usize);
        let (width, height) = (rect.w as usize, rect.h as usize);
        let light = (rect.key & LIGHT_MASK as u64) as u32;
        let layer = ((rect.key >> KEY_LAYER_SHIFT) & KEY_LAYER_MASK) as u32;
        let tint = ((rect.key >> KEY_TINT_SHIFT) & 0xFF) as u32;
        let target: &mut Vec<u32> =
            if rect.key & KEY_CUTOUT != 0 { &mut *leaves } else { &mut by_face[rect.face as usize] };
        let mottle = if rect.key & KEY_UNMOTTLED != 0 { 0 } else { MOTTLED_BIT };

        // How many cells the rectangle covers along each axis of the
        // world. The one along the face normal is always a single
        // cell -- a face has no thickness -- and `spanning_uv` never
        // reads it.
        let mut extent = [1.0f32; 3];
        extent[axis_a] = width as f32;
        extent[axis_b] = height as f32;

        // One vertex of the polygon, at cell (u, v) of the plane. The
        // corners are the unit face stretched over the rectangle: each
        // keeps which *end* of the quad it is and moves to where that
        // end now is. A point on an edge is the same thing part of the
        // way along -- but its position is taken from the whole cell
        // numbers, never from the fraction: the vertex on the other
        // side of the T-junction was placed from the same integers, and
        // closing the crack means being equal to it to the last bit.
        let push_vertex = |vertices: &mut Vec<Vertex>, u: usize, v: usize| {
            let mut position = [0.0f32; 3];
            position[normal_axis] = rect.plane as f32 + face.corners[0][normal_axis];
            position[axis_a] = u as f32;
            position[axis_b] = v as f32;
            let mut along = face.corners[0];
            along[axis_a] = (u - u0) as f32 / width as f32;
            along[axis_b] = (v - v0) as f32 / height as f32;
            vertices.push(Vertex::tinted(
                position,
                spanning_uv(face_index, along, extent),
                layer,
                // The same bit the unmerged path sets: a rectangle
                // is still block faces, however many of them it
                // stands for. `head.key` carries only the light
                // word, so it has to be put back here -- unless the
                // material is one sheet, which the key does carry.
                light | mottle,
                tint,
            ));
        };

        let base = vertices.len() as u32;
        // The four corners first, in the face's own order, so that a
        // rectangle with nothing on its edges is byte-identical to the
        // quad it always was -- and so the tests can read a polygon's
        // rectangle off its first four vertices.
        let mut named = [0u32; 4];
        for (i, corner) in face.corners.iter().enumerate() {
            let u = u0 + if corner[axis_a] > 0.5 { width } else { 0 };
            let v = v0 + if corner[axis_b] > 0.5 { height } else { 0 };
            push_vertex(vertices, u, v);
            let which = match (corner[axis_a] > 0.5, corner[axis_b] > 0.5) {
                (false, false) => 0,
                (true, false) => 1,
                (true, true) => 2,
                (false, true) => 3,
            };
            named[which] = base + i as u32;
        }

        t_points(rect, &lattice, &face_defs, &mut points);
        if points.len() == 0 {
            // The winding the unmerged path uses when the four ambient
            // occlusion values are equal -- which, for anything that got
            // this far, they are. Matching it rather than picking one
            // is what makes a rectangle covering a single cell
            // byte-identical to the face it replaced -- including the
            // rotation that puts the base vertex in the first triangle.
            target.extend_from_slice(&[base + 3, base, base + 1, base + 1, base + 2, base + 3]);
            continue;
        }
        let first_point = vertices.len() as u32;
        let (v_bottom, v_top) = (v0, v0 + height);
        let (u_left, u_right) = (u0, u0 + width);
        for u in &points.bottom {
            push_vertex(vertices, *u as usize, v_bottom);
        }
        for u in &points.top {
            push_vertex(vertices, *u as usize, v_top);
        }
        for v in &points.left {
            push_vertex(vertices, u_left, *v as usize);
        }
        for v in &points.right {
            push_vertex(vertices, u_right, *v as usize);
        }
        triangulate_edged_rect(&points, named, first_point, winding_ccw[face_index], target);
    }
    #[cfg(test)]
    phase_clock::add(2, emitting_started);
}

/// How many cells of liquid stand at this cell and below it, capped.
///
/// Counted downward from the face's own cell rather than handed in,
/// because the only place that knows it is the mesher: the *server*
/// stores a column of water blocks and nothing anywhere records how
/// tall the column is.
///
/// **This answers a question about the surface, and only the surface
/// asks it.** Downward is the direction a ray takes through the top of
/// a lake; it is not the direction it takes through a wall of water,
/// and handing this number to the sides of a waterfall drew a
/// one-cell-thick sheet as opaquely as the pool it was falling into.
/// See where the tint byte is written, which is the one caller.
///
/// **Capped at `MAX_WATER_DEPTH`, and the cap is not arbitrary.** The
/// byte this rides in has to stay clear of the foliage tints, which
/// start at 1 and run to 225, so a small number is wanted anyway; and
/// the shader's fade has flattened out long before this many blocks, so
/// counting further would cost the loop and change nothing. The count
/// stops at the bottom of the chunk, which is stone, so it terminates.
const MAX_WATER_DEPTH: u32 = 24;

fn liquid_depth_below(cache: &Neighbourhood, x: i32, y: i32, z: i32) -> u32 {
    let mut depth = 0;
    while depth < MAX_WATER_DEPTH {
        let below = y - depth as i32;
        // A trap in a column is the water it holds (`trap_in_water`): the
        // walk only ever meets one from water above it, which is what makes
        // it wet, or starts in one whose own water is being drawn.
        // ...and a crown is, for the same reason (`crown_in_water`): a bush
        // sunk in a pond is not a shelf the depth under the water starts
        // again below, or the pool over a drowned hedge is drawn shallower
        // than the pool beside it.
        let here = cache.block(x, below.max(0), z);
        let wet = is_liquid(here)
            || primitive_shared::types::block_kind(here) == primitive_shared::types::BLOCK_FISH_TRAP
            || cubed_crown(here);
        if below < 0 || !wet {
            break;
        }
        depth += 1;
    }
    depth
}

/// **Is this a fish trap standing in water**, and so drawn holding it?
///
/// Water on any side of it or over it -- or a trap over it that is, so a
/// stack is wet to the bottom. The rules call the trap solid and the water
/// round it stops at its faces (`types::BLOCK_FISH_TRAP`); what that drew
/// was a dry cell wrapped in walls of water surface, each lying in the
/// plane of the wicker and blended over it, and through the holes the far
/// walls again -- a basket of blue glass on the river bed with an air
/// pocket in it, and from above, a dry hole in the surface. So the cell
/// draws its water, and the water round it draws no wall into it
/// (`seen_by_water`).
///
/// **Asked only of the cell and upward**, because that is all the padding
/// reaches: a neighbour one to the side is one cell from the edge of what a
/// chunk can read, and its far side is not there. Water beside or over a
/// trap never needs to ask -- being there is what makes the trap wet.
///
/// Rejected: making the trap a liquid row, as kelp is. The flow rewrites a
/// liquid cell, and the fish would go with it.
fn trap_in_water(cache: &Neighbourhood, cell: usize, y: i32, id: BlockId) -> bool {
    use primitive_shared::types::{block_kind, BLOCK_FISH_TRAP};
    let (mut cell, mut y, mut id) = (cell, y, id);
    loop {
        if block_kind(id) != BLOCK_FISH_TRAP {
            return false;
        }
        let above = cache.block_near(cell, y, 0, 1, 0);
        if is_liquid(above)
            || [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().any(|(dx, dz)| is_liquid(cache.block_near(cell, y, dx, 0, dz)))
        {
            return true;
        }
        if y + 1 >= CHUNK_SIZE_Y as i32 {
            return false;
        }
        (cell, y, id) = ((cell as isize + STRIDE_Y) as usize, y + 1, above);
    }
}

/// Is this a crown the mesher draws as a cube of leaf, and so one that can
/// be made to hold the water it stands in (`crown_in_water`)?
///
/// **Not a palm's crown**, which is canopy by every rule and not a cube by
/// any: its cells are fronds reaching out of a heart, drawn by a path of
/// their own (`palm_crown_block`) that this one would take the cell away
/// from. A drowned frond would come out as a cube of frond picture -- the
/// one shape the palm was written to stop being. So a frond drooping into
/// the sea keeps its bubble; a palm's crown stands on a trunk, which puts
/// that bubble at the top of a tree rather than in a pond.
#[inline]
fn cubed_crown(id: BlockId) -> bool {
    use primitive_shared::types::{block_kind, is_leafy, BLOCK_PALM_COCONUTS, BLOCK_PALM_FRONDS};
    is_leafy(id) && !matches!(block_kind(id), BLOCK_PALM_FRONDS | BLOCK_PALM_COCONUTS)
}

/// **Is this crown standing in water**, and so drawn holding it?
///
/// Water on any side of it, or water above it, or a crown above it that is
/// -- so a bush whose top cell is under the surface is wet to its root, and
/// a crown poking out into the air is not.
///
/// The trap's problem and the trap's shape (`trap_in_water`), including why
/// it only ever walks *up*: a cell one to the side is one cell from the edge
/// of what a chunk can read, and its own far side is not there.
///
/// **...and a crown walled in by crowns that are** (`wet_through_the_leaves`).
/// This used to stop at the walk, on the argument that a cell with leaves on
/// all four sides "cannot be seen from the water". It can: through the holes
/// in the leaves round it, which is what leaves are. The middle of any bush
/// three wide in a pond has water on no side of it, and where its top layer
/// is the surface there is air over it too -- so it was dry, the water round
/// it drew no wall into it (the pond's rule: a crown beside water is wet),
/// and the pond had a square hole in it over every sunk bush, with the sky
/// seen through it in specks from under the surface.
///
/// `x` and `z` are the cell's place in its chunk: the neighbour's evidence is
/// two cells away, and is only read where the chunk has it.
/// Rejected, twice over, was **a drowned crown of its own** -- the kelp
/// bargain, a row that is liquid so that every rule sees water
/// (`types::BLOCK_DROWNED_BOUGH` weighs the same three ways for a bough).
/// A bough is two ids; a crown is twelve, one per wood, and each would want
/// its row, its name, its picture and its line in every table that lists
/// leaves. The bit the drowned bough's note calls free is not: bits 10, 11
/// and 15 are the furniture's wood and 12 to 14 the variant, which a crown
/// already spends on whether its fruit has been picked. What the id would
/// buy over this is the flow running *through* a bush and a broken crown
/// leaving water behind rather than a hole the flow fills a cell later --
/// which is the trap's bargain exactly, and the trap has stood.
///
fn crown_in_water(cache: &Neighbourhood, cell: usize, [x, y, z]: [i32; 3], id: BlockId) -> bool {
    let (mut cell, mut y, mut id) = (cell, y, id);
    loop {
        if !cubed_crown(id) {
            return false;
        }
        let above = cache.block_near(cell, y, 0, 1, 0);
        if is_liquid(above) || wet_beside(cache, cell, y) || wet_through_the_leaves(cache, cell, [x, y, z]) {
            return true;
        }
        if y + 1 >= CHUNK_SIZE_Y as i32 {
            return false;
        }
        (cell, y, id) = ((cell as isize + STRIDE_Y) as usize, y + 1, above);
    }
}

/// Water on one of the four sides of this cell.
#[inline]
fn wet_beside(cache: &Neighbourhood, cell: usize, y: i32) -> bool {
    [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().any(|(dx, dz)| is_liquid(cache.block_near(cell, y, dx, 0, dz)))
}

/// **A crown with crowns on all four sides, one of which has water on a side
/// of it or over it** -- the middle of a bush sunk in a pond. See
/// `crown_in_water`.
///
/// Walled in, because that is what makes it safe: a cell with air or ground
/// on a side is at the edge of the bush, and flooded on its neighbour's word
/// it would stand a pane of water against the air -- the hedge along a bank
/// drawn full of pond to the far end. A walled-in cell has no side the water
/// in it could show against except leaves.
///
/// **One cell of reach, and only inside the chunk.** The neighbour's own
/// sides are two cells from this one; the padding is one. A neighbour in
/// the padding gives no evidence, so the middle of a bush that straddles a
/// chunk seam, or of one five wide, keeps its dry cell -- walled off by the
/// water round it (`dry_beside_a_flooded_crown`) rather than left open.
/// Rejected: flooding a crown on any wet neighbour's word, transitively.
/// Every cell of a canopy that touched a pond anywhere would fill, and
/// whether it did would depend on how far the walk was allowed to go --
/// which a chunk and its neighbour would answer differently along the seam.
fn wet_through_the_leaves(cache: &Neighbourhood, cell: usize, [x, y, z]: [i32; 3]) -> bool {
    const SIDES: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    if !SIDES.into_iter().all(|(dx, dz)| cubed_crown(cache.block_near(cell, y, dx, 0, dz))) {
        return false;
    }
    SIDES.into_iter().any(|(dx, dz)| {
        let inside = (0..CHUNK_SIZE_X as i32).contains(&(x + dx)) && (0..CHUNK_SIZE_Z as i32).contains(&(z + dz));
        let side = (cell as isize + dx as isize * STRIDE_X + dz as isize * STRIDE_Z) as usize;
        inside && (is_liquid(cache.block_near(side, y, 0, 1, 0)) || wet_beside(cache, side, y))
    })
}

/// **Is the crown beside a flooded crown dry**, so that the water in the
/// flooded one has to draw a wall into it?
///
/// `seen_by_water` answers "water" for any crown beside water, because a
/// crown beside a cell of *water* is wet by being there. A flooded crown's
/// cell is not water to that rule -- it is leaves drawn holding water -- so
/// the crown beside it need not be wet: the middle of a bush five wide, or
/// of one whose middle lies across a chunk seam, is dry even after
/// `wet_through_the_leaves`. The water round such a cell took the pond's
/// word for it and drew no wall into it, and nothing else did either, so
/// the cell was a hole through the water -- the sky in specks through the
/// leaves from under the surface.
///
/// So the flooded crown's water asks the neighbour itself, and walls off a
/// dry one: a pocket of air in the bush, which is what the cell is by every
/// rule the mesher has.
///
/// **Only inside the chunk**, for the reason `crown_in_water` gives: a
/// neighbour in the padding has its far side past what a chunk can read.
/// There the pond's answer stands, and the one face lying in the seam stays
/// open -- the price of not reading a second ring of every neighbouring
/// chunk for a bush.
#[inline]
fn dry_beside_a_flooded_crown(cache: &Neighbourhood, cell: usize, [x, y, z]: [i32; 3], n: [i32; 3], neighbour: BlockId) -> bool {
    let (nx, nz) = (x + n[0], z + n[2]);
    n[1] == 0
        && cubed_crown(neighbour)
        && (0..CHUNK_SIZE_X as i32).contains(&nx)
        && (0..CHUNK_SIZE_Z as i32).contains(&nz)
        && !crown_in_water(
            cache,
            (cell as isize + n[0] as isize * STRIDE_X + n[2] as isize * STRIDE_Z) as usize,
            [nx, y, nz],
            neighbour,
        )
}

/// What a cell of water sees in its neighbour at offset `n`: a trap holding
/// water (`trap_in_water`) or a crown standing in it (`crown_in_water`) is
/// water, so no wall is drawn into it, the column stays one box through it
/// and the surface meets over it. Anything else is itself. Only a neighbour
/// *above* has to be asked; one beside or under water is wet by being there,
/// and the asking cell is the water that makes it so.
#[inline]
fn seen_by_water(cache: &Neighbourhood, cell: usize, [x, y, z]: [i32; 3], n: [i32; 3], neighbour: BlockId) -> BlockId {
    use primitive_shared::types::{block_kind, BLOCK_FISH_TRAP, BLOCK_WATER};
    let wet_above = |neighbour| {
        let (cell, y) = ((cell as isize + STRIDE_Y) as usize, y + 1);
        if cubed_crown(neighbour) {
            crown_in_water(cache, cell, [x, y, z], neighbour)
        } else {
            trap_in_water(cache, cell, y, neighbour)
        }
    };
    if block_kind(neighbour) != BLOCK_FISH_TRAP && !cubed_crown(neighbour) {
        return neighbour;
    }
    if n != [0, 1, 0] || wet_above(neighbour) {
        BLOCK_WATER
    } else {
        neighbour
    }
}

#[cfg(test)]
thread_local! {
    /// Plants drawn from their own cell's floor, as they were before
    /// `types::stand_drop`: the "before" of a before-and-after taken with one
    /// binary (`scenario`, the tufts on a slope's lips). Per thread, because
    /// the scenarios mesh side by side.
    pub(crate) static PLANTS_ON_THEIR_OWN_FLOOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// How far below its cell a cross in the cache is drawn: `types::stand_drop`,
/// asked of the cells under it. The second cell down is read only for the
/// upper half of a tall plant, the one cross that stands on another.
fn cross_drop(cache: &Neighbourhood, cell: usize, y: i32, id: BlockId) -> f32 {
    #[cfg(test)]
    if PLANTS_ON_THEIR_OWN_FLOOR.with(std::cell::Cell::get) {
        return 0.0;
    }
    let ground = cache.block_near(cell, y, 0, -1, 0);
    let under = if primitive_shared::types::is_plant_top(id) { cache.block_near(cell, y, 0, -2, 0) } else { BLOCK_AIR };
    primitive_shared::types::stand_drop(id, ground, under)
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
///
/// **A stem of kelp is one ribbon from the floor to its top**, and is the one
/// cross that stacks. Its cells took their wander and their height from the
/// cell like a tuft's, so each length of stem stood up to a tenth of a cell
/// aside from the one under it and stopped up to a sixth short of the one
/// over it: "ламинарии не связаны текстурой по вертикали", a stem drawn as a
/// pile of offset slips with water between them. Kelp takes its wander from
/// its column and fills its cell, so the planes of one stem are one pair of
/// planes cut at the cell floors, and the picture -- drawn to tile down its
/// height -- runs through the cuts.
pub(crate) fn cross_planes(cell: [i32; 3], at: [f32; 3], block: BlockId) -> [[[f32; 3]; 4]; 2] {
    /// How far in from the cell's edges the planes sit.
    const INSET: f32 = 0.08;
    /// Plants stop short of the ceiling; a tuft of grass filling the
    /// whole cell looks like a hedge.
    const HEIGHT: f32 = 0.94;
    /// How far a tuft may wander from the middle of its cell.
    const JITTER: f32 = 0.07;
    /// ...and how much of its height it may gain or lose.
    const HEIGHT_JITTER: f32 = 0.12;

    use primitive_shared::types::{block_kind, is_plant_shoot, is_plant_top, is_tall_plant, BLOCK_KELP, BLOCK_KELP_TOP};
    let kelp = matches!(block_kind(block), BLOCK_KELP | BLOCK_KELP_TOP);
    // **A tall plant's two halves are one plant**, so they wander from the
    // middle of the column together, as a stem of kelp does: hashed per cell,
    // the upper half stood a jitter's width off its own stalk and the seam
    // between them was a step in every stem.
    let tall = is_tall_plant(block);
    let noise = if kelp || tall { cell_hash(cell[0], 0, cell[2]) } else { cell_hash(cell[0], cell[1], cell[2]) };
    let signed = |shift: u32| (((noise >> shift) & 0xFF) as f32 / 255.0) * 2.0 - 1.0;
    let offset_x = signed(0) * JITTER;
    let offset_z = signed(8) * JITTER;
    // Clamped to the cell. The jitter can add an eighth to a height of
    // 0.94, which is 1.05 -- a tall tuft grew *through* the block above
    // it, and the box you aim at cannot follow it there without
    // becoming clickable from inside that block.
    //
    // The lower half of a tall plant runs to its ceiling, where its upper
    // half starts -- stopped short, the stalk had a band of daylight through
    // it at the seam. A shoot is the lower half's picture at a little over
    // half height: something coming up, not a plant with its top cut off.
    let height = if kelp || (tall && !is_plant_top(block) && !is_plant_shoot(block)) {
        1.0
    } else if is_plant_shoot(block) {
        0.55
    } else {
        (HEIGHT * (1.0 + signed(16) * HEIGHT_JITTER)).min(1.0)
    };

    let (lo, hi) = (INSET, 1.0 - INSET);
    [((lo, lo), (hi, hi)), ((lo, hi), (hi, lo))].map(|(a, b)| {
        [(a.0, 0.0, a.1), (b.0, 0.0, b.1), (b.0, height, b.1), (a.0, height, a.1)].map(
            |(x, y, z)| {
                [at[0] + x + offset_x, at[1] + y, at[2] + z + offset_z]
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
pub(crate) fn flat_quad(at: [f32; 3], block: BlockId) -> [[f32; 3]; 4] {
    let inset = primitive_shared::types::flat_inset(block);
    let lift = primitive_shared::types::flat_lift(block);
    let (lo, hi) = (inset, 1.0 - inset);
    [(lo, lo), (hi, lo), (hi, hi), (lo, hi)].map(|(x, z)| [at[0] + x, at[1] + lift, at[2] + z])
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
/// How far up the cell the flame reaches, and how far in from its walls.
///
/// It starts at the top of the stones -- a quarter of the cell, see
/// `blocks::QUARTER_BLOCK` -- and stops short of the ceiling, because a
/// flame that fills its cell edge to edge is a block of fire rather than
/// a fire. Narrower than a tuft of grass as well: what is burning is the
/// wood in the middle of the ring, not the ring.
const FLAME_FOOT: f32 = 0.22;
const FLAME_TOP: f32 = 0.98;
const FLAME_INSET: f32 = 0.22;

/// Two crossing quads of fire standing on a lit hearth.
///
/// The two wear **different pictures** -- see `FaceLayers::flame` -- and
/// that is not decoration: one picture on both quads is the same
/// silhouette at right angles to itself, which from any angle but the
/// diagonals reads as a flat X of flame rather than as something
/// burning. Only one of those sheets is drawn; the other is it mirrored
/// and a few frames behind, made when the atlas is built. Nothing here
/// has to know which is which.
///
/// Lit at full block light rather than from the cell, and the reason is
/// the same one that makes a torch texture bright: this *is* the light
/// source. Sampling the cell would light the flame by its own glow one
/// tick late, which flickers as the light engine catches up.
fn flame_block(
    cell: [i32; 3],
    at: [f32; 3],
    layers: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let sky = light & 0x0F;
    // Face 0 (up), like every other billboard here: a flame has no
    // normal worth the name and must not be darker from one side.
    let packed = pack_light(sky, primitive_shared::types::MAX_LIGHT, 3, 0);
    // The same jitter a tuft gets, from the same hash, so two fires
    // beside each other are not the same flame twice.
    let noise = cell_hash(cell[0], cell[1], cell[2]);
    let signed = |shift: u32| (((noise >> shift) & 0xFF) as f32 / 255.0) * 2.0 - 1.0;
    let lean = signed(0) * 0.05;
    let height = FLAME_TOP * (1.0 + signed(16) * 0.10);

    let (lo, hi) = (FLAME_INSET, 1.0 - FLAME_INSET);
    for (plane, (a, b)) in [((lo, lo), (hi, hi)), ((lo, hi), (hi, lo))].into_iter().enumerate() {
        let layer = layers.flame(plane as u32);
        let base = vertices.len() as u32;
        let corners = [
            (a.0, FLAME_FOOT, a.1),
            (b.0, FLAME_FOOT, b.1),
            (b.0, height, b.1),
            (a.0, height, a.1),
        ];
        for (corner, uv) in corners
            .into_iter()
            .zip([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
        {
            // The lean is applied to the top corners only, so the flame
            // is rooted where the wood is and drifts at the tip.
            let drift = if corner.1 > FLAME_FOOT { lean } else { 0.0 };
            vertices.push(Vertex::new(
                [at[0] + corner.0 + drift, at[1] + corner.1, at[2] + corner.2 + drift],
                uv,
                layer,
                packed,
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

// ---- the drying rack ----
//
// **The one block in this game that is not a box or a billboard.** A
// tanner's rack is a square of poles standing on end with the skin
// stretched inside it, and every way of saying that with a cube was
// wrong in the same way: as a half-height slab it read as a doormat, and
// as a full cube with a picture of a frame on each face it read as a
// crate. What it needs is five boxes.
//
// The numbers below are in sixteenths of a cell, which is the unit the
// textures are drawn in and the one a person editing this can count on
// the picture.

/// One sixteenth, the unit everything here is written in.
const T: f32 = 1.0 / 16.0;

/// Draws a drying rack: the frame always, the skin if it has one.
///
/// The skin is a *variant bit* on the block (see `types::RACK_LOADED`),
/// which is what lets a player see across a camp which of their racks
/// are working -- the thing the screen tells them one at a time.
#[allow(clippy::too_many_arguments)]
/// A nest: a bowl of twigs, and the eggs in it.
///
/// **Drawn as a model rather than as the squat cube its row says**, for
/// the reason a carcass is: what a player sees when they climb to a
/// branch has to be a nest with eggs in it, and a two-eighths cube with
/// a picture of a nest on top of it is a *tile* seen from above and a
/// brown stripe from anywhere else. The bowl is four low walls with a
/// hollow in the middle, and the eggs are three small boxes sitting in
/// that hollow -- so the shape reads from the side, which is where a
/// player standing on the branch beside it is.
///
/// Costs no picture: the bowl wears the nest tile the block already
/// had, and the eggs wear `food/egg.png`, which is the egg the player
/// carries away. The empty nest is the same bowl with nothing in it,
/// which is exactly what breaking the full one leaves behind.
/// A bracket fungus: a shelf growing out of the wall it is fixed to.
///
/// **Two boxes and not one, and the taper is the whole silhouette.** A
/// bracket is thick where it leaves the bark and thin at its rim, and a
/// single slab reads as a shelf somebody screwed on. The lower box is
/// also inset, which is what puts the pale pored underside in shadow
/// from every angle a player stands at -- the pores face down because
/// that is how the thing drops spores, and it is the one feature that
/// says fungus rather than plank.
///
/// It grows from the wall at -z before the turn, which is the offset
/// `types::support_at` starts from; `quarters` turns both together.
pub(crate) fn bracket_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let skin = textures.layer_for_face(block, 0);
    let quarters = primitive_shared::types::block_facing(block).quarters();
    // In sixteenths of the cell. The shelf sits above the middle
    // because a bracket grows where the trunk is already dead, which is
    // never the very foot of it -- and because a shelf at floor level
    // would be hidden by whatever is growing in the cell below.
    //
    // **Two thirds of the cell wide and nearly half of it deep.** The
    // first go at this was a third as big and read as a twig somebody
    // had stuck in the bark: a bracket is a *shelf*, and the whole of
    // what makes one recognisable is that it is broad enough to stand a
    // cup on. Kept clear of the cell's own edges so two on neighbouring
    // trunks never touch.
    const SHELF: [([f32; 3], [f32; 3]); 2] = [
        // The thick root against the bark.
        ([2.5, 6.0, 0.0], [13.5, 9.5, 3.5]),
        // ...and the brim: wider than it is thick, dropping to a lip.
        ([3.5, 6.6, 3.5], [12.5, 8.6, 7.5]),
    ];
    for (from, to) in SHELF {
        push_box(at, from, to, quarters, skin, true, sky, block_light, vertices, indices);
    }
}

/// A clay jug: a foot, a belly, a shoulder, a neck, a lip round an
/// opening, and a handle.
///
/// **The taper is most of it.** A jug drawn as a cube is a clay crate,
/// and a jug drawn as one narrow box is a chimney: what says "vessel" is
/// that the widest part is low and the opening is much narrower than the
/// body. The body rounds in at both ends -- foot, lower belly, belly,
/// shoulder -- because two steps at each end read as a curve at the size
/// a jug is seen, and one reads as a corner.
///
/// **The lip is a ring, and there is something dark inside it.** The
/// first model promised a lip in this comment and drew a solid neck: a
/// jug with no opening is a clay post, and it was one. The ring steps out
/// past the neck -- an edge that steps out is what an eye reads as a rim
/// -- and the hole in it shows a plate of basalt a little way down, which
/// is the shadow in a narrow mouth. Basalt because the crop takes a
/// picture's corner and the basalt's corner is dark stone edge to edge.
///
/// **The handle stands off the belly.** A bar pressed against the side is
/// a rib; the gap between the grip and the body is what makes it
/// something a hand goes through. It is written on +X, which is where it
/// is on a jug facing north -- every jug from before jugs turned -- and a
/// jug turns with whoever set it down (`turned_from_north`), so the handle
/// is on the same side of it from where they stood.
///
/// It stands in from the cell's edges (a sixteenth and a half at the
/// handle, three and a half at the belly) so two jugs on neighbouring cells never
/// touch, and stops at ten sixteenths, which is the height its table row
/// claims. Its clay is `jug_clay.png`, not the icon: see `blocks.toml`.
pub(crate) fn jug_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let clay = textures.layer_for_face(block, 0);
    let shadow = textures.layer_for_face(primitive_shared::types::BLOCK_BASALT, 0);
    let quarters = turned_from_north(primitive_shared::types::block_facing(block));
    const CLAY: [([f32; 3], [f32; 3]); 12] = [
        // The foot, narrower than the belly so the jug is not a tube.
        ([5.0, 0.0, 5.0], [11.0, 1.0, 11.0]),
        // The lower belly, rounding out...
        ([4.0, 1.0, 4.0], [12.0, 2.5, 12.0]),
        // ...the belly, which is the widest part of it...
        ([3.5, 2.5, 3.5], [12.5, 6.0, 12.5]),
        // ...and the shoulder, drawing in.
        ([4.5, 6.0, 4.5], [11.5, 7.5, 11.5]),
        // The neck.
        ([6.0, 7.5, 6.0], [10.0, 8.5, 10.0]),
        // The lip: a ring round a two-sixteenth opening, the long sides
        // across the whole width and the short ones between them, so no
        // two of its boxes share a plane.
        ([5.5, 8.5, 5.5], [10.5, 10.0, 7.0]),
        ([5.5, 8.5, 9.0], [10.5, 10.0, 10.5]),
        ([5.5, 8.5, 7.0], [7.0, 10.0, 9.0]),
        ([9.0, 8.5, 7.0], [10.5, 10.0, 9.0]),
        // The handle: an arm out of the neck, a grip standing off the
        // belly, and a short foot back into it low down. A whole sixteenth
        // of daylight between grip and belly: the first draft left half
        // of one, and photographed from a step away the handle was a lump
        // on the jug's side.
        ([10.0, 7.5, 7.25], [14.5, 8.5, 8.75]),
        ([13.5, 3.5, 7.25], [14.5, 7.5, 8.75]),
        ([12.5, 3.5, 7.25], [13.5, 4.5, 8.75]),
    ];
    for (from, to) in CLAY {
        push_box(at, from, to, quarters, clay, true, sky, block_light, vertices, indices);
    }
    // The dark in the mouth, a quarter of a sixteenth above the neck's top
    // so the two never share a plane facing the same way.
    push_box(
        at,
        [7.0, 8.5, 7.0],
        [9.0, 8.75, 9.0],
        quarters,
        shadow,
        true,
        sky,
        block_light,
        vertices,
        indices,
    );
}

/// A water barrel: four walls of staves, two hoops standing proud of
/// them, and -- when there is any -- the water inside, at its level.
///
/// **Walls and no lid**, because the water is the point of the thing: a
/// player who pours a jug in has to see the level come up, or the barrel
/// is a chest without a window and "is it full" is a question answered
/// by trying. Each jug raises the surface twelve sixteenths over
/// `types::BARREL_JUGS`, from the floor to two below the rim.
///
/// **The walls wear their whole picture** (`cropped: false`) and the
/// hoops a crop of theirs. A crop takes the corner of a picture, and the
/// staves are not a corner of anything: `barrel_side.png` is one drawing
/// of staves and bands made for exactly this face, so the painted bands
/// sit under the hoop boxes on every wall whatever its width. The boxes
/// are what make a band catch light as an edge instead of reading as a
/// stripe.
///
/// The long walls run the full width and the short ones fit between
/// them, so no two walls share a plane; the hoops are four boxes a ring
/// for the same reason.
pub(crate) fn barrel_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types::{barrel_contents, BARREL_JUGS, BLOCK_WATER};

    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let hoop = textures.layer_for_face(block, 0);
    let staves = textures.layer_for_face(block, 2);
    const FLOOR: ([f32; 3], [f32; 3]) = ([3.5, 0.0, 3.5], [12.5, 1.0, 12.5]);
    const WALLS: [([f32; 3], [f32; 3]); 4] = [
        ([2.0, 0.0, 2.0], [14.0, 14.0, 3.5]),
        ([2.0, 0.0, 12.5], [14.0, 14.0, 14.0]),
        ([2.0, 0.0, 3.5], [3.5, 14.0, 12.5]),
        ([12.5, 0.0, 3.5], [14.0, 14.0, 12.5]),
    ];
    // Where the two bands start, in sixteenths, a sixteenth and a half
    // tall each: a quarter of the way up and three quarters. They match
    // rows 11-12 and 3-4 of `barrel_side.png`, which is drawn to them.
    const HOOPS: [f32; 2] = [2.5, 10.0];
    const HOOP_HEIGHT: f32 = 1.5;

    push_box(at, FLOOR.0, FLOOR.1, 0, staves, true, sky, block_light, vertices, indices);
    for (from, to) in WALLS {
        push_box(at, from, to, 0, staves, false, sky, block_light, vertices, indices);
    }
    for low in HOOPS {
        let high = low + HOOP_HEIGHT;
        let ring: [([f32; 3], [f32; 3]); 4] = [
            ([1.5, low, 1.5], [14.5, high, 2.0]),
            ([1.5, low, 14.0], [14.5, high, 14.5]),
            ([1.5, low, 2.0], [2.0, high, 14.0]),
            ([14.0, low, 2.0], [14.5, high, 14.0]),
        ];
        for (from, to) in ring {
            push_box(at, from, to, 0, hoop, true, sky, block_light, vertices, indices);
        }
    }

    // **Grain at its level, on the water's scale**: a jug of wheat raises
    // the heap as far as a jug of water raises the surface, because both
    // are counted in the same jugs (`types::BARREL_JUGS`). Its picture is
    // the barrel's own bottom face (see `blocks.toml`), so the three grains
    // are three rows of the table and not a match on kinds here.
    let (jugs, contents) = match primitive_shared::types::barrel_goods(block) {
        Some((_, jugs)) => (jugs, textures.layer_for_face(block, 1)),
        None => (
            barrel_contents(block).map_or(0, |(_, jugs)| jugs),
            textures.layer_for_face(BLOCK_WATER, 0),
        ),
    };
    if jugs > 0 {
        let surface = 1.0 + 12.0 * jugs.min(BARREL_JUGS) as f32 / BARREL_JUGS as f32;
        push_box(
            at,
            [3.5, 1.0, 3.5],
            [12.5, surface, 12.5],
            0,
            contents,
            true,
            sky,
            block_light,
            vertices,
            indices,
        );
    }
}

/// A piece of an experimental tree: a square post of bark as wide as the
/// piece is, and an arm out to every piece of branch beside it.
///
/// **The boxes are `branch::wood_boxes`, the ones the collider walks into**,
/// and everything about their shape is argued there: which way the post runs,
/// why an arm is the thinner piece's width, why an arm between two upright
/// posts runs their height (`two_columns_of_trunk_leave_no_hole_in_the_seam_between_them`),
/// and which faces are buried and not drawn. The shape used to be worked out
/// here and nowhere else, and a bough was walked into as its whole cell while
/// a twig was walked through: "добавь коллизию веткам".
/// `a_piece_of_branch_is_drawn_exactly_where_it_is_walked_into` keeps the two
/// from parting again.
///
/// `joins` is what `branch::joins` reads off the cells round the piece; a
/// piece drawn with no world round it is given them by hand.
///
/// **Cover nothing.** The table rows say opacity zero for both pieces, so
/// `cover_table` gives them nothing and the ground and leaves round a
/// trunk keep every face -- see `a_piece_of_branch_hides_no_face_of_its_neighbours`.
///
/// Bark is the log's side -- the oak's, a birch's for a piece in birch bark
/// (`types::birch_branch`), a palm's own for a palm -- cut (`FINE_UV_BIT`) to
/// each face's width: a post four sixteenths wide wears four texels of bark,
/// not the whole picture squeezed into it.
pub(crate) fn branch_block(
    at: [f32; 3],
    block: BlockId,
    joins: BranchJoins,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    // A palm wears its own ringed bark, and every other piece the side of its
    // wood's log (`types::piece_log`: a birch's white bark, a fir's red-brown
    // -- the log's own picture, no new layer).
    let wood = if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_PALM_TRUNK {
        primitive_shared::types::BLOCK_PALM_TRUNK
    } else {
        primitive_shared::types::piece_log(block).unwrap_or(primitive_shared::types::BLOCK_LOG)
    };
    let bark = textures.layer_for_face(wood, 2);
    for piece in primitive_shared::branch::wood_boxes(block, joins) {
        push_box_open(at, piece.from, piece.to, 0, bark, true, sky, block_light, piece.open, vertices, indices);
    }
}

/// What `branch_block` draws a piece of branch from: how wide the wood across
/// each face is (`None` where there is none), and for each face whether that
/// wood goes on past this post's two ends, `[plus, minus]` along the post.
pub(crate) type BranchJoins = (primitive_shared::branch::Joins, primitive_shared::branch::Beside);

/// Nothing beside a piece goes on anywhere: a piece drawn outside the world,
/// in a hand or a test.
pub(crate) const ALONE: primitive_shared::branch::Beside = primitive_shared::branch::ALONE;

/// One cell of a palm's trunk, as the slices `palm::trunk_slices` finds along
/// its course -- the leaning line the steps are read back into, see
/// `palm::PalmCourse` for why.
///
/// **The same slices `geometry::for_each_block_box` collides.** The course
/// used to live here and nowhere else, and the collider kept taking every
/// piece as its whole cell: a player walked into air beside the bark and
/// through the part of it that leaned out over the sand. One computation in
/// `primitive_shared` is what keeps the two from parting again, and
/// `a_palm_trunk_is_drawn_exactly_where_it_is_walked_into` holds them to it.
///
/// **Each slice is moved, not cut from a moved picture.** The box is centred in
/// its own frame and `at` carries the offset, so every slice wears the same
/// columns of bark and the grain follows the lean -- and a slice pushed half a
/// cell out does not ask `with_fine_uv` for a place left of the picture, which
/// it would clamp into a smear.
fn palm_trunk_block(
    at: [f32; 3],
    block: BlockId,
    near: impl Fn(i32, i32, i32) -> BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types::{block_kind, is_palm_crown, BLOCK_PALM_TRUNK};
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let bark = textures.layer_for_face(BLOCK_PALM_TRUNK, 2);
    // **The cut end (`palm_top.png`) only where the trunk ends.** Three places
    // for it were weighed:
    //
    // * *Every slice's top and bottom.* A leaning trunk shows a sliver of each
    //   slice's top at every step of its lean, and the palm came out banded
    //   like a stack of coins.
    // * *Whatever face of the cell has no trunk over it.* The lower piece of a
    //   step has air over it and bark going on sideways, half way up: its top
    //   is a join, and it would have worn a cut.
    // * **The cell's own floor and ceiling, with nothing of the palm past them
    //   (chosen).** A slice that reaches the top of its cell is the run's top
    //   there, because the lower piece of a step stops at half height
    //   (`palm::PalmCourse`); and "nothing of the palm" counts the crown, so a
    //   living palm is not cut under its heart. What is left is the root, the
    //   top of a palm whose crown was taken, and both sides of a piece broken
    //   out of it -- `a_palm_trunk_shows_its_cut_end_only_where_nothing_of_the_palm_goes_on`.
    let goes_on = |dy: i32| {
        let next = near(0, dy, 0);
        block_kind(next) == BLOCK_PALM_TRUNK || is_palm_crown(next)
    };
    let top = if goes_on(1) { bark } else { textures.layer_for_face(BLOCK_PALM_TRUNK, 0) };
    let bottom = if goes_on(-1) { bark } else { textures.layer_for_face(BLOCK_PALM_TRUNK, 1) };
    for slice in primitive_shared::palm::trunk_slices(block, &near) {
        let (lo, hi) = (8.0 - slice.half_width * 16.0, 8.0 + slice.half_width * 16.0);
        let [ox, oz] = slice.offset;
        let mut layers = [bark; 6];
        if slice.top >= 1.0 {
            layers[0] = top;
        }
        if slice.bottom <= 0.0 {
            layers[1] = bottom;
        }
        push_box_faces(
            [at[0] + ox, at[1], at[2] + oz],
            [lo, slice.bottom * 16.0, lo],
            [hi, slice.top * 16.0, hi],
            0,
            layers,
            true,
            sky,
            block_light,
            0,
            vertices,
            indices,
        );
    }
}

/// Which length of its frond a cell of a palm's crown holds. See `CrownPart`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrondLength {
    /// Out of the heart: the trunk's top piece is under it on the inside.
    Root,
    /// The rest of the rise, level with the root.
    Middle,
    /// The drooping end, one lower than the length inside it.
    Tip,
}

/// **What one cell of a palm's crown is**, read off the cells round it.
///
/// A crown was a flat star of leaf cubes, and the player's word for it was
/// "странная": from the sea a palm was a green slab on a pole. A frond is a
/// long leaf that rises out of the top of the trunk and droops, and to draw
/// one a cell has to know which way its frond runs and how far along it the
/// cell is -- which the id does not say.
///
/// Three ways to say it were weighed:
///
/// * *A heading in the variant.* Eight headings fill its three bits, and the
///   picked frond already spends a value (`types::BLOCK_PALM_FRONDS_PICKED`);
///   the heading would have to be taught to growth, felling and the list of
///   ids the anti-cheat believes, for a picture.
/// * *Walking to the trunk.* The top piece is up to three columns from a tip,
///   and the mesher's padding holds one: a crown over a chunk seam would have
///   drawn the fronds on the far side pointing nowhere.
/// * **Read off the neighbours (chosen).** Every cell of the star the
///   generator grows (`worldgen::palm_cells`) can tell what it is from the
///   cells beside it. Over the trunk is the heart; level with the trunk's top
///   piece and beside it hangs a bunch of coconuts, or the stalk one was picked
///   from. Anything else leans away from the crown cells level with it and one
///   above, and from trunk at any of the three levels; it is the root of its
///   frond if the trunk is under it on the inside, the tip if its frond goes
///   on one level up on the inside, and the middle otherwise. Crown cells one
///   level *down* are not counted: a middle length's own tip is outward and
///   below it, and a bunch of coconuts is inward and below, and with one of
///   the diagonals bare -- as the generator grew one, and as a player can
///   still break one off -- either was enough to turn it towards a corner.
///   `a_palm_crown_reads_each_frond_off_the_cells_round_it` holds every palm
///   the generator can grow to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrownPart {
    /// Over the top of the trunk, where every frond starts.
    Heart,
    /// Beside the top of the trunk and level with it. `away` is the side
    /// away from the trunk.
    Bunch { away: (i32, i32) },
    /// A length of one frond: which of eight ways it runs out of the heart.
    Frond { heading: (i32, i32), length: FrondLength },
    /// A crown cell with nothing of its crown round it.
    Stray,
}

/// See `CrownPart`. `near` is the block at an offset from the cell: at most
/// one away on every axis.
pub(crate) fn crown_part(near: impl Fn(i32, i32, i32) -> BlockId) -> CrownPart {
    use primitive_shared::types::{block_kind, BLOCK_PALM_COCONUTS, BLOCK_PALM_FRONDS, BLOCK_PALM_TRUNK};
    const SIDES: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let trunk = |dx: i32, dy: i32, dz: i32| block_kind(near(dx, dy, dz)) == BLOCK_PALM_TRUNK;
    let crown = |dx: i32, dy: i32, dz: i32| matches!(block_kind(near(dx, dy, dz)), BLOCK_PALM_FRONDS | BLOCK_PALM_COCONUTS);
    if trunk(0, -1, 0) {
        return CrownPart::Heart;
    }
    if let Some((sx, sz)) = SIDES.into_iter().find(|&(sx, sz)| trunk(sx, 0, sz)) {
        return CrownPart::Bunch { away: (-sx, -sz) };
    }
    // Away from the trunk, weighed three to a crown cell's one: the trunk is
    // the one neighbour that is never a broken-off diagonal or a picked frond.
    let mut away = (0i32, 0i32);
    for dy in -1..=1 {
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let weight = if trunk(dx, dy, dz) {
                    3
                } else if dy >= 0 && crown(dx, dy, dz) {
                    1
                } else {
                    0
                };
                away.0 -= weight * dx;
                away.1 -= weight * dz;
            }
        }
    }
    let (ax, az) = (away.0.abs(), away.1.abs());
    if ax == 0 && az == 0 {
        return CrownPart::Stray;
    }
    // A diagonal only when the two pulls are close: a middle length with one
    // diagonal broken off beside it is pulled two out and one across.
    let heading = if 2 * ax.min(az) > ax.max(az) {
        (away.0.signum(), away.1.signum())
    } else if ax >= az {
        (away.0.signum(), 0)
    } else {
        (0, away.1.signum())
    };
    let length = if trunk(-heading.0, -1, -heading.1) {
        FrondLength::Root
    } else if crown(-heading.0, 1, -heading.1) {
        FrondLength::Tip
    } else {
        FrondLength::Middle
    };
    CrownPart::Frond { heading, length }
}

/// How far a frond reaches from the heart of its crown, in cells: the far
/// side of the drooping tip the generator puts `worldgen::PALM_FROND_REACH`
/// out, and a little past it so the tip is a point rather than a cut.
const FROND_LENGTH: f32 = 3.6;

/// How high a frond's rib is, `s` cells out from the heart, above the floor
/// of the trunk's top piece.
///
/// **A rise and a fall**, fitted through three heights the crown's cells give:
/// out of the heart a little over the trunk's top (1.15), back down to the
/// floor of the cells over the top piece where the rising lengths meet the
/// tips (1.0 at two and a half out), and near the floor of the tip's cell at
/// the end (0.3 at three and a half). The top of the arch is about a metre
/// out and a third of a cell up -- a frond seen from the beach is a curve,
/// not a ramp.
fn frond_rib(s: f32) -> f32 {
    1.15 + 0.397 * s - 0.183 * s * s
}

/// Half a frond's width `s` cells out: narrow where it leaves the heart,
/// widest along the middle, a point at the tip.
///
/// **Seven tenths of a cell at the widest, and it was half.** "мало листвы":
/// from under the crown a frond a cell across left the sky showing between
/// every two of them past a couple of cells out -- measured by
/// `a_palm_crown_hides_most_of_the_sky_over_its_trunk`. Wider still was
/// weighed and refused: the leaflets stand out of their cell sideways by
/// what passes half a cell, a chunk is culled by its own box
/// (`frustum::contains_chunk`), and a frond cut off at the edge of the
/// screen is a worse picture than a gap.
fn frond_half_width(s: f32) -> f32 {
    0.1 + 0.6 * (std::f32::consts::PI * (s / FROND_LENGTH).clamp(0.0, 1.0)).sin()
}

/// A palm's crown cell: fronds, the heart they rise from, or a bunch of
/// coconuts. See `CrownPart` for how a cell knows which.
#[allow(clippy::too_many_arguments)]
fn palm_crown_block(
    cell: [i32; 3],
    at: [f32; 3],
    block: BlockId,
    (part, near): (CrownPart, impl Fn(i32, i32, i32) -> BlockId),
    textures: &crate::engine::texture::FaceLayers,
    (light, model): (u8, u8),
    tint: u32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    sprites: &mut Vec<u32>,
) {
    use primitive_shared::types::{block_kind, BLOCK_PALM_COCONUTS, BLOCK_PALM_FRONDS};
    let leaf = textures.layer_for_face(BLOCK_PALM_FRONDS, 0);
    match part {
        CrownPart::Heart => heart_tuft(at, leaf, light, tint, vertices, sprites),
        CrownPart::Frond { heading, length } => frond_block(at, heading, length, leaf, light, tint, vertices, sprites),
        // A lone cell of crown still droops, some way the cell's hash picks.
        CrownPart::Stray => {
            const HEADINGS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
            let heading = HEADINGS[(cell_hash(cell[0], cell[1], cell[2]) & 3) as usize];
            frond_block(at, heading, FrondLength::Tip, leaf, light, tint, vertices, sprites);
        }
        CrownPart::Bunch { away } => {
            // The top piece's bark as it is drawn, which a leaning trunk has
            // moved off the middle of its cell: see `bunch_nuts`.
            let slices: Vec<primitive_shared::palm::TrunkSlice> = primitive_shared::palm::slices_beside(away, near).collect();
            if block_kind(block) == BLOCK_PALM_COCONUTS {
                let nuts = bunch_nuts(away, &slices);
                coconut_bunch(cell, at, &nuts, textures.layer_for_face(BLOCK_PALM_COCONUTS, 0), model, vertices, indices);
            } else {
                // Picked: the stub of the stalk the bunch hung from, which
                // is where the next one sets -- out of the same bark.
                let bark = textures.layer_for_face(primitive_shared::types::BLOCK_PALM_TRUNK, 2);
                let (face, across) = bark_beside(away, &slices, 11.0, 15.0);
                let (a, b) = (bunch_point(away, face - 0.5, across - 1.0, 11.0), bunch_point(away, face + 3.5, across + 1.0, 15.0));
                let (from, to) = ([a[0].min(b[0]), a[1], a[2].min(b[2])], [a[0].max(b[0]), b[1], a[2].max(b[2])]);
                push_box(at, from, to, 0, bark, true, model & 0x0F, (model >> 4) & 0x0F, vertices, indices);
            }
        }
    }
}

/// The young leaves still folded in the heart of a crown: two narrow blades
/// crossing over the top of the trunk.
fn heart_tuft(at: [f32; 3], layer: u32, light: u8, tint: u32, vertices: &mut Vec<Vertex>, sprites: &mut Vec<u32>) {
    let packed = pack_light(light & 0x0F, (light >> 4) & 0x0F, 3, 0);
    for ((ax, az), (bx, bz)) in [((0.3, 0.3), (0.7, 0.7)), ((0.3, 0.7), (0.7, 0.3))] {
        let base = vertices.len() as u32;
        for ((x, y, z), uv) in [
            ((ax, 0.0, az), [0.0, 1.0]),
            ((bx, 0.0, bz), [1.0, 1.0]),
            ((bx, 0.6, bz), [1.0, 0.0]),
            ((ax, 0.6, az), [0.0, 0.0]),
        ] {
            vertices.push(Vertex::tinted([at[0] + x, at[1] + y, at[2] + z], uv, layer, packed, tint));
        }
        sprites.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// One cell's length of a frond: a rib along `frond_rib`, and a leaflet
/// hanging down from it on each side, in the cutout pass.
///
/// **The frond is drawn from where it is on the whole leaf, not from the cell.**
/// Each length draws its own stretch of the arch out of the heart -- the root
/// from beside the heart to half way into the second cell, the middle to half
/// way into the third, the tip to the end -- so the lengths of one frond meet
/// end to end whichever cells they were drawn from, and a length whose cell was
/// broken leaves a gap in the leaf rather than a floating square.
///
/// Rejected: *a textured cube with the frond painted on it*, which is what the
/// crown was, and a crown of cubes cannot droop. And *one quad per length* --
/// a flat plank of leaf from the side, where the pair hanging from the rib
/// reads as a leaf from any side the beach looks from.
///
/// **Two ranks of leaflets on each side**, since "мало листвы": the full width
/// spreading nearly flat off the rib, and under it a narrower rank hanging
/// steeply. One rank was a thin V from the beach, a line of leaf with sky
/// over and under it; two are a frond with body at eye height, and from under
/// the crown the flat rank is what closes the sky. More fronds on headings
/// between the eight were weighed -- drawn by the heart, they would stand
/// three cells out of the cell that draws them, past what a chunk's culling
/// box allows for.
#[allow(clippy::too_many_arguments)]
fn frond_block(
    at: [f32; 3],
    heading: (i32, i32),
    length: FrondLength,
    layer: u32,
    light: u8,
    tint: u32,
    vertices: &mut Vec<Vertex>,
    sprites: &mut Vec<u32>,
) {
    use std::f32::consts::{FRAC_1_SQRT_2, SQRT_2};
    let packed = pack_light(light & 0x0F, (light >> 4) & 0x0F, 3, 0);
    let diagonal = heading.0 != 0 && heading.1 != 0;
    let unit = if diagonal { FRAC_1_SQRT_2 } else { 1.0 };
    let (dx, dz) = (heading.0 as f32 * unit, heading.1 as f32 * unit);
    // How far out this cell's middle is, which stretch of the frond it draws,
    // and where the trunk's top piece is below its floor.
    let cells_out = |n: f32| if diagonal { n * SQRT_2 } else { n };
    let (middle, from, to, floor) = match length {
        FrondLength::Root => (cells_out(1.0), 0.35, cells_out(1.5), -1.0),
        FrondLength::Middle if !diagonal => (2.0, 1.5, 2.5, -1.0),
        // A diagonal frond is two lengths: the second is its tip, and a
        // diagonal the neighbours call a middle is drawn as one.
        FrondLength::Middle => (cells_out(2.0), cells_out(1.5), FROND_LENGTH, -1.0),
        FrondLength::Tip => (cells_out(if diagonal { 2.0 } else { 3.0 }), cells_out(if diagonal { 1.5 } else { 2.5 }), FROND_LENGTH, 0.0),
    };
    let rib = |s: f32| [at[0] + 0.5 + (s - middle) * dx, at[1] + floor + frond_rib(s), at[2] + 0.5 + (s - middle) * dz];
    // The leaflets hang further below the rib towards the tip, as a frond's
    // do: the spreading rank a little, the hanging rank most of its width.
    // (share of the half width, how far it drops at the heart, and how much
    // more by the tip.)
    const RANKS: [(f32, f32, f32); 2] = [(1.0, 0.22, 0.3), (0.55, 0.95, 0.35)];
    let edge = |s: f32, side: f32, (share, drop, droop): (f32, f32, f32)| {
        let r = rib(s);
        let w = frond_half_width(s) * share;
        [r[0] - dz * w * side, r[1] - w * (drop + droop * s / FROND_LENGTH), r[2] + dx * w * side]
    };
    let pieces = ((to - from) / 0.5).ceil().max(1.0) as usize;
    for piece in 0..pieces {
        let a = from + (to - from) * piece as f32 / pieces as f32;
        let b = from + (to - from) * (piece + 1) as f32 / pieces as f32;
        for rank in RANKS {
            for side in [-1.0, 1.0] {
                let base = vertices.len() as u32;
                for (corner, uv) in [(rib(a), [0.0, 0.0]), (rib(b), [0.0, 1.0]), (edge(b, side, rank), [1.0, 1.0]), (edge(a, side, rank), [1.0, 0.0])] {
                    vertices.push(Vertex::tinted(corner, uv, layer, packed, tint));
                }
                sprites.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
}

/// A place in a bunch's cell, in sixteenths: `along` out from the face it
/// shares with the trunk, `across` from the middle of that face, `up` from
/// the floor.
fn bunch_point(away: (i32, i32), along: f32, across: f32, up: f32) -> [f32; 3] {
    match away {
        (1, _) => [along, up, 8.0 + across],
        (-1, _) => [16.0 - along, up, 8.0 - across],
        (_, 1) => [8.0 - across, up, along],
        _ => [8.0 + across, up, 16.0 - along],
    }
}

/// **One coconut: three boxes crossed through one middle**, in sixteenths.
///
/// A cube a coconut's size is a crate; three bars through one middle, each
/// long on its own axis, leave its corners cut off, which at a few metres is
/// a round thing. The short sides are two different fractions of the long one
/// so no two of the three boxes share a plane -- a shared plane is two
/// surfaces the depth buffer cannot tell apart (see `BITE`).
fn nut_boxes(centre: [f32; 3], radius: f32) -> [([f32; 3], [f32; 3]); 3] {
    let (long, wide, narrow) = (radius, radius * 0.74, radius * 0.62);
    let spread = |e: [f32; 3]| {
        (
            [centre[0] - e[0], centre[1] - e[1], centre[2] - e[2]],
            [centre[0] + e[0], centre[1] + e[1], centre[2] + e[2]],
        )
    };
    [spread([long, wide, narrow]), spread([narrow, long, wide]), spread([wide, narrow, long])]
}

/// Where the bark of the top piece beside a bunch stands, between two heights
/// of the bunch's cell: how far out from the face the cell shares with the
/// trunk (negative, inside the trunk's cell) and how far across from that
/// face's middle, both in sixteenths, as `bunch_point` takes them.
///
/// The slice that stands out furthest towards the bunch of those level with
/// any part of the span, so a nut pressed to it goes into no other; with no
/// slice there, the bark of a straight top piece eight sixteenths across.
fn bark_beside(away: (i32, i32), slices: &[primitive_shared::palm::TrunkSlice], low: f32, high: f32) -> (f32, f32) {
    let (ax, az) = (away.0 as f32, away.1 as f32);
    slices
        .iter()
        .filter(|slice| slice.bottom * 16.0 < high && slice.top * 16.0 > low)
        .map(|slice| {
            let [ox, oz] = slice.offset;
            ((ox * ax + oz * az + slice.half_width - 0.5) * 16.0, (oz * ax - ox * az) * 16.0)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap_or((-4.0, 0.0))
}

/// **Every nut a bunch can hang, pressed against the bark of the top of its
/// trunk as it is drawn**: its middle and its radius, in sixteenths of the
/// bunch's cell. The cell's hash takes the first two or all three.
///
/// "кокосы часто даже не касаются ствола". The nuts were placed against the
/// face the cell shares with the trunk, as though the bark were always four
/// sixteenths behind it, and two things were wrong with that. **The low nut
/// hung two and a half sixteenths further out than the other two**, so it
/// never met the bark at all -- 3.8 sixteenths of air beside a straight trunk,
/// where the upper two were a sixteenth short. **And a leaning trunk's top
/// piece is not over the middle of its cell**: its course comes up from the
/// step below (`palm::PalmCourse`), so level with the nuts its bark stands up
/// to a sixteenth nearer a bunch on the side it leans from and half a
/// sixteenth further from one on the side it leans to. Measured by the gap
/// `every_coconut_hangs_against_the_bark_of_the_top_of_its_trunk` asks for,
/// over every palm the generator grows: 3012 nuts of 4524 were more than a
/// sixteenth from their bark, the low nut every time, and the worst 4.2
/// sixteenths, on the side the palm leans to.
///
/// So each nut is pressed to the slice of bark level with it
/// (`bark_beside`), moved across with it, and a little into it -- a
/// different little for each nut, because nuts pressed to one plane to the
/// same depth put their faces in one plane (see `BITE`). Rejected: *moving
/// the bunch's cell* in the generator to the side the trunk comes nearer,
/// which fixes no palm already grown; and *one bark for the whole bunch*,
/// which pressed the low nut to a slice above it and left it a sixteenth off
/// its own.
fn bunch_nuts(away: (i32, i32), slices: &[primitive_shared::palm::TrunkSlice]) -> [([f32; 3], f32); 3] {
    // (across, up, radius, how deep into the bark), in sixteenths.
    // The low nut sits between and under the other two, as low as the upper
    // half of the cell lets it; at 9.2 its top would share a plane with the
    // bottom of the second nut's flat box.
    const NUTS: [(f32, f32, f32, f32); 3] = [(-2.7, 11.8, 2.5, 0.25), (2.6, 12.3, 2.4, 0.35), (0.2, 9.1, 2.6, 0.5)];
    NUTS.map(|(across, up, radius, depth)| {
        let (face, shift) = bark_beside(away, slices, up - radius, up + radius);
        (bunch_point(away, face + radius - depth, across + shift, up), radius)
    })
}

/// **A bunch of coconuts: two or three round nuts hanging against the top of
/// the trunk**, under the first lengths of the fronds.
///
/// "кокосы надо добавлять как модели": the bunch was a cube of frond with
/// brown blots painted in it, standing beside the trunk -- from underneath, a
/// crate. The nuts hang in the upper half of the cell, pressed against the
/// bark (`bunch_nuts`), which is where a player looking up for them finds
/// them. Two or three by the cell's hash, so a crown's bunches are not
/// stamped.
fn coconut_bunch(
    cell: [i32; 3],
    at: [f32; 3],
    nuts: &[([f32; 3], f32); 3],
    husk: u32,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let count = 2 + (cell_hash(cell[0], cell[1], cell[2]) & 1) as usize;
    for &(centre, radius) in &nuts[..count] {
        for (from, to) in nut_boxes(centre, radius) {
            push_box(at, from, to, 0, husk, true, sky, block_light, vertices, indices);
        }
    }
}

/// A coconut on its own, as it lies dropped or is held: one nut of the bunch
/// the palm draws (`coconut_bunch`), standing on its floor in the middle of
/// its cell.
pub(crate) fn coconut_block(
    at: [f32; 3],
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let husk = textures.layer_for_face(primitive_shared::types::BLOCK_PALM_COCONUTS, 0);
    for (from, to) in nut_boxes([8.0, 3.4, 8.0], 3.4) {
        push_box(at, from, to, 0, husk, true, light & 0x0F, (light >> 4) & 0x0F, vertices, indices);
    }
}

/// Which faces of a piece of branch join the wood beside it, and how wide
/// that wood is: `branch::joins`, the collider's own reading, asked of the
/// padding ring -- which holds the next chunk's cells, so a limb that crosses
/// a border is joined across it. See `branch_block`.
fn branch_joins(cache: &Neighbourhood, cell: usize, y: i32) -> BranchJoins {
    primitive_shared::branch::joins(|dx, dy, dz| cache.block_near(cell, y, dx, dy, dz))
}

pub(crate) fn nest_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types::{BLOCK_EGG, BLOCK_NEST_EGGS};

    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let twigs = textures.layer_for_face(block, 0);
    // The bowl: a floor and four walls, all in sixteenths of the cell.
    // Two sixteenths tall, which is what the block's own row says it is
    // (`blocks`, `thickness: 2`) -- the collider and the step height
    // read that number, and a model taller than its box is a thing you
    // walk through.
    // The floor fits inside the walls, as the barrel's does. It used to run
    // under them to the outside, and every outer face of the bowl's foot
    // was two faces in one plane (`model_overlap`).
    const FLOOR: ([f32; 3], [f32; 3]) = ([3.5, 0.0, 3.5], [12.5, 1.0, 12.5]);
    const WALLS: [([f32; 3], [f32; 3]); 4] = [
        ([2.0, 0.0, 2.0], [14.0, 2.0, 3.5]),
        ([2.0, 0.0, 12.5], [14.0, 2.0, 14.0]),
        ([2.0, 0.0, 3.5], [3.5, 2.0, 12.5]),
        ([12.5, 0.0, 3.5], [14.0, 2.0, 12.5]),
    ];
    push_box(at, FLOOR.0, FLOOR.1, 0, twigs, true, sky, block_light, vertices, indices);
    for (from, to) in WALLS {
        push_box(at, from, to, 0, twigs, true, sky, block_light, vertices, indices);
    }

    if primitive_shared::types::block_kind(block) != BLOCK_NEST_EGGS {
        return;
    }
    // Three eggs, at three sizes and three places, because a clutch
    // laid in a row reads as a machine. They stand a whisker proud of
    // the bowl's rim -- an egg entirely inside a two-sixteenth bowl is
    // an egg nobody can see.
    let shell = textures.layer_for_face(BLOCK_EGG, 0);
    const EGGS: [([f32; 3], [f32; 3]); 3] = [
        ([4.5, 1.0, 5.0], [7.5, 3.5, 8.0]),
        ([8.0, 1.0, 4.5], [10.5, 3.0, 7.0]),
        ([6.0, 1.0, 8.5], [9.0, 3.2, 11.5]),
    ];
    for (from, to) in EGGS {
        push_box(at, from, to, 0, shell, true, sky, block_light, vertices, indices);
    }
}

/// A cairn (`types::BLOCK_CAIRN`): three stones stacked, each smaller than
/// the one under it, in the cobble it wears.
///
/// Three boxes and not six: a heap of six pebbles drawn stone for stone
/// is a pile of dice at any distance a cairn is meant to be seen from,
/// and what reads as "somebody put this here" is the tapering stack.
/// Exactly `thickness` (six eighths) tall, the nest's rule: the collider
/// and the step read that number, and a heap taller than its box is a heap
/// walked through.
pub(crate) fn cairn_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let stone = textures.layer_for_face(block, 0);
    // In sixteenths, square about the middle so a quarter turn changes
    // nothing (the cairn has no front). Each stone stands in from the one
    // below, so no two faces share a plane (`model_overlap`); the undersides
    // lie on the stone below, where nothing can see them.
    const STONES: [([f32; 3], [f32; 3]); 3] = [
        ([2.5, 0.0, 2.5], [13.5, 5.0, 13.5]),
        ([4.0, 5.0, 4.0], [12.0, 9.5, 12.0]),
        ([5.5, 9.5, 5.5], [10.5, 12.0, 10.5]),
    ];
    for (from, to) in STONES {
        push_box(at, from, to, 0, stone, true, sky, block_light, vertices, indices);
    }
}

/// What one cell of a rack draws.
///
/// **Three answers, and they were two.** This was an `Option`: `Some` for the
/// near bottom of a whole rack, which draws all four cells' frame, and `None`
/// for everything else -- which meant both "a top or far cell of a whole
/// rack, whose frame the near bottom has already drawn" and "a lone cell from
/// an older save, which draws its own". `rack_block` could only take `None`
/// the second way, so every whole rack was drawn four times: the big frame
/// and a small one in each of its other three cells. Players saw a rack made
/// of four racks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RackColumns {
    /// A cell that is not one of four: the one-cell frame it always was.
    Lone,
    /// A top or far cell of a whole rack: drawn by its near bottom.
    Part,
    /// The near bottom of a whole rack, with the goods index of its near
    /// column and of its far one (`rack::HANGING`).
    Whole(u8, u8),
}

/// What this rack cell draws, read off the cells round it.
///
/// Whether the rack is whole is `types::rack_whole`, the rules' own answer,
/// so the mesher and the server agree on which racks are four cells.
fn rack_columns(cache: &Neighbourhood, cell: usize, y: i32, block: BlockId) -> RackColumns {
    use primitive_shared::types as t;
    let near = |(dx, dy, dz): (i32, i32, i32)| cache.block_near(cell, y, dx, dy, dz);
    if !t::rack_whole((0, 0, 0), block, |at| Some(near(at))) {
        return RackColumns::Lone;
    }
    if t::rack_anchor((0, 0, 0), block) != (0, 0, 0) {
        return RackColumns::Part;
    }
    let (dx, dz) = t::rack_far_step(t::block_facing(block));
    let near_top = near((0, 1, 0));
    let (far_bottom, far_top) = (near((dx, 0, dz)), near((dx, 1, dz)));
    RackColumns::Whole(t::rack_column_goods(block, near_top), t::rack_column_goods(far_bottom, far_top))
}

pub(crate) fn rack_block(
    at: [f32; 3],
    block: BlockId,
    columns: RackColumns,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types::rack_is_loaded;

    // Through `turned_from_north` and not `Facing::quarters`, which turns
    // the other way: an east rack was drawn as a west one. The frame used to
    // stand a half sixteenth off the cell's middle, and the wrong turn put it
    // that far from the box `geometry::block_box` collides it as; it is
    // centred now, and the turn is still the right one.
    let quarters = turned_from_north(primitive_shared::types::block_facing(block));
    // Rough poles with the bark on and nothing else, as the player's
    // photograph is: two crossed A-frames and a ridge pole, every box `pole`
    // (`misc/drying_rack.bbmodel`, and `logic::model_notes` for why each piece
    // is where it is). No planks: a sawn board is not what a camp lashes a
    // rack out of. The skin is its `loaded` box, and worn uncropped: its
    // picture is drawn for that slab (`Material::StretchedHide`).
    // **Four cells, and only the near bottom draws them.** The frame is two
    // cells along its ridge and two high (`types::rack_cells`), so the model
    // is written to thirty-two sixteenths and stands out of its own cell --
    // the palm's arrangement, and the same reason: a thing that is bigger
    // than a cell is drawn once rather than sliced into four models that
    // have to meet exactly.
    if columns == RackColumns::Part {
        return;
    }
    if let RackColumns::Whole(near, far) = columns {
        let model = crate::logic::models::prop(crate::logic::models::Prop::DryingRack2x2);
        push_prop(at, model, quarters, false, Hinged::Whole, |_| 0, 0, textures, light, vertices, indices);
        // ...and what hangs under each ridge, the things themselves, so a
        // ridge of red strips turning dark from the far end is what a player
        // sees from across the camp (`rack::columns_showing`). Each column
        // keeps clear of the poles that cross at its end (x 1.2..4.4 and
        // 27.6..30.8) and of the joint in the middle; the far ridge sits
        // 0.15 higher than the near one, and what hangs from it with it.
        for (goods, x0, ridge) in [(near, 5.0, 25.2), (far, 17.0, 25.35)] {
            hang_goods(at, goods, x0, ridge, quarters, textures, light, vertices, indices);
        }
        return;
    }
    // **The hide frame is a skin laced into a standing frame of poles**, its
    // own model (`misc/hide_frame.bbmodel`): the lacing hangs loose from the
    // poles while the frame is bare and runs taut to the skin once there is
    // one. A cured skin is the same boxes in leather's browns
    // (`Material::cured`), which is how a tanner sees from across the camp
    // that there is something to take up.
    if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_HIDE_FRAME {
        let pieces = hide_frame_pieces(block);
        push_prop(at, &pieces, quarters, rack_is_loaded(block), Hinged::Whole, |_| 0, 0, textures, light, vertices, indices);
        return;
    }
    // ...and a lone cell of the rack of two by two -- an old save's one-cell
    // rack before `World::load` has made it a hide frame, and the rack as it
    // is held in the hand -- is still the old frame of poles.
    let model = crate::logic::models::prop(crate::logic::models::Prop::DryingRack);
    push_prop(at, model, quarters, rack_is_loaded(block), Hinged::Whole, |_| 0, 0, textures, light, vertices, indices);
}

/// The boxes of a hide frame as this one is drawn: the model's own, with
/// the skin in leather's browns once it has cured (`types::hide_is_cured`).
/// Borrowed as it is for every frame but a cured one.
fn hide_frame_pieces(block: BlockId) -> std::borrow::Cow<'static, [PropBox]> {
    let model = crate::logic::models::prop(crate::logic::models::Prop::HideFrame);
    if !primitive_shared::types::hide_is_cured(block) {
        return std::borrow::Cow::Borrowed(model);
    }
    let cured = |piece: &PropBox| PropBox { material: if piece.loaded { piece.material.cured() } else { piece.material }, ..*piece };
    std::borrow::Cow::Owned(model.iter().map(cured).collect())
}

/// The picture a hung good is dressed in: the carried one where it has one
/// (a hide's is the folded skin, not the stretched one) and its face
/// otherwise. `texture::is_hung` reads its `Hung` off the same picture, so
/// a swatch is always a piece of the layer it is sampled from.
fn hung_picture(item: BlockId, textures: &crate::engine::texture::FaceLayers) -> u32 {
    textures.layer_for_item(item).unwrap_or_else(|| textures.layer_for_face(item, 0))
}

/// What a box of a hung good wears.
#[derive(Clone, Copy)]
enum Dress {
    /// A material laid on the box the way a pole wears its bark
    /// (`push_box`'s `cropped`).
    Material(u32, bool),
    /// One hole-free piece of a picture that has holes in it
    /// (`relief::Hung::swatch`), stretched over every face.
    Swatch(u32, [f32; 4]),
}

impl Dress {
    /// The picture a box of this good wears: its own material where the
    /// atlas has one with no holes, and a swatch of its carried picture
    /// where it has none.
    fn of(item: BlockId, material: Option<(BlockId, bool)>, textures: &crate::engine::texture::FaceLayers) -> Self {
        if let Some((block, cropped)) = material {
            return Dress::Material(textures.layer_for_face(block, 0), cropped);
        }
        let swatch = textures.hung(item).map_or([0.0, 0.0, 1.0, 1.0], |hung| hung.swatch);
        Dress::Swatch(hung_picture(item, textures), swatch)
    }
}

/// One box of a hung good, in the rack model's sixteenths.
///
/// A swatch is laid on after `push_box` has built the box, so the winding,
/// the turned face index and the hair's growth (`BITE`) are that function's
/// and nothing here can get them wrong: only the texture coordinate of each
/// face is moved, each face's own range squeezed into the swatch.
#[allow(clippy::too_many_arguments)]
fn push_dressed(
    at: [f32; 3],
    from: [f32; 3],
    to: [f32; 3],
    quarters: u32,
    dress: Dress,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let (layer, cropped, swatch) = match dress {
        Dress::Material(layer, cropped) => (layer, cropped, None),
        Dress::Swatch(layer, swatch) => (layer, true, Some(swatch)),
    };
    let first = vertices.len();
    push_box(at, from, to, quarters, layer, cropped, sky, block_light, vertices, indices);
    let Some([u0, v0, u1, v1]) = swatch else {
        return;
    };
    for quad in vertices[first..].chunks_exact_mut(4) {
        let uvs = [0, 1, 2, 3].map(|k| quad[k].uv());
        let low = [0, 1].map(|a| uvs.iter().map(|uv| uv[a]).fold(f32::MAX, f32::min));
        let high = [0, 1].map(|a| uvs.iter().map(|uv| uv[a]).fold(f32::MIN, f32::max));
        for (vertex, uv) in quad.iter_mut().zip(uvs) {
            let along = |a: usize| if high[a] > low[a] { (uv[a] - low[a]) / (high[a] - low[a]) } else { 0.5 };
            *vertex = vertex.with_fine_uv([u0 + (u1 - u0) * along(0), v0 + (v1 - v0) * along(1)]);
        }
    }
}

/// A picture's silhouette hung by its tail (`relief::Hung`): stood on end
/// in the plane of the ridge, turned so its long axis hangs plumb, and
/// mirrored through that plane into a solid -- a pebble's relief has no
/// underside, since it lies on one.
///
/// `hang` is where the tail is, in the model's sixteenths; `scale` is
/// sixteenths a texel.
///
/// **The face index is read off the winding of what is emitted**, not
/// turned by a table: a silhouette hung plumb is turned by whatever angle
/// its picture was drawn at, which no quarter-turn table holds, and the
/// shader makes the light's direction out of that index. Snapped to the
/// nearest of six, which is all a light word says (`item_model::nearest_face`).
#[allow(clippy::too_many_arguments)]
fn push_hung_silhouette(
    at: [f32; 3],
    hung: &crate::engine::relief::Hung,
    layer: u32,
    hang: [f32; 3],
    scale: f32,
    quarters: u32,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let [dx, dy] = hung.down;
    // In the picture x runs right and y down; `down` goes to -Y, and the
    // axis across it to X by (dy, -dx), which is the choice that keeps the
    // picture's winding: the map from (x, height, y) to (X, Y, Z) has
    // determinant dx^2 + dy^2 = 1 for the front half and -1 for the mirror.
    let place = |[x, h, y]: [f32; 3], side: f32| -> [f32; 3] {
        let (px, py) = (x - hung.tail[0], y - hung.tail[1]);
        let model = [
            hang[0] + scale * (px * dy - py * dx),
            hang[1] - scale * (px * dx + py * dy),
            hang[2] + side * scale * h,
        ];
        let (cx, cz) = (model[0] / 16.0 - 0.5, model[2] / 16.0 - 0.5);
        let (rx, rz) = match quarters % 4 {
            1 => (cz, -cx),
            2 => (-cx, -cz),
            3 => (-cz, cx),
            _ => (cx, cz),
        };
        [at[0] + rx + 0.5, at[1] + model[1] / 16.0, at[2] + rz + 0.5]
    };
    for side in [1.0f32, -1.0] {
        for facet in &hung.relief.drawn {
            let mut corners = [0, 1, 2, 3].map(|k| (place(facet.corners[k], side), facet.uv[k]));
            if side < 0.0 {
                // A mirror turns every quad inside out; run it the other way.
                corners.reverse();
            }
            let p = |k: usize| glam::Vec3::from_array(corners[k].0);
            let face = crate::engine::item_model::nearest_face((p(1) - p(0)).cross(p(2) - p(1)));
            let packed = pack_light(sky, block_light, 3, face);
            let base = vertices.len() as u32;
            for (corner, uv) in corners {
                vertices.push(Vertex::new(corner, [0.0, 0.0], layer, packed).with_fine_uv(uv));
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

/// What hangs under one column of a whole rack's ridge, in the rack model's
/// sixteenths before `quarters` turns it: the goods of row `goods` of
/// `rack::HANGING`, along `x0 .. x0 + 10`, under a ridge whose underside is
/// at `ridge`.
///
/// **"в сушилке пусть будет не плоская хрень, а рыба или мясо".** This was
/// one slab twelve sixteenths by eight wearing the good's carried picture:
/// a board with a fish painted on it, and the picture's transparent margin
/// cut out of the board. Each good is now hung the way it is hung to dry,
/// and looks like itself from any side:
///
/// * **a hide or leather** is thrown over the ridge, one side longer than
///   the other -- still the stretched hide the lone frame shows;
/// * **meat** is strips draped over the ridge, four to a column, of four
///   lengths, since a haunch is cut into strips to be dried;
/// * **fish and fronds** hang by the tail from a cord looped round the
///   ridge, three to a column, each its own silhouette (`relief::Hung`);
/// * **peat** is two sods hung on cords.
///
/// Raw on one column and cured on the other still reads from across the
/// camp (`rack::columns_showing`): strips of red beside strips of brown.
#[allow(clippy::too_many_arguments)]
fn hang_goods(
    at: [f32; 3],
    goods: u8,
    x0: f32,
    ridge: f32,
    quarters: u32,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types as t;
    let Some(Some(item)) = primitive_shared::rack::HANGING.get(goods as usize).copied() else {
        return;
    };
    // The ridge is a pole a sixteenth and a half thick across z 7.25..8.75
    // (`misc/drying_rack_2x2.bbmodel`); what is thrown over it lies a
    // hair clear of its faces, and what is tied to it loops round them.
    const RIDGE_THICK: f32 = 1.5;
    let top = ridge + RIDGE_THICK;
    // Thrown over the ridge: a sheet down each side and a fold over the top,
    // the fold bigger than the sheets every way by more than the depth buffer
    // needs to keep two faces apart (`model_overlap::CLEARANCE`): a hair less
    // and the fold and the sheet under it boil where they meet.
    let drape = |from: f32, to: f32, front: f32, back: f32, thick: f32, dress: Dress, v: &mut Vec<Vertex>, i: &mut Vec<u32>| {
        let (near, far) = (7.15 - thick, 8.85 + thick);
        push_dressed(at, [from, front, near], [to, top + 0.3, near + thick], quarters, dress, light, v, i);
        push_dressed(at, [from, back, far - thick], [to, top + 0.3, far], quarters, dress, light, v, i);
        push_dressed(at, [from - 0.2, top, near - 0.2], [to + 0.2, top + thick, far + 0.2], quarters, dress, light, v, i);
    };
    // A cord from the ridge down to `low` at `z`, looped round the ridge at
    // `x`. It runs down the middle of what it holds, so its faces are inside
    // the thing rather than a hair off the thing's own.
    let cord = Dress::of(t::BLOCK_CORD, None, textures);
    let tie = |x: f32, low: f32, z: f32, v: &mut Vec<Vertex>, i: &mut Vec<u32>| {
        push_dressed(at, [x - 0.2, low, z - 0.2], [x + 0.2, ridge, z + 0.2], quarters, cord, light, v, i);
        push_dressed(at, [x - 0.4, ridge - 0.25, 7.0], [x + 0.4, top + 0.25, 9.0], quarters, cord, light, v, i);
    };

    match t::block_kind(item) {
        // The stretched hide the lone frame wears, drawn for its slab and so
        // worn uncropped (`Material::StretchedHide`).
        t::BLOCK_HIDE => drape(x0 + 0.5, x0 + 9.5, 17.0, 19.5, 0.6, Dress::Material(textures.layer_for_face(t::BLOCK_HIDE, 0), false), vertices, indices),
        t::BLOCK_LEATHER => drape(x0 + 0.5, x0 + 9.5, 17.5, 19.0, 0.6, Dress::of(item, None, textures), vertices, indices),
        t::BLOCK_RAW_MEAT
        | t::BLOCK_DRIED_MEAT
        | t::BLOCK_SALTED_MEAT
        | t::BLOCK_DRIED_SALTED_MEAT => {
            // Raw meat has a material with no holes -- the flesh a carcass
            // is cut open to -- and the cured kinds only their icons.
            let material = (t::block_kind(item) == t::BLOCK_RAW_MEAT).then_some((t::BLOCK_RAW_MEAT, true));
            let dress = Dress::of(item, material, textures);
            // Four lengths, so the strips read as cut by hand and not as a
            // comb; the back halves a little shorter, as a strip thrown
            // over a pole falls.
            const STRIPS: [(f32, f32); 4] = [(0.8, 18.6), (3.2, 20.2), (5.6, 17.9), (8.0, 19.4)];
            for (dx, low) in STRIPS {
                drape(x0 + dx, x0 + dx + 1.6, low, low + 1.6, 0.5, dress, vertices, indices);
            }
        }
        t::BLOCK_PEAT | t::BLOCK_DRIED_PEAT => {
            let material = (t::block_kind(item) == t::BLOCK_PEAT).then_some((t::BLOCK_PEAT, true));
            let dress = Dress::of(item, material, textures);
            // Two sods, each on its own cord, at two heights.
            for (dx, cord_length) in [(2.6, 1.6), (7.4, 2.6)] {
                let x = x0 + dx;
                let low = ridge - cord_length;
                tie(x, low - 0.3, 8.0, vertices, indices);
                push_dressed(at, [x - 1.8, low - 3.2, 6.8], [x + 1.8, low, 9.2], quarters, dress, light, vertices, indices);
            }
        }
        // Fish, fronds, and whatever a later row adds: the thing's own
        // silhouette, hung by its tail. Three to a column, the middle one
        // lower and hung a little behind the other two: a fish is up to
        // three and a half sixteenths across once its slant is taken out,
        // wider than the three and a third between cords, and two fish
        // overlapping in one plane are two faces the depth buffer cannot
        // tell apart (`model_overlap`).
        _ => {
            let Some(hung) = textures.hung(item) else {
                return;
            };
            let layer = hung_picture(item, textures);
            // Half a sixteenth a texel: a fish drawn corner to corner of its
            // picture hangs a little over half a block long.
            const SCALE: f32 = 0.5;
            for (dx, cord_length, z) in [(1.7, 0.9, 8.0), (5.0, 2.0, 8.4), (8.3, 1.3, 8.0)] {
                let x = x0 + dx;
                let tail = ridge - cord_length;
                tie(x, tail - 0.4, z, vertices, indices);
                push_hung_silhouette(at, hung, layer, [x, tail, z], SCALE, quarters, light, vertices, indices);
            }
        }
    }
}

/// How far a pile's log is bevelled at each of its four long edges, in
/// sixteenths: what makes a log of boxes read round.
///
/// **One sixteenth**, a texel of bark off each corner of a log a little over
/// five across. Square logs are what the pit kiln draws, and from the side a
/// stack of square logs is a wall of bark with seams in it -- the cube
/// again. A bevel puts a dark notch
/// where two logs meet, and the notches are what the eye counts.
const PILE_LOG_BEVEL: f32 = 1.0;

/// An unlit pile of logs, in its cell's space: the logs in it
/// (`pit::PILE_LOGS_AT`), each lying the length of the cell along z with its
/// cut ends facing north and south, in the wood the pile is made of.
///
/// **What it replaced.** A pile was the cube its row describes, wearing the
/// log's end picture on its four sides and the bark on its top, the same
/// whatever the count -- "у дровницы непонятная текстура": one picture of
/// rings sixteen texels across does not read as eight logs, it reads as a
/// strange block. Now one log is one log lying on the grass, and eight are a
/// woodpile three, two and three high.
///
/// **Each end wears the middle of the end picture**, so every log shows its
/// own heart and rings, cut to its size (`FINE_UV_BIT`) and never squeezed:
/// the log is written centred on the cell and moved into place by `at`,
/// which moves the corners and not the place in the picture they read.
///
/// **Written standing and laid down by a hinge** (`Swing`, a quarter about
/// x) rather than written lying, and the reason is the bark's grain.
/// `face_uv` reads a side's `v` down the cell's height, so a box written
/// lying wears the grain of a standing trunk -- across the log, which from
/// two paces reads as corrugated iron. Written standing, the grain runs up
/// the log; laid down, it runs along it, and the four faces are lit as the
/// directions they come to point (`Swing::face_after`).
///
/// Each log is a cross of three boxes -- the core the full height of the
/// log and a slab on either flank a bevel shorter -- so its four long edges
/// are notched a sixteenth ([`PILE_LOG_BEVEL`]). The slabs' inner faces lie
/// inside the core and are left out.
pub(crate) fn log_pile_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::pit::{pile_log_boxes, PILE_LOG};
    use primitive_shared::types::furniture_wood;
    let log = primitive_shared::wood::WOODS[furniture_wood(block)].log;
    let end = textures.layer_for_face(log, crate::engine::texture::FACE_TOP);
    let bark = textures.layer_for_face(log, 2);
    let layers = [end, end, bark, bark, bark, bark];
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    // A quarter turn about x through the middle of the cell: written (x, y,
    // z) lies at (x, 1 - z, y), so the written height becomes the length
    // along z and the written z the height -- centred, a log's middle.
    let lay = Swing { axis: 0, pivot: [0.0, 0.5, 0.5], angle: std::f32::consts::FRAC_PI_2 };
    let (lo, hi) = (8.0 - PILE_LOG * 8.0, 8.0 + PILE_LOG * 8.0);
    let b = PILE_LOG_BEVEL;
    // (from, to, faces left out): the core, the west slab, the east slab.
    let parts: [([f32; 3], [f32; 3], u8); 3] = [
        ([lo + b, 0.0, lo], [hi - b, 16.0, hi], 0),
        ([lo, 0.0, lo + b], [lo + b, 16.0, hi - b], 1 << 2),
        ([hi - b, 0.0, lo + b], [hi, 16.0, hi - b], 1 << 3),
    ];
    for (from, _) in pile_log_boxes(block) {
        let place = [
            at[0] + from[0] + PILE_LOG * 0.5 - 0.5,
            at[1] + from[1] + PILE_LOG * 0.5 - 0.5,
            at[2],
        ];
        for (part_from, part_to, open) in parts {
            push_box_moved(
                place, part_from, part_to, 0, Some(lay), layers, true, sky, block_light, 0, open, vertices, indices,
            );
        }
    }
}

// ---- the standing torch ----

/// How far up its top cell a standing torch's flame stands: its foot on
/// the wad (`FLAME_FOOT` is where a hearth's flame starts in its own cell).
const TORCH_FLAME_RISE: f32 = 0.28;

/// A standing torch's cell, in its cell's space: the pole, and on the top
/// cell the wad of resin bound round its end -- black when it has burnt
/// out.
///
/// **Two sixteenths across**, a haft's thickness (`BLOCK_WORKED_STICK`): the
/// pole is two hafts end to end, and a post as thick as a fence rail would
/// be a lamp-post, not a torch. The wad is five across and four tall, which
/// from standing height is the knot a torch head is. Pictures the world
/// already wears -- bark for the pole, the row's own top for the wad -- so
/// the torch costs the atlas its icon and nothing else.
pub(crate) fn standing_torch_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types::{block_kind, BLOCK_LOG, BLOCK_STANDING_TORCH};
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let pole = textures.layer_for_face(BLOCK_LOG, 2);
    if block_kind(block) == BLOCK_STANDING_TORCH {
        push_box(at, [7.0, 0.0, 7.0], [9.0, 16.0, 9.0], 0, pole, true, sky, block_light, vertices, indices);
        return;
    }
    push_box(at, [7.0, 0.0, 7.0], [9.0, 4.0, 9.0], 0, pole, true, sky, block_light, vertices, indices);
    // The wad, off the top cell's own row: resin alight or a black knot.
    let wad = textures.layer_for_face(block, 0);
    push_box(at, [5.5, 4.0, 5.5], [10.5, 8.0, 10.5], 0, wad, true, sky, block_light, vertices, indices);
}

// ---- the door ----

/// Half a door, in its cell's space: one slab of boards three sixteenths
/// thick, across the back of the cell shut and a quarter turn round open
/// (`types::door_quarters`), wearing its own half's picture.
///
/// **One box, and the picture is the door.** A door read as a door from
/// across a clearing is a board face with a frame round it and a handle,
/// and that is a picture, not a model -- a handle modelled a sixteenth
/// proud is four more boxes a door, in a wall of them, for a thing the eye
/// finds just as well painted. The thin faces wear the edge of the same
/// picture, which is the frame's stile, cropped rather than squeezed
/// (`push_box`).
///
/// **Written at its north and turned by `turned_from_north`**, the rack's
/// arrangement, so the slab turns the way `geometry::block_box` turns the
/// box a player walks into; `a_door_is_drawn_exactly_where_it_is_walked_into`
/// holds the two together for every facing, open and shut.
///
/// Which face of the door the handle is on is whichever way the picture
/// lands on each side of the slab, and the two sides are mirror images of
/// each other -- which is what a real door's two faces are: the handle is at
/// the free edge from both sides.
///
/// **`lights` is three: the edges, the front face and the back face**, and
/// the broad faces are lit from the rooms they look into. Light is kept per
/// cell and a shut door's own cell is dark (`types::light_opacity`), so the
/// door used to be drawn whole from the brighter of its neighbours: the
/// inside of a hut's door at noon was lit like the sunny side, and at night
/// the outside glowed with the hearth indoors. The front face looks through
/// the door's own cell to the one beyond; the back face is at the cell's
/// edge and looks straight into its neighbour (`door_face_offset`).
pub(crate) fn door_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    lights: [u8; 3],
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types::{block_kind, DOOR_THICKNESS};
    // The kind, not the id: the picture is one for every facing, and the
    // facing is the box's to turn rather than the texture table's.
    let piece = if block_kind(block) == primitive_shared::types::BLOCK_DOOR_TOP {
        crate::engine::texture::WoodPiece::DoorTop
    } else {
        crate::engine::texture::WoodPiece::Door
    };
    let boards = crate::engine::texture::extra_in_wood(primitive_shared::types::furniture_wood(block), piece)
        .map_or_else(|| textures.layer_for_face(block_kind(block), 0), |index| textures.extra(index));
    // Face 4 is the box's +z (back), face 5 its -z (front), as written.
    const BACK: u8 = 1 << 4;
    const FRONT: u8 = 1 << 5;
    for (light, open) in [(lights[0], BACK | FRONT), (lights[1], !FRONT), (lights[2], !BACK)] {
        push_box_open(
            at,
            [0.0, 0.0, 16.0 - DOOR_THICKNESS * 16.0],
            [16.0, 16.0, 16.0],
            door_turn(block),
            boards,
            true,
            light & 0x0F,
            (light >> 4) & 0x0F,
            open,
            vertices,
            indices,
        );
    }
}

/// How far a door's box is turned from north, in the quarters `push_box` takes.
fn door_turn(block: BlockId) -> u32 {
    use primitive_shared::types::{door_quarters, Facing};
    turned_from_north(match door_quarters(block) {
        1 => Facing::East,
        2 => Facing::South,
        3 => Facing::West,
        _ => Facing::North,
    })
}

/// The neighbour a door's front (`toward_back` false) or back face looks
/// into: +z or -z as the box is written, turned the way `push_box_faces`
/// turns a corner.
fn door_face_offset(block: BlockId, toward_back: bool) -> (i32, i32) {
    let z = if toward_back { 1 } else { -1 };
    match door_turn(block) % 4 {
        1 => (z, 0),
        2 => (0, -z),
        3 => (-z, 0),
        _ => (0, z),
    }
}

/// A stake in its cell: a bundle of sharpened poles stood in the ground, or
/// three driven into the wall behind it -- `misc/stake.bbmodel` and
/// `misc/stake_wall.bbmodel`, in the wood's own bark with pale cut points.
///
/// **A model, and a bundle of spikes rather than one pole**, for "у кола нету
/// модели, и я хотел шипы, а не одну палку". It was two boxes of the row's
/// own picture -- a leaning pole drawn for the *pack*, mostly transparent --
/// laid on a two-sixteenths stick, so what stood in the world was a stick
/// with a few texels of bark on it and holes where the picture had none.
/// What a stake is for is keeping something off, and a thing that keeps
/// something off points at it from several sides: four poles leaning out
/// round one upright, each sharpened.
///
/// **Two poses and one block** (`types::STAKE_UPRIGHT`). A driven stake is
/// written pointing out of the -z wall and turned by its facing's own count
/// of quarters -- the turn `types::wall_behind` makes of that wall -- so the
/// cell that holds it up (`types::support_at`) is the cell it is drawn
/// driven into. **Not `turned_from_north`**, which it was: that counts the
/// other way round, and every stake driven into an east or a west wall was
/// drawn sticking out of the far side of its cell into the air, butt first,
/// while the wall behind it held it up. North and south are the same either
/// way, which is how it went unseen.
///
/// Rejected: the cross of two quads its row's shape would give
/// (`Shape::Cross`). A cross is the same picture from every side, so a
/// driven stake would look like a stake standing in the air beside a wall,
/// and turning it would show nothing -- which is what
/// `a_block_turns_when_it_is_placed_exactly_when_turning_it_would_show`
/// says out loud. The row keeps the cross for what it decides: a thing
/// walked through rather than into, and aimed at across its cell -- which is
/// where the spikes reach (`a_stake_is_aimed_at_round_what_is_drawn_of_it`).
pub(crate) fn stake_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::logic::models::{prop, Prop};
    let (model, quarters) = if primitive_shared::types::stake_is_upright(block) {
        (Prop::Stake, 0)
    } else {
        (Prop::StakeWall, primitive_shared::types::block_facing(block).quarters())
    };
    // Wood 0: the plain log's bark and planed wood -- a stake is cut from
    // sticks, which carry no wood of their own.
    push_prop(at, prop(model), quarters, false, Hinged::Whole, |_| 0, 0, textures, light, vertices, indices);
}

// ---- steps ----

/// A step, in its cell's space: the tread, the lower half of the cell, and
/// the riser over the back of it, cut or joined at a corner of two flights
/// by the steps round it (`geometry::StepShape`) -- the boxes
/// `geometry::step_boxes` collides, taken in their written pose from the
/// same list (`geometry::step_pose_boxes`) and turned by
/// `turned_from_north`, the door's arrangement. `near` is the block at an
/// offset from the step; a step carried in a hand has no neighbours and is
/// the straight one it is placed as.
///
/// **Turned rather than written where it stands**, so a tread's courses of
/// tiles or planks run along the step whichever way it faces: a roof whose
/// east slope showed its tiles across the slope and whose north slope
/// showed them along it would read as two materials.
///
/// **Cropped**, so a tread shows the eight rows of planks or tiles under it
/// at the density of the wall beside it rather than the whole picture
/// squeezed into half a cell: a roof of steps reads as one surface of
/// courses, which is what a roof is.
///
/// Rejected: drawing a step as a cube with its back corner cut away by the
/// cube path's layer table. That table is heights up from the floor and
/// nothing else (`blocks::BlockDef::thickness`), and a riser is a height
/// over half a cell only.
pub(crate) fn step_block(
    at: [f32; 3],
    block: BlockId,
    near: impl Fn(i32, i32, i32) -> BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::geometry::{step_pose_boxes, step_shape};
    use primitive_shared::types::{block_facing, block_kind};
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let quarters = turned_from_north(block_facing(block));
    // The kind: the picture is one for every facing, as the door's is.
    let layer = textures.layer_for_face(block_kind(block), 0);
    for (min, max) in step_pose_boxes(step_shape(block, near)).iter() {
        let sixteenths = |p: [f32; 3]| p.map(|v| v * 16.0);
        push_box(at, sixteenths(min), sixteenths(max), quarters, layer, true, sky, block_light, vertices, indices);
    }
}

// ---- dripstone ----

/// A stalagmite or a stalactite, in its cell's space: the stepped spike
/// `dripstone::tiers` gives, each tier a box of the row's own picture.
///
/// **Cropped**, so every face shows the texels of rock under it at the
/// density of the wall beside it: a tip two sixteenths wide wears two
/// columns of the picture, not all sixteen squeezed (`push_box`).
///
/// **Every face of every tier, and the depth test buries the rest.** A
/// tier's top is covered by the tier on it only where the two overlap, and
/// the ring outside is a ledge a player sees from above -- so no face is
/// known to be buried whole, and `push_box_open` has nothing to leave out.
pub(crate) fn dripstone_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let rock = textures.layer_for_face(block, 2);
    for (from, to) in primitive_shared::dripstone::tiers(block) {
        push_box(at, from, to, 0, rock, true, sky, block_light, vertices, indices);
    }
}

// ---- the pit kiln ----
//
// **A hole with things laid in it, in the player's order**: «кладутся в
// низ предметы для обжога и кладётся 8 сена а сверху 8 брёвен». So the
// pots stand on the floor, the fibre goes in a quarter of the floor at a time
// and then a second layer over the first, and the logs go on in two
// courses, the second across the first -- which is how a stack of logs
// stays a stack and how a player counts to eight at a glance.
//
// Every material is a picture the world already wears, read off the
// stage's own row in `blocks.toml` (see the note there): no stage costs a
// layer of the atlas.
//
// **Each piece is drawn as itself**: a pot as a pot, a jug with its neck
// and handle, a mould as a shallow tray and a brick as a brick. The id says
// only how many and whether fired (`pit::Stage`); which pieces comes from
// the server beside it (`ServerMessage::PitPottery`), and a pit nobody has
// been told about draws that many pots, which is all this ever drew. "You
// can't see what the player places -- a brick or a mould looks like jugs"
// was the report.

/// How far the flame over a burning kiln is raised from the floor of its
/// pit: onto the upper course of logs, so it burns over the ground rather
/// than inside the wood.
const PIT_FLAME_RISE: f32 = 0.7;

/// Where each pot stands, by the order it went in: the four quarters of the
/// floor, the first two diagonal so a pit of two reads as a pit of two.
const PIT_POTS: [[f32; 2]; 4] = [[4.0, 4.0], [12.0, 12.0], [12.0, 4.0], [4.0, 12.0]];

/// Where each armful of fibre lies: the corner of its quarter of the floor.
const PIT_FIBRE: [[f32; 2]; 4] = [[0.0, 0.0], [8.0, 8.0], [8.0, 0.0], [0.0, 8.0]];

/// A pit kiln at whatever stage its id says, in its cell's space.
pub(crate) fn pit_kiln_block(
    at: [f32; 3],
    block: BlockId,
    pieces: Option<&[BlockId]>,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::pit::{Stage, LOGS_NEEDED};
    use primitive_shared::types::{BLOCK_PIT_KILN, BLOCK_PIT_KILN_FIBRE, BLOCK_PIT_KILN_LIT, BLOCK_PIT_KILN_LOGS};
    let Some(stage) = Stage::of(block) else {
        return;
    };
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let mut boxed = |from: [f32; 3], to: [f32; 3], layer: u32| {
        push_box(at, from, to, 0, layer, true, sky, block_light, vertices, indices);
    };
    // **The straw a little higher** ("сделай солому в pit kiln выше чуть"): an
    // armful is five sixteenths, and it was four, which from the rim read as
    // a thin mat over the pots rather than a heap; the full bed under the
    // logs is ten. The logs go three thick instead of four, so two courses
    // on the higher bed still end at the top of the cell.
    const ARMFUL: f32 = 5.0;
    const FIBRE_BED: f32 = 10.0;
    // A log is a course of the stack: the lower four along x, the upper
    // four across them. **Half a texel short of its neighbour on each
    // side**, so the fibre shows pale between them. The first picture had
    // them a quarter apart, and eight logs seen from the rim were a square
    // of dug earth -- bark from above is brown, and so is soil.
    let log = |index: u8| -> ([f32; 3], [f32; 3]) {
        let (course, place) = (f32::from(index / 4), f32::from(index % 4) * 4.0);
        let (y0, y1) = (FIBRE_BED + course * 3.0, FIBRE_BED + 3.0 + course * 3.0);
        if index / 4 == 0 {
            ([0.0, y0, place + 0.5], [16.0, y1, place + 3.5])
        } else {
            ([place + 0.5, y0, 0.0], [place + 3.5, y1, 16.0])
        }
    };
    match stage {
        Stage::Pottery { pieces: count, fired } => {
            for (index, [cx, cz]) in PIT_POTS.into_iter().take(count as usize).enumerate() {
                let piece = pieces.and_then(|known| known.get(index).copied());
                for (from, to, fired_clay) in pottery_boxes(piece, fired) {
                    // Raw clay off the row's side, fired clay off its top.
                    let clay = textures.layer_for_face(BLOCK_PIT_KILN, if fired_clay { 0 } else { 2 });
                    boxed([cx + from[0], from[1], cz + from[2]], [cx + to[0], to[1], cz + to[2]], clay);
                }
            }
        }
        Stage::Fibre(armfuls) => {
            // **The pottery is still there under the fibre**, and only
            // what an armful lies over is hidden. The report: "placing
            // straw deletes all elements -- delete only what the straw
            // covers". The first armful fills the quarter the first piece
            // stands in to five sixteenths (`ARMFUL`), so a pot shows its neck over it
            // and a brick is gone under it; the second layer covers the
            // tallest. Drawn only when the server has said what is there:
            // this id does not carry how many pieces.
            for (index, piece) in pieces.unwrap_or(&[]).iter().take(PIT_POTS.len()).enumerate() {
                let [cx, cz] = PIT_POTS[index];
                for (from, to, fired_clay) in pottery_boxes(Some(*piece), false) {
                    let clay = textures.layer_for_face(BLOCK_PIT_KILN, if fired_clay { 0 } else { 2 });
                    boxed([cx + from[0], from[1], cz + from[2]], [cx + to[0], to[1], cz + to[2]], clay);
                }
            }
            let fibre = textures.layer_for_face(BLOCK_PIT_KILN_FIBRE, 0);
            for index in 0..armfuls {
                let [qx, qz] = PIT_FIBRE[usize::from(index % 4)];
                let y0 = f32::from(index / 4) * ARMFUL;
                boxed([qx, y0, qz], [qx + 8.0, y0 + ARMFUL, qz + 8.0], fibre);
            }
        }
        Stage::Logs(logs) => {
            boxed([0.0; 3], [16.0, FIBRE_BED, 16.0], textures.layer_for_face(BLOCK_PIT_KILN_FIBRE, 0));
            let bark = textures.layer_for_face(BLOCK_PIT_KILN_LOGS, 2);
            for index in 0..logs {
                let (from, to) = log(index);
                boxed(from, to, bark);
            }
        }
        Stage::Burning => {
            // The fibre gone to embers, the lower course still bark and the
            // upper course glowing: what a kiln an hour from done looks like
            // from the rim.
            let embers = textures.layer_for_face(BLOCK_PIT_KILN_LIT, 0);
            let bark = textures.layer_for_face(BLOCK_PIT_KILN_LIT, 2);
            boxed([0.0; 3], [16.0, FIBRE_BED, 16.0], embers);
            for index in 0..LOGS_NEEDED {
                let (from, to) = log(index);
                boxed(from, to, if index < 4 { bark } else { embers });
            }
        }
    }
}

/// The boxes one piece of pottery in a pit is drawn as, centred on its
/// place on the floor: from, to, and whether the clay is fired. `None` is a
/// piece nobody has said the kind of, drawn as the pot every pit drew
/// before it could be told; `fired` is the stage's word for it then.
///
/// In sixteenths. **Each the shape a player knows the piece by** from its
/// icon: the pot squat with a narrower mouth; the jug taller, with a neck
/// and a handle standing off one side; the mould a shallow open tray, its
/// hollow showing from the rim of the pit; the brick a bar longer than it
/// is wide, lying flat.
fn pottery_boxes(piece: Option<BlockId>, fired: bool) -> Vec<([f32; 3], [f32; 3], bool)> {
    use primitive_shared::types::{
        block_kind, BLOCK_BRICK, BLOCK_BRICK_RAW, BLOCK_JUG, BLOCK_JUG_RAW, BLOCK_MOULD, BLOCK_MOULD_RAW,
    };
    let fired = match piece {
        Some(piece) => primitive_shared::pit::fires_into(piece).is_none(),
        None => fired,
    };
    match piece.map(block_kind) {
        Some(BLOCK_JUG | BLOCK_JUG_RAW) => vec![
            ([-2.5, 0.0, -2.5], [2.5, 6.0, 2.5], fired),
            ([-1.5, 6.0, -1.5], [1.5, 8.0, 1.5], fired),
            ([2.5, 3.0, -0.5], [3.5, 6.0, 0.5], fired),
        ],
        Some(BLOCK_MOULD | BLOCK_MOULD_RAW) => vec![
            ([-3.5, 0.0, -3.5], [3.5, 1.0, 3.5], fired),
            ([-3.5, 1.0, -3.5], [3.5, 2.5, -2.5], fired),
            ([-3.5, 1.0, 2.5], [3.5, 2.5, 3.5], fired),
            ([-3.5, 1.0, -2.5], [-2.5, 2.5, 2.5], fired),
            ([2.5, 1.0, -2.5], [3.5, 2.5, 2.5], fired),
        ],
        Some(BLOCK_BRICK | BLOCK_BRICK_RAW) => vec![([-3.5, 0.0, -1.75], [3.5, 2.5, 1.75], fired)],
        // **Six sixteenths across and seven tall.** The first picture drew
        // pots four across, and from the rim of a pit a metre deep a pot
        // that size was a pebble on the floor of a hole.
        _ => vec![([-3.0, 0.0, -3.0], [3.0, 5.0, 3.0], fired), ([-2.0, 5.0, -2.0], [2.0, 7.0, 2.0], fired)],
    }
}

/// What a box of furniture is made of, which is which picture it wears.
///
/// **Materials, never a picture drawn for the piece.** Every one of these
/// is a surface that tiles -- boards, bark, dry grass, wool, hide -- so a
/// face of any size wears the part of it under that face at one texel to a
/// sixteenth (see `push_box_open`) and nothing is cut through a feature.
/// And all but one of them are pictures the world already wears: a stool
/// made of boards should look like the boards it was made of, which is the
/// argument `types` makes for furniture costing no texture. The one is
/// straw, because nothing in the atlas was -- see `Material::Straw`.
///
/// **That argument lost, to a player looking at it.** "A stool made of
/// boards should look like the boards it was made of" made every stool,
/// chair, table and bed out of the wall's own planks -- black seams every
/// four texels and nail heads in a seat nobody nailed -- and legs out of a
/// log's bark, and the player's word for all of it was "just wood". So
/// furniture has four pictures of its own now, still surfaces that tile and
/// are still cut under each face: `Timber`, `Post`, `Iron` and `Fur`. What
/// the old argument got right survives: a stool is still the wood of the
/// camp, only planed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Material {
    /// Sawn boards, `planks.png`.
    Boards,
    /// A pole with the bark on: the oak log's side, as the rack's frame.
    Pole,
    /// Loose straw, `plants/straw.png`, which is the straw pallet's own row:
    /// what a pallet is and what a mattress is stuffed with.
    ///
    /// **Twice wrong before it was straw.** The pallet's row named
    /// `fiber.png`, the *icon* of a hank of fibre, mostly empty pixels --
    /// drawn as a solid cube it was a white slab with fibres painted on it.
    /// Then this was the savanna turf's top, which is ground: a khaki speckle
    /// with no stalk in it, and a player looking at a pallet of it asked for
    /// one that "looks like straw".
    Straw,
    /// Wool, for the pillow.
    Wool,
    /// The stretched hide the bed's top wore, for the blanket.
    Hide,
    /// The drying rack's skin: [`crate::engine::texture::EXTRA_STRETCHED_HIDE`],
    /// a picture *drawn for that slab* -- margin, lace holes and all -- and
    /// so the one material here worn whole rather than cut (see `cropped`).
    StretchedHide,
    /// The same skin cured: [`crate::engine::texture::EXTRA_STRETCHED_LEATHER`],
    /// drawn for the same slab and worn whole for the same reason. What a
    /// hide frame's skin wears once it has dried (`types::HIDE_CURED`).
    StretchedLeather,
    /// A piece of that cured skin cut the way `Hide` is cut from the raw one
    /// (`cropped`), for a piece of skin too small to wear the whole picture:
    /// its laced margin squeezed into a sixteenth reads as noise. So that a
    /// model cutting a raw skin into pieces (`Hide`) cures as one.
    Leather,
    /// Planed boards, grain running along the piece: a seat, a table top, a
    /// chest's sides. `furniture/timber.png`.
    Timber,
    /// A squared post, grain running up it: a leg, an upright, a rung.
    /// `furniture/post.png`. Its own picture rather than `Timber` turned,
    /// because a box's picture is never turned -- a leg wearing `Timber`
    /// would have its grain running across it like a stack of coins.
    Post,
    /// Wrought iron: a chest's bands, a vice screw, a knife on a bench.
    /// `furniture/iron.png`.
    Iron,
    /// A skin with the hair on, for a bed's blanket. `furniture/fur.png`.
    /// The bed costs two leathers and was covered in the drying rack's
    /// laced skin, lace holes and all.
    Fur,
    /// Dressed stone, the world's own: the mason's slab and the wheel's
    /// flywheel are the rock they came out of.
    Stone,
    /// Cast bronze: the anvil, and the one material here that is a block
    /// nobody would tile a wall with. `furniture/bronze.png`, which is the
    /// anvil's own row in `blocks.toml`.
    Bronze,
    /// Wet clay, the world's own: what is on the potter's wheel.
    Clay,
    /// A chest's boards: the chest row's own side picture
    /// (`terrain/chest_side.png`), boards four texels deep with a dark seam
    /// between each, so a body ten sixteenths tall shows three seams and the
    /// lid its own.
    ///
    /// **Its own picture and not `Timber`**, because "сундук выглядит как
    /// табурет": `Timber`'s boards are eight texels deep, so a chest's front
    /// was one seam across a plain plank, the stool's seat stood on end. And
    /// **the row's picture rather than a new one**, because the row already
    /// has a layer nobody saw since the chest became a model -- the far,
    /// simplified chest wears it -- and a layer is the one thing the atlas
    /// runs out of.
    Chest,
    /// A heap of fallen leaves: the leaf litter's own picture
    /// (`plants/leaf_litter.png`), which is what a lean-to is thatched and
    /// bedded with. The litter rather than the green handful, because what a
    /// traveller rakes up for a roof is last year's leaves off the floor of a
    /// wood, and the litter's row is already in the atlas.
    Leaves,
}

impl Material {
    /// Every material, in the order a model file's reader lists them when
    /// a texture names none of them.
    pub(crate) const ALL: [Material; 17] = [
        Material::Boards,
        Material::Pole,
        Material::Straw,
        Material::Wool,
        Material::Hide,
        Material::StretchedHide,
        Material::StretchedLeather,
        Material::Leather,
        Material::Timber,
        Material::Post,
        Material::Iron,
        Material::Fur,
        Material::Stone,
        Material::Clay,
        Material::Chest,
        Material::Bronze,
        Material::Leaves,
    ];

    /// The picture in a wood (`types::furniture_wood`): boards, poles and
    /// the planed pieces come in the wood the furniture was made of, and
    /// everything else -- iron, fur, straw, stone -- is what it is.
    pub(crate) fn layer_in(self, textures: &crate::engine::texture::FaceLayers, wood: usize) -> u32 {
        use crate::engine::texture::{extra_in_wood, WoodPiece};
        let Some(timber) = primitive_shared::wood::WOODS.get(wood).filter(|_| wood > 0) else {
            return self.layer(textures);
        };
        let piece = match self {
            Material::Boards => return textures.layer_for_face(timber.planks, 0),
            Material::Pole => return textures.layer_for_face(timber.log, 2),
            Material::Timber => WoodPiece::Timber,
            Material::Post => WoodPiece::Post,
            Material::Chest => WoodPiece::ChestSide,
            _ => return self.layer(textures),
        };
        extra_in_wood(wood, piece).map_or_else(|| self.layer(textures), |index| textures.extra(index))
    }

    pub(crate) fn layer(self, textures: &crate::engine::texture::FaceLayers) -> u32 {
        use crate::engine::texture::EXTRA_FURNITURE;
        use primitive_shared::types::{BLOCK_BED, BLOCK_CLAY, BLOCK_LOG, BLOCK_PLANKS, BLOCK_STONE, BLOCK_STRAW_BED, BLOCK_WOOL};
        match self {
            Material::Boards => textures.layer_for_face(BLOCK_PLANKS, 0),
            Material::Pole => textures.layer_for_face(BLOCK_LOG, 2),
            Material::Straw => textures.layer_for_face(BLOCK_STRAW_BED, 0),
            Material::Wool => textures.layer_for_face(BLOCK_WOOL, 0),
            Material::Hide => textures.layer_for_face(BLOCK_BED, 0),
            Material::StretchedHide => textures.stretched_hide(),
            Material::StretchedLeather | Material::Leather => textures.stretched_leather(),
            Material::Timber => textures.extra(EXTRA_FURNITURE),
            Material::Post => textures.extra(EXTRA_FURNITURE + 1),
            Material::Iron => textures.extra(EXTRA_FURNITURE + 2),
            Material::Fur => textures.extra(EXTRA_FURNITURE + 3),
            Material::Stone => textures.layer_for_face(BLOCK_STONE, 0),
            Material::Clay => textures.layer_for_face(BLOCK_CLAY, 0),
            // **The anvil's own block picture**, the way stone and clay take
            // theirs. An extra texture would have been a fifth index in a run
            // `extra_in_wood` counts past (`EXTRA_FURNITURE + 4`), and a
            // number in two places is a number that drifts; a block that *is*
            // the metal needs none of it.
            Material::Bronze => textures.layer_for_face(primitive_shared::types::BLOCK_ANVIL, 0),
            // Face 2, a side: the row's north is the front picture with the
            // hasp painted on, for the far chest that has no hasp box.
            Material::Chest => textures.layer_for_face(primitive_shared::types::BLOCK_CHEST, 2),
            Material::Leaves => textures.layer_for_face(primitive_shared::types::BLOCK_LEAF_LITTER, 0),
        }
    }

    /// Whether a face wears the piece of the picture under it (a material
    /// that tiles) or the whole picture (one drawn for the face). See
    /// `push_box`.
    pub(crate) fn cropped(self) -> bool {
        !matches!(self, Material::StretchedHide | Material::StretchedLeather)
    }

    /// What this is once the skin it is cut from has dried: the raw hide's
    /// two materials become the cured one's, and nothing else changes. Asked
    /// of a hide frame's `loaded` boxes when its skin is cured
    /// (`rack_block`), so a model file says "the skin" once and not twice.
    pub(crate) fn cured(self) -> Self {
        match self {
            Material::StretchedHide => Material::StretchedLeather,
            Material::Hide => Material::Leather,
            other => other,
        }
    }

    /// The texture name that means this material in a model file
    /// (`assets/models/README.md`).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Material::Boards => "boards",
            Material::Pole => "pole",
            Material::Straw => "straw",
            Material::Wool => "wool",
            Material::Hide => "hide",
            Material::StretchedHide => "stretched_hide",
            Material::StretchedLeather => "stretched_leather",
            Material::Leather => "leather",
            Material::Timber => "timber",
            Material::Post => "post",
            Material::Iron => "iron",
            Material::Fur => "fur",
            Material::Stone => "stone",
            Material::Clay => "clay",
            Material::Chest => "chest",
            Material::Bronze => "bronze",
            Material::Leaves => "leaves",
        }
    }
}

/// One box of a model read from `assets/models/furniture` or `misc`: from
/// and to in sixteenths of the cell as the model is written, and what it
/// is made of. See `logic::models`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PropBox {
    /// Read by a person and by the tests; nothing that draws looks at it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) name: &'static str,
    pub(crate) from: [f32; 3],
    pub(crate) to: [f32; 3],
    pub(crate) material: Material,
    /// Drawn only when the block carries its load -- the rack's skin (a
    /// group named `loaded` in the file).
    pub(crate) loaded: bool,
    /// Drawn only when it does not -- a hide frame's cords lying loose on
    /// the ground, which are pulled taut to the skin once there is one (a
    /// group named `bare`). See `misc/hide_frame.bbmodel` in
    /// `logic::model_notes`.
    pub(crate) bare: bool,
    /// **Lit as the inside of something shut** -- no sky, and a quarter of
    /// the fire its cell has ([`in_the_dark`]) -- because it is: the floor and the
    /// lining of a chest (a group named `inside` in the file). See
    /// `furniture/chest.bbmodel` in `logic::model_notes`.
    pub(crate) inside: bool,
    /// Turned about one axis through a point, as the file's `rotation` and
    /// `origin` say -- a stake's spikes leaning out of their bundle. `None`
    /// for a box square to its cell, which is every box of every model
    /// but those.
    pub(crate) tilt: Option<Tilt>,
}

/// **A box turned about one axis, as its file writes it**: the axis, the
/// point it turns about in sixteenths, and degrees -- the file's own numbers,
/// kept as they were typed so a model written back out (`prop_project`) is
/// the same file to the digit. Drawn as a [`Swing`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tilt {
    pub axis: usize,
    pub origin: [f32; 3],
    pub degrees: f32,
}

impl Tilt {
    pub(crate) fn swing(self) -> Swing {
        Swing { axis: self.axis, pivot: self.origin.map(|o| o / 16.0), angle: self.degrees.to_radians() }
    }
}

/// **The light inside a box that is open at the top**: no sky, and a
/// quarter of the fire.
///
/// "сундук внутри не полый, сделай там черноту": a chest drawn with its lid
/// up showed a floor of boards as bright as its lid, and a well-lit floor
/// one board under the rim reads as a solid block with a picture of boards
/// on it. Sky light is what the sun is let through by (`sun_sky` in
/// shader.wgsl): where shadows are cast, *any* sky at all takes the whole
/// beam, so a quarter of the sky -- which this was first -- drew the floor in
/// full noon sun, photographed as bright as the rim. None puts it at the
/// floor every shut space has, which is the black asked for; a quarter of
/// the fire keeps a torch held over an open chest showing what is in it.
///
/// Rejected: a black picture. It would be the one texture in the atlas that
/// is not a material, a layer spent on nothing, and black by a fire, which
/// a box is not; the lighting word already carries exactly the quantity
/// that is wrong.
pub(crate) fn in_the_dark(block_light: u8) -> (u8, u8) {
    (0, block_light / 4)
}

/// The light a model is lit by: channel by channel, the brightest of its
/// own cell and the cells round it that light can stand in.
///
/// **A model's own cell is lit through the model**, and a block that stops
/// any light takes its share off its own cell as the flood passes into it:
/// a drying rack (opacity two) holds three levels less than the air it
/// stands in, sky and fire alike. By day that is a frame a shade dimmer
/// than it should be. At night, where a fire is the only light and its
/// reach runs out a level a cell, it is the difference between a frame lit
/// like the stones either side of it and a frame at nought -- and a player
/// photographed exactly that: a rack drawn pitch black between two lit
/// blocks. The six cells round a model are the air its boxes face into,
/// which is what a cube's face is lit by as well; full ones are passed
/// over, or a model standing against a wall would be lit through it.
///
/// Rejected: lighting each box face from the cell it faces, as the cube
/// path does. A model's faces are mostly inside its cell -- the inner side
/// of a rack's upright faces its other upright -- so "the cell it faces"
/// is the model's own cell for most of them, which is the value that was
/// wrong.
#[inline]
fn model_light(cache: &Neighbourhood, cell: usize, y: i32, cover_table: &[u8; 1 << 16]) -> u8 {
    let (mut sky, mut block) = (0u8, 0u8);
    for (dx, dy, dz) in [(0, 0, 0), (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
        let open = (dx, dy, dz) == (0, 0, 0)
            || cover_table[cache.block_near(cell, y, dx, dy, dz) as usize] != FULL_COVER;
        if open {
            let light = cache.light_near(cell, y, dx, dy, dz);
            sky = sky.max(light & 0x0F);
            block = block.max((light >> 4) & 0x0F);
        }
    }
    sky | (block << 4)
}

/// **Which half of a hinged model this call draws**, and how far the hinge
/// has turned.
///
/// One model in the game has a moving part, and it moves while the chunk it
/// is in is not being meshed: a chest's lid, up while somebody has the chest
/// open and down after (`chest_lid_block`). So the chest is drawn from two
/// places and this says from which -- the chunk mesh draws `Bodied` for as
/// long as the frame is drawing `Lid`, and `Whole` the rest of the time,
/// which is every chest in the world that nobody is standing at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Hinged {
    /// Every box of the model, standing still. Everything that is not a
    /// chest, and every chest whose lid is shut and staying shut.
    Whole,
    /// Everything but the lid, which the frame is drawing.
    Bodied,
    /// The lid alone, swung this far.
    Lid(Swing),
}

/// **The pieces that go up when a chest is opened**, by the names they carry
/// in `furniture/chest.bbmodel`.
///
/// The lid and its crown, obviously; the two straps, which are nailed across
/// the lid and hang a quarter of a sixteenth below its underside, so a strap
/// left behind would be two bands floating in the air over an open chest;
/// and the hasp, the plate that hangs down the front off the lid. **Not the
/// staple** -- the loop the hasp drops over is driven into the body, and it
/// is what an open chest is open *against*.
fn on_the_lid(piece: &PropBox) -> bool {
    matches!(piece.name.split_whitespace().next(), Some("lid" | "strap" | "hasp"))
}

/// **Where a chest's lid is hinged**: (y, z) in the cell, at the bottom of
/// the lid along its back edge, as `furniture/chest.bbmodel` writes it -- the
/// lid's underside at 9.5 sixteenths and its back face at 14.25.
///
/// The chest is written facing north (`furniture_block`) and `Swing` lifts
/// what lies in front of the hinge, so the lid rises toward whoever is
/// standing at the chest, whichever way it was put down.
pub(crate) const LID_HINGE: [f32; 2] = [9.5 / 16.0, 14.25 / 16.0];

/// **How far a chest's lid stands open**: a right angle, so the lid stands
/// up and what is inside is in plain view from in front of it.
///
/// **It leans into the cell above, and every angle worth drawing does.** The
/// lid is twelve and a half sixteenths deep, so it is past the top of its own
/// cell at anything over a quarter of this -- an angle nobody would read as
/// open. A chest under a low ceiling has its lid drawn through the ceiling
/// for the third of a second it is moving and while it stands open; the
/// alternative is a lid that opens a hand's width, which is a chest that
/// looks stuck.
pub(crate) const LID_OPEN: f32 = std::f32::consts::FRAC_PI_2;

/// How long the lid takes to go from shut to open, in seconds.
///
/// The length of the sound it is drawn with (`Sfx::ChestOpen`, 0.47 s of
/// recorded hinge) less the part of it that is the latch letting go. Slower
/// than this and the chest is opened by somebody being careful; faster and
/// the lid is a cut rather than a swing -- which is what "a lid that snaps"
/// means, and it is the thing this was written to stop being.
pub(crate) const LID_SWING_SECONDS: f32 = 0.35;

/// The boxes of a model read from a file, turned and put in their cell.
///
/// `open` says, per box, which of its faces as written to leave out (see
/// `push_box_open`); `loaded` whether the block carries what a `loaded`
/// box shows; `hinged` which of a chest's two halves this is (`Hinged`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_prop(
    at: [f32; 3],
    boxes: &[PropBox],
    quarters: u32,
    loaded: bool,
    hinged: Hinged,
    open: impl Fn(&PropBox) -> u8,
    wood: usize,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    let wanted = |piece: &PropBox| match hinged {
        Hinged::Whole => true,
        Hinged::Bodied => !on_the_lid(piece),
        Hinged::Lid(_) => on_the_lid(piece),
    };
    // **What is inside is drawn only while the lid is off it.** A shut chest
    // -- every chest in the world nobody is standing at, and every one in a
    // hand -- has its floor and lining boxed in on six sides, and a quad
    // nobody can see still costs its vertices in every chunk that has one.
    let seen = |piece: &PropBox| !piece.inside || hinged != Hinged::Whole;
    let swing = match hinged {
        Hinged::Lid(swing) => Some(swing),
        _ => None,
    };
    let carried = |piece: &PropBox| if loaded { !piece.bare } else { !piece.loaded };
    for piece in boxes.iter().filter(|piece| carried(piece) && wanted(piece) && seen(piece)) {
        let (sky, block_light) = if piece.inside { in_the_dark(block_light) } else { (sky, block_light) };
        push_box_moved(
            at,
            piece.from,
            piece.to,
            quarters,
            // A lid's hinge or a box's own lean: never both, because the
            // only thing with a hinge is a chest's lid and nothing on a lid
            // is written leaning.
            swing.or(piece.tilt.map(Tilt::swing)),
            [piece.material.layer_in(textures, wood); 6],
            piece.material.cropped(),
            sky,
            block_light,
            0,
            open(piece),
            vertices,
            indices,
        );
    }
}

/// Is this a piece of furniture the mesher draws as boxes?
#[inline]
pub(crate) fn is_furniture(id: BlockId) -> bool {
    use primitive_shared::types::{
        block_kind, BLOCK_BED, BLOCK_CHAIR, BLOCK_CHEST, BLOCK_STOOL, BLOCK_STRAW_BED, BLOCK_TABLE,
    };
    matches!(block_kind(id), BLOCK_BED | BLOCK_STRAW_BED | BLOCK_STOOL | BLOCK_TABLE | BLOCK_CHAIR | BLOCK_CHEST)
        || primitive_shared::crafting::Station::of_workshop(id).is_some()
        // **The anvil is not a workshop `Station`**, and that is on purpose:
        // it runs no recipe rows, it runs a mini-game (`minigame`), so it is
        // not in `Station::WORKSHOPS` and `of_workshop` has never heard of
        // it. It is furniture here all the same -- a model on a stump that
        // leaves the floor of its cell showing round the base.
        || block_kind(id) == primitive_shared::types::BLOCK_ANVIL
        // ...and the barter stall, a counter on legs under an awning.
        || block_kind(id) == primitive_shared::types::BLOCK_STALL
        // ...and the two stations of the edge, models on legs and a stump.
        || block_kind(id) == primitive_shared::types::BLOCK_SAWHORSE
        || block_kind(id) == primitive_shared::types::BLOCK_HONING_STONE
        // ...and the lean-to, a hut of sticks and leaves drawn by its middle.
        || block_kind(id) == primitive_shared::types::BLOCK_LEAN_TO
}

/// How many of `push_box`'s quarter turns lay a bed written head-toward
/// -z so that its head points away from where its foot faces.
///
/// **Not `Facing::quarters`, and the difference is the whole of which end
/// the pillow is at.** `push_box` turns (x, z) to (z, -x) per quarter, which
/// carries the written front (+z, south) to east, then north, then west --
/// the reverse of the order `Facing` counts in. Using the facing's own
/// count put every east- and west-facing bed's pillow at the foot. Held by
/// `a_bed_has_its_pillow_at_the_head_whichever_way_it_lies`.
fn bed_quarters(facing: primitive_shared::types::Facing) -> u32 {
    use primitive_shared::types::Facing;
    match facing {
        Facing::South => 0,
        Facing::East => 1,
        Facing::North => 2,
        Facing::West => 3,
    }
}

/// How many of `push_box`'s quarter turns carry a model, as it is written,
/// round to `facing`, for a model whose written pose is its north.
///
/// `push_box` turns north to west, the opposite sense to the order
/// `Facing` counts in, so an east-facing model takes three of its turns
/// and not one. The rack used `Facing::quarters` directly and was drawn
/// mirrored east for west; the jug and the stool turn through this, and
/// `every_turned_model_looks_the_same_from_where_its_placer_stood` holds
/// all of them to it. North is none, so a jug or a stool from a save made
/// before they turned is drawn exactly as it was. The bed, the straw pallet
/// and the chair are written facing south and have `bed_quarters`, which is
/// this and a half turn.
fn turned_from_north(facing: primitive_shared::types::Facing) -> u32 {
    (4 - facing.quarters()) % 4
}

/// Furniture: the two halves of a bed or a straw pallet, a stool and a table,
/// each a handful of boxes wearing what it is made of.
///
/// **Drawn as models rather than as the part-height cubes their rows
/// say**, for the reason a carcass and a barrel are. A cube three eighths
/// tall with a picture of hide on top and boards squeezed down its sides
/// is a crate, and it was one: a player photographed "a planks bed-like
/// block with dark stripes" in the gallery, the stripes being board seams
/// squeezed into six sixteenths. The rows still say what the collider and
/// the step height need, and the models are written to their heights.
///
/// `partnered` is whether the other half of a bed is where it should be;
/// the boxes that run into it leave their seam faces open only then, so a
/// lone half from an older save is a closed box and not a hollow one.
/// Covers nothing -- see `drawn_as_model`.
pub(crate) fn furniture_block(
    at: [f32; 3],
    block: BlockId,
    partnered: bool,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    furniture_block_hinged(at, block, partnered, Hinged::Whole, textures, light, vertices, indices);
}

/// The same, told which half of a chest to draw (`Hinged`). Every other
/// piece of furniture is `Whole` and has no other answer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn furniture_block_hinged(
    at: [f32; 3],
    block: BlockId,
    partnered: bool,
    hinged: Hinged,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use primitive_shared::types::{
        block_facing, block_kind, is_bed, is_bed_head, BLOCK_BED, BLOCK_CHAIR, BLOCK_CHEST, BLOCK_LEATHER_BENCH,
        BLOCK_MASON_BLOCK, BLOCK_POTTERS_WHEEL, BLOCK_STOOL, BLOCK_STRAW_BED, BLOCK_TABLE, BLOCK_WORKBENCH,
    };
    use crate::logic::models::{prop, Prop};
    let bed = is_bed(block);
    let head = is_bed_head(block);
    let (model, quarters) = match block_kind(block) {
        BLOCK_BED if head => (Prop::BedHead, bed_quarters(block_facing(block))),
        BLOCK_BED => (Prop::BedFoot, bed_quarters(block_facing(block))),
        // Written as the bed is, so turned as the bed is: a pallet turned by
        // the stool's count -- which it was, as a one-cell heap -- would lie
        // with its bolster at the foot of every east- and west-facing one.
        BLOCK_STRAW_BED if head => (Prop::StrawBedHead, bed_quarters(block_facing(block))),
        BLOCK_STRAW_BED => (Prop::StrawBedFoot, bed_quarters(block_facing(block))),
        // Not the same after a quarter turn -- the odd leg -- so it faces
        // whoever put it down.
        BLOCK_STOOL => (Prop::Stool, turned_from_north(block_facing(block))),
        // Written front toward +z like the bed's foot, so the bed's count
        // of quarter turns is the chair's too: see `bed_quarters`, and
        // `a_chair_has_its_back_behind_whoever_sits_in_it_whichever_way_it_faces`.
        BLOCK_CHAIR => (Prop::Chair, bed_quarters(block_facing(block))),
        BLOCK_TABLE => (Prop::Table, 0),
        // **The chest is a model now**, lid, bands and hasp, where it was a
        // cube wearing a painting of them. Written front toward north, as
        // its `north` picture was, so turned the stool's way.
        BLOCK_CHEST => (Prop::Chest, turned_from_north(block_facing(block))),
        // The workshops are written with the side a player works from toward
        // north as well: the bench's vice, the wheel's open side.
        BLOCK_WORKBENCH => (Prop::Workbench, turned_from_north(block_facing(block))),
        BLOCK_MASON_BLOCK => (Prop::MasonBlock, turned_from_north(block_facing(block))),
        BLOCK_POTTERS_WHEEL => (Prop::PottersWheel, turned_from_north(block_facing(block))),
        BLOCK_LEATHER_BENCH => (Prop::LeatherBench, turned_from_north(block_facing(block))),
        // The anvil, written with its face toward north and the horn to the
        // east, so it is turned the way the four workshops are.
        primitive_shared::types::BLOCK_ANVIL => {
            (Prop::Anvil, turned_from_north(block_facing(block)))
        }
        // Written with its front -- the drape, where a buyer stands -- toward
        // north, so turned the way the chest is: it faces whoever put it down,
        // and the owner stands behind it.
        primitive_shared::types::BLOCK_STALL => (Prop::Stall, turned_from_north(block_facing(block))),
        // Worked from north, as the bench is: the joiner at the end of the
        // board, the grinder in front of the slab.
        primitive_shared::types::BLOCK_SAWHORSE => (Prop::Sawhorse, turned_from_north(block_facing(block))),
        primitive_shared::types::BLOCK_HONING_STONE => (Prop::HoningStone, turned_from_north(block_facing(block))),
        // **A lean-to is drawn whole by its middle cell** and by no other: the
        // hut is fifteen cells (`lean_to::PARTS`), written as one model three
        // cells long with its mouth toward +z where a bed's foot is, so turned
        // as the bed is (`lean_to::quarters` is this count, and
        // `a_lean_to_is_drawn_where_it_is_walked_into_whichever_way_it_faces`
        // holds the two together). Drawn once rather than sliced into fifteen
        // models that have to meet: the rack's reason (`rack_block`). **Not
        // asked whether the other fourteen are there**, unlike the rack: the
        // server puts down and takes away all fifteen together, and the one
        // time a mesher sees fewer is a neighbouring chunk that has not
        // arrived yet -- when a hut drawn whole is right and a hut drawn as
        // nothing is a hole in the camp until it does.
        primitive_shared::types::BLOCK_LEAN_TO if primitive_shared::lean_to::is_anchor(block) => {
            lean_to_block(at, block, textures, light, vertices, indices);
            return;
        }
        primitive_shared::types::BLOCK_LEAN_TO => return,
        _ => return,
    };
    // Face 4 is +z and 5 is -z, as the piece is written. Measured on the
    // boxes rather than named in the file, so a blanket lengthened to the
    // seam in Blockbench opens its end there too.
    // Not a lean-to's: its model is the whole hut, and nothing in it runs
    // into a partner's cell to be left open at the seam.
    let bed = bed && block_kind(block) != primitive_shared::types::BLOCK_LEAN_TO;
    let open = |piece: &PropBox| match (bed && partnered, head) {
        (true, true) if piece.to[2] >= 16.0 => 1 << 4,
        (true, false) if piece.from[2] <= 0.0 => 1 << 5,
        _ => 0,
    };
    push_prop(at, prop(model), quarters, false, hinged, open, primitive_shared::types::furniture_wood(block), textures, light, vertices, indices);
}

/// **A lean-to, drawn whole by its middle cell** (`lean_to::is_anchor`):
/// `furniture/lean_to.bbmodel`, turned as a bed is (`bed_quarters`, which is
/// `lean_to::quarters`).
///
/// **Its `inside` is the hollow under the thatch, drawn always and in the
/// shade.** In a chest the group is the lining a shut lid boxes in, so it is
/// left out unless the lid is up (`push_prop`); a hut has no lid, and its
/// bed, its ribs and the undersides of its leaves are what anybody looking
/// in at the mouth sees. Lit by the cell as the rest of the model is, they
/// were as bright as the grass outside -- a hut that was a heap of leaves
/// round a sunlit floor. So they are drawn with a third of the sky and half
/// the fire: dim enough to read as the inside of something roofed, not the
/// black of a shut box, because a debris hut is open at one end.
pub(crate) fn lean_to_block(
    at: [f32; 3],
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::logic::models::{prop, Prop};
    let quarters = bed_quarters(primitive_shared::types::block_facing(block));
    let model = prop(Prop::LeanTo);
    let outside: Vec<PropBox> = model.iter().filter(|piece| !piece.inside).copied().collect();
    let under: Vec<PropBox> = model.iter().filter(|piece| piece.inside).map(|piece| PropBox { inside: false, ..*piece }).collect();
    push_prop(at, &outside, quarters, false, Hinged::Whole, |_| 0, 0, textures, light, vertices, indices);
    let (sky, fire) = (light & 0x0F, (light >> 4) & 0x0F);
    let shaded = (sky / 3) | ((fire / 2) << 4);
    push_prop(at, &under, quarters, false, Hinged::Whole, |_| 0, 0, textures, shaded, vertices, indices);
}

/// **A chest's lid alone, swung `angle` radians open**, in its cell's space
/// like everything the frame draws (`at` is the cell's corner measured from
/// the render origin).
///
/// Drawn by the frame and not by the mesher for the reason the things a hand
/// set down are (`ChunkManager::set_down`): it moves while the chunk stands
/// still, and a chunk remeshed on every frame of a third of a second of
/// swing is forty-odd rebuilds for one lid. The chunk mesh leaves the lid
/// out for exactly as long as this is drawing it (`Hinged::Bodied`), so the
/// two never both draw it and there is never a gap where neither does.
pub(crate) fn chest_lid_block(
    at: [f32; 3],
    block: BlockId,
    angle: f32,
    textures: &crate::engine::texture::FaceLayers,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let hinged = Hinged::Lid(Swing::lid(angle));
    furniture_block_hinged(at, block, false, hinged, textures, light, vertices, indices);
}

/// Whether a block that is carried or lies dropped is drawn as its own model
/// -- the boxes `build_mesh` stands in the world -- rather than as a cube or
/// a sprite. See [`carried_model`].
pub(crate) fn has_carried_model(block: BlockId) -> bool {
    use primitive_shared::types as t;
    let kind = t::block_kind(block);
    t::is_barrel(block)
        || t::is_branch(block)
        || t::is_campfire(block)
        || is_furniture(block)
        || t::is_door(block)
        || t::is_step(block)
        || matches!(kind, t::BLOCK_JUG | t::BLOCK_DRYING_RACK | t::BLOCK_HIDE_FRAME | t::BLOCK_NEST | t::BLOCK_NEST_EGGS | t::BLOCK_COCONUT | t::BLOCK_CAIRN)
}

/// A block as it is carried or dropped: the geometry `build_mesh` draws it
/// with in the world, built outside the world, in its own cell's space -- 0
/// to 1 across, standing on its floor at 0. Emits nothing and answers false
/// for a block with no model of its own.
///
/// **The report this answers**: "у предметов нету 3д модели только блок или
/// текстура". A barrel stood in the camp as staves round water and was held
/// in the hand as a flat picture of a barrel given a texel of thickness,
/// because `TextureManager::load` cuts an item model out of any block with a
/// carried picture, and the carried picture is the *icon*. A table with no
/// icon was a cube of planks. Both were the rule "a block with no model of its
/// own is a cube", written before any block had one.
///
/// Three ways were weighed:
///
/// * **A model per item, drawn or modelled separately.** A second shape for
///   every piece of furniture, free to disagree with the first the next time
///   either changes -- the bed has already been rebuilt twice.
/// * **Meshing a one-block world around it** with `build_mesh`. The same
///   geometry, but a neighbourhood of chunks to fill per model and a
///   frame's worth of meshing for every barrel on the floor.
/// * **The emitters themselves** (chosen). `jug_block`, `barrel_block` and
///   the rest take a place and a light and write boxes; called here with a
///   place of nought they are the world's model and nothing else, for a few
///   dozen quads a frame.
///
/// **Built with a light that is a marker, not a light**: all the sky and no
/// fire. [`place_carried`] relights every vertex where the thing actually is,
/// and the one thing it keeps is fire found in the model -- the flame of a lit
/// hearth, which `flame_block` builds at full block light because it is the
/// source.
pub(crate) fn carried_model(
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) -> bool {
    use primitive_shared::types as t;
    const OPEN_SKY: u8 = 0x0F;
    let kind = t::block_kind(block);
    if t::is_barrel(block) {
        barrel_block([0.0; 3], block, textures, OPEN_SKY, vertices, indices);
    } else if kind == t::BLOCK_JUG {
        jug_block([0.0; 3], block, textures, OPEN_SKY, vertices, indices);
    } else if kind == t::BLOCK_LEAN_TO {
        // **The whole hut**, drawn by its middle cell as it stands in the
        // world: carried and dropped as one thing, and placed as fifteen.
        furniture_block([0.0; 3], primitive_shared::lean_to::cell(t::Facing::South, 8), true, textures, OPEN_SKY, vertices, indices);
    } else if t::is_bed(block) {
        // **Both halves**, the head to the north of the foot and the seam
        // between them left open, as a bed stands in the world. A bed -- or a
        // straw pallet -- is carried as one thing and placed as two cells, and
        // half of one on its own is a crate with a pillow at one end.
        furniture_block([0.0, 0.0, -1.0], t::bed_half_of(kind, t::Facing::South, true), true, textures, OPEN_SKY, vertices, indices);
        furniture_block([0.0; 3], t::bed_half_of(kind, t::Facing::South, false), true, textures, OPEN_SKY, vertices, indices);
    } else if is_furniture(block) {
        furniture_block([0.0; 3], block, false, textures, OPEN_SKY, vertices, indices);
    } else if t::is_door(block) {
        // **Both halves**, the top over the bottom, for the bed's reason: a
        // door is carried as one thing and hung as two cells, and the lower
        // half alone is a square of boards with a handle on it.
        let lower = t::faced(t::BLOCK_DOOR, t::Facing::North);
        door_block([0.0; 3], lower, textures, [OPEN_SKY; 3], vertices, indices);
        door_block([0.0, 1.0, 0.0], t::door_partner((0, 0, 0), lower).map_or(lower, |(_, top)| top), textures, [OPEN_SKY; 3], vertices, indices);
    } else if t::is_step(block) {
        step_block([0.0; 3], block, |_, _, _| t::BLOCK_AIR, textures, OPEN_SKY, vertices, indices);
    } else if matches!(kind, t::BLOCK_DRYING_RACK | t::BLOCK_HIDE_FRAME) {
        rack_block([0.0; 3], block, RackColumns::Lone, textures, OPEN_SKY, vertices, indices);
    } else if matches!(kind, t::BLOCK_NEST | t::BLOCK_NEST_EGGS) {
        nest_block([0.0; 3], block, textures, OPEN_SKY, vertices, indices);
    } else if kind == t::BLOCK_CAIRN {
        cairn_block([0.0; 3], block, textures, OPEN_SKY, vertices, indices);
    } else if kind == t::BLOCK_COCONUT {
        // A nut of the palm's own bunch, not the icon given a texel of
        // thickness: "кокосы надо добавлять как модели".
        coconut_block([0.0; 3], textures, OPEN_SKY, vertices, indices);
    } else if kind == t::BLOCK_PALM_TRUNK {
        // A lone piece, as the world draws one with no palm round it: an
        // upright post the height of its cell, with its cut at both ends the
        // way a carried log shows its own. The branch path below would wear
        // bark all round.
        palm_trunk_block([0.0; 3], block, |_, _, _| t::BLOCK_AIR, textures, OPEN_SKY, vertices, indices);
    } else if t::is_branch(block) {
        // **Standing on end, the height of its cell.** A lone piece joined
        // to nothing is drawn as a knot -- a cube its own width -- which is
        // right in a tree and reads as a block of bark in a hand. Joined
        // above and below to "a piece of width nought" it runs the full
        // height and keeps both cut ends, because nothing nought wide can
        // bury them.
        branch_block([0.0; 3], block, ([Some(0), Some(0), None, None, None, None], ALONE), textures, OPEN_SKY, vertices, indices);
    } else if t::is_campfire(block) {
        hearth_box(block, textures, vertices, indices);
        if matches!(kind, t::BLOCK_CAMPFIRE_LIT | t::BLOCK_FIREPIT_LIT) {
            flame_block([0, 0, 0], [0.0; 3], textures, OPEN_SKY, vertices, indices);
        }
    } else {
        return false;
    }
    true
}

/// A ring of stones as `build_mesh` draws one: the block's own six pictures
/// on a cube as tall as its row, the sides wearing the top rows of their
/// picture as `side_crop` cuts them in the world.
fn hearth_box(
    block: BlockId,
    textures: &crate::engine::texture::FaceLayers,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let top = primitive_shared::types::block_height(block);
    for (face_index, face) in faces().iter().enumerate() {
        let layer = textures.layer_for_face(block, face_index);
        let packed = pack_light(15, 0, 3, face_index as u8);
        let base = vertices.len() as u32;
        for corner in face.corners.iter() {
            let uv = face_uv(face_index, *corner);
            let vertex = Vertex::new([corner[0], corner[1] * top, corner[2]], uv, layer, packed);
            vertices.push(if face_index >= 2 { vertex.with_fine_uv([uv[0], uv[1] * top]) } else { vertex });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The smallest and largest corner of a model, per axis.
pub(crate) fn extent(vertices: &[Vertex]) -> (glam::Vec3, glam::Vec3) {
    vertices.iter().fold((glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)), |(low, high), v| {
        let p = glam::Vec3::from_array(v.position);
        (low.min(p), high.max(p))
    })
}

/// A model from [`carried_model`], put somewhere: every corner through
/// `transform`, and every vertex lit by `light` -- the sky and the fire where
/// the thing now is.
///
/// **The face index turns with the geometry**, and this is the third place in
/// this codebase to learn it. The shader makes a normal out of the index, so a
/// barrel spun on the floor with its model-space indices is lit as though it
/// had not turned: its shading welded to the barrel instead of to the sun
/// (`entities::spun_face`, `animal_model::world_face_of`). Each index is
/// turned by the same transform and snapped to the nearest of the six --
/// all a light word can say -- the answer `item_model::nearest_face` gives a
/// sprite. Turning the declared index rather than asking the winding keeps a
/// flame a flame: its crossed quads say "up" so a fire is as bright from
/// every side, and the winding would say "sideways".
///
/// **...but a face whose index *is* its winding is asked its winding again.**
/// A leaning box -- a lace of a hide frame, a stake's spike -- was already snapped once when it was built, to the
/// nearest of six; turning that snapped axis and snapping again answers for
/// an axis the face never pointed along, and a hide frame held at an angle
/// lit a cord's top as its side. So a quad whose built index is the nearest
/// face to its built winding takes the nearest face to its placed winding,
/// which is the index it would have had if it had been built where it is;
/// only a quad lit on purpose against its winding -- the flame -- has its
/// declared index turned.
///
/// **Fire the model carries is kept**: a vertex built with block light keeps
/// the brighter of its own and the cell's, so the flame on a lit hearth in the
/// hand burns in the dark.
pub(crate) fn place_carried(
    model: &[Vertex],
    model_indices: &[u32],
    transform: glam::Mat4,
    (sky, block_light): (u8, u8),
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    let winding = |corners: &[glam::Vec3]| (corners[1] - corners[0]).cross(corners[2] - corners[1]);
    // Quads when the model is quads, which every carried model is; a vertex
    // on its own when it is not, and then the declared index is all there is.
    let quad = if model.len().is_multiple_of(4) { 4 } else { 1 };
    for corners in model.chunks_exact(quad) {
        // Arrays and not vectors: this runs for the thing in the hand on
        // every frame.
        let mut built = [glam::Vec3::ZERO; 4];
        let mut placed = [glam::Vec3::ZERO; 4];
        for (k, vertex) in corners.iter().enumerate() {
            built[k] = glam::Vec3::from_array(vertex.position);
            placed[k] = transform.transform_point3(built[k]);
        }
        let declared = (((corners[0].packed & LIGHT_MASK) >> 10) & 7).min(5) as u8;
        let lit_by_winding = quad == 4
            && winding(&built).length_squared() > 0.0
            && winding(&placed).length_squared() > 0.0
            && crate::engine::item_model::nearest_face(winding(&built)) == declared;
        for (vertex, position) in corners.iter().zip(placed.iter()) {
            let word = vertex.packed & LIGHT_MASK;
            let own_fire = ((word >> 4) & 15) as u8;
            let turned = if lit_by_winding {
                crate::engine::item_model::nearest_face(winding(&placed))
            } else {
                let (axis, sign) = FACE_OUTWARD[((word >> 10) & 7).min(5) as usize];
                let mut normal = glam::Vec3::ZERO;
                normal[axis] = sign as f32;
                crate::engine::item_model::nearest_face(transform.transform_vector3(normal))
            };
            vertices.push(Vertex {
                position: position.to_array(),
                packed: (vertex.packed & !LIGHT_MASK) | pack_light(sky, block_light.max(own_fire), 3, turned),
                uv: vertex.uv,
            });
        }
    }
    indices.extend(model_indices.iter().map(|index| base + index));
}

/// One axis-aligned box of a bespoke model, in sixteenths of a cell.
///
/// `quarters` turns it about the middle of the cell, so a model is
/// written once facing north and the four facings come out of the same
/// numbers -- see `types::Facing`.
///
/// **Every face wears a strip of the picture cut to its own size.** A
/// terrain vertex carries one *bit* of texture coordinate per axis, so
/// for a long time every face wore the whole picture corner to corner --
/// a pole two sixteenths wide showed all sixteen columns of bark
/// squeezed into two, which reads as noise glued to a stick. A crop code
/// in the tint byte fixed that for sizes that were powers of two, and a
/// real place in the picture fixes it for every size (see `FINE_UV_BIT`):
/// a face two sixteenths wide shows two texels of bark, the density the
/// block beside it is drawn at, and a face six sixteenths tall shows six.
/// `cropped` says whether the faces wear the piece of the picture that
/// lies under them (a material: bark, bare wood) or the middle of it (a
/// picture drawn for the face, like the stretched hide) -- the same
/// distinction `animal_model::Skin::tiles` makes. Neither squeezes.
#[allow(clippy::too_many_arguments)]
fn push_box(
    at: [f32; 3],
    from: [f32; 3],
    to: [f32; 3],
    quarters: u32,
    layer: u32,
    cropped: bool,
    sky: u8,
    block_light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    push_box_open(at, from, to, quarters, layer, cropped, sky, block_light, 0, vertices, indices);
}

/// `push_box`, leaving out the faces whose bits are set in `open` -- bit
/// `n` for face `n` of the box as it is written, before `quarters` turns
/// it.
///
/// For a model whose boxes run into each other end to end, where a face is
/// known to be buried: see `branch_block`. Every other model draws all six
/// and lets the depth test bury them, which is right for a box whose
/// neighbour might not cover it.
#[allow(clippy::too_many_arguments)]
fn push_box_open(
    at: [f32; 3],
    from: [f32; 3],
    to: [f32; 3],
    quarters: u32,
    layer: u32,
    cropped: bool,
    sky: u8,
    block_light: u8,
    open: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    push_box_faces(at, from, to, quarters, [layer; 6], cropped, sky, block_light, open, vertices, indices);
}

/// **A hinge**: which axis a box turns about, through where, and how far.
///
/// Two things turn on one. A chest's lid, about an east-west axis along its
/// back edge ([`Swing::lid`]): a chest is written facing north
/// (`furniture_block`), so the lid rises toward the player standing in front
/// of it. And a box a model file writes leaning -- a stake's spikes -- about
/// whichever one axis its `rotation` names (`models::prop_boxes`).
///
/// **One axis, never three.** Blockbench turns a box about x, y and z in an
/// order the file does not state, and a reader that guessed the order would
/// draw a box the editor shows somewhere else; about one axis there is no
/// order to guess. It is also what Minecraft's own model format allows, so a
/// modeller already works within it.
///
/// In cell coordinates, 0..1, like everything `push_box_moved` works in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Swing {
    /// 0 x (east-west), 1 y, 2 z (north-south).
    pub axis: usize,
    /// A point the axis passes through.
    pub pivot: [f32; 3],
    /// Radians, turned right-handed about the axis, as Blockbench turns.
    /// About x, positive lifts what lies in front of the axis (`-z`).
    pub angle: f32,
}

impl Swing {
    /// A chest's lid, `angle` radians open about [`LID_HINGE`].
    pub(crate) fn lid(angle: f32) -> Swing {
        Swing { axis: 0, pivot: [0.0, LID_HINGE[0], LID_HINGE[1]], angle }
    }

    /// Where a point ends up, turned about the axis through the pivot.
    #[inline]
    fn turn(self, point: [f32; 3]) -> [f32; 3] {
        let (u, v) = ((self.axis + 1) % 3, (self.axis + 2) % 3);
        let (du, dv) = (point[u] - self.pivot[u], point[v] - self.pivot[v]);
        let (sin, cos) = self.angle.sin_cos();
        let mut out = point;
        out[u] = self.pivot[u] + du * cos - dv * sin;
        out[v] = self.pivot[v] + du * sin + dv * cos;
        out
    }

    /// Which of the six faces (`push_box_moved`'s order) face `face` has come
    /// to look along: its normal turned, and the nearest of the six.
    fn face_after(self, face: usize) -> usize {
        const NORMALS: [[f32; 3]; 6] =
            [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        let normal = Swing { pivot: [0.0; 3], ..self }.turn(NORMALS[face]);
        (0..6)
            .max_by(|&a, &b| {
                let dot = |f: usize| (0..3).map(|k| NORMALS[f][k] * normal[k]).sum::<f32>();
                dot(a).total_cmp(&dot(b))
            })
            .unwrap_or(face)
    }
}

/// `push_box_open`, with a picture for each face: `layers` in the box's face
/// order as it is written, before `quarters` turns it.
///
/// For the one model whose ends are not its sides, a palm's trunk
/// (`palm_trunk_block`). Every other box is one material all round.
#[allow(clippy::too_many_arguments)]
fn push_box_faces(
    at: [f32; 3],
    from: [f32; 3],
    to: [f32; 3],
    quarters: u32,
    layers: [u32; 6],
    cropped: bool,
    sky: u8,
    block_light: u8,
    open: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    push_box_moved(at, from, to, quarters, None, layers, cropped, sky, block_light, 0, open, vertices, indices);
}

/// **A box on a hinge, wearing a tint**: everything `push_box_faces` does,
/// and the two things exactly one caller each needs.
///
/// * `swing` turns the box about an axis running east-west -- a chest's
///   lid rising off its body ([`LID_HINGE`]) -- before `quarters` turns
///   the model. Written as a hinge and an angle rather than a matrix
///   because the picture on each face has to stay the picture on that
///   face: the texture coordinates come from the corner *as the model is
///   written* (`face_uv`), and only the position and the face's own
///   direction move.
/// * `tint` is a foliage tint (`pack_tint`), for a crown standing in water:
///   drawn here rather than by the cube path, and untinted it came out the
///   grey-green of a leaf with no climate beside the tinted crown over it
///   (`crown_in_water`).
///
/// Split off rather than folded into `push_box_faces` so the twenty-odd
/// models that want neither pay for neither and read as they did.
#[allow(clippy::too_many_arguments)]
fn push_box_moved(
    at: [f32; 3],
    from: [f32; 3],
    to: [f32; 3],
    quarters: u32,
    swing: Option<Swing>,
    layers: [u32; 6],
    cropped: bool,
    sky: u8,
    block_light: u8,
    tint: u32,
    open: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    // About the centre of the cell, in the same direction `Facing`
    // counts: anticlockwise from north.
    let turn = |x: f32, z: f32| -> (f32, f32) {
        let (cx, cz) = (x - 0.5, z - 0.5);
        let (rx, rz) = match quarters % 4 {
            1 => (cz, -cx),
            2 => (-cx, -cz),
            3 => (-cz, cx),
            _ => (cx, cz),
        };
        (rx + 0.5, rz + 0.5)
    };

    // **Grown by a hair, and that hair is the whole of a bug report.**
    // Two boxes of a model that touch exactly share a plane -- a nest's
    // wall against its floor, an egg on that floor, a bracket against
    // the bark it grows out of -- and a shared plane is two surfaces the
    // depth buffer cannot tell apart: which one wins is rounding, it
    // changes as the camera moves, and it reads as the model flickering.
    // A player reported it as "textures laid over each other".
    //
    // Pushing every box out by a fiftieth of a sixteenth buries the
    // hidden face inside its neighbour, where the depth test throws it
    // away and keeps throwing it away. One eight-hundredth of a block:
    // nothing an eye can measure, and no extra geometry, pass or sort.
    //
    // Named rather than written twice: the tests that hold a model
    // inside its cell have to allow exactly this much and no more, and a
    // tolerance somebody typed by hand would go out of step with it.
    let lo = [
        (from[0] - BITE) * T,
        (from[1] - BITE) * T,
        (from[2] - BITE) * T,
    ];
    let hi = [(to[0] + BITE) * T, (to[1] + BITE) * T, (to[2] + BITE) * T];
    // Face order is the mesher's own: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
    // Each face is four corners wound so the outside is what is seen,
    // and two of the box's three sizes for its texture.
    let faces: [(usize, [[f32; 3]; 4], [f32; 2]); 6] = [
        (0, [[lo[0], hi[1], hi[2]], [hi[0], hi[1], hi[2]], [hi[0], hi[1], lo[2]], [lo[0], hi[1], lo[2]]], [hi[0] - lo[0], hi[2] - lo[2]]),
        (1, [[lo[0], lo[1], lo[2]], [hi[0], lo[1], lo[2]], [hi[0], lo[1], hi[2]], [lo[0], lo[1], hi[2]]], [hi[0] - lo[0], hi[2] - lo[2]]),
        (2, [[hi[0], lo[1], hi[2]], [hi[0], lo[1], lo[2]], [hi[0], hi[1], lo[2]], [hi[0], hi[1], hi[2]]], [hi[2] - lo[2], hi[1] - lo[1]]),
        (3, [[lo[0], lo[1], lo[2]], [lo[0], lo[1], hi[2]], [lo[0], hi[1], hi[2]], [lo[0], hi[1], lo[2]]], [hi[2] - lo[2], hi[1] - lo[1]]),
        // **These two are wound the way they are for a reason, and it
        // was got wrong for as long as this model existed.** A face is
        // front-facing when its corners run counter-clockwise *seen
        // from outside* -- that is what `FrontFace::Ccw` means. These
        // two ran the other way, so the pipeline culled the face
        // pointing at the player and drew the one behind it: every box
        // of the rack was rendered inside-out. Nothing about the
        // coordinates was ever wrong, which is why every check on them
        // passed while the frame kept sprouting thin bright slivers at
        // its corners -- those were the far side of a pole, seen
        // through the near side that should have hidden it.
        (4, [[lo[0], lo[1], hi[2]], [hi[0], lo[1], hi[2]], [hi[0], hi[1], hi[2]], [lo[0], hi[1], hi[2]]], [hi[0] - lo[0], hi[1] - lo[1]]),
        (5, [[hi[0], lo[1], lo[2]], [lo[0], lo[1], lo[2]], [lo[0], hi[1], lo[2]], [hi[0], hi[1], lo[2]]], [hi[0] - lo[0], hi[1] - lo[1]]),
    ];
    // **Which world face this box face becomes once the model is
    // turned.** One quarter turn about Y sends +X to -Z, -Z to -X, -X
    // to +Z and +Z to +X; the top and the bottom stay where they are.
    // Applying it `quarters` times is the whole mapping.
    //
    // This is the fix for the blades. The geometry was rotated below
    // and the *face index* was not, so a turned rack carried a light
    // word claiming each face pointed the opposite way. The shader
    // turns that index into a normal (see `face_normal` in
    // shader.wgsl), so on a south-facing frame every surface got the
    // shade meant for the surface behind it: the uprights' inner sides
    // -- which should sit at the dark end of the lambert term -- came
    // out near twice as bright as the front they are attached to, and a
    // sliver of wood lit like that, seen edge-on at a wide field of
    // view, reads as a thin bright blade standing off the model.
    // Nothing was ever wrong with the boxes, which is why every check
    // on their coordinates passed.
    const TURN_FACE: [usize; 6] = [0, 1, 5, 4, 2, 3];
    // **And the same for the hinge**: the face's own direction turned about
    // it (`Swing::face_after`) -- for a lid, a quarter of a swing sends +Y
    // to +Z, +Z to -Y, -Y to -Z and -Z to +Y, and leaves the two ends of the
    // hinge where they are.
    //
    // **Counted to the nearest quarter, which is a choice and not a
    // rounding.** A face index is one of six directions and a lid mid-swing
    // points between two of them; the shader has nothing finer to be told
    // (`face_normal` in shader.wgsl). So the lid is lit as the shut lid
    // until it is past halfway and as a standing one after: one step in
    // brightness, in the middle of a swing that takes a third of a second,
    // on a piece that is moving -- against the alternative, which is a lid
    // standing wide open lit as though it were still lying shut, bright
    // side up, for as long as somebody is rummaging in the chest.
    // A stake's spike leans a third of a quarter, and so is lit as the
    // upright it nearly is.
    for (face, corners, _) in faces {
        if open & (1 << face) != 0 {
            continue;
        }
        let mut world_face = swing.map_or(face, |swing| swing.face_after(face));
        for _ in 0..(quarters % 4) {
            world_face = TURN_FACE[world_face];
        }
        // Lit as the face it is, so the four sides of a pole are not the
        // same brightness -- which is the whole of what makes a box read
        // as round enough. Unoccluded: nothing in a frame occludes
        // anything else in it.
        let packed = pack_light(sky, block_light, 3, world_face as u8);
        // **One texel to a sixteenth, on every face, at every size.** Each
        // corner wears the place in the picture that lies under it, read
        // the way a whole block's face reads its picture (`face_uv`) from
        // the corner's position in the cell as the model is written -- so
        // the grain turns with the model, a pole two sixteenths wide shows
        // two texels of bark, and two boxes side by side continue one
        // picture across the join instead of each starting it again.
        //
        // A box whose picture was drawn for it (`cropped` false: the skin
        // on a rack) wears its middle rather than the piece under it, so
        // what the picture was drawn around stays in view -- still cut,
        // never squeezed. See `FINE_UV_BIT` for what this replaced.
        let middle = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let place = |corner: [f32; 3]| {
            let uv = face_uv(face, corner);
            if cropped {
                uv
            } else {
                let from_middle = face_uv(face, middle);
                [uv[0] - from_middle[0] + 0.5, uv[1] - from_middle[1] + 0.5]
            }
        };
        // **Moved by whole pictures to where the coordinate cannot be
        // negative.** `face_uv` reads a side's `v` as `1 - y`, which is the
        // picture's own top-down order for anything inside its cell; a model
        // bigger than its cell -- the two-by-two drying rack, thirty-two
        // sixteenths tall -- has corners at `y` 2, and `1 - 2` is -1. A fine
        // coordinate is unsigned (`with_fine_uv`), so everything below
        // nought came out as nought: the upper cell of every pole wore one
        // row of bark stretched the length of it, in streaks, and the far
        // end of the ridge the same along `1 - x`. The picture repeats
        // (`texture::BLOCK_ADDRESS_MODE`), so a whole picture's shift is the
        // same bark, continued, and the face still meets the one beside it.
        let shift = [0, 1].map(|axis| {
            let least = corners.iter().map(|&corner| place(corner)[axis]).fold(f32::INFINITY, f32::min);
            // Only where the packing would round below nought: the hair
            // every box is grown by (`BITE`) puts a corner a fraction of a
            // texel under it, which rounds to nought and was always right.
            if least * FINE_UNITS < -0.5 {
                (-least).ceil()
            } else {
                0.0
            }
        });
        let base = vertices.len() as u32;
        for corner in corners {
            // The hinge first and the yaw after, which is the order the
            // model is written in: the lid swings in the chest's own
            // north-facing frame, and then the whole chest is turned to
            // face where it was put down.
            let [cx, cy, cz] = swing.map_or(corner, |swing| swing.turn(corner));
            let (x, z) = turn(cx, cz);
            let uv = place(corner);
            let uv = [uv[0] + shift[0], uv[1] + shift[1]];
            vertices.push(
                Vertex::tinted([at[0] + x, at[1] + cy, at[2] + z], [0.0, 0.0], layers[face], packed, tint)
                    .with_fine_uv(uv),
            );
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Which face's picture a cross-shaped plant is drawn with.
///
/// **The top picture, except the lower half of a tall plant and its shoot,
/// which wear the side.** Every cross was drawn from face 0, and a row that
/// names one picture puts it on every face, so nothing changes for them. A
/// tall plant's row names two -- `top` for its upper half, `side` for its
/// stalk (`blocks.toml`) -- which is how one row draws two cells without a
/// second id (`types::PLANT_TOP`).
pub(crate) fn cross_face(id: BlockId) -> usize {
    use primitive_shared::types::{is_plant_top, is_tall_plant};
    if is_tall_plant(id) && !is_plant_top(id) {
        2
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
fn cross_block(
    cell: [i32; 3],
    at: [f32; 3],
    block: BlockId,
    layer: u32,
    light: u8,
    tint: u32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    // Face 0 (up) for the light direction: a plant has no normal worth
    // the name, and shading it as an upward face keeps it the same
    // brightness from every side, which is what a billboard wants.
    let packed = pack_light(sky, block_light, 3, 0);

    // The corners come from `cross_planes`, which the mining overlay
    // reads too -- see there for why they are not worked out here.
    for plane in cross_planes(cell, at, block) {
        let base = vertices.len() as u32;
        for (corner, uv) in plane.into_iter().zip([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
        {
            vertices.push(Vertex::tinted(corner, uv, layer, packed, tint));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[allow(clippy::too_many_arguments)]
fn flat_block(
    cell: [i32; 3],
    at: [f32; 3],
    block: BlockId,
    layer: u32,
    relief: Option<&crate::engine::relief::Relief>,
    light: u8,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    // **A stone stands up from the ground; a coating lies on it.** Which
    // blocks have a thickness, and why ash, snow and a lily pad do not, is
    // `relief::has_relief`; the shape is the picture's own, see `relief`.
    // The quad below is what is left for those three -- and for a table
    // with no shapes in it, which is how a before-and-after is taken from
    // one binary (`FaceLayers::without_reliefs`).
    if let Some(relief) = relief {
        let inset = primitive_shared::types::flat_inset(block);
        relief.append(cell, at, inset, layer, light, vertices, indices);
        return;
    }
    let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
    // Face 0 (up), which is the way it faces and the light it should
    // catch.
    let packed = pack_light(sky, block_light, 3, 0);

    // A quarter turn per step, as a rotation of which corner gets which
    // texture coordinate.
    let turn = (cell_hash(cell[0], cell[1], cell[2]) & 3) as usize;
    const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

    let base = vertices.len() as u32;
    // The corners come from `flat_quad`, shared with the mining overlay.
    for (corner, position) in flat_quad(at, block).into_iter().enumerate() {
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

    /// **A stake is aimed at round what is drawn of it**, standing and
    /// driven into a wall on each side: every spike inside the box a ray
    /// stops at, and the box no more than a sixteenth and a half past the
    /// spikes on any side but the wall's -- or a click in the empty corner of
    /// the cell takes the stake. Below the floor does not count: a leaning
    /// pole's foot is in the ground.
    #[test]
    fn a_stake_is_aimed_at_round_what_is_drawn_of_it() {
        use primitive_shared::types::{faced, Facing, BLOCK_STAKE, STAKE_UPRIGHT};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let mut stakes = vec![BLOCK_STAKE | STAKE_UPRIGHT];
        stakes.extend([Facing::North, Facing::East, Facing::South, Facing::West].map(|f| faced(BLOCK_STAKE, f)));
        for block in stakes {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            stake_block([0.0; 3], block, &layers, 0xFF, &mut v, &mut i);
            assert!(v.len() > 24 * 5, "{block:#x} is drawn as {} corners: not a bundle of spikes", v.len());
            let (low, high) = extent(&v);
            let low = low.max(glam::Vec3::new(f32::MIN, 0.0, f32::MIN));
            let (min, max) = primitive_shared::geometry::block_box_for_aim(block, 0, 0, 0, false).expect("a stake is aimed at");
            let wall = primitive_shared::types::support_at(block);
            for axis in 0..3 {
                // The butts are in the wall's cell, which is where they
                // are driven: that side is only the wall's.
                let (wall_low, wall_high) = match axis {
                    0 => (wall.0 < 0, wall.0 > 0),
                    1 => (wall.1 < 0, wall.1 > 0),
                    _ => (wall.2 < 0, wall.2 > 0),
                };
                let slack = BITE * T + 1e-4;
                assert!(
                    (wall_low || low[axis] >= min[axis] - slack) && (wall_high || high[axis] <= max[axis] + slack),
                    "{block:#x}: drawn {low}..{high}, aimed {min:?}..{max:?}"
                );
                let loose = 1.5 / 16.0 + slack;
                assert!(wall_low || low[axis] - min[axis] <= loose, "{block:#x}: aimed {} past the spikes below on axis {axis}", low[axis] - min[axis]);
                assert!(wall_high || max[axis] - high[axis] <= loose, "{block:#x}: aimed {} past the spikes above on axis {axis}", max[axis] - high[axis]);
            }
        }
    }

    /// **An open chest is a box with a dark inside, and a shut one is the
    /// chest it always was.** "сундук внутри не полый, сделай там черноту и
    /// стенки не забудь": the body drawn while its lid is up has boxes lit
    /// with no sky (`in_the_dark`) and at most a quarter of the fire; the
    /// whole chest draws none of them -- they would be boxed in -- and its
    /// outside is where the one solid body's was.
    #[test]
    fn an_open_chest_is_dark_inside_and_a_shut_one_draws_no_inside() {
        use primitive_shared::types::{faced, Facing, BLOCK_CHEST};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let chest = faced(BLOCK_CHEST, Facing::South);
        let draw = |hinged| {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            furniture_block_hinged([0.0; 3], chest, false, hinged, &layers, 0xFF, &mut v, &mut i);
            v
        };
        let dark = |v: &Vec<Vertex>| v.iter().filter(|v| v.light() & 0x0F == 0).count();
        let (whole, bodied) = (draw(Hinged::Whole), draw(Hinged::Bodied));
        assert_eq!(dark(&whole), 0, "a shut chest draws its inside");
        assert!(dark(&bodied) >= 4 * 5, "an open chest's inside is lit by the sky");
        assert!(bodied.iter().all(|v| (v.light() >> 4) & 0x0F <= 15 / 4 || v.light() & 0x0F != 0), "the inside takes the whole fire");
        let (low, high) = extent(&whole);
        let body = |x: f32| x / 16.0;
        assert!(low.y > -0.01 && (high.y - body(14.25)).abs() < 0.01, "a shut chest stands {low}..{high}");
    }

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

    /// Every number this file packs with is written down again in
    /// `shader.wgsl`, and the two copies have to agree.
    ///
    /// The roundtrip above proves the Rust half is self-consistent,
    /// which is exactly what it would still do if the shader had been
    /// left reading the old layout. That is not hypothetical: the
    /// texture coordinate moved out of `packed` into a word of its own
    /// with five bits an axis, and the shader had to grow `V_SHIFT`,
    /// `UV_MASK` and a whole new `@location` to follow it. A mismatch
    /// compiles, binds and draws the wrong picture on every block in
    /// the world.
    #[test]
    fn every_stage_of_weather_on_a_board_is_a_surface_tint_of_its_own_and_fits_the_byte() {
        use primitive_shared::types::{BLOCK_PEGGED_PLANKS, BLOCK_PLANKS};
        use primitive_shared::weathering::{weathered, STAGES};
        use primitive_shared::wildfire::with_soot;
        let mut seen = vec![surface_tint(with_soot(BLOCK_PLANKS, 1)), surface_tint(with_soot(BLOCK_PLANKS, 3))];
        for stage in 1..=STAGES {
            let tint = surface_tint(weathered(BLOCK_PEGGED_PLANKS, stage));
            assert!(tint.is_some_and(|code| code <= 255), "a board weathered to {stage} is drawn untinted");
            assert!(!seen.contains(&tint), "weather stage {stage} is drawn as something else's tint");
            seen.push(tint);
        }
        assert_eq!(surface_tint(BLOCK_PLANKS), None, "a new board is drawn weathered");
        // The shader has an arm for them: without it the codes fall into the
        // soot arm and a grey roof is drawn black.
        assert!(include_str!("shader.wgsl").contains("stage >= 5u"), "shader.wgsl does not draw weather");
    }

    #[test]
    fn a_tired_furrow_is_drawn_pale_and_not_as_a_dressed_one_or_a_rotten_board() {
        use primitive_shared::types::BLOCK_FARMLAND;
        use primitive_shared::wildfire::{after_harvest, dressed};
        let tired = after_harvest(BLOCK_FARMLAND).unwrap();
        assert_eq!(surface_tint(tired), Some(TIRED_FURROW_TINT));
        assert_ne!(surface_tint(dressed(BLOCK_FARMLAND).unwrap()), Some(TIRED_FURROW_TINT));
        assert_eq!(surface_tint(BLOCK_FARMLAND), None);
        let shader = include_str!("shader.wgsl");
        let arm = shader.find("stage == 10u").expect("shader.wgsl has no arm for a tired furrow");
        let weather = shader.find("stage >= 5u").expect("shader.wgsl lost the weather arm");
        assert!(arm < weather, "the weather arm comes first and draws a tired furrow as a rotten board");
    }

    #[test]
    fn the_chip_picture_in_the_shader_is_the_one_drawn_here() {
        // The drawing is sixteen rows of sixteen texels, in the four marks it
        // knows -- a row a character short would shift every mark after it
        // one texel left, and a stray letter would draw nothing at all.
        for (row, line) in CHIP_MARKS.iter().enumerate() {
            assert_eq!(line.len(), 16, "row {row} of the chip picture is {} texels", line.len());
            assert!(line.bytes().all(|b| b".#+o".contains(&b)), "row {row} has a mark the shader cannot draw");
        }
        let source = include_str!("shader.wgsl");
        let head = "var<private> CHIP_ROWS: array<u32, 16> = array<u32, 16>(";
        let start = source.find(head).expect("shader.wgsl has no CHIP_ROWS") + head.len();
        let body = &source[start..];
        let body = &body[..body.find(");").expect("CHIP_ROWS is never closed")];
        let shader: Vec<u32> = body
            .split(',')
            .map(|word| word.trim().trim_end_matches('u'))
            .filter(|word| !word.is_empty())
            .map(|word| word.parse().expect("CHIP_ROWS holds something that is not a number"))
            .collect();
        assert_eq!(shader, chip_rows().to_vec(), "the shader draws a different chip picture from CHIP_MARKS");
        // ...and it is read: the bit reaches the fragment and the fragment
        // asks for the picture.
        assert!(source.contains("(in.uv_cells & CHIPPED_BIT)"), "the vertex shader drops the chip bit");
        assert!(source.contains("chip_shade(in.uv)"), "nothing draws the chip picture");
    }

    #[test]
    fn only_the_face_a_pick_opened_wears_the_chip_marks() {
        use primitive_shared::dig;
        const AT: (i32, i32, i32) = (8, 4, 8);
        use super::transparency_tests::{cache_of, mesh_of};
        let alone = |block: BlockId| mesh_of(&cache_of(|x, y, z| if (x, y, z) == AT { block } else { BLOCK_AIR }));
        for side in [dig::Side::PosX, dig::Side::NegX, dig::Side::PosY, dig::Side::NegY, dig::Side::PosZ, dig::Side::NegZ] {
            let bitten = dig::next_bite(BLOCK_STONE, side).unwrap();
            // Alone in the air, so every face of the box is drawn.
            let mesh = alone(bitten);
            let chipped: Vec<&Vertex> = mesh.vertices.iter().filter(|v| v.uv & CHIPPED_BIT != 0).collect();
            assert_eq!(chipped.len(), 4, "{side:?}: {} corners chipped, not one face", chipped.len());
            let (axis, sign) = side.outward();
            let (min, max) = dig::bite_box(bitten).unwrap();
            let corner = [AT.0, AT.1, AT.2][axis] as f32;
            let plane = corner + if sign > 0 { max[axis] } else { min[axis] };
            for v in chipped {
                assert!(v.uv & FINE_UV_BIT != 0, "{side:?}: a chip mark without a fine coordinate");
                assert!(
                    (v.position[axis] - plane).abs() < 1e-4,
                    "{side:?}: the marks are on a face at {}, not the cut at {plane}",
                    v.position[axis]
                );
            }
        }
        assert!(alone(BLOCK_STONE).vertices.iter().all(|v| v.uv & CHIPPED_BIT == 0), "a whole block wears chip marks");
    }

    #[test]
    fn a_turf_lip_is_drawn_as_turf_at_its_own_height_and_never_as_a_cut() {
        // The generator's lip on a slope (`dig::is_turf_lip`): the meadow,
        // lowered, and not a face anybody opened.
        use primitive_shared::dig;
        const AT: (i32, i32, i32) = (8, 4, 8);
        use super::transparency_tests::{cache_of, mesh_of};
        for quarters in 1..dig::SLICES {
            let lip = dig::lowered(primitive_shared::types::BLOCK_GRASS, quarters);
            let mesh = mesh_of(&cache_of(|x, y, z| if (x, y, z) == AT { lip } else { BLOCK_AIR }));
            assert!(mesh.vertices.iter().all(|v| v.uv & CHIPPED_BIT == 0), "{quarters} quarters of turf wear chip marks");
            let top = AT.1 as f32 + f32::from(quarters) / f32::from(dig::SLICES);
            let highest = mesh.vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            assert!((highest - top).abs() < 1e-4, "{quarters} quarters of turf drawn up to {highest}, not {top}");
            // The grass is coloured by the climate it grew in, as the whole
            // turf's is: tinted, and so drawn as grass.
            assert!(
                mesh.vertices.iter().filter(|v| (v.position[1] - top).abs() < 1e-4).all(|v| v.tint() != 0),
                "the top of {quarters} quarters of turf is not the meadow's colour"
            );
        }
    }

    #[test]
    fn snow_on_a_lip_is_drawn_on_the_lips_top_and_the_grass_under_it_is_not() {
        // The cover is in the cell over the lip (`types::rest_drop`) and has
        // to be drawn where the lip's top is: from its own cell's floor it
        // hung over the turf with the green showing under it, and a turf top
        // left drawn under it would fight it for every pixel.
        use primitive_shared::dig;
        use primitive_shared::types::{BLOCK_ASH, BLOCK_GRASS, BLOCK_SNOW_COVER};
        use super::transparency_tests::{cache_of, mesh_of};
        const AT: (i32, i32, i32) = (8, 4, 8);
        for coating in [BLOCK_SNOW_COVER, BLOCK_ASH] {
            for quarters in 1..dig::SLICES {
                let lip = dig::lowered(BLOCK_GRASS, quarters);
                let over = (AT.0, AT.1 + 1, AT.2);
                let meshed = |on: BlockId| {
                    mesh_of(&cache_of(|x, y, z| match (x, y, z) {
                        c if c == AT => lip,
                        c if c == over => on,
                        _ => BLOCK_AIR,
                    }))
                };
                let (bare, mesh) = (meshed(BLOCK_AIR), meshed(coating));
                let top = AT.1 as f32 + f32::from(quarters) / f32::from(dig::SLICES);
                let at_top = |mesh: &MeshBuffers, tinted: bool| {
                    mesh.vertices.iter().filter(|v| (v.position[1] - top).abs() < 1e-4 && (v.tint() != 0) == tinted).count()
                };
                let highest = mesh.vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
                assert!((highest - top).abs() < 1e-4, "{coating} on {quarters} quarters drawn up to {highest}, not {top}");
                assert!(at_top(&mesh, false) >= 4, "no {coating} was drawn on the top of {quarters} quarters of turf");
                // The turf's sides reach the top too; what the coating takes
                // away is the four corners of the grass top, and only those.
                assert_eq!(
                    at_top(&mesh, true) + 4,
                    at_top(&bare, true),
                    "the grass top was drawn under the {coating} on {quarters} quarters"
                );
            }
        }
    }

    #[test]
    fn the_shader_unpacks_the_vertex_with_the_numbers_that_packed_it() {
        let source = include_str!("shader.wgsl");
        let declared = |name: &str| -> u32 {
            let needle = format!("const {name}: u32 = ");
            let at = source
                .find(&needle)
                .unwrap_or_else(|| panic!("shader.wgsl declares no {name}"))
                + needle.len();
            let digits: String = source[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
            digits
                .parse()
                .unwrap_or_else(|_| panic!("{name} in shader.wgsl is not a plain number"))
        };

        for (name, ours) in [
            ("LAYER_SHIFT", LAYER_SHIFT),
            ("LAYER_MASK", 0xFF),
            ("LAYER_HIGH_SHIFT", LAYER_HIGH_SHIFT),
            ("LAYER_TOP_SHIFT", LAYER_TOP_SHIFT),
            ("TINT_SHIFT", TINT_SHIFT),
            ("TINT_LEVELS", TINT_LEVELS),
            ("SURFACE_TINT_BASE", SURFACE_TINT_BASE),
            ("LIGHT_MASK", LIGHT_MASK),
            ("V_SHIFT", V_SHIFT),
            ("UV_MASK", UV_MASK),
            ("TRANSLUCENT_BIT", TRANSLUCENT_BIT),
            ("MOTTLED_BIT", MOTTLED_BIT),
            ("FINE_UV_BIT", FINE_UV_BIT),
            ("FINE_V_SHIFT", FINE_V_SHIFT),
            ("FINE_MASK", FINE_MASK),
            ("CHIPPED_BIT", CHIPPED_BIT),
        ] {
            assert_eq!(declared(name), ours, "{name} disagrees between mesh.rs and shader.wgsl");
        }

        // **The ninth bit has to be read, not merely declared.**
        // Deleting the shift from the expression leaves every constant
        // above agreeing and the shader quietly back on eight bits,
        // which draws layer 300 as layer 44 -- a real picture of
        // something else, in one place in the world.
        assert!(
            source.contains("LAYER_HIGH_SHIFT) & 1u) << 8u"),
            "shader.wgsl declares LAYER_HIGH_SHIFT but does not put it in the layer"
        );
        // ...and the tenth and eleventh, which live in the other word.
        assert!(
            source.contains("(((in.uv_cells >> LAYER_TOP_SHIFT) & 3u) << 9u)"),
            "shader.wgsl declares LAYER_TOP_SHIFT but does not put it in the layer"
        );

        // A fine coordinate is in 256ths of a picture on both sides, and
        // the scale is a float, which the reader above cannot parse. A
        // shader dividing by 16 would draw every model face sixteen times
        // over; one dividing by 4096, one sixteenth of the picture
        // stretched across it.
        assert!(
            source.contains(&format!("const FINE_UNITS: f32 = {FINE_UNITS:.1};")),
            "FINE_UNITS disagrees between mesh.rs and shader.wgsl"
        );
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
    fn a_fine_coordinate_survives_the_vertex_and_a_whole_cell_is_not_one() {
        // The packing half of `FINE_UV_BIT`: a place in the picture goes in
        // and comes back to a 256th, a box grown past its cell by `BITE`
        // clamps rather than wrapping to the far edge, and the plain
        // counted-cell coordinate is untouched -- a merged rectangle must
        // never read as a cut one.
        let v = Vertex::tinted([0.0; 3], [0.0; 2], 300, 0, 0).with_fine_uv([0.375, 0.8125]);
        assert_ne!(v.uv & FINE_UV_BIT, 0);
        assert_eq!(v.uv(), [0.375, 0.8125]);
        assert_eq!(v.tex_layer(), 300, "the coordinate disturbed the layer");
        let grown = Vertex::tinted([0.0; 3], [0.0; 2], 0, 0, 0).with_fine_uv([-0.001, 1.001]);
        assert_eq!(grown.uv()[0], 0.0, "a coordinate a hair below zero wrapped");
        let merged = Vertex::tinted([0.0; 3], [7.0, 31.0], 0, 0, 0);
        assert_eq!(merged.uv & FINE_UV_BIT, 0);
        assert_eq!(merged.uv(), [7.0, 31.0]);
        // The layer's top bits share the coordinate's word, and a cut
        // coordinate at its widest must leave them alone -- in both
        // directions.
        let widest = (FINE_MASK as f32) / FINE_UNITS;
        let high = Vertex::tinted([0.0; 3], [0.0; 2], MAX_TEXTURE_LAYERS - 1, 0, 0).with_fine_uv([widest, widest]);
        assert_eq!(high.tex_layer(), MAX_TEXTURE_LAYERS - 1, "a fine coordinate ate the layer's top bits");
        assert_eq!(high.uv(), [widest, widest], "the layer's top bits bled into a fine coordinate");
        let merged_high = Vertex::tinted([0.0; 3], [31.0, 31.0], 1536, 0, 0);
        assert_eq!(merged_high.uv(), [31.0, 31.0], "the layer's top bits bled into a counted coordinate");
        assert_eq!(merged_high.tex_layer(), 1536);
    }

    #[test]
    fn the_vertex_is_as_small_as_it_claims() {
        // The whole reason for the packing. A regression here is a
        // silent increase in GPU memory for the terrain, multiplied by
        // every vertex in a loaded world.
        //
        // Twenty rather than the sixteen this held for most of its life.
        // The four bytes are `Vertex::uv`, and they were bought
        // deliberately: merging coplanar faces needs a texture
        // coordinate that counts cells rather than names a corner, and
        // there was no room left in `packed` for it. The trade is
        // 25% more vertex bandwidth against roughly a third fewer
        // triangles, and it was made on the measurement that says the
        // solid pass is bound by chunks and triangles rather than by
        // bandwidth. See `MERGE_COPLANAR_FACES`.
        assert_eq!(std::mem::size_of::<Vertex>(), 20);
    }

    #[test]
    fn every_uv_a_single_face_produces_lands_on_a_cell_boundary() {
        // The packing assumes faces are mapped corner to corner, so a
        // coordinate is a whole number of cells. If `face_uv` ever
        // returned anything between 0 and 1, it would silently snap --
        // and a merged rectangle, which multiplies these by its width,
        // would snap to a different place along its length. See
        // `Vertex::uv`.
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

    /// How long the mesher takes on real terrain, in milliseconds.
    ///
    /// ```text
    /// cargo test -p primitive_client --release --lib how_long_meshing_takes     ///     -- --ignored --nocapture
    /// ```
    ///
    /// A tool, not a check: there is no number here that can fail, and a
    /// threshold would only measure the machine it last ran on. It
    /// exists because the merge key is the mesher's hot loop -- a sort
    /// over every visible face of a chunk -- and anything that changes
    /// the key's width or its layout has to be able to show what it
    /// cost. Release only; a debug mesher is a different program.
    #[test]
    #[ignore = "a tool: times the mesher on real terrain"]
    fn how_long_meshing_takes() {
        use crate::logic::chunk_manager::ChunkManager;
        use primitive_shared::lighting::LightMap;
        use primitive_shared::types::ChunkPos;

        let gen = primitive_shared::worldgen::WorldGen::new(1234);
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        // Eight by eight of generated world, which is a walk rather than
        // a corner: forest, meadow and a slope all get meshed.
        let positions: Vec<ChunkPos> =
            (-4..4).flat_map(|x| (-4..4).map(move |z| ChunkPos::new(x, z))).collect();
        let mut chunks = ChunkManager::new(16);
        for pos in &positions {
            chunks.insert(gen.generate_chunk(*pos));
        }
        let mut light = LightMap::new();
        for pos in &positions {
            light.load_chunk(&chunks, *pos);
        }

        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut cache = Neighbourhood::default();
            let mut out = MeshBuffers::default();
            let mut faces = 0usize;
            let start = std::time::Instant::now();
            for pos in &positions {
                cache.fill(*pos, &chunks, &light);
                build_mesh(*pos, &cache, &layers, &gen, &mut out);
                faces += out.vertices.len();
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            println!(
                "meshing {} chunks: {ms:.1} ms total, {:.2} ms a chunk, {faces} vertices",
                positions.len(),
                ms / positions.len() as f64
            );
            best = best.min(ms);
        }
        println!("best of three: {best:.1} ms");
    }

    #[test]
    fn packing_saturates_instead_of_corrupting_neighbouring_fields() {
        // A light level above 15 must not bleed into the block-light bits.
        let packed = pack_light(200, 0, 0, 0);
        assert_eq!(packed & 0xF, 15);
        assert_eq!((packed >> 4) & 0xF, 0);
    }

    /// **The 257th picture is the one that used to be invisible.**
    ///
    /// The layer was eight bits in the vertex and eight in the merge
    /// key, and both silently kept the low byte: layer 300 drew as layer
    /// 44, which is a real picture of something else. Nothing crashed
    /// and no test failed -- the world simply wore the wrong textures
    /// in one place, which is the hardest kind of fault to trace back to
    /// a number.
    ///
    /// Every neighbour is checked with it, because the ninth bit was
    /// taken from the hole the texture coordinate left and sits *below*
    /// the other eight rather than above them: a fault here shows up as
    /// a face that is lit wrong or drawn in the wrong pass, not as a
    /// wrong picture.
    #[test]
    fn a_layer_past_the_old_ceiling_of_two_hundred_and_fifty_six_survives_the_packing() {
        // The vertex.
        let light = pack_light(15, 9, 2, 5) | TRANSLUCENT_BIT | MOTTLED_BIT;
        for layer in [0, 1, 255, 256, 257, 511, 512, 1023, 1024, 1536, MAX_TEXTURE_LAYERS - 1] {
            let v = Vertex::tinted([1.0, 2.0, 3.0], [4.0, 5.0], layer, light, 233);
            assert_eq!(v.tex_layer(), layer, "layer {layer} did not survive the vertex");
            assert_eq!(v.tint(), 233, "layer {layer} corrupted the tint");
            assert_eq!(v.light(), light & LIGHT_MASK, "layer {layer} corrupted the light");
            assert_eq!(v.uv(), [4.0, 5.0], "layer {layer} corrupted the coordinate");
            assert_ne!(v.packed & MOTTLED_BIT, 0, "layer {layer} cleared the block-face flag");
        }

        // The merge key, packed as `mergeable` packs it and taken apart
        // as `emit_merged` takes it apart.
        for layer in [0u32, 255, 256, 511, 512, 1024, MAX_TEXTURE_LAYERS - 1] {
            let key = pack_light(15, 9, 2, 5) as u64
                | ((layer as u64) << KEY_LAYER_SHIFT)
                | ((250u64 & 0xFF) << KEY_TINT_SHIFT)
                | KEY_CUTOUT
                | KEY_UNMOTTLED;
            assert_eq!(
                ((key >> KEY_LAYER_SHIFT) & KEY_LAYER_MASK) as u32,
                layer,
                "layer {layer} did not survive the merge key"
            );
            assert_eq!(((key >> KEY_TINT_SHIFT) & 0xFF) as u32, 250);
            assert_eq!((key & LIGHT_MASK as u64) as u32, pack_light(15, 9, 2, 5));
            assert_ne!(key & KEY_CUTOUT, 0, "layer {layer} ate the cutout flag");
            assert_ne!(key & KEY_UNMOTTLED, 0, "layer {layer} ate the one-sheet flag");
        }
    }

    /// **The picture and the support rule are one answer, and this is
    /// the line where they are made to agree.**
    ///
    /// A bracket fungus is the only block in the game held up from the
    /// side, and two separate pieces of arithmetic decide which side:
    /// `types::support_at` turns the facing into the cell that holds
    /// it, and `push_box` turns the same facing into which wall the
    /// shelf grows from. They rotate in opposite senses -- the enum
    /// counts anticlockwise from north, the box rotation carries a
    /// point the other way -- so getting them to match was not a matter
    /// of writing the same expression twice.
    ///
    /// Wrong, this is a shelf standing in mid-air with the trunk behind
    /// its back: it draws, it drops when the trunk goes, every test
    /// about support passes, and it looks like a mistake in the world
    /// generator.
    #[test]
    fn a_bracket_grows_out_of_the_wall_that_holds_it_up_whichever_way_it_faces() {
        use primitive_shared::types::{
            faced, support_at, Facing, BLOCK_BRACKET_FUNGUS,
        };
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let block = faced(BLOCK_BRACKET_FUNGUS, facing);
            let (dx, dy, dz) = support_at(block);
            assert_eq!(dy, 0, "a bracket is held sideways, not from below");

            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            bracket_block(
                [0.0, 0.0, 0.0],
                block,
                &crate::engine::texture::FaceLayers::empty_for_test(),
                0xFF,
                &mut vertices,
                &mut indices,
            );
            assert!(!vertices.is_empty(), "{facing:?} drew nothing");

            let axis = if dx != 0 { 0 } else { 2 };
            let toward = if dx != 0 { dx } else { dz };
            // The tolerance is the seam bite and a hair: every box is
            // grown by `BITE` sixteenths so its buried faces sit inside
            // their neighbours rather than fighting them, and a shelf
            // against the cell wall therefore pokes exactly that far out
            // of it. See `push_box`.
            let slack = BITE * T + 1e-4;
            let (mut low, mut high) = (f32::MAX, f32::MIN);
            for vertex in &vertices {
                low = low.min(vertex.position[axis]);
                high = high.max(vertex.position[axis]);
                // Nothing leaves the cell on the other two axes either:
                // a shelf poking through the trunk's neighbour is a
                // shelf growing inside somebody's wall.
                // The tolerance is the seam bite and a hair: every box
                // is grown by `BITE` sixteenths so its buried faces sit
                // inside their neighbours rather than fighting them, and
                // a shelf against the cell wall therefore pokes exactly
                // that far out of it. See `push_box`.
                for other in [0usize, 1, 2] {
                    assert!(
                        (-slack..=1.0 + slack).contains(&vertex.position[other]),
                        "{facing:?} put a corner at {:?}",
                        vertex.position
                    );
                }
            }
            // Touching the wall it hangs on...
            let (against, away) = if toward < 0 { (low, high) } else { (high, low) };
            let wall = if toward < 0 { 0.0 } else { 1.0 };
            // Touching means *at least* touching: the seam bite pushes
            // the back face a fraction of a texel into the trunk, which
            // is what stops the two flickering against each other. What
            // would be wrong is a gap.
            assert!(
                (against - wall).abs() < slack,
                "{facing:?} should touch the wall at {wall} and reaches {against}"
            );
            // ...and standing clear of the opposite one, or it is a
            // slab filling the cell rather than a shelf on a trunk.
            assert!(
                (away - wall).abs() > 0.25,
                "{facing:?} spans the whole cell: {low}..{high}"
            );
        }
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
        face_visible(current, cover_of(current), neighbor, cover_of(neighbor), face, false)
    }

    /// ...and the same question asked of a chunk whose crowns are shells:
    /// one `lod::coarsen` rewrote, or one past the see-through canopy line.
    fn shows_face_at_range(current: BlockId, neighbor: BlockId, face: usize) -> bool {
        face_visible(current, cover_of(current), neighbor, cover_of(neighbor), face, true)
    }

    #[test]
    fn a_distant_crown_is_a_shell_and_a_near_one_is_not() {
        // **The canopy was the quarter of the world's triangles that the
        // detail setting never touched**: 932k at full detail and 897k at
        // both coarse levels, while the ground under it halved. Near, the
        // faces between two leaf cells are the inside of a tree somebody is
        // standing under; at range they are geometry behind geometry.
        use primitive_shared::types::{BLOCK_BIRCH_LEAVES, BLOCK_LEAVES};
        for (a, b) in [(BLOCK_LEAVES, BLOCK_LEAVES), (BLOCK_LEAVES, BLOCK_BIRCH_LEAVES)] {
            assert_eq!(
                (0..6).filter(|&f| shows_face(a, b, f)).count(),
                3,
                "near, leaves against leaves draw the shared face exactly once"
            );
            assert_eq!(
                (0..6).filter(|&f| shows_face_at_range(a, b, f)).count(),
                0,
                "at range, the inside of a crown is still being drawn"
            );
        }
        // ...and the outside of the crown is untouched: the shell is every
        // leaf face that looks at open air, on all six sides. (Against rock
        // there is nothing to draw either way -- the rock covers it.)
        for face in 0..6 {
            assert!(shows_face_at_range(BLOCK_LEAVES, BLOCK_AIR, face), "a distant crown lost its own surface at face {face}");
        }
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
    fn an_apple_among_plain_leaves_shares_each_face_with_them_exactly_once() {
        // The fruiting cell, the picked one and the plain canopy are three
        // ids, and an apple tree is all three side by side. Between any two
        // of them -- and between two woods' crowns -- exactly one of the
        // pair draws the face, as it is between two identical leaves.
        use primitive_shared::types::{
            BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_FRUIT, BLOCK_APPLE_LEAVES_PICKED, BLOCK_LEAVES,
            BLOCK_MAPLE_LEAVES,
        };
        for (a, b) in [
            (BLOCK_APPLE_LEAVES_FRUIT, BLOCK_APPLE_LEAVES),
            (BLOCK_APPLE_LEAVES_PICKED, BLOCK_APPLE_LEAVES),
            (BLOCK_APPLE_LEAVES_FRUIT, BLOCK_APPLE_LEAVES_PICKED),
            (BLOCK_LEAVES, BLOCK_MAPLE_LEAVES),
        ] {
            for face in 0..6 {
                let opposite = face ^ 1;
                let mine = shows_face(a, b, face);
                let theirs = shows_face(b, a, opposite);
                assert!(
                    mine ^ theirs,
                    "{a:#x} face {face} and {b:#x} face {opposite} both {}",
                    if mine { "draw" } else { "skip" }
                );
            }
        }
        // ...and a solid block against the fruit still draws its own face.
        assert_eq!(faces_drawn(BLOCK_STONE, BLOCK_APPLE_LEAVES_FRUIT), 6);
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

    /// The quads a pit kiln is drawn with, as boxes' tops: the height of
    /// every upward face, sorted.
    fn pit_tops(block: BlockId, pieces: Option<&[BlockId]>) -> Vec<(i32, i32, i32)> {
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        pit_kiln_block([0.0; 3], block, pieces, &layers, 0xFF, &mut vertices, &mut indices);
        let mut tops: Vec<(i32, i32, i32)> = vertices
            .chunks_exact(4)
            .filter(|quad| quad.iter().all(|v| (v.position[1] - quad[0].position[1]).abs() < 1e-4))
            .map(|quad| {
                let (lo, hi) = quad.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.position[0]), hi.max(v.position[0])));
                ((quad[0].position[1] * 16.0).round() as i32, (lo * 16.0).round() as i32, (hi * 16.0).round() as i32)
            })
            .collect();
        tops.sort_unstable();
        tops
    }

    #[test]
    fn a_brick_and_a_mould_in_a_pit_are_drawn_as_themselves_and_not_as_pots() {
        // "Placing a brick or a mould looks like jugs." The pit's id says two
        // pieces, unfired; what they are comes beside it, and each is drawn
        // as its own shape -- no brick stands as tall as a pot, and a mould
        // is a tray with a rim.
        use primitive_shared::pit::Stage;
        use primitive_shared::types::{BLOCK_BRICK_RAW, BLOCK_MOULD_RAW, BLOCK_VESSEL_RAW};
        let two = Stage::Pottery { pieces: 2, fired: false }.block();
        let pots = pit_tops(two, None);
        let same_pots = pit_tops(two, Some(&[BLOCK_VESSEL_RAW, BLOCK_VESSEL_RAW]));
        assert_eq!(pots, same_pots, "a pit told it holds pots draws something else than the pots it drew");
        let brick_and_mould = pit_tops(two, Some(&[BLOCK_BRICK_RAW, BLOCK_MOULD_RAW]));
        assert_ne!(brick_and_mould, pots, "a brick and a mould are still drawn as pots");
        let tallest = brick_and_mould.iter().map(|&(y, _, _)| y).max().unwrap();
        assert!(tallest < 7, "something in a pit of a brick and a mould stands {tallest} sixteenths tall");
    }

    #[test]
    fn fibre_over_the_pottery_hides_only_what_it_lies_on() {
        // "Placing straw deletes all elements." One armful goes over the
        // first piece's quarter, five sixteenths deep: a pot still shows
        // its top over it, and the second piece, in a quarter with no
        // fibre on it yet, shows whole.
        use primitive_shared::pit::Stage;
        use primitive_shared::types::BLOCK_VESSEL_RAW;
        let one_armful = Stage::Fibre(1).block();
        let tops = pit_tops(one_armful, Some(&[BLOCK_VESSEL_RAW, BLOCK_VESSEL_RAW]));
        let pot_tops = tops.iter().filter(|&&(y, _, _)| y == 7).count();
        assert!(pot_tops >= 2, "fibre in one quarter took the pottery out of the pit ({pot_tops} pot tops drawn)");
        // ...and a pit nobody has been told about draws the fibre alone,
        // as it always did.
        assert!(pit_tops(one_armful, None).iter().all(|&(y, _, _)| y != 7));
    }

    #[test]
    fn a_campfire_with_a_block_laid_over_it_is_not_drawn_black() {
        // The report: "if a block stands above a campfire it turns black".
        // The hearth's top is inside its own cell and was lit from the
        // block above it, which is solid and holds no light at all. The
        // fire lights its own cell at thirteen; its top has to show that.
        use primitive_shared::types::{BLOCK_CAMPFIRE_LIT, BLOCK_PLANKS, BLOCK_STONE};
        let pos = ChunkPos::new(0, 0);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..16 {
            for x in 0..16 {
                blocks[Chunk::index(x, 8, z)] = BLOCK_STONE;
            }
        }
        blocks[Chunk::index(8, 9, 8)] = BLOCK_CAMPFIRE_LIT;
        blocks[Chunk::index(8, 10, 8)] = BLOCK_PLANKS;
        let mut chunks = ChunkManager::new(4);
        chunks.insert(Chunk { pos, blocks });
        let mut light = LightMap::new();
        light.load_chunk(&chunks, pos);
        let out = mesh_it(&chunks, &light, pos);
        let mut tops = 0;
        for quad in out.vertices.chunks_exact(4) {
            let flat_at = |y: f32| quad.iter().all(|v| (v.position[1] - y).abs() < 1e-3);
            let over_the_fire = quad.iter().all(|v| (7.9..=9.1).contains(&v.position[0]) && (7.9..=9.1).contains(&v.position[2]));
            if !(flat_at(9.25) && over_the_fire && (quad[0].light() >> 10) & 7 == 0) {
                continue;
            }
            tops += 1;
            for v in quad {
                let (sky, fire) = (v.light() & 15, (v.light() >> 4) & 15);
                assert!(fire >= 10, "the top of a lit campfire under planks is lit {fire} by its own flame");
                assert!(sky > 0, "the top of a campfire under planks gets no daylight from the open sides");
            }
        }
        assert_eq!(tops, 1, "the fixture drew {tops} campfire tops");
    }

    #[test]
    fn a_rack_is_a_frame_and_a_loaded_one_has_a_skin_in_it() {
        // The model, checked for the three things a picture cannot be
        // asked to prove: that it is not a cube, that the skin is extra
        // geometry rather than a different texture on the same box, and
        // that all of it stays inside the cell it belongs to. The last
        // one matters more than it sounds -- geometry that leaves its
        // cell is geometry the mesher's own neighbour culling has
        // already decided is not there.
        use primitive_shared::types::{faced, rack_with_hide, Facing, BLOCK_DRYING_RACK};

        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let empty = faced(BLOCK_DRYING_RACK, Facing::North);
        let build = |block| {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            rack_block([0.0, 0.0, 0.0], block, RackColumns::Lone, &layers, 0xFF, &mut v, &mut i);
            (v, i)
        };

        let (bare, bare_indices) = build(empty);
        let (loaded, _) = build(rack_with_hide(empty, true));
        assert_eq!(
            bare.len(),
            crate::logic::models::prop(crate::logic::models::Prop::DryingRack).iter().filter(|b| !b.loaded).count() * 24,
            "six faces a box"
        );
        assert_eq!(bare_indices.len(), bare.len() / 4 * 6);
        assert_eq!(
            loaded.len(),
            bare.len() + 24,
            "the skin should be one box more, not a different texture"
        );
        let slack = BITE * T + 1e-4; // the seam bite; see `push_box`
        for v in &loaded {
            for axis in 0..3 {
                assert!(
                    (-slack..=1.0 + slack).contains(&v.position[axis]),
                    "the model left its cell: {:?}",
                    v.position
                );
            }
        }
    }

    #[test]
    fn a_rack_turns_with_its_facing() {
        // It has a front, so the same numbers have to come out four
        // ways -- and the turn is about the middle of the cell, or a
        // rack placed facing east would stand in its neighbour's square.
        use primitive_shared::types::{faced, Facing, BLOCK_DRYING_RACK};

        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let corners = |facing| {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            rack_block(
                [0.0, 0.0, 0.0],
                faced(BLOCK_DRYING_RACK, facing),
                RackColumns::Lone,
                &layers,
                0xFF,
                &mut v,
                &mut i,
            );
            let mut span = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];
            for vertex in &v {
                span[0] = span[0].min(vertex.position[0]);
                span[1] = span[1].max(vertex.position[0]);
                span[2] = span[2].min(vertex.position[2]);
                span[3] = span[3].max(vertex.position[2]);
            }
            span
        };
        let north = corners(Facing::North);
        let east = corners(Facing::East);
        // A frame is wide one way and thin the other, so a quarter turn
        // swaps the two.
        assert!(north[1] - north[0] > north[3] - north[2], "the frame is not flat");
        assert!(
            (east[3] - east[2] - (north[1] - north[0])).abs() < 1e-5,
            "a quarter turn did not swap the frame's width and depth"
        );
        // ...and it is still standing in its own cell.
        for span in [north, east] {
            assert!(span[0] >= 0.0 && span[1] <= 1.0 && span[2] >= 0.0 && span[3] <= 1.0);
        }
    }

    /// Every model that turns looks the same from where its placer stood,
    /// whichever way they were looking.
    ///
    /// **The mirror this exists for.** `push_box` turns the other way from
    /// `Facing`, so a model turned by `Facing::quarters` is right facing
    /// north and south and mirrored facing east and west -- the rack was,
    /// by the half sixteenth its frame sits off the middle of its cell. So
    /// each model is put down four times through `types::placed`, as a
    /// player looking along each axis would put it, and each drawing is
    /// turned back into that player's own frame: the four must be one
    /// drawing.
    ///
    /// Not the bracket fungus, whose direction is its trunk's and not its
    /// placer's -- see
    /// `a_bracket_grows_out_of_the_wall_that_holds_it_up_whichever_way_it_faces`.
    #[test]
    fn every_turned_model_looks_the_same_from_where_its_placer_stood() {
        use primitive_shared::types::{
            faced, has_front, is_placeable, placed, support_at, Facing, ALL_BLOCK_IDS,
        };
        let mut checked = 0;
        for &(id, name) in ALL_BLOCK_IDS {
            if !has_front(id) || !is_placeable(id) || !drawn_as_model(id) {
                continue;
            }
            if support_at(faced(id, Facing::North)).1 == 0 {
                continue;
            }
            checked += 1;
            let mut first = None;
            for turn in 0..4u32 {
                let yaw = turn as f32 * std::f32::consts::FRAC_PI_2;
                // `turn` of `look_of`'s quarter turns take this placer's
                // line of sight round to the first one's, +x.
                let seen = look_of(placed(id, yaw, (0, 1, 0)), (8, 4, 8), turn);
                match &first {
                    None => first = Some(seen),
                    Some(first) => assert!(
                        *first == seen,
                        "{name} put down looking along {yaw} is not, from where its placer stood, \
                         the {name} put down looking along +x"
                    ),
                }
            }
        }
        assert!(checked >= 6, "only {checked} turned models were looked at");
    }

    /// A turned chest has its hasp on the side it faces, through the whole
    /// mesher.
    ///
    /// **It used to ask which picture that side wore**, when a chest was a
    /// cube with a painting of a lock on its north face. The chest is a model
    /// now (`models::Prop::Chest`), and the lock is a box: the one thing that
    /// stands a quarter sixteenth proud of the front, in the middle, where the
    /// back has only the two straps either side. So the question is where that
    /// box went.
    #[test]
    fn the_mesher_puts_a_turned_chests_hasp_on_the_side_it_faces() {
        use crate::engine::texture::FaceLayers;
        use primitive_shared::types::{faced, Facing, BLOCK_CHEST};
        let layers = FaceLayers::empty_for_test();
        let proud = 1.25 / 16.0;
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let cache = super::plant_tests::cache_of(|x, y, z| {
                if (x, y, z) == (8, 4, 8) {
                    faced(BLOCK_CHEST, facing)
                } else {
                    BLOCK_AIR
                }
            });
            let mut mesh = MeshBuffers::default();
            build_mesh(
                ChunkPos::new(0, 0),
                &cache,
                &layers,
                &primitive_shared::worldgen::WorldGen::new(0),
                &mut mesh,
            );
            let (dx, dz) = facing.step();
            // Across the front, a sixteenth and a half either side of the
            // middle: the hasp is two wide, the straps start three and a half
            // out.
            let hasp_at = |p: [f32; 3], toward: i32| {
                let (along, across, step) = if dx != 0 { (p[0], p[2], dx) } else { (p[2], p[0], dz) };
                let plane = 8.5 + (0.5 - proud) * (step * toward) as f32;
                (along - plane).abs() < 0.006 && (across - 8.5).abs() < 1.5 / 16.0
            };
            let front = mesh.vertices.iter().filter(|v| hasp_at(v.position, 1)).count();
            let back = mesh.vertices.iter().filter(|v| hasp_at(v.position, -1)).count();
            assert!(front > 0, "a chest facing {facing:?} has no hasp on the side it faces");
            assert_eq!(back, 0, "a chest facing {facing:?} has its hasp on its back");
        }
    }

    /// **The lid is up while somebody has the chest open and down after**,
    /// and it is never drawn twice or not at all.
    ///
    /// Three pictures of the same chest: the one in the chunk mesh with
    /// nobody at it, the same chunk while the frame has the lid
    /// (`Hinged::Bodied`), and the lid the frame draws at the two ends of
    /// its swing. A shut lid lies inside the cell; an open one stands up out
    /// of it (`LID_OPEN`), which is what "open" looks like from across a
    /// room and is a thing no measurement of the body can be confused with.
    #[test]
    fn a_chest_somebody_has_open_has_its_lid_up_and_a_shut_one_has_it_down() {
        use crate::engine::texture::FaceLayers;
        use primitive_shared::types::{faced, Facing, BLOCK_CHEST};
        const AT: (i32, i32, i32) = (8, 4, 8);
        let layers = FaceLayers::empty_for_test();
        let world = primitive_shared::worldgen::WorldGen::new(0);
        let chest = faced(BLOCK_CHEST, Facing::South);
        let mut cache = super::plant_tests::cache_of(|x, y, z| if (x, y, z) == AT { chest } else { BLOCK_AIR });

        // The lid's underside is at 9.5 sixteenths and nothing else on the
        // chest reaches it: the body stops there, the corner posts and the
        // staple below it.
        let lid_line = AT.1 as f32 + LID_HINGE[0] + 0.01;
        let above = |mesh: &MeshBuffers| mesh.vertices.iter().filter(|v| v.position[1] > lid_line).count();

        let mut whole = MeshBuffers::default();
        build_mesh(ChunkPos::new(0, 0), &cache, &layers, &world, &mut whole);
        assert!(above(&whole) > 0, "a chest nobody is at has no lid in the chunk mesh");

        cache.set_swung_lids(vec![AT]);
        let mut bodied = MeshBuffers::default();
        build_mesh(ChunkPos::new(0, 0), &cache, &layers, &world, &mut bodied);
        assert_eq!(above(&bodied), 0, "the lid is in the chunk mesh as well as in the frame's hands");
        assert!(
            bodied.vertices.len() < whole.vertices.len(),
            "leaving the lid out took nothing out of the mesh"
        );

        // ...and the frame's lid, at both ends of the swing. Drawn in the
        // cell's own space, as the mesher's is.
        for (angle, standing) in [(0.0, false), (LID_OPEN, true)] {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            chest_lid_block([0.0, 0.0, 0.0], chest, angle, &layers, 0xFF, &mut v, &mut i);
            assert!(!v.is_empty(), "the lid was not drawn at all at {angle} radians");
            let top = v.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            assert_eq!(
                top > 1.0,
                standing,
                "a lid at {angle} radians reaches {top} of a cell, which is the wrong way up"
            );
        }
    }

    #[test]
    fn a_part_height_side_wears_a_strip_and_a_full_block_does_not() {
        // The smeared-campfire bug: a quarter-height side held all
        // thirty-two rows of its picture, so past a few paces the
        // squeezed axis dropped into minification and the whole face
        // smeared. The side now carries a crop cut to the block's
        // height; the stone under it, being a full cube, carries none.
        use primitive_shared::types::BLOCK_CAMPFIRE;
        let pos = ChunkPos::new(0, 0);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        blocks[Chunk::index(4, 8, 4)] = primitive_shared::types::BLOCK_STONE;
        blocks[Chunk::index(4, 9, 4)] = BLOCK_CAMPFIRE;
        let mut chunks = ChunkManager::new(4);
        chunks.insert(Chunk { pos, blocks });
        let mut light = LightMap::new();
        light.load_chunk(&chunks, pos);
        let out = mesh_it(&chunks, &light, pos);

        // By quads, not by vertices: the stone's side reaches y 9 too,
        // so a corner alone cannot say whose face it is. A whole quad
        // can -- the fire's sides live entirely inside y 9..9.25 and
        // the stone's span a full block.
        let mut fire_side_quads = 0;
        let mut stone_side_quads = 0;
        for quad in out.vertices.chunks_exact(4) {
            let face = (quad[0].light() >> 10) & 7;
            if face < 2 {
                continue;
            }
            let lo = quad.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let hi = quad.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            if lo >= 9.0 - 1e-4 && hi <= 9.3 {
                fire_side_quads += 1;
                // Cut to exactly its height: the tallest coordinate on the
                // side is the side's height in pictures, a quarter, and not
                // the whole picture squeezed in -- nor the power of two a
                // crop code would have snapped a height to.
                let tallest = quad.iter().map(|v| v.uv()[1]).fold(0.0, f32::max);
                for v in quad {
                    assert_ne!(
                        v.uv & FINE_UV_BIT,
                        0,
                        "a quarter-height side wears the whole picture squeezed in"
                    );
                }
                assert!(
                    (tallest - (hi - lo)).abs() < 2.0 / FINE_UNITS,
                    "a side {} tall wears {tallest} of its picture",
                    hi - lo
                );
            } else if hi <= 9.0 + 1e-4 {
                stone_side_quads += 1;
                for v in quad {
                    assert_eq!(v.uv & FINE_UV_BIT, 0, "a full cube's side got cut");
                }
            }
        }
        assert!(fire_side_quads >= 4, "the fixture drew {fire_side_quads} campfire sides");
        assert!(stone_side_quads >= 4, "the fixture drew {stone_side_quads} stone sides");
    }

    /// Every quad a model emitter drew, with the model and a name for the
    /// failure message: the furniture in all its facings, and every other
    /// model built out of `push_box`.
    fn every_box_model() -> Vec<(String, Vec<Vertex>)> {
        use primitive_shared::body::Water;
        use primitive_shared::types::{
            barrel_of, bed_half_of, block_name, faced, rack_with_hide, Facing, BLOCK_BED,
            BLOCK_BRACKET_FUNGUS, BLOCK_DRYING_RACK, BLOCK_JUG, BLOCK_NEST_EGGS, BLOCK_STOOL,
            BLOCK_STRAW_BED, BLOCK_TABLE,
        };
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let facings = [Facing::North, Facing::East, Facing::South, Facing::West];
        let mut models = Vec::new();
        let mut add = |name: String, draw: &dyn Fn(&mut Vec<Vertex>, &mut Vec<u32>)| {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            draw(&mut v, &mut i);
            assert!(!v.is_empty(), "{name} drew nothing");
            models.push((name, v));
        };
        for facing in facings {
            for kind in [BLOCK_BED, BLOCK_STRAW_BED] {
                for head in [false, true] {
                    for partnered in [false, true] {
                        add(format!("{} {facing:?} head={head} partnered={partnered}", block_name(kind)), &|v, i| {
                            furniture_block([0.0; 3], bed_half_of(kind, facing, head), partnered, &layers, 0xFF, v, i)
                        });
                    }
                }
            }
            for loaded in [false, true] {
                add(format!("rack {facing:?} loaded={loaded}"), &|v, i| {
                    rack_block([0.0; 3], rack_with_hide(faced(BLOCK_DRYING_RACK, facing), loaded), RackColumns::Lone, &layers, 0xFF, v, i)
                });
            }
            add(format!("bracket {facing:?}"), &|v, i| {
                bracket_block([0.0; 3], faced(BLOCK_BRACKET_FUNGUS, facing), &layers, 0xFF, v, i)
            });
            add(format!("chair {facing:?}"), &|v, i| {
                furniture_block([0.0; 3], faced(primitive_shared::types::BLOCK_CHAIR, facing), false, &layers, 0xFF, v, i)
            });
        }
        for block in [BLOCK_STOOL, BLOCK_TABLE] {
            add(primitive_shared::types::block_name(block).to_string(), &|v, i| {
                furniture_block([0.0; 3], block, false, &layers, 0xFF, v, i)
            });
        }
        add("jug".into(), &|v, i| jug_block([0.0; 3], BLOCK_JUG, &layers, 0xFF, v, i));
        add("nest".into(), &|v, i| nest_block([0.0; 3], BLOCK_NEST_EGGS, &layers, 0xFF, v, i));
        add("barrel".into(), &|v, i| barrel_block([0.0; 3], barrel_of(Water::Fresh, 5), &layers, 0xFF, v, i));
        for logs in 1..=primitive_shared::pit::PILE_LOGS_MAX {
            add(format!("log pile of {logs}"), &|v, i| {
                log_pile_block([0.0; 3], primitive_shared::pit::log_pile(logs), &layers, 0xFF, v, i)
            });
        }
        models
    }

    /// How many quads one log of a pile is drawn with: the core's six and
    /// the two slabs' five each (`log_pile_block`).
    const QUADS_A_PILED_LOG: usize = 16;

    #[test]
    fn a_log_pile_is_drawn_exactly_where_it_is_walked_into() {
        // **Drawn = collided, log by log.** A pile used to be a cube drawn
        // and a cube collided; now it is its logs in both
        // (`pit::pile_log_boxes`), and the two have to stay one list. If the
        // drawing moved a log and the collider did not, a player would stand
        // on air beside a log or walk into one. So: as many logs drawn as the
        // pile holds, and each log's quads fill exactly the box a body
        // walks into -- no further out than the hair every model box is
        // grown by, and no shorter.
        use primitive_shared::pit::{log_pile, pile_log_boxes, PILE_LOGS_MAX};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        for logs in 1..=PILE_LOGS_MAX {
            let block = log_pile(logs);
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            log_pile_block([0.0; 3], block, &layers, 0xFF, &mut vertices, &mut indices);
            let boxes: Vec<_> = pile_log_boxes(block).collect();
            assert_eq!(boxes.len(), usize::from(logs));
            assert_eq!(vertices.len(), boxes.len() * QUADS_A_PILED_LOG * 4, "a pile of {logs} is not {logs} logs drawn");
            for (drawn, (from, to)) in vertices.chunks_exact(QUADS_A_PILED_LOG * 4).zip(&boxes) {
                let lo = drawn.iter().fold([f32::MAX; 3], |m, v| std::array::from_fn(|k| m[k].min(v.position[k])));
                let hi = drawn.iter().fold([f32::MIN; 3], |m, v| std::array::from_fn(|k| m[k].max(v.position[k])));
                for k in 0..3 {
                    let hair = BITE * T + 1e-5;
                    assert!(
                        (lo[k] - from[k]).abs() <= hair && (hi[k] - to[k]).abs() <= hair,
                        "a pile of {logs}: a log drawn over {lo:?}..{hi:?} is walked into as {from:?}..{to:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_piled_log_shows_its_ends_north_and_south_and_its_bark_elsewhere_in_the_piles_own_wood() {
        // The pile's wood is in its id (`types::furniture_wood`), set by the
        // server from the logs that went in: a birch pile drawn in oak is
        // the report that put the wood there ("в стопке полен непонятно что
        // за дерево"). And the ends are the faces the logs' length points
        // along -- a swing that laid the log one way and lit or dressed it
        // another would put rings on its flank.
        use primitive_shared::pit::log_pile;
        use primitive_shared::types::in_wood;
        use primitive_shared::wood::WOODS;
        // The numbered fixture numbers the first 512 ids; a wood whose log
        // lies past them comes back as picture nought on every face, and is
        // left out rather than passed. Oak and birch, at least, are in it.
        let textures = crate::engine::texture::FaceLayers::numbered_for_test();
        let mut asked = 0;
        for (wood, kind) in WOODS.iter().enumerate() {
            let (end, bark) = (textures.layer_for_face(kind.log, crate::engine::texture::FACE_TOP), textures.layer_for_face(kind.log, 2));
            if end == bark {
                continue;
            }
            asked += 1;
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            log_pile_block([0.0; 3], in_wood(log_pile(8), wood), &textures, 0xFF, &mut vertices, &mut indices);
            for quad in vertices.chunks_exact(4) {
                let face = ((quad[0].light() >> 10) & 7) as usize;
                let want = if face >= 4 { end } else { bark };
                assert_eq!(quad[0].tex_layer(), want, "wood {wood}: face {face} of a log wears the wrong picture");
            }
        }
        assert!(asked >= 2, "only {asked} woods could be told apart");
    }

    #[test]
    fn every_model_face_wears_its_picture_at_one_texel_to_a_sixteenth() {
        // **The bug this exists for.** The player: "не растягивай не
        // сжимай текстуры просто обрезай". A model's small faces used to
        // be told their size in a crop code that could only say 1, 2, 4,
        // 8 or 16 sixteenths, so a bed's six-sixteenth sides wore eight
        // rows of board, a barrel's fourteen-sixteenth walls wore the
        // whole picture, and a rack's hide was the picture squeezed onto a
        // slab. So the property, on every quad of every box model: along
        // each edge, as far across the picture as along the world, one
        // picture to one block -- which is one texel to one sixteenth.
        for (name, vertices) in every_box_model() {
            for quad in vertices.chunks_exact(4) {
                for v in quad {
                    assert_ne!(v.uv & FINE_UV_BIT, 0, "{name}: a face wears a whole picture, not a cut of one");
                }
                for (a, b) in [(0, 1), (1, 2)] {
                    let world = glam::Vec3::from(quad[b].position).distance(glam::Vec3::from(quad[a].position));
                    let [ua, va] = quad[a].uv();
                    let [ub, vb] = quad[b].uv();
                    let picture = ((ub - ua).powi(2) + (vb - va).powi(2)).sqrt();
                    // A 256th of a picture of rounding at each end, and the
                    // `BITE` a box is grown by, which the texture does not.
                    let slack = 2.0 / FINE_UNITS + 2.0 * BITE / 16.0;
                    assert!(
                        (picture - world).abs() <= slack,
                        "{name}: an edge {world:.4} blocks long wears {picture:.4} of its picture"
                    );
                }
            }
        }
    }

    #[test]
    fn every_face_of_the_furniture_is_wound_to_face_outward() {
        // The rack's inside-out bug (see the test below) for every model
        // `push_box` builds, the furniture above all: a bed turned four
        // ways is four chances to wind a face backwards or light it as the
        // face behind it.
        for (name, vertices) in every_box_model() {
            for quad in vertices.chunks_exact(4) {
                let p = |k: usize| glam::Vec3::from_array(quad[k].position);
                let wound = (p(1) - p(0)).cross(p(2) - p(1)).normalize();
                let declared = [
                    glam::Vec3::Y,
                    glam::Vec3::NEG_Y,
                    glam::Vec3::X,
                    glam::Vec3::NEG_X,
                    glam::Vec3::Z,
                    glam::Vec3::NEG_Z,
                ][((quad[0].light() >> 10) & 7) as usize];
                assert!(wound.dot(declared) > 0.99, "{name}: a face wound {wound:?} is lit as {declared:?}");
            }
        }
    }

    #[test]
    fn a_bed_has_its_pillow_at_the_head_whichever_way_it_lies() {
        // The head half is the cell behind the foot (`types::bed_partner`),
        // and the model is turned by `bed_quarters` rather than by the
        // facing's own count because `push_box` turns the other way round.
        // Getting that wrong puts the pillow at the foot of every east- and
        // west-facing bed, and nothing about the geometry is otherwise
        // wrong -- so the test asks where the pillow is.
        use primitive_shared::types::{bed_half, Facing};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let piece_centre = |v: &[Vertex], piece: usize| {
            let quads = &v[piece * 24..(piece + 1) * 24];
            let sum = quads.iter().fold(glam::Vec3::ZERO, |s, x| s + glam::Vec3::from(x.position));
            sum / 24.0
        };
        let head_half = crate::logic::models::prop(crate::logic::models::Prop::BedHead);
        let pillow = head_half.iter().position(|p| p.material == Material::Wool).unwrap();
        let headboard = head_half.iter().position(|p| p.name == "headboard").expect("the bed has a headboard");
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            // Unpartnered, so every box has all six faces and the piece
            // index is the quad index over six.
            furniture_block([0.0; 3], bed_half(facing, true), false, &layers, 0xFF, &mut v, &mut i);
            let (dx, dz) = facing.step();
            let toward_head = glam::Vec2::new(-dx as f32, -dz as f32);
            // The pillow's middle is three sixteenths from the cell's, the
            // headboard's six and a half; a turned-round bed puts either on
            // the far side, a negative distance.
            for (piece, what) in [(pillow, "pillow"), (headboard, "headboard")] {
                let c = piece_centre(&v, piece);
                let offset = glam::Vec2::new(c.x - 0.5, c.z - 0.5);
                assert!(
                    offset.dot(toward_head) > 0.1,
                    "a bed facing {facing:?} has its {what} at {offset:?}, not toward its head {toward_head:?}"
                );
            }
        }
    }

    #[test]
    fn a_straw_pallet_has_its_bolster_at_the_head_whichever_way_it_lies() {
        // The pallet has no posts and no headboard: the roll of straw at one
        // end is the whole of which way it lies, and the body is laid head
        // toward the head half (`lying_place` on the server). Turned by the
        // stool's count, as the one-cell heap was, every east- and
        // west-facing pallet would put a sleeper's feet on the bolster.
        use primitive_shared::types::{bed_half_of, Facing, BLOCK_STRAW_BED};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            // Unpartnered, so every box has all six faces.
            furniture_block([0.0; 3], bed_half_of(BLOCK_STRAW_BED, facing, true), false, &layers, 0xFF, &mut v, &mut i);
            let bolster = crate::logic::models::prop(crate::logic::models::Prop::StrawBedHead)
                .iter()
                .position(|p| p.name == "bolster")
                .expect("the pallet has a bolster");
            let quads = &v[bolster * 24..(bolster + 1) * 24];
            let c = quads.iter().fold(glam::Vec3::ZERO, |s, x| s + glam::Vec3::from(x.position)) / 24.0;
            let (dx, dz) = facing.step();
            let toward_head = glam::Vec2::new(-dx as f32, -dz as f32);
            let offset = glam::Vec2::new(c.x - 0.5, c.z - 0.5);
            assert!(
                offset.dot(toward_head) > 0.1,
                "a pallet facing {facing:?} has its bolster at {offset:?}, not toward its head {toward_head:?}"
            );
        }
    }

    #[test]
    fn a_chair_has_its_back_behind_whoever_sits_in_it_whichever_way_it_faces() {
        // The server turns a sitter to `types::seat_yaw`, which is the
        // chair's facing; the model is turned by `bed_quarters`. If the two
        // disagree about which way is the front, every east- and west-facing
        // chair is sat in facing its own back -- and nothing about the
        // geometry is otherwise wrong, so the test asks where the back is.
        use primitive_shared::types::{faced, seat_yaw, Facing, BLOCK_CHAIR};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let chair = faced(BLOCK_CHAIR, facing);
            let (mut v, mut i) = (Vec::new(), Vec::new());
            furniture_block([0.0; 3], chair, false, &layers, 0xFF, &mut v, &mut i);
            let rail = crate::logic::models::prop(crate::logic::models::Prop::Chair)
                .iter()
                .position(|p| p.name == "top rail")
                .expect("the chair has a top rail");
            let quads = &v[rail * 24..(rail + 1) * 24];
            let centre = quads.iter().fold(glam::Vec3::ZERO, |s, x| s + glam::Vec3::from(x.position)) / 24.0;
            let yaw = seat_yaw(chair).expect("a chair faces a way");
            let looking = glam::Vec2::new(yaw.cos(), yaw.sin());
            let offset = glam::Vec2::new(centre.x - 0.5, centre.z - 0.5);
            assert!(
                offset.dot(looking) < -0.2,
                "a chair facing {facing:?} has its back at {offset:?}, in front of a sitter looking along {looking:?}"
            );
            assert!(centre.y > 0.9, "the top rail of a chair facing {facing:?} is not at the top");
        }
    }

    #[test]
    fn a_model_is_lit_by_the_air_round_it_not_by_its_own_cell() {
        // **The bug this exists for**: a drying rack drawn pitch black at
        // night between two lit blocks. Light flooding into a rack's cell
        // pays the rack's own opacity, so at the edge of a fire's reach the
        // cell holds nothing while the air beside it still holds some, and
        // the model took its own cell's light. Fixed in `model_light`.
        use primitive_shared::types::{faced, Facing, BLOCK_DRYING_RACK, BLOCK_GRASS};
        let rack = faced(BLOCK_DRYING_RACK, Facing::South);
        let mut cache = super::plant_tests::cache_of(|x, y, z| match (x, y, z) {
            (_, 0, _) => BLOCK_GRASS,
            (8, 1, 8) => rack,
            _ => BLOCK_AIR,
        });
        let at = |x: i32, y: usize, z: i32| padded_index((x + PAD) as usize, y, (z + PAD) as usize);
        // The rack's cell: dim sky, no fire. The air east of it: firelight.
        cache.light[at(8, 1, 8)] = 0x0B;
        cache.light[at(9, 1, 8)] = 0x3F;
        let mesh = super::plant_tests::mesh_of(&cache);
        // Strictly above the turf's top at y 1 and below the rack's own
        // top, which is where nothing but the rack has a corner.
        let rack_vertices: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| {
                (8.0..=9.0).contains(&v.position[0])
                    && v.position[1] > 1.01
                    && v.position[1] < 1.99
                    && (8.0..=9.0).contains(&v.position[2])
            })
            .collect();
        assert!(!rack_vertices.is_empty(), "the rack drew nothing");
        for v in rack_vertices {
            let light = v.light();
            assert_eq!(light & 15, 15, "the rack took its own cell's sky, not the open sky over it");
            assert_eq!((light >> 4) & 15, 3, "the rack took its own cell's firelight, not the air's beside it");
        }
    }

    #[test]
    fn no_block_leaves_a_hole_in_the_floor_under_it() {
        // **The bug this exists for, found five times one block at a
        // time.** A block whose row is a part-height cube hides the top
        // face of whatever it stands on, which is right for a drift and a
        // square of sky for anything drawn narrower than its cell: a dead
        // boar, a barrel, a jug, a nest, and -- the fifth photograph -- a
        // straw bed standing in a pale blue square in the test world's
        // gallery. So every block there is, on one cell of turf: either the
        // turf's top is still drawn, or what stands on it draws a roof over
        // the whole of the cell itself.
        use primitive_shared::types::{BLOCK_GRASS, KIND_MASK};
        // A hundredth of a block of slack, not a thousandth: a part-height
        // cube and a model's boxes are grown past their cell by a hair
        // (`BITE`, an eight-hundredth) so they overlap what they touch, and
        // with a tighter footprint every corner of a campfire fell outside
        // it and the fire read as drawing nothing at all.
        let footprint = |p: [f32; 3]| (8.0 - 0.01..=9.0 + 0.01).contains(&p[0]) && (8.0 - 0.01..=9.0 + 0.01).contains(&p[2]);
        let mut holes = Vec::new();
        for id in 1..=KIND_MASK {
            if !primitive_shared::blocks::is_defined(id) || primitive_shared::types::is_air(id) {
                continue;
            }
            let cache = super::plant_tests::cache_of(|x, y, z| match (x, y, z) {
                (8, 0, 8) => BLOCK_GRASS,
                (8, 1, 8) => id,
                _ => BLOCK_AIR,
            });
            let mesh = super::plant_tests::mesh_of(&cache);
            let (mut floor, mut roof) = (0.0f32, 0.0f32);
            for tri in mesh.indices.chunks_exact(3) {
                let v = [0, 1, 2].map(|k| &mesh.vertices[tri[k] as usize]);
                if !v.iter().all(|x| footprint(x.position)) || (v[0].light() >> 10) & 7 != 0 {
                    continue;
                }
                let p = v.map(|x| glam::Vec3::from(x.position));
                let area = (p[1] - p[0]).cross(p[2] - p[0]).y.abs() * 0.5;
                if p.iter().all(|q| (q.y - 1.0).abs() < 1e-4) {
                    floor += area;
                } else if p.iter().all(|q| q.y > 1.0 + 1e-4) {
                    roof += area;
                }
            }
            if floor < 0.999 && roof < 0.999 {
                // What *was* drawn over the cell, so a hole can be told
                // from a roof this count missed: face, height, and whether
                // the triangle sits in the solid or the cut-out range.
                let mut drawn = std::collections::BTreeSet::new();
                for (n, tri) in mesh.indices.chunks_exact(3).enumerate() {
                    let v = [0, 1, 2].map(|k| &mesh.vertices[tri[k] as usize]);
                    if v.iter().all(|x| footprint(x.position)) {
                        let ys = v.map(|x| (x.position[1] * 1000.0).round() as i32);
                        let range = if (n * 3) < mesh.leaf_end as usize { "solid/leaf" } else { "sprite/blend" };
                        drawn.insert(format!("face {} y {:?} {range}", (v[0].light() >> 10) & 7, ys));
                    }
                }
                let above = mesh
                    .vertices
                    .iter()
                    .filter(|v| footprint(v.position) && v.position[1] > 1.0 + 1e-3)
                    .count();
                holes.push(format!(
                    "{} ({id}): turf top {floor:.3}, roof {roof:.3}; cover {}, cutout {}, flat {}, cross {}; \
                     {above} vertices above the turf, indices {} (leaf_end {}, sprite_end {}); over the cell: {:?}",
                    primitive_shared::types::block_name(id),
                    cover_of(id),
                    is_cutout(id),
                    is_flat(id),
                    is_cross(id),
                    mesh.indices.len(),
                    mesh.leaf_end,
                    mesh.sprite_end,
                    drawn.into_iter().take(12).collect::<Vec<_>>()
                ));
            }
        }
        assert!(
            holes.is_empty(),
            "these hide the turf's top under them and draw no roof over the cell, so the sky \
             shows through the floor:\n  {}",
            holes.join("\n  ")
        );
    }

    #[test]
    fn every_face_of_a_model_box_is_wound_to_face_outward() {
        // **The bug this test exists for.** `push_box` wound its +Z and
        // -Z faces backwards. `FrontFace::Ccw` therefore culled the
        // face pointing at the player and drew the one behind it:
        // every box of the drying rack was rendered inside-out, and
        // what a player saw was thin bright slivers of a pole's far
        // side standing at the model's corners like blades. Nothing
        // about the coordinates was wrong -- so the tests that checked
        // coordinates all passed, twice, while the model stayed broken.
        //
        // Winding and the light word are checked together because they
        // are two statements of one fact. The shader turns the face
        // index into a normal (`face_normal` in shader.wgsl); if that
        // disagrees with the geometry, the box is lit as though it
        // faced another way -- which is the second bug this caught, on
        // racks that had been turned.
        use primitive_shared::types::{faced, rack_with_hide, Facing, BLOCK_DRYING_RACK};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            for loaded in [false, true] {
                let block = rack_with_hide(faced(BLOCK_DRYING_RACK, facing), loaded);
                let (mut v, mut i) = (Vec::new(), Vec::new());
                rack_block([0.0, 0.0, 0.0], block, RackColumns::Lone, &layers, 0xFF, &mut v, &mut i);
                assert!(!v.is_empty());
                for quad in v.chunks_exact(4) {
                    let p = |k: usize| glam::Vec3::from_array(quad[k].position);
                    let wound = (p(1) - p(0)).cross(p(2) - p(1)).normalize();
                    let face = (quad[0].light() >> 10) & 7;
                    let declared = match face {
                        0 => glam::Vec3::Y,
                        1 => glam::Vec3::NEG_Y,
                        2 => glam::Vec3::X,
                        3 => glam::Vec3::NEG_X,
                        4 => glam::Vec3::Z,
                        _ => glam::Vec3::NEG_Z,
                    };
                    assert!(
                        wound.dot(declared) > 0.99,
                        "{facing:?} loaded={loaded}: a face wound toward {wound:?} \
                         carries face index {face}, which the shader reads as {declared:?}"
                    );
                }
            }
        }
    }

    /// A hide frame in every facing, bare, with a raw skin and with a cured
    /// one: what the three tests below walk round.
    fn laced_hides() -> Vec<(String, BlockId, Vec<Vertex>)> {
        use primitive_shared::types::{faced, hide_frame_showing, Facing, BLOCK_HIDE_FRAME};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let mut out = Vec::new();
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            for (state, raw, cured) in [("bare", false, false), ("raw", true, false), ("cured", false, true)] {
                let block = hide_frame_showing(faced(BLOCK_HIDE_FRAME, facing), raw, cured);
                let (mut v, mut i) = (Vec::new(), Vec::new());
                rack_block([0.0; 3], block, RackColumns::Lone, &layers, 0xFF, &mut v, &mut i);
                assert!(!v.is_empty(), "a {state} hide frame facing {facing:?} drew nothing");
                out.push((format!("{state} {facing:?}"), block, v));
            }
        }
        out
    }

    #[test]
    fn a_laced_hide_is_drawn_exactly_where_it_is_walked_into_and_aimed_at() {
        // **Drawn = collided.** The hide frame has been drawn three ways -- the
        // old rack's A of poles, a skin pegged out on the ground, and now a
        // skin laced into a standing frame -- and each time the collider and
        // the aim have had to follow, or a player walks through a pole or into
        // air and the cracks are drawn beside the thing being broken. So in
        // every facing and state, what is drawn above the ground is inside the
        // box a body walks into and a ray stops at, and that box comes to
        // within a quarter of a sixteenth of the drawing on every side but
        // the floor.
        let slack = BITE * T + 1e-4;
        let loose = 0.25 * T + slack;
        for (name, block, v) in laced_hides() {
            let (low, high) = extent(&v);
            let low = low.max(glam::Vec3::new(f32::MIN, 0.0, f32::MIN));
            let mut walked = Vec::new();
            primitive_shared::geometry::for_each_block_box(block, 0, 0, 0, |_, _, _| BLOCK_AIR, |min, max| walked.push((min, max)));
            let aimed = primitive_shared::geometry::block_box_for_aim(block, 0, 0, 0, false).expect("a hide frame is aimed at");
            assert_eq!(walked, [aimed], "{name}: walked into as {walked:?} and aimed at as {aimed:?}");
            let (min, max) = aimed;
            for axis in 0..3 {
                assert!(
                    low[axis] >= min[axis] - slack && high[axis] <= max[axis] + slack,
                    "{name}: drawn {low}..{high} out of the box {min:?}..{max:?}"
                );
                assert!(max[axis] - high[axis] <= loose, "{name}: the box stands {} past the drawing on axis {axis}", max[axis] - high[axis]);
                if axis != 1 {
                    assert!(low[axis] - min[axis] <= loose, "{name}: the box stands {} short of the drawing on axis {axis}", low[axis] - min[axis]);
                }
            }
            // ...and it is a frame a body walks round: a cell tall, and thin.
            assert!(max[1] - min[1] > primitive_shared::geometry::PLAYER_STEP_HEIGHT, "{name}: {} tall is stepped over", max[1] - min[1]);
            assert!((max[0] - min[0]).min(max[2] - min[2]) < 3.0 * T, "{name}: {min:?}..{max:?} is not a frame's depth");
        }
    }

    #[test]
    fn every_face_of_a_laced_hide_is_wound_outward_and_lit_as_the_way_it_points() {
        // The rack's inside-out bug (`every_face_of_a_model_box_is_wound_to_face_outward`)
        // on a model whose boxes lean: the lacing runs from the skin to the
        // poles in a zig-zag, each lace turned about one axis (`Tilt`). A leaning face points along no axis, so what the shader
        // is told is the nearest of the six to its winding -- and the winding
        // must point out of its own box, or `FrontFace::Ccw` culls the side a
        // player sees and draws the one behind it.
        for (name, _, v) in laced_hides() {
            for (index, quad) in v.chunks_exact(4).enumerate() {
                let p = |k: usize| glam::Vec3::from_array(quad[k].position);
                let wound = (p(1) - p(0)).cross(p(2) - p(1));
                assert_eq!(
                    (quad[0].light() >> 10) & 7,
                    crate::engine::item_model::nearest_face(wound) as u32,
                    "{name}: face {index}, wound toward {wound:?}, is lit as another"
                );
                // Every box is six faces, emitted together (`push_box_moved`).
                let first = index / 6 * 24;
                let middle = v[first..first + 24].iter().map(|c| glam::Vec3::from_array(c.position)).sum::<glam::Vec3>() / 24.0;
                let centre = (p(0) + p(1) + p(2) + p(3)) / 4.0;
                assert!(wound.dot(centre - middle) > 0.0, "{name}: face {index} is wound into its own box");
            }
        }
    }

    #[test]
    fn a_hide_frame_shows_loose_lacing_when_bare_and_its_skin_raw_or_cured_when_loaded() {
        // **The skin stays in the frame until it is taken up.** It used to show
        // on the frame only while it was raw, so the frame looked bare exactly
        // when there was leather in it to collect (`types::HIDE_CURED`). Now:
        // bare is the frame with its lacing hanging loose; loaded is the same
        // frame, the lacing taut to the skin, and the skin in the hide's pictures
        // while raw and leather's once cured -- the same boxes, so the only
        // thing that changes as it dries is its colour.
        use crate::logic::models::{prop, Prop};
        use primitive_shared::types::{faced, hide_frame_showing, Facing, BLOCK_HIDE_FRAME};
        let model = prop(Prop::HideFrame);
        let (bare, skin) = (model.iter().filter(|p| p.bare).count(), model.iter().filter(|p| p.loaded).count());
        let always = model.len() - bare - skin;
        assert!(bare > 0 && skin > 0 && always >= 4 * 2, "a frame of {always} boxes, {bare} loose and {skin} of skin and taut cord");
        let raw_skin = [Material::StretchedHide, Material::Hide];
        let cured_skin = [Material::StretchedLeather, Material::Leather];
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let frame = faced(BLOCK_HIDE_FRAME, facing);
            let quads = |block: BlockId| {
                let (mut v, mut i) = (Vec::new(), Vec::new());
                rack_block([0.0; 3], block, RackColumns::Lone, &crate::engine::texture::FaceLayers::empty_for_test(), 0xFF, &mut v, &mut i);
                v.len() / 4
            };
            let (empty, raw, cured) = (frame, hide_frame_showing(frame, true, false), hide_frame_showing(frame, false, true));
            assert_eq!(quads(empty), (always + bare) * 6, "{facing:?}: the bare frame is not its poles and loose lacing");
            assert_eq!(quads(raw), (always + skin) * 6, "{facing:?}: the loaded frame is not its poles, skin and lacing");
            assert_eq!(quads(cured), quads(raw), "{facing:?}: the skin changed shape as it cured");
            for (block, want, not) in [(raw, raw_skin, cured_skin), (cured, cured_skin, raw_skin)] {
                let pieces = hide_frame_pieces(block);
                let worn: Vec<Material> = pieces.iter().filter(|p| p.loaded).map(|p| p.material).collect();
                assert!(
                    worn.iter().any(|m| want.contains(m)) && !worn.iter().any(|m| not.contains(m)),
                    "{facing:?} {block:#x}: the skin wears {worn:?}"
                );
                assert!(pieces.iter().filter(|p| !p.loaded).all(|p| !raw_skin.contains(&p.material) && !cured_skin.contains(&p.material)));
            }
        }
    }

    /// **The report**: "у предметов нету 3д модели только блок или
    /// текстура" -- a barrel held as a picture of a barrel, a table dropped
    /// as a cube of planks. Every block that `build_mesh` draws as a model and
    /// a player can carry has to answer `has_carried_model`, and asking must
    /// be the same as building: a block that says yes and builds nothing is
    /// an invisible barrel, and one that builds without saying so is never
    /// asked.
    #[test]
    fn every_block_with_a_carried_model_builds_it_and_no_other_block_builds_one() {
        use primitive_shared::types::{
            ALL_BLOCK_IDS, BLOCK_BARREL, BLOCK_BED, BLOCK_BOUGH, BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT,
            BLOCK_DRYING_RACK, BLOCK_JUG, BLOCK_STONE, BLOCK_STOOL, BLOCK_STRAW_BED, BLOCK_TABLE,
            BLOCK_TWIG,
        };
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        for &(block, name) in ALL_BLOCK_IDS {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            let built = carried_model(block, &layers, &mut v, &mut i);
            assert_eq!(built, has_carried_model(block), "{name}: asking and building disagree");
            assert_eq!(built, !v.is_empty(), "{name}: built {} vertices", v.len());
            assert_eq!(i.len(), v.len() / 4 * 6, "{name}: a model that is not made of quads");
        }
        for block in [
            BLOCK_BARREL,
            BLOCK_JUG,
            BLOCK_BED,
            BLOCK_TABLE,
            BLOCK_STOOL,
            BLOCK_STRAW_BED,
            BLOCK_DRYING_RACK,
            BLOCK_CAMPFIRE,
            BLOCK_CAMPFIRE_LIT,
            BLOCK_TWIG,
            BLOCK_BOUGH,
        ] {
            assert!(
                has_carried_model(block),
                "{} is carried as a cube or a sprite",
                primitive_shared::types::block_name(block)
            );
        }
        // ...and a full cube is still a cube.
        assert!(!has_carried_model(BLOCK_STONE));
    }

    /// **A carried model is lit as the world lights it, and where it is.**
    /// Spun on the floor or swung in a hand, every face of it has to carry
    /// the direction it actually points once it has been turned -- the
    /// shader makes the normal out of that index, and a model-space index
    /// welds the shading to the model, which is the bug `spun_face` and
    /// `world_face_of` were each written for. Checked against the winding of
    /// the corners that come out, not against the rotation that put them
    /// there: arithmetic checked with its own arithmetic proves nothing.
    ///
    /// And the light is the place's, except for the fire the model carries:
    /// the flame of a lit hearth keeps its own full block light.
    #[test]
    fn a_placed_carried_model_points_its_faces_where_they_went_and_takes_the_light_where_it_is() {
        use primitive_shared::types::{ALL_BLOCK_IDS, MAX_LIGHT};
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let turns = [
            glam::Mat4::IDENTITY,
            glam::Mat4::from_rotation_y(0.7),
            glam::Mat4::from_rotation_y(-2.4) * glam::Mat4::from_scale(glam::Vec3::splat(0.45)),
            // The hand's grip, pitched and turned.
            glam::Mat4::from_rotation_y(-0.6) * glam::Mat4::from_rotation_x(0.2),
            glam::Mat4::from_rotation_z(1.3) * glam::Mat4::from_rotation_x(-0.9),
        ];
        let mut checked = 0;
        for &(block, name) in ALL_BLOCK_IDS {
            let (mut model, mut model_indices) = (Vec::new(), Vec::new());
            if !carried_model(block, &layers, &mut model, &mut model_indices) {
                continue;
            }
            for transform in turns {
                let (mut v, mut i) = (Vec::new(), Vec::new());
                place_carried(&model, &model_indices, transform, (9, 2), &mut v, &mut i);
                assert_eq!(v.len(), model.len());
                for (placed, built) in v.chunks_exact(4).zip(model.chunks_exact(4)) {
                    let word = placed[0].light();
                    let fire = ((built[0].light() >> 4) & 15) as u8;
                    assert_eq!(word & 15, 9, "{name}: the sky was not the place's");
                    if fire == MAX_LIGHT {
                        assert_eq!((word >> 4) & 15, MAX_LIGHT as u32, "{name}: the flame went out");
                        // A flame faces up whichever way it is seen from:
                        // its winding is not what lights it.
                        continue;
                    }
                    assert_eq!((word >> 4) & 15, 2, "{name}: the fire was not the place's");
                    let p = |k: usize| glam::Vec3::from_array(placed[k].position);
                    let wound = (p(1) - p(0)).cross(p(2) - p(1));
                    assert_eq!(
                        (word >> 10) & 7,
                        crate::engine::item_model::nearest_face(wound) as u32,
                        "{name} under {transform:?}: a face wound toward {wound:?} says it points elsewhere"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 1000, "only {checked} faces were checked");
    }

    #[test]
    fn a_meshed_rack_is_made_of_planar_axis_aligned_quads() {
        // The fins bug: in the world -- though not when the model
        // emitter was called on its own -- a rack grew paper-thin sails
        // off its pole tops and bar ends. A sail is a quad whose four
        // corners are not in one axis-aligned plane, so that is the
        // thing to assert.
        use primitive_shared::types::{faced, Facing, BLOCK_DRYING_RACK, BLOCK_GRASS};
        let pos = ChunkPos::new(0, 0);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                blocks[Chunk::index(x, 0, z)] = BLOCK_GRASS;
            }
        }
        blocks[Chunk::index(4, 1, 8)] = faced(BLOCK_DRYING_RACK, Facing::South);
        let mut chunks = ChunkManager::new(4);
        chunks.insert(Chunk { pos, blocks });
        let mut light = LightMap::new();
        light.load_chunk(&chunks, pos);
        let out = mesh_it(&chunks, &light, pos);

        let mut rack_quads = 0;
        for quad in out.vertices.chunks_exact(4) {
            // Anything above the grass in the rack's neighbourhood is
            // the rack's own geometry -- and all of it must stay in
            // the rack's cell. The fins were exactly this: real quads
            // standing in the air beside the block they belong to.
            let above_grass = quad.iter().any(|v| v.position[1] > 1.0 + 1e-3);
            let near = quad.iter().any(|v| {
                (2.0..=7.0).contains(&v.position[0]) && (6.0..=11.0).contains(&v.position[2])
            });
            if !above_grass || !near {
                continue;
            }
            rack_quads += 1;
            let span = |axis: usize| {
                let lo = quad.iter().map(|v| v.position[axis]).fold(f32::MAX, f32::min);
                let hi = quad.iter().map(|v| v.position[axis]).fold(f32::MIN, f32::max);
                hi - lo
            };
            let planar = (0..3).any(|axis| span(axis) < 1e-4);
            assert!(
                planar,
                "a twisted quad in the rack: {:?}",
                quad.iter().map(|v| v.position).collect::<Vec<_>>()
            );
            for v in quad {
                let slack = BITE * T + 1e-3; // the seam bite; see `push_box`
                assert!(
                    (4.0 - slack..=5.0 + slack).contains(&v.position[0])
                        && (1.0 - slack..=2.0 + slack).contains(&v.position[1])
                        && (8.0 - slack..=9.0 + slack).contains(&v.position[2]),
                    "the rack left its cell: {:?}",
                    quad.iter().map(|v| v.position).collect::<Vec<_>>()
                );
            }
        }
        assert!(rack_quads >= 20, "only {rack_quads} rack quads; the fixture is wrong");

        // ...and the indices must agree with the vertices: a triangle
        // stitched between two far-apart quads is exactly the kind of
        // paper sail the vertex check above can never see.
        //
        // **Flat, not small.** This used to bound how far a triangle
        // could reach, which caught a sail because a sail is long. It
        // cannot any more: the grass floor under the rack merges into
        // rectangles that legitimately span the chunk (see
        // `MERGE_COPLANAR_FACES`), so length stopped being evidence of
        // anything. What a sail actually is, and what a merged rectangle
        // never is, is a triangle that is not flat against one of the
        // three axes -- so that is what is checked now, which is also
        // what this test is called.
        for triangle in out.indices.chunks_exact(3) {
            let p: Vec<[f32; 3]> = triangle
                .iter()
                .map(|&i| out.vertices[i as usize].position)
                .collect();
            let flat = (0..3).any(|axis| {
                (p[0][axis] - p[1][axis]).abs() < 1e-3 && (p[0][axis] - p[2][axis]).abs() < 1e-3
            });
            assert!(
                flat,
                "a triangle lies across all three axes: {p:?} (indices {triangle:?})"
            );
        }
    }

    #[test]
    fn a_lit_hearth_grows_two_crossing_sheets_of_flame() {
        // **The fire is drawn the way a tuft of grass is.** It used to
        // be a picture on the top face of a block four pixels tall,
        // which from standing height is a bright smear on the ground.
        //
        // Two quads, and -- the part that is easy to get wrong -- two
        // *different* pictures on them. One picture on both is the same
        // silhouette at right angles to itself, which reads as a
        // cardboard X from every angle except the two diagonals.
        use primitive_shared::types::{BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT};
        let mesh_with = |block: primitive_shared::types::BlockId| {
            let pos = ChunkPos::new(0, 0);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            blocks[Chunk::index(4, 8, 4)] = primitive_shared::types::BLOCK_STONE;
            blocks[Chunk::index(4, 9, 4)] = block;
            let mut chunks = ChunkManager::new(4);
            chunks.insert(Chunk { pos, blocks });
            let mut light = LightMap::new();
            light.load_chunk(&chunks, pos);
            mesh_it(&chunks, &light, pos)
        };

        let lit = mesh_with(BLOCK_CAMPFIRE_LIT);
        let cold = mesh_with(BLOCK_CAMPFIRE);
        // The sprite pass is where cut-out geometry goes.
        let sprites = (lit.sprite_end - lit.leaf_end) as usize;
        assert_eq!(sprites, 12, "a lit hearth should add two quads of flame");
        assert_eq!(
            cold.sprite_end - cold.leaf_end,
            0,
            "an unlit fire is burning"
        );

        // The two quads wear different layers, and both are flames.
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let (a, b) = (layers.flame(0), layers.flame(1));
        assert_ne!(a, b, "both quads of the fire wear the same picture");
        let flame_vertices: Vec<&Vertex> = lit
            .vertices
            .iter()
            .filter(|v| v.tex_layer() == a || v.tex_layer() == b)
            .collect();
        assert_eq!(flame_vertices.len(), 8, "two quads is eight corners");
        assert_eq!(
            flame_vertices.iter().filter(|v| v.tex_layer() == a).count(),
            4,
            "the two sheets are not one quad each"
        );

        // ...and it stands *up*, out of the stones rather than lying on
        // them: the block is a quarter of a cell and the flame reaches
        // most of the way to the ceiling.
        let feet = flame_vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let tips = flame_vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        assert!(feet >= 9.0, "the flame starts below the hearth: {feet}");
        assert!(tips > feet + 0.5, "the flame is flat: {feet} to {tips}");
        assert!(tips < 10.2, "the flame grew through the ceiling: {tips}");
    }

    #[test]
    fn a_chunk_a_million_blocks_out_is_meshed_at_the_same_numbers_as_one_at_the_origin() {
        // **The bug this test exists for.** A vertex used to carry its
        // absolute place in the world. An `f32` a million blocks from
        // the origin has about six centimetres between one representable
        // value and the next, so out where players actually go the mesh
        // itself was quantised: block faces stopped meeting, the texture
        // filter's derivatives went to noise, and the whole world
        // shimmered as the camera moved. It is the one bug in this game
        // that gets *worse* the further you play.
        //
        // The fix is that a vertex now says where it is inside its own
        // chunk and the draw says where the chunk is (see
        // `Vertex::instance_layout`), so the two chunks below have to
        // come out byte for byte identical.
        let far = ChunkPos::new(62_500, -62_500); // a million blocks out
        let near = ChunkPos::new(0, 0);
        let mesh_at = |pos: ChunkPos| {
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 8, z)] = primitive_shared::types::BLOCK_STONE;
                }
            }
            let mut chunks = ChunkManager::new(4);
            chunks.insert(Chunk { pos, blocks });
            let mut light = LightMap::new();
            light.load_chunk(&chunks, pos);
            mesh_it(&chunks, &light, pos)
        };
        let there = mesh_at(far);
        let here = mesh_at(near);
        assert!(!here.vertices.is_empty(), "the fixture drew nothing");
        assert_eq!(
            there.vertices.len(),
            here.vertices.len(),
            "a chunk a million blocks out is a different amount of geometry"
        );
        for (a, b) in there.vertices.iter().zip(here.vertices.iter()) {
            assert_eq!(
                a.position, b.position,
                "the same block came out at different coordinates at {far:?} and {near:?}"
            );
        }
        // ...and every one of those numbers is small, which is the whole
        // property. A padded neighbourhood reaches one block either side
        // of the chunk.
        for vertex in &there.vertices {
            for axis in vertex.position {
                assert!(
                    axis.abs() <= 400.0,
                    "a vertex still carries a world coordinate: {:?}",
                    vertex.position
                );
            }
        }
    }

    /// "у листвы, когда стоит рядом с неполным, чернеет бок": a face takes
    /// its light from the cell it looks into, and the cell of a lip, a slab,
    /// a step or a bite -- anything that stops light and does not fill its
    /// cell -- was left at nought by the flood under open noon sky. The side
    /// of a column of leaves beside the lip of a slope was drawn black from
    /// its foot up; the next block of turf up the slope and the water beside
    /// a slab were dimmed the same way. Through the real flood
    /// (`lighting::holds_light`) and the real mesher, every such face now
    /// takes the daylight standing over the partial block.
    #[test]
    fn a_face_beside_a_partial_block_is_lit_by_the_air_over_its_top() {
        use primitive_shared::dig::{self, Side};
        use primitive_shared::types::{
            faced, Facing, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LEAVES, BLOCK_PLANK_STAIRS, BLOCK_THATCH_ROOF,
            BLOCK_TILE_SLAB,
        };
        let partials = [
            ("a lip a quarter down", dig::lowered(BLOCK_GRASS, 3)),
            ("a lip three quarters down", dig::lowered(BLOCK_GRASS, 1)),
            ("a heap", dig::heaped(BLOCK_DIRT)),
            ("a side bite", dig::next_bite(BLOCK_DIRT, Side::PosZ).expect("dirt bites")),
            ("a slab", BLOCK_TILE_SLAB),
            ("a step", faced(BLOCK_PLANK_STAIRS, Facing::South)),
            ("a roof", faced(BLOCK_THATCH_ROOF, Facing::East)),
        ];
        let pos = ChunkPos::new(0, 0);
        for (what, partial) in partials {
            for (beside, block) in [("leaves", BLOCK_LEAVES), ("a whole block", BLOCK_DIRT), ("water", BLOCK_WATER)] {
                let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        blocks[Chunk::index(x, 0, z)] = BLOCK_DIRT;
                        blocks[Chunk::index(x, 1, z)] = BLOCK_GRASS;
                    }
                }
                // The partial at (4, 2, 4); what looks into it north of it.
                blocks[Chunk::index(4, 2, 4)] = partial;
                blocks[Chunk::index(4, 2, 5)] = block;
                let mut chunks = ChunkManager::new(4);
                chunks.insert(Chunk { pos, blocks });
                let mut light = LightMap::new();
                light.load_chunk(&chunks, pos);
                let mesh = mesh_it(&chunks, &light, pos);
                // Its face toward the partial: facing -Z, in the plane z = 5.
                let face: Vec<_> = mesh
                    .vertices
                    .iter()
                    .filter(|v| {
                        (v.light() >> 10) & 7 == 5
                            && (v.position[2] - 5.0).abs() < 1e-4
                            && (4.0..=5.0).contains(&v.position[0])
                            && (2.0..=3.0).contains(&v.position[1])
                    })
                    .collect();
                assert!(!face.is_empty(), "{beside} beside {what} drew no face toward it");
                for v in face {
                    assert!(
                        v.light() & 15 >= 12,
                        "{beside} beside {what} is lit at sky {} where it looks into the partial's cell",
                        v.light() & 15
                    );
                }
            }
        }
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
        // Top and bottom are 256 faces each; the four edges add 16 more
        // apiece, and those are the ones that would vanish if opaque
        // blocks were culled against unknown territory too.
        //
        // **Faces, not quads.** A flat slab is the best case merging
        // has: its top is one rectangle and so is its bottom, so a quad
        // count here would read six and say nothing about culling. What
        // the test means is how much surface exists, which is the area
        // the quads cover. See `MERGE_COPLANAR_FACES`.
        let solid = &mesh.indices[..mesh.solid_index_count as usize];
        let faces: f32 = solid
            .chunks_exact(6)
            .map(|triangles| {
                let base = *triangles.iter().min().expect("six indices") as usize;
                let quad = &mesh.vertices[base..base + 4];
                (0..3)
                    .map(|axis| {
                        let values = quad.iter().map(|v| v.position[axis]);
                        let lo = values.clone().fold(f32::MAX, f32::min);
                        (values.fold(f32::MIN, f32::max) - lo).round().max(1.0)
                    })
                    .product::<f32>()
            })
            .sum();
        assert!(
            faces > (CHUNK_SIZE_X * CHUNK_SIZE_Z * 2) as f32,
            "the slab's edges were culled away: {faces} faces"
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
    fn a_block_with_a_bite_out_of_it_is_drawn_exactly_where_it_is_collided() {
        // **The one property a partial block has to have.** A dug block is
        // one box (`dig::bite_box`), the collider stops at that box
        // (`geometry::block_box`) and the mesher draws it -- and if the two
        // ever part company the player sees a wall they can walk through,
        // or walks into rock that is not there. Both come from the same
        // function, and this is what says the drawing still does.
        //
        // The slack is the hair a part-height cube is grown by, which is
        // why the cube's own faces never crack against the wall beside it
        // (`reach`).
        use primitive_shared::dig;
        const AT: (i32, i32, i32) = (8, 4, 8);
        let slack = BITE * T + 1e-4;
        for side in [
            dig::Side::PosX,
            dig::Side::NegX,
            dig::Side::PosY,
            dig::Side::NegY,
            dig::Side::PosZ,
            dig::Side::NegZ,
        ] {
            let mut block = BLOCK_STONE;
            while let Some(next) = dig::next_bite(block, side) {
                block = next;
                let mesh = mesh_of(&cache_of(|x, y, z| if (x, y, z) == AT { block } else { BLOCK_AIR }));
                assert!(!mesh.vertices.is_empty(), "{side:?} at {block:#x} drew nothing at all");
                let (min, max) = primitive_shared::geometry::block_box(block, AT.0, AT.1, AT.2)
                    .expect("a block with rock left in it is collided");
                for v in &mesh.vertices {
                    for axis in 0..3 {
                        assert!(
                            v.position[axis] >= min[axis] - slack && v.position[axis] <= max[axis] + slack,
                            "{side:?} at {block:#x}: a vertex at {:?} is outside the box {min:?}..{max:?}",
                            v.position
                        );
                    }
                }
                // ...and the whole of the box is drawn, not a face of it:
                // the rock reaches every wall of what a player walks into.
                for axis in 0..3 {
                    let lo = mesh.vertices.iter().fold(f32::MAX, |a, v| a.min(v.position[axis]));
                    let hi = mesh.vertices.iter().fold(f32::MIN, |a, v| a.max(v.position[axis]));
                    assert!(
                        (lo - min[axis]).abs() <= slack && (hi - max[axis]).abs() <= slack,
                        "{side:?} at {block:#x}: axis {axis} is drawn {lo}..{hi} and collided {}..{}",
                        min[axis],
                        max[axis]
                    );
                }
            }
        }
    }

    #[test]
    fn a_bite_opens_the_faces_of_the_blocks_round_it() {
        // A wall of stone with one cell quarried from the +X side: the
        // cells beside, above and below the bite share a wall with it that
        // is no longer covered, and each has to draw that wall or the
        // player is looking into a hole with nothing in it. This is the
        // cover byte doing its job -- a dug block hides nothing
        // (`types::is_opaque`) -- and it is the answer that stops a bite
        // being a window through the hillside.
        use primitive_shared::dig;
        const AT: (i32, i32, i32) = (8, 4, 8);
        let wall = |dug: BlockId| {
            move |x: i32, y: i32, z: i32| {
                if (2..14).contains(&x) && (2..8).contains(&y) && (2..14).contains(&z) {
                    if (x, y, z) == AT { dug } else { BLOCK_STONE }
                } else {
                    BLOCK_AIR
                }
            }
        };
        let solid = mesh_of(&cache_of(wall(BLOCK_STONE))).vertices.len();
        let bitten = dig::next_bite(BLOCK_STONE, dig::Side::PosX).unwrap();
        let opened = mesh_of(&cache_of(wall(bitten))).vertices.len();
        assert!(
            opened > solid,
            "a bite inside a solid wall drew {opened} vertices and the whole wall drew {solid}:              the faces the bite opened were culled"
        );
    }

    #[test]
    fn a_palm_trunk_leans_one_way_and_never_steps_back_toward_its_root() {
        // **"Palm trunks are zigzag staircases."** A palm leans by stepping
        // sideways at one height, and every piece used to be drawn as a post
        // with an arm to each neighbour, so each step was a right-angled Z of
        // bark: up, out a whole cell, up. See `palm::palm_course`.
        //
        // The mechanism, checked on what is emitted: the trunk comes out as
        // boxes, and taken bottom to top their middles must move only toward
        // the lean, never more than a slice's worth at once, never sideways
        // across it, and start over the root and finish under the crown.
        // The generator's own palms (`worldgen::palm_cells`), every height
        // and both bend heights, leaning each way.
        use primitive_shared::types::{block_kind, BLOCK_PALM_TRUNK};
        const ROOT: (i32, i32) = (7, 7);
        const GROUND: i32 = 20;
        for variant in 0u32..32 {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let trunk: Vec<((i32, i32, i32), BlockId)> = primitive_shared::worldgen::palm_cells(variant, lean)
                    .into_iter()
                    .filter(|(_, id)| block_kind(*id) == BLOCK_PALM_TRUNK)
                    .map(|((dx, dy, dz), id)| ((ROOT.0 + dx, GROUND + dy, ROOT.1 + dz), id))
                    .collect();
                let top = trunk.iter().map(|((_, y, _), _)| *y).max().expect("a palm has a trunk");
                let crown = trunk.iter().find(|((_, y, _), _)| *y == top).map(|((x, _, z), _)| (*x, *z)).unwrap();
                let out = mesh_of(&cache_of(|x, y, z| {
                    trunk.iter().find(|(at, _)| *at == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id)
                }));
                // Every box is 24 corners, in emission order.
                assert_eq!(out.vertices.len() % 24, 0, "a palm's trunk is whole boxes");
                let mut boxes: Vec<(f32, f32, f32, f32)> = out
                    .vertices
                    .chunks(24)
                    .map(|corners| {
                        let n = corners.len() as f32;
                        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
                        for c in corners {
                            x += c.position[0];
                            y += c.position[1];
                            z += c.position[2];
                        }
                        let (x, y, z) = (x / n, y / n, z / n);
                        let along = (x - (ROOT.0 as f32 + 0.5)) * lean.0 as f32 + (z - (ROOT.1 as f32 + 0.5)) * lean.1 as f32;
                        let across = (x - (ROOT.0 as f32 + 0.5)) * lean.1 as f32 - (z - (ROOT.1 as f32 + 0.5)) * lean.0 as f32;
                        let height = corners.iter().map(|c| c.position[1]).fold(f32::MIN, f32::max)
                            - corners.iter().map(|c| c.position[1]).fold(f32::MAX, f32::min);
                        (y, along, across, height)
                    })
                    .collect();
                boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
                let name = format!("palm {variant:#x} leaning {lean:?}");
                // Within a sixteenth: the lowest slice's middle is two
                // sixteenths up a trunk that already leans from the ground,
                // and the highest two sixteenths short of the crown.
                assert!(boxes[0].1.abs() < 1.0 / 16.0, "{name} does not stand over its root: {:?}", boxes[0]);
                let last = boxes[boxes.len() - 1];
                let crown_along = (crown.0 - ROOT.0) * lean.0 + (crown.1 - ROOT.1) * lean.1;
                assert!(
                    (last.1 - crown_along as f32).abs() < 1.0 / 16.0,
                    "{name} ends at {last:?}, not under its crown at {crown_along}"
                );
                for pair in boxes.windows(2) {
                    let (below, above) = (pair[0], pair[1]);
                    assert!(above.1 >= below.1 - 1e-3, "{name} steps back toward its root: {below:?} then {above:?}");
                    assert!(above.1 - below.1 <= 2.5 / 16.0, "{name} jumps sideways: {below:?} then {above:?}");
                    assert!(above.2.abs() < 1e-3, "{name} wanders across its lean: {above:?}");
                    assert!(above.3 <= 0.26, "{name} has a box that is not a slice of trunk: {above:?}");
                }
            }
        }
    }

    /// The (min, max) corners of each 24-corner box a model emitted, in order.
    fn boxes_of(vertices: &[Vertex]) -> Vec<([f32; 3], [f32; 3])> {
        vertices
            .chunks(24)
            .map(|corners| {
                corners.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), c| {
                    let p = c.position;
                    ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
                })
            })
            .collect()
    }

    #[test]
    fn a_palm_trunk_is_drawn_exactly_where_it_is_walked_into() {
        // **"у пальмы поломаны коллизия она не соответствует модели".** The
        // trunk was drawn along a leaning line and walked into as whole
        // cells. Both come from `palm::trunk_slices` now, and this is what
        // says they agree, on what is emitted against what is collided: every
        // box of bark the mesher draws is a box the collider stops at
        // (`geometry::for_each_block_box`), and there are no others. Every
        // palm height, both bend heights, leaning each way.
        use primitive_shared::types::{block_kind, BLOCK_PALM_TRUNK};
        const ROOT: (i32, i32) = (7, 7);
        const GROUND: i32 = 20;
        let slack = BITE * T + 1e-4;
        for variant in 0u32..32 {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let trunk: Vec<((i32, i32, i32), BlockId)> = primitive_shared::worldgen::palm_cells(variant, lean)
                    .into_iter()
                    .filter(|(_, id)| block_kind(*id) == BLOCK_PALM_TRUNK)
                    .map(|((dx, dy, dz), id)| ((ROOT.0 + dx, GROUND + dy, ROOT.1 + dz), id))
                    .collect();
                let at = |x: i32, y: i32, z: i32| trunk.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id);
                let drawn = boxes_of(&mesh_of(&cache_of(at)).vertices);
                let mut walked = Vec::new();
                for &((x, y, z), id) in &trunk {
                    primitive_shared::geometry::for_each_block_box(
                        id,
                        x,
                        y,
                        z,
                        |dx, dy, dz| at(x + dx, y + dy, z + dz),
                        |min, max| walked.push((min, max)),
                    );
                }
                let name = format!("palm {variant:#x} leaning {lean:?}");
                assert_eq!(drawn.len(), walked.len(), "{name} is drawn as {} boxes of bark and walked into as {}", drawn.len(), walked.len());
                for (min, max) in &walked {
                    assert!(
                        drawn.iter().any(|(lo, hi)| (0..3).all(|a| (lo[a] - min[a]).abs() <= slack && (hi[a] - max[a]).abs() <= slack)),
                        "{name} is walked into at {min:?}..{max:?}, where no bark is drawn"
                    );
                }
            }
        }
    }

    #[test]
    fn a_door_is_drawn_exactly_where_it_is_walked_into() {
        // The rack's lesson, kept for the door: the slab is written at its
        // north and turned by the mesher's count of quarters, and the box a
        // player walks into is turned by `Facing`'s -- which run opposite
        // ways. Every facing, open and shut, both halves: the one box drawn is
        // the one box collided.
        use primitive_shared::types::{door_partner, door_swung, faced, Facing, BLOCK_DOOR};
        let slack = BITE * T + 1e-4;
        let (cell, top_cell) = ((7, 20, 7), (7, 21, 7));
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            for open in [false, true] {
                let mut lower = faced(BLOCK_DOOR, facing);
                if open {
                    lower = door_swung(lower);
                }
                let (_, top) = door_partner(cell, lower).unwrap();
                let at = |x: i32, y: i32, z: i32| match (x, y, z) {
                    c if c == cell => lower,
                    c if c == top_cell => top,
                    _ => BLOCK_AIR,
                };
                let drawn = boxes_of(&mesh_of(&cache_of(at)).vertices);
                assert_eq!(drawn.len(), 2, "{facing:?} open {open}: a door drawn as {} boxes", drawn.len());
                for (c, id) in [(cell, lower), (top_cell, top)] {
                    let (min, max) = primitive_shared::geometry::block_box(id, c.0, c.1, c.2).unwrap();
                    assert!(
                        drawn.iter().any(|(lo, hi)| (0..3).all(|a| (lo[a] - min[a]).abs() <= slack && (hi[a] - max[a]).abs() <= slack)),
                        "{facing:?} open {open}: walked into at {min:?}..{max:?}, drawn at {drawn:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_doors_back_face_is_lit_from_the_side_its_boards_stand_against() {
        // The boards stand at one edge of the cell, and the face on that
        // edge looks into the neighbour there; the other broad face looks
        // back through the cell. Asked of the collider, which the test above
        // holds to the drawing, so a turn got backwards shows up here as a
        // door lit from the wrong room.
        use primitive_shared::types::{faced, Facing, BLOCK_DOOR};
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let door = faced(BLOCK_DOOR, facing);
            let (min, max) = primitive_shared::geometry::block_box(door, 0, 0, 0).unwrap();
            let middle = [(min[0] + max[0]) * 0.5 - 0.5, (min[2] + max[2]) * 0.5 - 0.5];
            let (bx, bz) = door_face_offset(door, true);
            assert!(bx as f32 * middle[0] > 0.3 || bz as f32 * middle[1] > 0.3, "{facing:?}: boards at {middle:?}, back face looks to {bx},{bz}");
            assert_eq!(door_face_offset(door, false), (-bx, -bz), "{facing:?}: the two faces look the same way");
        }
    }

    #[test]
    fn a_step_is_drawn_exactly_where_it_is_walked_into() {
        // The door's test, for a step: drawn through `turned_from_north`,
        // collided through `Facing::quarters`, and the two count opposite
        // ways. A step that disagreed would be walked up where its riser is
        // drawn.
        // Every kind of step, not only the roof it was written for: the
        // wooden and cobbled stairs are the ones a player walks up.
        use primitive_shared::types::{
            faced, Facing, BLOCK_BRANCH_ROOF, BLOCK_COBBLESTONE_STAIRS, BLOCK_PLANK_STAIRS, BLOCK_THATCH_ROOF,
            BLOCK_TILE_ROOF,
        };
        // **And every neighbour that bends it** (`geometry::StepShape`): a
        // step turned each way behind it and in front of it, and the same
        // with a flight carrying straight on past the turn. The corner is
        // decided from the neighbours on both sides of this comparison, so
        // this is the one place that says the two decide it alike: the boxes
        // drawn in the cell are the boxes collided in it, none missing and
        // none extra.
        let slack = BITE * T + 1e-4;
        let cell = (7, 20, 7);
        let every = [BLOCK_PLANK_STAIRS, BLOCK_COBBLESTONE_STAIRS, BLOCK_TILE_ROOF, BLOCK_THATCH_ROOF, BLOCK_BRANCH_ROOF];
        let facings = [Facing::North, Facing::East, Facing::South, Facing::West];
        let same = |a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3])| {
            (0..3).all(|k| (a.0[k] - b.0[k]).abs() <= slack && (a.1[k] - b.1[k]).abs() <= slack)
        };
        let inside = |(lo, hi): &([f32; 3], [f32; 3])| {
            (0..3).all(|k| {
                let c = [cell.0, cell.1, cell.2][k] as f32;
                lo[k] >= c - slack && hi[k] <= c + 1.0 + slack
            })
        };
        let mut shapes = std::collections::HashSet::new();
        for (kind, facing) in every.into_iter().flat_map(|kind| facings.map(|f| (kind, f))) {
            let step = faced(kind, facing);
            let (fx, fz) = facing.step();
            // Offsets round the step and what stands there; nothing else.
            let mut around: Vec<Vec<((i32, i32), BlockId)>> = vec![Vec::new()];
            for by in 0..4 {
                let other = faced(kind, facings[((facing.quarters() + by) % 4) as usize]);
                for (ox, oz) in [(-fx, -fz), (fx, fz)] {
                    around.push(vec![((ox, oz), other)]);
                    // ...with a flight going on past the turn, either side.
                    for side in [(fz, fx), (-fz, -fx)] {
                        around.push(vec![((ox, oz), other), (side, step)]);
                    }
                }
            }
            for neighbours in around {
                let at = |x: i32, y: i32, z: i32| {
                    if (x, y, z) == cell {
                        return step;
                    }
                    neighbours
                        .iter()
                        .find(|((dx, dz), _)| (x, y, z) == (cell.0 + dx, cell.1, cell.2 + dz))
                        .map_or(BLOCK_AIR, |&(_, id)| id)
                };
                let drawn: Vec<_> = boxes_of(&mesh_of(&cache_of(at)).vertices).into_iter().filter(inside).collect();
                let mut walked = Vec::new();
                primitive_shared::geometry::for_each_block_box(
                    step,
                    cell.0,
                    cell.1,
                    cell.2,
                    |dx, dy, dz| at(cell.0 + dx, cell.1 + dy, cell.2 + dz),
                    |min, max| walked.push((min, max)),
                );
                shapes.insert(walked.len());
                assert_eq!(drawn.len(), walked.len(), "{kind} {facing:?} beside {neighbours:?}: drawn {drawn:?}, walked into {walked:?}");
                for &w in &walked {
                    assert!(
                        drawn.iter().any(|&d| same(d, w)),
                        "{kind} {facing:?} beside {neighbours:?}: walked into at {w:?}, drawn at {drawn:?}"
                    );
                }
            }
        }
        // An inside corner is three boxes: were no neighbour ever to bend a
        // step, this test would pass for nothing.
        assert!(shapes.contains(&3), "no inside corner was ever made: {shapes:?}");
    }

    #[test]
    fn a_palm_trunk_shows_its_cut_end_only_where_nothing_of_the_palm_goes_on() {
        // **"добавь palm_top это спил как у log".** A palm's trunk is drawn in
        // slices, and a leaning one shows a sliver of each slice's top at every
        // step of its lean: were every top and bottom face the cut, the trunk
        // would be banded like a stack of coins. The cut belongs at the root,
        // on the top of a palm whose crown was taken, and on both sides of a
        // piece broken out -- and not on a ledge, across a step or under the
        // crown. Every palm height, both bend heights, leaning each way; and a
        // straight trunk broken through.
        use primitive_shared::types::{block_kind, BLOCK_PALM_TRUNK};
        const ROOT: (i32, i32) = (7, 7);
        const GROUND: i32 = 20;
        use primitive_shared::types::{BLOCK_PALM_COCONUTS, BLOCK_PALM_FRONDS};
        let textures = FaceLayers::numbered_for_test();
        // **Masked to what a vertex carries.** The numbered fixture gives the
        // palm's top 356 * 7 + 1, and a vertex keeps nine bits of it: compared
        // whole, no vertex ever wore the cut, and "a crowned palm is not cut on
        // top" passed for nothing. And no picture of the palm may share a
        // masked number with the cut, or the count would be someone else's.
        let vertex_layer = |block: BlockId, face: usize| textures.layer_for_face(block, face) & (MAX_TEXTURE_LAYERS - 1);
        let (cut_top, cut_bottom) = (vertex_layer(BLOCK_PALM_TRUNK, 0), vertex_layer(BLOCK_PALM_TRUNK, 1));
        for block in [BLOCK_PALM_TRUNK, BLOCK_PALM_FRONDS, BLOCK_PALM_COCONUTS] {
            for face in 0..crate::engine::texture::SLOTS {
                if block != BLOCK_PALM_TRUNK || face >= 2 {
                    let other = vertex_layer(block, face);
                    assert!(other != cut_top && other != cut_bottom, "the fixture gives block {block} face {face} the cut's layer {other}");
                }
            }
        }
        let slack = BITE * T + 1e-4;
        // The height of every face that wears the cut facing up, and of every
        // one that wears it facing down, lowest first. Read vertex by vertex
        // and four corners to a face, not four vertices at a time from the
        // start: a crown's fronds are not all quads, and one of them ahead of
        // the trunk put every later "quad" across two faces.
        let cuts = |at: &dyn Fn(i32, i32, i32) -> BlockId| {
            let mut out = MeshBuffers::default();
            build_mesh(ChunkPos::new(0, 0), &cache_of(at), &textures, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
            let heights = |layer: u32| {
                let mut corners: Vec<f32> = out.vertices.iter().filter(|v| v.tex_layer() == layer).map(|v| v.position[1]).collect();
                corners.sort_by(f32::total_cmp);
                assert_eq!(corners.len() % 4, 0, "a cut face that is not four corners: {corners:?}");
                corners.chunks(4).map(|face| face[0]).collect::<Vec<f32>>()
            };
            (heights(cut_top), heights(cut_bottom))
        };
        let at_heights = |found: &[f32], wanted: &[i32]| {
            found.len() == wanted.len() && found.iter().zip(wanted).all(|(y, w)| (y - *w as f32).abs() <= slack)
        };
        for variant in 0u32..32 {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let palm: Vec<((i32, i32, i32), BlockId)> = primitive_shared::worldgen::palm_cells(variant, lean)
                    .into_iter()
                    .map(|((dx, dy, dz), id)| ((ROOT.0 + dx, GROUND + dy, ROOT.1 + dz), id))
                    .collect();
                let trunk: Vec<((i32, i32, i32), BlockId)> =
                    palm.iter().copied().filter(|(_, id)| block_kind(*id) == BLOCK_PALM_TRUNK).collect();
                let foot = trunk.iter().map(|((_, y, _), _)| *y).min().expect("a palm has a trunk");
                let head = trunk.iter().map(|((_, y, _), _)| *y).max().unwrap() + 1;
                let name = format!("palm {variant:#x} leaning {lean:?}");

                let (tops, bottoms) = cuts(&|x, y, z| palm.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id));
                assert!(tops.is_empty(), "{name}, crowned, is cut at {tops:?}");
                assert!(at_heights(&bottoms, &[foot]), "{name}, standing on nothing, is cut underneath at {bottoms:?}, not at its root {foot}");

                let (tops, bottoms) = cuts(&|x, y, z| trunk.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id));
                assert!(at_heights(&tops, &[head]), "{name}, its crown taken, is cut on top at {tops:?}, not at {head}");
                assert!(at_heights(&bottoms, &[foot]), "{name}, its crown taken, is cut underneath at {bottoms:?}, not at {foot}");
            }
        }

        let straight = primitive_shared::worldgen::palm_cells(0, (1, 0))[0].1;
        let broken = |x: i32, y: i32, z: i32| {
            if (x, z) == ROOT && (GROUND + 1..=GROUND + 6).contains(&y) && y != GROUND + 3 {
                straight
            } else {
                BLOCK_AIR
            }
        };
        let (tops, bottoms) = cuts(&broken);
        assert!(at_heights(&tops, &[GROUND + 3, GROUND + 7]), "a trunk broken through is cut on top at {tops:?}");
        assert!(at_heights(&bottoms, &[GROUND + 1, GROUND + 4]), "a trunk broken through is cut underneath at {bottoms:?}");
    }

    #[test]
    fn a_piece_of_branch_is_drawn_exactly_where_it_is_walked_into() {
        // **"добавь коллизию веткам".** A bough was walked into as its whole
        // cell and a twig not at all, while both were drawn as a post with
        // arms. Both are `branch::wood_boxes` now, drawn by `branch_block` and
        // collided by `geometry::for_each_block_box`, and this is what says
        // the two agree on what is emitted against what is collided: every
        // quad of wood the mesher draws lies on a face of a box the collider
        // stops at, and every box the collider stops at has wood drawn on it.
        // The generator's saplings, young trees and grown trees, on dirt.
        use primitive_shared::types::{is_branch, BLOCK_DIRT, BLOCK_LEAVES};
        const ROOT: (i32, i32) = (8, 8);
        const GROUND: i32 = 20;
        let slack = BITE * T + 1e-4;
        for stage in 0..primitive_shared::worldgen::TREE_STAGES {
            for variant in [0u32, 1, 2, 7, 11, 40] {
                let wood: Vec<((i32, i32, i32), BlockId)> =
                    primitive_shared::worldgen::tree_stage_cells(stage, variant, BLOCK_LEAVES, |_, _| 0)
                        .expect("every stage has cells")
                        .into_iter()
                        .filter(|(_, id)| is_branch(*id))
                        .map(|((dx, dy, dz), id)| ((ROOT.0 + dx, GROUND + dy, ROOT.1 + dz), id))
                        .collect();
                let at = |x: i32, y: i32, z: i32| {
                    if y <= GROUND {
                        return BLOCK_DIRT;
                    }
                    wood.iter().rev().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id)
                };
                // The dirt's own faces are all at or under its top; wood stands
                // over it.
                let quads: Vec<[[f32; 3]; 4]> = mesh_of(&cache_of(at))
                    .vertices
                    .chunks_exact(4)
                    .map(|q| [q[0].position, q[1].position, q[2].position, q[3].position])
                    .filter(|q| q.iter().any(|p| p[1] > GROUND as f32 + 1.0 + 1e-3))
                    .collect();
                let mut walked = Vec::new();
                for &((x, y, z), id) in &wood {
                    if !(0..16).contains(&x) || !(0..16).contains(&z) || at(x, y, z) != id {
                        continue;
                    }
                    primitive_shared::geometry::for_each_block_box(id, x, y, z, |dx, dy, dz| at(x + dx, y + dy, z + dz), |min, max| {
                        walked.push((min, max))
                    });
                }
                let name = format!("stage {stage} tree {variant:#x}");
                assert!(!walked.is_empty(), "{name} has no wood to walk into");
                let on_a_face = |q: &[[f32; 3]; 4], (min, max): &([f32; 3], [f32; 3])| {
                    let inside = q.iter().all(|p| (0..3).all(|a| p[a] >= min[a] - slack && p[a] <= max[a] + slack));
                    let flat = (0..3).any(|a| {
                        q.iter().all(|p| (p[a] - min[a]).abs() <= slack) || q.iter().all(|p| (p[a] - max[a]).abs() <= slack)
                    });
                    inside && flat
                };
                for q in &quads {
                    assert!(walked.iter().any(|b| on_a_face(q, b)), "{name} draws wood at {q:?} where nothing is walked into");
                }
                for b in &walked {
                    // A limb out of the side of a trunk as wide as its cell has
                    // an arm of no length, which nothing can meet.
                    if (0..3).any(|a| b.1[a] - b.0[a] < 1e-4) {
                        continue;
                    }
                    assert!(quads.iter().any(|q| on_a_face(q, b)), "{name} is walked into at {b:?}, where no wood is drawn");
                }
            }
        }
    }

    #[test]
    fn a_palm_crown_reads_each_frond_off_the_cells_round_it() {
        // **"листва пальмы тоже странная".** The crown was cubes of leaf. Each
        // cell is drawn now as what it is in the star the generator grew --
        // the heart, a bunch of coconuts, or a length of a frond running one
        // of eight ways -- and it has only the cells round it to know that by
        // (`CrownPart`). Every palm the generator grows, whole and with every
        // bunch picked, each of whose crown cells has to come out as its place
        // in the star relative to the top of the trunk says.
        use primitive_shared::types::{block_kind, BLOCK_PALM_COCONUTS, BLOCK_PALM_FRONDS, BLOCK_PALM_FRONDS_PICKED, BLOCK_PALM_TRUNK};
        for variant in 0u32..64 {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                for picked in [false, true] {
                    let cells: Vec<((i32, i32, i32), BlockId)> = primitive_shared::worldgen::palm_cells(variant, lean)
                        .into_iter()
                        .map(|(c, id)| (c, if picked && id == BLOCK_PALM_COCONUTS { BLOCK_PALM_FRONDS_PICKED } else { id }))
                        .collect();
                    let top = cells
                        .iter()
                        .filter(|(_, id)| block_kind(*id) == BLOCK_PALM_TRUNK)
                        .max_by_key(|((_, y, _), _)| *y)
                        .map(|(c, _)| *c)
                        .expect("a palm has a trunk");
                    let at = |x: i32, y: i32, z: i32| cells.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id);
                    for &((x, y, z), id) in &cells {
                        if !matches!(block_kind(id), BLOCK_PALM_FRONDS | BLOCK_PALM_COCONUTS) {
                            continue;
                        }
                        let (dx, dy, dz) = (x - top.0, y - top.1, z - top.2);
                        let expected = if (dx, dy, dz) == (0, 1, 0) {
                            CrownPart::Heart
                        } else if dy == 0 && dx.abs() + dz.abs() == 1 {
                            CrownPart::Bunch { away: (dx, dz) }
                        } else {
                            let length = match (dy, dx.abs().max(dz.abs())) {
                                (1, 1) => FrondLength::Root,
                                (1, _) => FrondLength::Middle,
                                _ => FrondLength::Tip,
                            };
                            CrownPart::Frond { heading: (dx.signum(), dz.signum()), length }
                        };
                        assert_eq!(
                            crown_part(|ox, oy, oz| at(x + ox, y + oy, z + oz)),
                            expected,
                            "palm {variant:#x} leaning {lean:?}{}: the crown cell {:?} from the top of its trunk",
                            if picked { ", picked" } else { "" },
                            (dx, dy, dz)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_coconut_hangs_against_the_bark_of_the_top_of_its_trunk() {
        // **"кокосы часто даже не касаются ствола".** Every palm the generator
        // grows -- each height, both bends, every count and first side of
        // bunches, leaning each way -- and every nut each bunch can hang: from
        // what the bunch sees (one column round itself, as at a chunk's edge),
        // the nut has to come within a sixteenth of the bark of the top piece
        // as that piece draws it, and not have its middle inside the bark.
        use primitive_shared::palm::{slices_beside, trunk_slices, TrunkSlice};
        use primitive_shared::types::{block_kind, BLOCK_PALM_COCONUTS, BLOCK_PALM_TRUNK, BLOCK_SAND};
        let (mut nuts, mut worst) = (0usize, 0.0f32);
        for height in 0u32..4 {
            for bend in 0u32..2 {
                for bunches in 0u32..16 {
                    let variant = height | bend << 4 | bunches << 10;
                    for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                        let cells = primitive_shared::worldgen::palm_cells(variant, lean);
                        let at = |x: i32, y: i32, z: i32| {
                            if y <= 0 {
                                return BLOCK_SAND;
                            }
                            cells.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id)
                        };
                        for &((x, y, z), id) in &cells {
                            if block_kind(id) != BLOCK_PALM_COCONUTS {
                                continue;
                            }
                            let CrownPart::Bunch { away } = crown_part(|dx, dy, dz| at(x + dx, y + dy, z + dz)) else {
                                panic!("palm {variant:#x} leaning {lean:?}: the bunch at {:?} is not drawn as one", (x, y, z));
                            };
                            let seen: Vec<TrunkSlice> = slices_beside(away, |dx, dy, dz| {
                                if dx.abs() > 1 || dz.abs() > 1 { BLOCK_AIR } else { at(x + dx, y + dy, z + dz) }
                            })
                            .collect();
                            let (tx, tz) = (x - away.0, z - away.1);
                            assert_eq!(block_kind(at(tx, y, tz)), BLOCK_PALM_TRUNK);
                            // The trunk's bark in the bunch's cell, in sixteenths.
                            let bark: Vec<([f32; 3], [f32; 3])> = trunk_slices(at(tx, y, tz), |dx, dy, dz| at(tx + dx, y + dy, tz + dz))
                                .map(|slice| {
                                    let (lo, hi) = slice.bounds();
                                    let shift = [-away.0 as f32, 0.0, -away.1 as f32];
                                    (std::array::from_fn(|a| (lo[a] + shift[a]) * 16.0), std::array::from_fn(|a| (hi[a] + shift[a]) * 16.0))
                                })
                                .collect();
                            for (centre, radius) in bunch_nuts(away, &seen) {
                                let gap = bark
                                    .iter()
                                    .map(|(lo, hi)| {
                                        (0..3)
                                            .map(|a| (lo[a] - (centre[a] + radius)).max(centre[a] - radius - hi[a]).max(0.0).powi(2))
                                            .sum::<f32>()
                                            .sqrt()
                                    })
                                    .fold(f32::MAX, f32::min);
                                let swallowed = bark.iter().any(|(lo, hi)| (0..3).all(|a| lo[a] < centre[a] && centre[a] < hi[a]));
                                assert!(
                                    gap <= 1.0 && !swallowed,
                                    "palm {variant:#x} leaning {lean:?}: a nut of the bunch at {:?} hanging {away:?} is {gap} sixteenths \
                                     from its bark{}",
                                    (x, y, z),
                                    if swallowed { ", with its middle inside it" } else { "" }
                                );
                                worst = worst.max(gap);
                                nuts += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(nuts > 1000, "only {nuts} nuts were asked about");
        println!("{nuts} nuts, the furthest {worst} sixteenths from its bark");
    }

    #[test]
    fn palm_fronds_rise_out_of_the_heart_and_droop_to_their_tips_instead_of_standing_as_cubes() {
        // What the crown looks like, on what is emitted: no face of a cube of
        // leaf anywhere in the cutout pass, fronds that reach past three cells
        // from the trunk, and leaf that is on average more than half a cell
        // lower out at the tips than at the heart.
        use primitive_shared::types::{block_kind, BLOCK_PALM_TRUNK};
        const ROOT: (i32, i32) = (7, 7);
        const GROUND: i32 = 20;
        let cells: Vec<((i32, i32, i32), BlockId)> = primitive_shared::worldgen::palm_cells(0x0802, (0, 1))
            .into_iter()
            .map(|((dx, dy, dz), id)| ((ROOT.0 + dx, GROUND + dy, ROOT.1 + dz), id))
            .collect();
        let top = cells
            .iter()
            .filter(|(_, id)| block_kind(*id) == BLOCK_PALM_TRUNK)
            .max_by_key(|((_, y, _), _)| *y)
            .map(|(c, _)| *c)
            .unwrap();
        let out = mesh_of(&cache_of(|x, y, z| cells.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id)));
        let cutout = &out.indices[out.solid_index_count as usize..out.sprite_end as usize];
        assert!(!cutout.is_empty(), "the crown drew no leaf");
        let (mut near, mut far, mut reach) = (Vec::new(), Vec::new(), 0.0f32);
        for quad in cutout.chunks(6) {
            let corners: Vec<[f32; 3]> = [quad[0], quad[1], quad[2], quad[5]].iter().map(|&i| out.vertices[i as usize].position).collect();
            for axis in 0..3 {
                let flat = corners.iter().all(|c| (c[axis] - corners[0][axis]).abs() < 1e-4);
                let span = |a: usize| corners.iter().map(|c| c[a]).fold(f32::MIN, f32::max) - corners.iter().map(|c| c[a]).fold(f32::MAX, f32::min);
                let others: Vec<f32> = (0..3).filter(|&a| a != axis).map(span).collect();
                assert!(
                    !(flat && others.iter().all(|s| (s - 1.0).abs() < 1e-3)),
                    "a face of a cube of leaf is still in the crown: {corners:?}"
                );
            }
            for c in corners {
                let out_from_trunk = ((c[0] - top.0 as f32 - 0.5).powi(2) + (c[2] - top.2 as f32 - 0.5).powi(2)).sqrt();
                reach = reach.max(out_from_trunk);
                let height = c[1] - top.1 as f32;
                if out_from_trunk < 0.6 {
                    near.push(height);
                } else if out_from_trunk > 3.0 {
                    far.push(height);
                }
            }
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        assert!(reach > 3.2, "the fronds reach only {reach} cells from the trunk");
        assert!(!near.is_empty() && !far.is_empty(), "no leaf at the heart or none at the tips");
        assert!(
            mean(&far) + 0.5 < mean(&near),
            "the fronds do not droop: leaf stands {} over the top piece at the heart and {} at the tips",
            mean(&near),
            mean(&far)
        );
    }

    /// How much of the sky over its own trunk a palm's crown hides, seen from
    /// straight under it: the share of a disc round the top of the trunk that
    /// the crown's leaf covers, on a grid of tenths, over every height, lean
    /// and bare corner the generator grows -- and how many leaf quads it took.
    fn crown_cover(radius: f32) -> (f32, usize) {
        use primitive_shared::types::{block_kind, BLOCK_PALM_TRUNK};
        const ROOT: (i32, i32) = (7, 7);
        const GROUND: i32 = 20;
        let (mut covered, mut asked, mut quads) = (0usize, 0usize, 0usize);
        let mut palms = 0usize;
        for variant in (0u32..64).step_by(5) {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let cells: Vec<((i32, i32, i32), BlockId)> = primitive_shared::worldgen::palm_cells(variant, lean)
                    .into_iter()
                    .map(|((dx, dy, dz), id)| ((ROOT.0 + dx, GROUND + dy, ROOT.1 + dz), id))
                    .collect();
                let top = cells
                    .iter()
                    .filter(|(_, id)| block_kind(*id) == BLOCK_PALM_TRUNK)
                    .max_by_key(|((_, y, _), _)| *y)
                    .map(|(c, _)| *c)
                    .expect("a palm has a trunk");
                let out = mesh_of(&cache_of(|x, y, z| cells.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id)));
                let cutout = &out.indices[out.solid_index_count as usize..out.sprite_end as usize];
                quads += cutout.len() / 6;
                let triangles: Vec<[[f32; 2]; 3]> = cutout
                    .chunks(3)
                    .map(|t| t.iter().map(|&i| [out.vertices[i as usize].position[0], out.vertices[i as usize].position[2]]))
                    .map(|mut corners| [corners.next().unwrap(), corners.next().unwrap(), corners.next().unwrap()])
                    .collect();
                let (cx, cz) = (top.0 as f32 + 0.5, top.2 as f32 + 0.5);
                let steps = (radius * 10.0) as i32;
                for iz in -steps..=steps {
                    for ix in -steps..=steps {
                        let (px, pz) = (cx + ix as f32 * 0.1, cz + iz as f32 * 0.1);
                        if (px - cx).powi(2) + (pz - cz).powi(2) > radius * radius {
                            continue;
                        }
                        asked += 1;
                        let inside = |[a, b, c]: &[[f32; 2]; 3]| {
                            let side = |p: [f32; 2], q: [f32; 2]| (q[0] - p[0]) * (pz - p[1]) - (q[1] - p[1]) * (px - p[0]);
                            let (s0, s1, s2) = (side(*a, *b), side(*b, *c), side(*c, *a));
                            (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0)
                        };
                        covered += usize::from(triangles.iter().any(inside));
                    }
                }
                palms += 1;
            }
        }
        (covered as f32 / asked.max(1) as f32, quads / palms.max(1))
    }

    #[test]
    fn a_palm_crown_hides_most_of_the_sky_over_its_trunk() {
        // **"почему то у них мало листвы".** Seen from the sand under it, a
        // crown was seven fronds a cell wide with the sky between them: 71 per
        // cent of the disc two cells round the trunk covered and 52 of the
        // disc three round, in 108 leaf quads. Eight fronds (`palm_cells`),
        // seven tenths of a cell to a side (`frond_half_width`) and two ranks
        // of leaflets (`frond_block`) made it 96 and 76, in 242.
        let (near, quads) = crown_cover(2.0);
        let (whole, _) = crown_cover(3.0);
        println!("a palm crown covers {:.0}% of the disc two cells round its trunk and {:.0}% of three, in {quads} leaf quads", near * 100.0, whole * 100.0);
        assert!(near >= 0.9, "a palm crown covers only {:.0}% of the sky two cells round its trunk", near * 100.0);
        assert!(whole >= 0.7, "a palm crown covers only {:.0}% of the sky three cells round its trunk", whole * 100.0);
    }

    #[test]
    fn coconuts_hang_as_round_nuts_against_the_trunk_and_a_dropped_one_is_the_same_nut() {
        // **"кокосы надо добавлять как модели".** A bunch was a cube of frond
        // with brown blots in it. It is two or three nuts now, each three
        // boxes crossed through one middle, hanging in the upper half of the
        // cell pressed against the bark and not sunk in it; and a coconut in
        // the hand or on the ground is one of those nuts.
        //
        // **The piece stands on stone.** A lone piece over air with no step
        // beside it is what a bunch takes for the foot of a run whose step is
        // out of its sight (`palm::slices_beside`), and leans its nuts to
        // match; no palm in a world hangs one there -- felling brings it down.
        use primitive_shared::types::{BLOCK_COCONUT, BLOCK_PALM_COCONUTS, BLOCK_PALM_TRUNK, BLOCK_STONE};
        let slack = BITE * T + 1e-4;
        let out = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            (7, 20, 7) => BLOCK_PALM_TRUNK,
            (8, 20, 7) => BLOCK_PALM_COCONUTS,
            (7, 19, 7) => BLOCK_STONE,
            _ => BLOCK_AIR,
        }));
        // The trunk's slices stand over the middle of their cell; the nuts
        // are the boxes over the face the two cells share and past it.
        let nuts: Vec<([f32; 3], [f32; 3])> = boxes_of(&out.vertices).into_iter().filter(|(lo, hi)| lo[0] + hi[0] > 2.0 * 7.8).collect();
        assert!(nuts.len() >= 6 && nuts.len().is_multiple_of(3), "a bunch is {} boxes, not two or three nuts of three", nuts.len());
        // The trunk's top piece is eight sixteenths wide: its bark is at 7.75,
        // and a nut is pressed into it by at most half a sixteenth.
        for (lo, hi) in &nuts {
            assert!(lo[1] >= 20.4 && hi[1] <= 21.0 + slack, "a nut hangs outside the upper half of its cell: {lo:?}..{hi:?}");
            assert!(lo[0] >= 7.75 - 0.5 * T - slack, "a nut is sunk into the bark of the trunk: {lo:?}");
            assert!(lo[0] <= 8.0, "a nut hangs away from the trunk: {lo:?}");
        }
        for nut in nuts.chunks(3) {
            let longest = |(lo, hi): &([f32; 3], [f32; 3])| (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap();
            let mut axes: Vec<usize> = nut.iter().map(longest).collect();
            axes.sort_unstable();
            assert_eq!(axes, [0, 1, 2], "a nut's three boxes are not crossed on three axes, so it is a crate: {nut:?}");
        }
        // ...and the one in the hand.
        assert!(has_carried_model(BLOCK_COCONUT), "a coconut is carried as its icon");
        let (mut model, mut model_indices) = (Vec::new(), Vec::new());
        assert!(carried_model(BLOCK_COCONUT, &FaceLayers::empty_for_test(), &mut model, &mut model_indices));
        assert_eq!(model.len(), 3 * 24, "a carried coconut is not one nut");
        let (low, high) = extent(&model);
        assert!(low.min_element() >= -slack && high.max_element() <= 1.0 + slack, "a carried coconut leaves its cell: {low}..{high}");
    }

    #[test]
    fn a_stem_of_kelp_is_one_ribbon_from_the_floor_to_its_top() {
        // **"ламинарии не связаны текстурой по вертикали".** Each cell of a
        // stem took its wander and its height from the cell, like a tuft:
        // the lengths stood aside from each other and stopped short of each
        // other. Every length of one stem is now the same two planes, each
        // starting at the height the one under it stops.
        use primitive_shared::types::{BLOCK_KELP, BLOCK_KELP_TOP, BLOCK_TALL_GRASS};
        for (x, z) in [(5, 5), (-3, 12), (100, -40), (7, 0)] {
            let lengths: Vec<[[[f32; 3]; 4]; 2]> = (31..=36)
                .map(|y| cross_planes([x, y, z], [x as f32, y as f32, z as f32], if y == 36 { BLOCK_KELP_TOP } else { BLOCK_KELP }))
                .collect();
            for pair in lengths.windows(2) {
                let (below, above) = (pair[0], pair[1]);
                for plane in 0..2 {
                    for corner in 0..4 {
                        assert_eq!(
                            (below[plane][corner][0], below[plane][corner][2]),
                            (above[plane][corner][0], above[plane][corner][2]),
                            "the stem at {x},{z} steps aside between two lengths"
                        );
                    }
                    assert!(
                        (below[plane][2][1] - above[plane][0][1]).abs() < 1e-5,
                        "the stem at {x},{z} stops at {} and goes on at {}",
                        below[plane][2][1],
                        above[plane][0][1]
                    );
                }
            }
        }
        // ...and a tuft of grass still wanders, which is what a meadow is.
        let (low, high) = (cross_planes([5, 31, 5], [5.0, 31.0, 5.0], BLOCK_TALL_GRASS), cross_planes([5, 32, 5], [5.0, 32.0, 5.0], BLOCK_TALL_GRASS));
        assert_ne!((low[0][0][0], low[0][0][2]), (high[0][0][0], high[0][0][2]), "two tufts stand in the same place");
    }

    #[test]
    fn the_kelp_picture_runs_on_from_the_top_of_one_length_to_the_bottom_of_the_next() {
        // The planes meeting is half of a stem that reads as one; the other
        // half is the picture. A length's top row sits under the next
        // length's bottom row -- kelp's under kelp's, and kelp's under the
        // top's -- and every ribbon in one has to go on within a texel in
        // the other, or the seam is drawn into the texture.
        let opaque_row = |name: &str, row: u32| -> Vec<bool> {
            let path = format!("{}/../assets/textures/plants/{name}.png", env!("CARGO_MANIFEST_DIR"));
            let picture = image::open(&path).unwrap_or_else(|e| panic!("{path}: {e}")).to_rgba8();
            (0..picture.width()).map(|x| picture.get_pixel(x, row)[3] > 0).collect()
        };
        let runs_on = |top: &[bool], bottom: &[bool]| {
            let near = |row: &[bool], x: usize| (x.saturating_sub(1)..=(x + 1).min(row.len() - 1)).any(|i| row[i]);
            (0..top.len()).all(|x| (!top[x] || near(bottom, x)) && (!bottom[x] || near(top, x)))
        };
        let kelp_top_row = opaque_row("kelp", 0);
        assert!(kelp_top_row.iter().any(|&o| o), "the top row of kelp is empty");
        assert!(runs_on(&kelp_top_row, &opaque_row("kelp", 15)), "kelp does not run on into the kelp over it");
        assert!(runs_on(&kelp_top_row, &opaque_row("kelp_top", 15)), "kelp does not run on into the top of its stem");
    }

    #[test]
    fn a_fish_trap_in_a_pool_holds_its_water_and_the_water_round_it_draws_no_wall() {
        // **"у ловушки для рыб проблемы с рендером под водой".** The trap is
        // solid to the rules (`types::BLOCK_FISH_TRAP`), so the water round it
        // drew a wall of surface on every face of it -- in the plane of the
        // wicker, blended over it -- and its own cell drew no water: from
        // under the river a basket of blue glass with air in it, and from
        // above a dry hole in the surface. See `trap_in_water`.
        use primitive_shared::types::BLOCK_FISH_TRAP;
        let pool = |x: i32, z: i32| (3..=7).contains(&x) && (3..=7).contains(&z);
        let out = mesh_of(&cache_of(|x, y, z| match y {
            _ if y < 10 => BLOCK_STONE,
            // One in the pool, and one dry on the ground beside it.
            10 if (x, z) == (5, 5) || (x, z) == (12, 12) => BLOCK_FISH_TRAP,
            10 if pool(x, z) => BLOCK_WATER,
            10 if x >= 10 => BLOCK_AIR,
            10 => BLOCK_STONE,
            _ => BLOCK_AIR,
        }));
        let quads_of = |range: &[u32]| -> Vec<Vec<[f32; 3]>> {
            range.chunks(6).map(|q| [q[0], q[1], q[2], q[5]].iter().map(|&i| out.vertices[i as usize].position).collect()).collect()
        };
        let span = |q: &Vec<[f32; 3]>, a: usize| {
            (q.iter().map(|c| c[a]).fold(f32::MAX, f32::min), q.iter().map(|c| c[a]).fold(f32::MIN, f32::max))
        };
        let within = |q: &Vec<[f32; 3]>, cx: f32, cz: f32| {
            let ((x0, x1), (z0, z1)) = (span(q, 0), span(q, 2));
            x0 > cx - 0.01 && x1 < cx + 1.01 && z0 > cz - 0.01 && z1 < cz + 1.01
        };
        let cutout = quads_of(&out.indices[out.solid_index_count as usize..out.sprite_end as usize]);
        for (cx, cz) in [(5.0, 5.0), (12.0, 12.0)] {
            assert!(
                cutout.iter().any(|q| within(q, cx, cz) && span(q, 1).0 > 10.99),
                "the wicker of the trap at {cx},{cz} is not drawn in the cutout pass"
            );
        }
        let blended = quads_of(&out.indices[out.sprite_end as usize..]);
        assert!(
            blended.iter().any(|q| {
                let (y0, y1) = span(q, 1);
                (y1 - y0).abs() < 1e-3 && y0 > 10.5 && y0 < 10.95 && within(q, 5.0, 5.0)
            }),
            "the trap in the pool holds no water: no surface over its cell"
        );
        assert!(!blended.iter().any(|q| within(q, 12.0, 12.0)), "the dry trap on the ground was filled with water");
        for q in &blended {
            let ((x0, x1), (y0, y1), (z0, z1)) = (span(q, 0), span(q, 1), span(q, 2));
            let wall_across_x = (x1 - x0).abs() < 1e-3 && (x0 - 5.0).abs().min((x0 - 6.0).abs()) < 1e-3 && z0 < 5.9 && z1 > 5.1;
            let wall_across_z = (z1 - z0).abs() < 1e-3 && (z0 - 5.0).abs().min((z0 - 6.0).abs()) < 1e-3 && x0 < 5.9 && x1 > 5.1;
            assert!(
                !((wall_across_x || wall_across_z) && y0 < 10.9 && y1 > 10.1),
                "a wall of water is drawn in the plane of the trap's wicker: {q:?}"
            );
        }
    }

    #[test]
    fn a_drowned_snag_is_a_post_standing_in_its_pool_and_not_a_hole_in_it() {
        // **"болотные ветки не заполнены водой и вытесняют ее".** A snag's
        // foot is drowned wood (`types::BLOCK_DROWNED_BOUGH`): the post of
        // bark is drawn, the cell's own water is drawn round it with its
        // surface over it, and the water beside it draws no wall against it.
        use primitive_shared::types::{branch, drowned};
        let pool = |x: i32, z: i32| (3..=7).contains(&x) && (3..=7).contains(&z);
        let out = mesh_of(&cache_of(|x, y, z| match y {
            _ if y < 10 => BLOCK_STONE,
            10 if (x, z) == (5, 5) => drowned(branch(12)),
            10 if pool(x, z) => BLOCK_WATER,
            10 => BLOCK_STONE,
            11 if (x, z) == (5, 5) => branch(8),
            _ => BLOCK_AIR,
        }));
        // The post runs from its foot on the stone to the dry piece over it,
        // so its corners are at the floor of the water cell, inside the snag's
        // column and off the cell's edges -- which nothing else there has.
        let solid = &out.indices[..out.solid_index_count as usize];
        assert!(
            solid.iter().any(|&i| {
                let p = out.vertices[i as usize].position;
                p[0] > 5.05 && p[0] < 5.95 && p[2] > 5.05 && p[2] < 5.95 && (p[1] - 10.0).abs() < 0.01
            }),
            "no bark is drawn in the water"
        );
        let blended = &out.indices[out.sprite_end as usize..];
        let quads: Vec<Vec<[f32; 3]>> = blended
            .chunks(6)
            .map(|q| [q[0], q[1], q[2], q[5]].iter().map(|&i| out.vertices[i as usize].position).collect())
            .collect();
        let span = |q: &Vec<[f32; 3]>, a: usize| {
            (q.iter().map(|c| c[a]).fold(f32::MAX, f32::min), q.iter().map(|c| c[a]).fold(f32::MIN, f32::max))
        };
        assert!(
            quads.iter().any(|q| {
                let ((x0, x1), (y0, y1), (z0, z1)) = (span(q, 0), span(q, 1), span(q, 2));
                (y1 - y0).abs() < 1e-3 && y0 > 10.5 && x0 <= 5.5 && x1 >= 5.5 && z0 <= 5.5 && z1 >= 5.5
            }),
            "the pool has no surface over the snag"
        );
        for q in &quads {
            let ((x0, x1), (y0, y1), (z0, z1)) = (span(q, 0), span(q, 1), span(q, 2));
            let wall_across_x = (x1 - x0).abs() < 1e-3 && (x0 - 5.0).abs().min((x0 - 6.0).abs()) < 1e-3 && z0 < 5.9 && z1 > 5.1;
            let wall_across_z = (z1 - z0).abs() < 1e-3 && (z0 - 5.0).abs().min((z0 - 6.0).abs()) < 1e-3 && x0 < 5.9 && x1 > 5.1;
            assert!(
                !((wall_across_x || wall_across_z) && y0 < 10.9 && y1 > 10.1),
                "the water beside the snag draws a wall against it: {q:?}"
            );
        }
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

    /// **The pass split is an index range and nothing else**, so the
    /// range has to be exact.
    ///
    /// `render` draws `[0..solid)` opaque, `[solid..sprite_end)` as
    /// cutouts -- both writing depth -- and `[sprite_end..)` blended
    /// with depth writes off and back faces kept. A water face that
    /// landed in either of the first two would be drawn opaque *and*
    /// would write depth, which is a lake as a slab of poured concrete
    /// with the world under it cut away; a leaf that landed in the last
    /// would stop writing depth and start compositing, which is a
    /// canopy that shows the trunk through itself in whatever order the
    /// chunks happened to be drawn.
    ///
    /// The test beside this one counts how many vertices carry the flag
    /// and neither of those faults changes that count. This walks the
    /// indices, which is the thing the draw calls actually use.
    #[test]
    fn every_index_in_the_blended_range_points_at_water_and_none_outside_it_does() {
        // All four buckets at once: stone, a lid of water on it, a
        // canopy over that and a tuft standing in the open. A scene
        // with an empty bucket proves nothing about the boundary
        // between it and the next.
        let out = mesh_of(&cache_of(|x, y, z| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            5 if (x + z) % 3 == 0 => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        }));
        assert!(out.solid_index_count > 0, "no stone in the scene");
        assert!(out.leaf_end > out.solid_index_count, "no leaves in the scene");
        assert!(
            (out.indices.len() as u32) > out.sprite_end,
            "no water in the scene"
        );

        let translucent =
            |index: &u32| out.vertices[*index as usize].light() & TRANSLUCENT_BIT != 0;
        assert!(
            out.indices[..out.sprite_end as usize].iter().all(|i| !translucent(i)),
            "a water face was drawn by a depth-writing pass"
        );
        assert!(
            out.indices[out.sprite_end as usize..].iter().all(translucent),
            "something that is not water was drawn by the blended pass"
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

    /// What the depth count costs the mesher, on the worst chunk there
    /// is for it: a full column of ocean.
    ///
    /// ```text
    /// cargo test -p primitive_client --release --lib
    ///     what_the_water_depth_scan_costs -- --ignored --nocapture
    /// ```
    ///
    /// Here rather than in a comment, because the in-game counter
    /// cannot answer it: three identical world loads reported
    /// `mesh_time/s` of 4.5, 10.8 and 8.1 ms, which is a spread far
    /// wider than anything this could add. A number that noisy is not a
    /// measurement, and "it is surely cheap" is not one either.
    #[test]
    #[ignore]
    fn what_the_water_depth_scan_costs() {
        // Sixteen blocks of water over stone: every column is liquid to
        // the top, so every surface cell pays the full walk down.
        let cache = cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4..=19 => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        // The *minimum* of many runs rather than the mean. A mean on
        // this machine wanders by a fifth of a millisecond between
        // identical runs -- which is the whole size of what is being
        // measured -- because the mean includes every scheduler
        // interruption. The fastest run is the one where nothing else
        // happened, and that is the number a change moves.
        let rounds = 400;
        let mut best = f64::MAX;
        let mut vertices = 0usize;
        for _ in 0..rounds {
            let start = std::time::Instant::now();
            let out = mesh_of(&cache);
            best = best.min(start.elapsed().as_secs_f64() * 1000.0);
            vertices = out.vertices.len();
        }
        println!("ocean chunk: {best:.3} ms per mesh, {vertices} vertices");
    }

    /// **A water face has to say how much water is under it**, or the
    /// sea cannot close over its own bed.
    ///
    /// The bug: `WATER_ALPHA` was one constant applied to every water
    /// fragment in the world, so ten blocks of ocean hid what lay under
    /// them exactly as poorly as a one-block puddle. Skylight loses
    /// three levels per block of water, so a shelving sea bed comes out
    /// in hard brightness terraces -- and through a fixed 28% window
    /// those terraces read, from a beach, as light dots and short
    /// horizontal dashes strewn over dark blue water. Measured on the
    /// headland of world `12345`: 142 levels of luminance above the
    /// water's own median, against 14 with the bed taken away
    /// altogether.
    ///
    /// This checks the number the shader needs, at its source. It is
    /// the *depth of the column*, counted from the face's own cell
    /// downward -- not the height of the water, not the distance to the
    /// bed measured from the surface of the sea.
    /// The top corners of the water surface, as `(x, z, y)` for every
    /// vertex of a translucent +Y face.
    fn surface_corners(out: &MeshBuffers) -> Vec<(f32, f32, f32)> {
        out.vertices
            .iter()
            .filter(|v| v.light() & TRANSLUCENT_BIT != 0 && (v.light() >> 10) & 7 == 0)
            .map(|v| (v.position[0], v.position[2], v.position[1]))
            .collect()
    }

    /// A shallow cell beside a full one slopes between them: the corners
    /// they share sit at the mean of the two heights, and the corners
    /// each has to itself sit at its own.
    ///
    /// This is what lets every depth be drawn at its own height without
    /// the step-with-a-wall that once forced them all to one: two cells
    /// compute a shared corner from the same four cells, so they meet.
    #[test]
    fn a_shallow_cell_beside_a_full_one_slopes_between_them() {
        use primitive_shared::fluid::{surface_height, with_depth};
        let shallow = with_depth(2);
        let out = mesh_of(&cache_of(move |x, y, z| match (x, y, z) {
            (_, 0..=3, _) => BLOCK_STONE,
            (8, 4, 8) => BLOCK_WATER,
            (9, 4, 8) => shallow,
            _ => BLOCK_AIR,
        }));
        let full = 4.0 + surface_height(BLOCK_WATER);
        let low = 4.0 + surface_height(shallow);
        let between = (full + low) / 2.0;
        let corners = surface_corners(&out);
        assert_eq!(corners.len(), 8, "two water tops, four corners each");
        for (x, _, y) in corners {
            let expected = if x == 8.0 {
                full
            } else if x == 9.0 {
                between
            } else {
                low
            };
            assert!(
                (y - expected).abs() < 1e-5,
                "corner at x={x} drawn at {y}, expected {expected}"
            );
        }
    }

    /// A sea is nothing but full cells, and it has not moved: every
    /// corner of its surface is exactly where it was before depths were
    /// drawn.
    #[test]
    fn a_sea_of_full_cells_is_still_flat_at_the_old_height() {
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4..=6 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }));
        let expected = 6.0 + (1.0 - primitive_shared::fluid::SURFACE_DROP);
        let corners = surface_corners(&out);
        assert!(!corners.is_empty());
        for (x, z, y) in corners {
            assert!((y - expected).abs() < 1e-6, "sea corner at ({x}, {z}) drawn at {y}");
        }
    }

    /// A spill an eighth deep is a film, and the cell it sits in reads
    /// as wadeable rather than swimmable to everything that asks.
    #[test]
    fn the_thin_end_of_a_spill_is_drawn_thin_and_waded_not_swum() {
        use primitive_shared::fluid::{covers, surface_height, with_depth};
        let film = with_depth(1);
        assert!(surface_height(film) < 0.2, "one eighth should be ankle deep");
        assert!(covers(film, 0.05), "...but still water at the floor");
        assert!(!covers(film, 0.5), "...and not at the knee");
        let out = mesh_of(&cache_of(move |x, y, z| match (x, y, z) {
            (_, 0..=3, _) => BLOCK_STONE,
            (8, 4, 8) => film,
            _ => BLOCK_AIR,
        }));
        for (_, _, y) in surface_corners(&out) {
            assert!((y - (4.0 + surface_height(film))).abs() < 1e-6);
        }
    }

    /// A carcass is drawn as the animal lying on its side: more than a
    /// box, resting on the ground, no taller than the animal is wide,
    /// and no more than a body's length from its cell.
    #[test]
    fn a_carcass_is_the_animal_lying_on_its_side_and_not_a_box() {
        use primitive_shared::animals::Species;
        use primitive_shared::types::BLOCK_CARCASS_DEER;
        let out = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            (_, 0..=3, _) => BLOCK_STONE,
            (8, 4, 8) => BLOCK_CARCASS_DEER,
            _ => BLOCK_AIR,
        }));
        // Everything above the stone floor is the carcass.
        let body: Vec<[f32; 3]> =
            out.vertices.iter().map(|v| v.position).filter(|p| p[1] > 4.0 + 1e-6).collect();
        assert!(body.len() > 24, "a carcass of {} vertices is a box", body.len());
        let low = body.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let high = body.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(low > 4.0 && low < 4.05, "the carcass floats or sinks: lowest point {low}");
        let half = crate::logic::animal_model::half_extents(Species::Deer);
        let standing = 2.0 * half.y;
        let width = 2.0 * half.x;
        assert!(high - low < standing, "the carcass stands: {} tall", high - low);
        assert!(
            (high - low - width).abs() < 0.1,
            "on its side it should be {width} tall, is {}",
            high - low
        );
        for p in &body {
            assert!(
                (p[0] - 8.5).abs() < half.z + 0.6 && (p[2] - 8.5).abs() < half.z + 0.6,
                "a part lies at {p:?}"
            );
        }
    }

    /// The ground under a carcass is still drawn: the carcass covers
    /// nothing, so the floor's top face is emitted as if the cell were
    /// air. A carcass that hid it left a hole with the sky in it.
    #[test]
    fn the_ground_under_a_carcass_is_still_drawn() {
        use primitive_shared::types::BLOCK_CARCASS_BOAR;
        let out = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            (_, 0..=3, _) => BLOCK_STONE,
            (8, 4, 8) => BLOCK_CARCASS_BOAR,
            _ => BLOCK_AIR,
        }));
        // The floor is one flat sheet: if the cell under the carcass had
        // been left out, the sheet would have a hole in it and the four
        // corners of that hole would be vertices of the +Y faces around
        // it. A whole floor has no vertex at any of them.
        let hole_corners = [(8.0, 8.0), (9.0, 8.0), (8.0, 9.0), (9.0, 9.0)];
        let floor_vertex_at = |x: f32, z: f32| {
            out.vertices.iter().any(|v| {
                (v.light() >> 10) & 7 == 0
                    && (v.position[1] - 4.0).abs() < 1e-6
                    && (v.position[0] - x).abs() < 1e-6
                    && (v.position[2] - z).abs() < 1e-6
            })
        };
        for (x, z) in hole_corners {
            assert!(
                !floor_vertex_at(x, z),
                "the floor has a corner at ({x}, {z}): a hole under the carcass, with the sky in it"
            );
        }
    }

    /// Once the skin is off, a carcass wears flesh: the picture changes
    /// with the stage, so a player can see how far the butchering got.
    #[test]
    fn a_skinned_carcass_shows_flesh_and_not_fur() {
        use primitive_shared::animals::{carcass_at_stage, Species};
        let whole = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            (_, 0..=3, _) => BLOCK_STONE,
            (8, 4, 8) => carcass_at_stage(Species::Boar, 0),
            _ => BLOCK_AIR,
        }));
        let skinned = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
            (_, 0..=3, _) => BLOCK_STONE,
            (8, 4, 8) => carcass_at_stage(Species::Boar, 1),
            _ => BLOCK_AIR,
        }));
        let layers = |m: &MeshBuffers| -> std::collections::BTreeSet<u32> {
            m.vertices.iter().filter(|v| v.position[1] > 4.0 + 1e-6).map(|v| v.tex_layer()).collect()
        };
        assert_eq!(whole.vertices.len(), skinned.vertices.len(), "the same animal, differently dressed");
        assert_ne!(layers(&whole), layers(&skinned), "skinning changed nothing on screen");
        assert_eq!(layers(&skinned).len(), 1, "flesh is one picture all over");
    }

    #[test]
    fn a_water_face_carries_the_depth_of_its_own_column() {
        for depth in [1usize, 2, 5, 9] {
            let floor = 4;
            let out = mesh_of(&cache_of(move |_, y, _| {
                if (y as usize) < floor {
                    BLOCK_STONE
                } else if (y as usize) < floor + depth {
                    BLOCK_WATER
                } else {
                    BLOCK_AIR
                }
            }));
            let depths: std::collections::BTreeSet<u32> = out
                .vertices
                .iter()
                .filter(|v| v.light() & TRANSLUCENT_BIT != 0)
                .map(|v| v.tint())
                .collect();
            assert_eq!(
                depths,
                [depth as u32].into_iter().collect(),
                "every water face of a {depth}-deep sea should say {depth}"
            );
        }
    }

    /// The byte has three readings and they must not meet.
    ///
    /// Foliage tints run 1..=225; a water depth is not one, and the
    /// thing that keeps them apart is that a translucent face is never
    /// foliage. Said out loud here, because the day someone gives glass
    /// or ice the translucent bit is the day this stops being true by
    /// accident.
    #[test]
    fn nothing_but_water_carries_a_water_depth() {
        let out = mesh_of(&cache_of(|_, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            5 => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        }));
        for v in &out.vertices {
            let translucent = v.light() & TRANSLUCENT_BIT != 0;
            if translucent {
                let depth = v.tint();
                assert!(
                    (1..=MAX_WATER_DEPTH).contains(&depth),
                    "a water face said its depth was {depth}"
                );
            }
        }
        // ...and the opaque world still uses the byte for what it
        // always did: a leaf is tinted, and a tint is not a depth.
        assert!(out
            .vertices
            .iter()
            .any(|v| v.light() & TRANSLUCENT_BIT == 0 && v.tint() > 0));
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

    #[test]
    fn a_waterfall_meets_the_pool_it_lands_in_with_no_hole_around_its_foot() {
        // **"A cube of water hanging in the air."** A player broke the
        // plug on the test world's water plot, stood in the pool it
        // pours into, and photographed a block of water floating over a
        // dark gap with the pool's bed visible underneath it.
        //
        // What made the gap: the cell the fall lands in is submerged, so
        // it used to be drawn to the *top* of its cell -- a twelfth of a
        // block proud of the pool around it. The faces it shared with
        // that pool were culled, because water against water always is;
        // its own top face was culled too, because the falling cell
        // covered it. Nothing was drawn in the band at all, so the
        // surface had a cell-sized hole in it and the column above
        // started a twelfth of a block higher than the water it stood
        // in. Now the *upper* cell hangs down to meet the lower one
        // instead -- see `fluid::underhang`.
        //
        // So a column of water is drawn at exactly three heights: the
        // bed it stands on, and one surface per cell, each a
        // `SURFACE_DROP` below the top of its own cell. A vertex on a
        // whole-numbered cell floor above the bed is the bug coming
        // back.
        let surface = 4.0 + primitive_shared::fluid::surface_height(BLOCK_WATER);
        let levels = [4.0, surface, surface + 1.0, surface + 2.0];
        let mut highest = f32::MIN;
        let mut lowest_above_the_bed = f32::MAX;
        for point in water_vertices(|x, y, z| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            // The fall: two cells of it, over the middle of the pool.
            5 | 6 if x == 8 && z == 8 => BLOCK_WATER,
            _ => BLOCK_AIR,
        }) {
            assert!(
                levels.iter().any(|level| (level - point[1]).abs() < 1e-5),
                "a water vertex at {} is neither the bed nor one of the levels                  a column is drawn at ({levels:?})",
                point[1]
            );
            highest = highest.max(point[1]);
            if point[1] > 4.0 + 1e-5 {
                lowest_above_the_bed = lowest_above_the_bed.min(point[1]);
            }
        }
        assert!(
            (highest - (surface + 2.0)).abs() < 1e-5,
            "the top of the fall is at {highest}, not {}",
            surface + 2.0
        );
        assert!(
            (lowest_above_the_bed - surface).abs() < 1e-5,
            "the fall's lowest edge is at {lowest_above_the_bed}, and the pool it lands              in has its surface at {surface}: that difference is the hole"
        );
    }

    #[test]
    fn only_the_top_of_water_is_faded_by_the_column_under_it() {
        // **"A patch of somebody else's transparency, hanging in the
        // middle of a waterfall."** A player photographed a fall three
        // cells wide with a wedge in its upper third, hatched and a
        // different shade, with soft edges.
        //
        // What it was: the depth fade (`WATER_DEPTH_FADE` in
        // shader.wgsl) closes the water over its own bed with the
        // number of cells standing *under* the face. That is the right
        // measure for the top of a lake -- straight down is the way the
        // ray goes -- and the wrong one for a wall, where what the ray
        // crosses is how thick the water is sideways. A fall is one
        // cell thick and stands over a pool, so its sides were handed 4
        // and 5 and drawn at alpha 0.95 and 0.97 instead of 0.72.
        //
        // The wedge itself is the far wall of the column: the blended
        // pass draws with no back-face culling and, inside one chunk,
        // in emission order, so the far wall composites *over* the near
        // one. At 0.72 it blends into it and reads as a sheen; at 0.97
        // it replaces it and reads as a patch. So the number is what
        // has to be right, and this is the property that keeps it so:
        // the fade rides on the surface and on nothing else.
        let cache = super::transparency_tests::cache_of(|x, y, z| match y {
            0..=3 => BLOCK_STONE,
            4..=6 => BLOCK_WATER,
            // Two cells of fall, over the middle of the pool.
            7 | 8 if x == 8 && z == 8 => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        let mesh = super::transparency_tests::mesh_of(&cache);
        let mut tops = 0;
        let mut sides = 0;
        for index in &mesh.indices[mesh.sprite_end as usize..] {
            let v = &mesh.vertices[*index as usize];
            let face = (v.light() >> 10) & 7;
            if face == 0 {
                // The pool is three cells deep and the fall two more,
                // so every surface in the fixture is 3 or 5 -- and
                // never 1, or the fade would have nothing to do.
                assert!(
                    v.tint() == 3 || v.tint() == 5,
                    "a water surface carries depth {}, not the column under it",
                    v.tint()
                );
                tops += 1;
            } else {
                assert_eq!(
                    v.tint(),
                    1,
                    "a vertical face of water carries depth {} -- the fade is                      measuring the column below a wall again",
                    v.tint()
                );
                sides += 1;
            }
        }
        assert!(tops > 0 && sides > 0, "the fixture drew {tops} tops and {sides} sides");
    }

    /// **Every water quad around a waterfall, with the depth byte each
    /// one carries.** A diagnostic, not a test:
    ///
    /// ```text
    /// cargo test -p primitive_client --lib \
    ///     what_the_mesher_draws_around_a_waterfall -- --ignored --nocapture
    /// ```
    ///
    /// The picture a player sends can say "a patch of the wrong
    /// transparency" and cannot say which quad it is. This prints the
    /// plane, the extent and the tint byte of every blended quad in the
    /// fixture, which can.
    #[test]
    #[ignore = "a diagnostic: prints the water quads around a fall"]
    fn what_the_mesher_draws_around_a_waterfall() {
        let cache = super::transparency_tests::cache_of(|x, y, z| match y {
            0..=3 => BLOCK_STONE,
            4..=6 => BLOCK_WATER,
            7 | 8 if x == 8 && z == 8 => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        let mesh = super::transparency_tests::mesh_of(&cache);
        let mut seen: Vec<String> = Default::default();
        for tri in mesh.indices[mesh.sprite_end as usize..].chunks(3) {
            let v: Vec<&Vertex> = tri.iter().map(|i| &mesh.vertices[*i as usize]).collect();
            let axis_range = |a: usize| {
                let lo = v.iter().map(|p| p.position[a]).fold(f32::MAX, f32::min);
                let hi = v.iter().map(|p| p.position[a]).fold(f32::MIN, f32::max);
                (lo, hi)
            };
            let (x0, x1) = axis_range(0);
            let (y0, y1) = axis_range(1);
            let (z0, z1) = axis_range(2);
            // Only the column and the cells it lands in: the rest of
            // the fixture is a flat lake and says nothing.
            if x1 < 6.0 || x0 > 11.0 || z1 < 6.0 || z0 > 11.0 {
                continue;
            }
            let face = (v[0].light() >> 10) & 7;
            let line = format!(
                "face {face}  x {x0:5.2}..{x1:5.2}  y {y0:5.2}..{y1:5.2}                   z {z0:5.2}..{z1:5.2}  depth {}",
                v[0].tint()
            );
            if seen.last() != Some(&line) {
                seen.push(line);
            }
        }
        for line in &seen {
            println!("{line}");
        }
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
    fn water_draws_nothing_against_ice_so_a_frozen_bay_has_no_false_surface_under_it() {
        // Half a pond frozen over (x < 8 at y = 5 is ice), half open. The
        // only water that should draw anything is the open surface: no
        // pane under the ice a twelfth short of it, and no side face at
        // the edge of the lid lying in the plane of the ice's own face.
        use primitive_shared::types::BLOCK_ICE;
        let surface = 5.0 + primitive_shared::fluid::surface_height(BLOCK_WATER);
        let points = water_vertices(|x, y, _| match y {
            0..=3 => BLOCK_STONE,
            4 => BLOCK_WATER,
            5 if x < 8 => BLOCK_ICE,
            5 => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        assert!(!points.is_empty(), "the open half drew no surface at all");
        for point in points {
            assert!(
                (point[1] - surface).abs() < 1e-5,
                "a water vertex at {point:?} is not on the open surface ({surface}):                  water drew a face under or against the ice"
            );
        }
    }

    /// **A bush standing in a pond holds the pond, and one standing out of
    /// it holds nothing** -- "сделай затопление листвы".
    ///
    /// The same cell of leaves twice, with the water at two heights round
    /// it. Sunk, the cell owes its own share of the surface, and without it
    /// there is a square hole in the pond with a bush in it. Dry -- the same
    /// leaves one cell above the water -- it owes nothing, or every hedge on
    /// a bank would be drawn wearing a pane of water.
    ///
    /// The crown itself is drawn either way, which is the other half of the
    /// bargain (`crown_in_water`): the cell draws both, not one instead of
    /// the other.
    #[test]
    fn a_bush_sunk_in_a_pond_draws_the_water_over_it_and_one_on_the_bank_does_not() {
        use primitive_shared::types::BLOCK_LEAVES;
        const AT: (i32, i32, i32) = (8, 5, 8);
        let pond = |brim: i32| {
            move |x: i32, y: i32, z: i32| match y {
                _ if (x, y, z) == AT => BLOCK_LEAVES,
                0..=3 => BLOCK_STONE,
                _ if y <= brim => BLOCK_WATER,
                _ => BLOCK_AIR,
            }
        };
        // Over the leaf cell's own footprint, above its floor: the surface
        // it owes, and nothing else reaches there.
        let over_the_bush = |points: Vec<[f32; 3]>| {
            points
                .into_iter()
                .filter(|p| {
                    p[1] > AT.1 as f32 + 0.5
                        && (p[0] - AT.0 as f32).abs() < 1.01
                        && (p[2] - AT.2 as f32).abs() < 1.01
                })
                .count()
        };
        assert!(
            over_the_bush(water_vertices(pond(AT.1))) > 0,
            "a bush sunk in a pond is a dry hole in the surface"
        );
        assert_eq!(
            over_the_bush(water_vertices(pond(AT.1 - 1))),
            0,
            "a bush standing over the water drew water in its own cell"
        );
        for brim in [AT.1, AT.1 - 1] {
            let mesh = super::transparency_tests::mesh_of(&super::transparency_tests::cache_of(pond(brim)));
            let leaves = mesh.solid_index_count as usize..mesh.leaf_end as usize;
            assert!(!mesh.indices[leaves].is_empty(), "the bush itself went missing with the brim at {brim}");
        }
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
/// cargo test --release -p primitive_client --lib \
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

    /// **Every id meshes the same whether the mesher asked about it or
    /// remembered** (`plain_cube`): every kind, with each of its variant
    /// bits and the furniture's wood bits, standing on stone with air
    /// round it, meshed once asking every question and once from what
    /// that taught the table -- byte for byte the same. A model added to
    /// the list that asks the neighbours, or falls through after drawing,
    /// would be learned as a cube by the first cell and drawn as one by the
    /// next, and this is where it shows.
    #[test]
    fn every_id_meshes_the_same_whether_the_mesher_asked_or_remembered() {
        let ids: Vec<BlockId> = (1..=primitive_shared::types::KIND_MASK)
            .flat_map(|kind| (0u16..64).map(move |high| kind | (high << 10)))
            // Only woods there are: the bits past the last one name
            // nothing, and a pile of logs of no wood has no bark to draw.
            .filter(|&id| primitive_shared::types::furniture_wood(id) < primitive_shared::wood::WOODS.len())
            .collect();
        // Two cells apart on every axis, so no model leans on another;
        // eight by eight a layer, one layer every other block of height.
        let per_chunk = 8 * 8 * ((CHUNK_SIZE_Y - 2) / 2);
        let layers = FaceLayers::by_kind_for_test();
        let generator = primitive_shared::worldgen::WorldGen::new(0);
        let mut differing = Vec::new();
        for batch in ids.chunks(per_chunk) {
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for (i, id) in batch.iter().enumerate() {
                let (x, z, y) = ((i % 8) * 2, (i / 8 % 8) * 2, 1 + (i / 64) * 2);
                blocks[Chunk::index(x, y - 1, z)] = BLOCK_STONE;
                blocks[Chunk::index(x, y, z)] = *id;
            }
            let world = World(blocks);
            let pos = ChunkPos::new(0, 0);
            let mut light = LightMap::new();
            light.load_chunk(&world, pos);
            let mut cache = Neighbourhood::default();
            cache.fill(pos, &world, &light);
            let mesh = |ask: bool| {
                plain_cube::ASK_EVERYTHING.with(|flag| flag.set(ask));
                let mut out = MeshBuffers::default();
                build_mesh(pos, &cache, &layers, &generator, &mut out);
                plain_cube::ASK_EVERYTHING.with(|flag| flag.set(false));
                (bytemuck::cast_slice::<Vertex, u8>(&out.vertices).to_vec(), out.indices)
            };
            let asked = mesh(true);
            // Teach the table everything this batch can teach it...
            let _ = mesh(false);
            // ...and draw from it.
            if mesh(false) != asked {
                differing.push(batch[0]);
            }
        }
        assert!(differing.is_empty(), "batches starting at these ids mesh differently from the table: {differing:?}");
    }

    /// **What every block kind costs when it is built**, one kind at a
    /// time: sixty-four of it on a stone floor, two cells apart, and the
    /// triangles and mesher time that are more than the bare floor's,
    /// per placed cell.
    ///
    /// ```text
    /// cargo test --release -p primitive_client --lib what_each_kind_costs_where_it_stands \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// The generated world holds none of the geometry 1.5 added -- racks,
    /// log piles, stake bundles, hide frames, chests -- because players
    /// build it, so the benchmark scene cannot say what it costs. This
    /// can. The top of the table is where a base gets dear.
    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn what_each_kind_costs_where_it_stands() {
        const FLOOR: usize = 10;
        let floor = || {
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    for y in 0..=FLOOR {
                        blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                    }
                }
            }
            blocks
        };
        let layers = FaceLayers::by_kind_for_test();
        let generator = primitive_shared::worldgen::WorldGen::new(0);
        let mut out = MeshBuffers::default();
        let mut mesh = |blocks: Vec<BlockId>, rounds: usize| -> (usize, f64) {
            let world = World(blocks);
            let pos = ChunkPos::new(0, 0);
            let mut light = LightMap::new();
            light.load_chunk(&world, pos);
            let mut cache = Neighbourhood::default();
            cache.fill(pos, &world, &light);
            build_mesh(pos, &cache, &layers, &generator, &mut out);
            let mut best = f64::MAX;
            for _ in 0..rounds {
                let started = Instant::now();
                build_mesh(pos, &cache, &layers, &generator, &mut out);
                best = best.min(started.elapsed().as_secs_f64());
            }
            (out.indices.len() / 3, best)
        };
        let (bare_tris, bare_time) = mesh(floor(), 5);
        let mut rows = Vec::new();
        for kind in 1..=primitive_shared::types::KIND_MASK {
            let name = primitive_shared::types::block_name(kind);
            if name == primitive_shared::types::block_name(BlockId::MAX & primitive_shared::types::KIND_MASK) || kind == BLOCK_STONE {
                continue;
            }
            let mut blocks = floor();
            for i in 0..64 {
                let (x, z) = ((i % 8) * 2, (i / 8) * 2);
                blocks[Chunk::index(x, FLOOR + 1, z)] = kind;
            }
            let (tris, time) = mesh(blocks, 3);
            rows.push((name, tris.saturating_sub(bare_tris) as f64 / 64.0, (time - bare_time).max(0.0) * 1e6 / 64.0));
        }
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        println!("[kind] bare floor: {bare_tris} triangles, {:.3} ms", bare_time * 1e3);
        for (name, tris, us) in rows.iter().take(45) {
            println!("[kind] {name:28} {tris:7.1} triangles a cell  {us:6.2} us a cell");
        }
        rows.sort_by(|a, b| b.2.total_cmp(&a.2));
        for (name, tris, us) in rows.iter().take(25) {
            println!("[kind-time] {name:28} {tris:7.1} triangles a cell  {us:6.2} us a cell");
        }
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn what_the_mesher_spends_its_time_on() {
        // **Where the 2.7 ms of a chunk goes**, asked because the merge
        // ablation said the pass costs 0.99 ms of it and saves 5.7% of the
        // vertices -- a trade worth knowing the inside of before touching.
        // Everything not named below is the face loop, which is why the
        // total is measured the same way and the remainder is printed as
        // one line rather than assumed.
        //
        // **The clock is on for this test and this test only**, and the
        // total below is therefore the mesher *plus* the stopwatch: two
        // `Instant::now()` calls a cell. That is the price of a breakdown
        // and it is paid here on purpose; the tests that report a plain
        // ms-a-chunk figure (`how_long_meshing_takes`,
        // `measure_real_terrain`) leave it off so that what they time is
        // the mesher. See `phase_clock::ENABLED`.
        phase_clock::ENABLED.with(|on| on.set(true));
        const ROUNDS: usize = 200;
        let blocks = terrain();
        let world = World(blocks.clone());
        let pos = ChunkPos::new(0, 0);
        let mut light = LightMap::new();
        light.load_chunk(&world, pos);
        let mut cache = Neighbourhood::default();
        cache.fill(pos, &world, &light);
        let layers = FaceLayers::empty_for_test();
        let mut out = MeshBuffers::default();

        // Warm, and thrown away: the first build grows every buffer.
        build_mesh(pos, &cache, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
        let _ = phase_clock::take();

        // **The fastest batch, not the average** -- `bench_meshing` says
        // why, and the first version of this tool ignored it and reported
        // a 33% swing between two runs of identical code.
        const BATCHES: usize = 7;
        let (mut total, mut phases) = (f64::MAX, [0u64; phase_clock::NAMES.len()]);
        for _ in 0..BATCHES {
            let _ = phase_clock::take();
            let started = Instant::now();
            for _ in 0..ROUNDS {
                build_mesh(pos, &cache, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
            }
            let per_round = started.elapsed().as_secs_f64() * 1000.0 / ROUNDS as f64;
            let taken = phase_clock::take();
            if per_round < total {
                total = per_round;
                phases = taken;
            }
        }

        let mut named = 0.0;
        println!("build_mesh {total:.3} ms/chunk, {} vertices", out.vertices.len());
        for (name, ns) in phase_clock::NAMES.iter().zip(phases) {
            let ms = ns as f64 / 1e6 / ROUNDS as f64;
            named += ms;
            println!("  {name:<24} {ms:.3} ms  ({:.0}%)", ms / total * 100.0);
        }
        println!("  {:<24} {:.3} ms  ({:.0}%)", "the face loop, and all", total - named, (total - named) / total * 100.0);

        // **The same chunk lit flat**, which is what a coarse chunk gets
        // (`Neighbourhood::mark_coarse`). The difference is the price of
        // smooth lighting and ambient occlusion: eighteen samples a face
        // against one, gathered out of two arrays, on every face that is
        // drawn. It is the one number that says whether this loop is
        // thinking or fetching.
        let mut flat = Neighbourhood::default();
        flat.fill(pos, &world, &light);
        flat.mark_coarse();
        let mut best_flat = f64::MAX;
        for _ in 0..BATCHES {
            let started = Instant::now();
            for _ in 0..ROUNDS {
                build_mesh(pos, &flat, &layers, &primitive_shared::worldgen::WorldGen::new(0), &mut out);
            }
            best_flat = best_flat.min(started.elapsed().as_secs_f64() * 1000.0 / ROUNDS as f64);
        }
        let _ = phase_clock::take();
        println!("  {:<24} {best_flat:.3} ms  ({} vertices)", "...the same, lit flat", out.vertices.len());
        // Put back: the harness hands this thread the next test, and a
        // stopwatch left running would be charged to it.
        phase_clock::ENABLED.with(|on| on.set(false));
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
    /// How many block faces a mesh covers -- **cells, not quads.**
    ///
    /// These tests are about which faces exist, and merging coplanar
    /// faces means the two stopped being the same number: two snow
    /// blocks side by side still show ten faces, but their tops are one
    /// rectangle now. Counting quads would make every one of these tests
    /// a statement about the merge instead of about the culling it is
    /// checking. See `MERGE_COPLANAR_FACES`.
    fn faces_of(mesh: &MeshBuffers) -> usize {
        assert_eq!(mesh.vertices.len() % 4, 0, "something emitted a partial quad");
        mesh.vertices
            .chunks_exact(4)
            .map(|quad| {
                let span = |axis: usize| {
                    let values = quad.iter().map(|v| v.position[axis]);
                    let lo = values.clone().fold(f32::MAX, f32::min);
                    values.fold(f32::MIN, f32::max) - lo
                };
                // A quad covers the cells its two in-plane extents make;
                // the third extent is zero, and `max(1.0)` turns it into
                // the multiplicative identity rather than a zero that
                // would swallow the answer.
                let cells: f32 = (0..3).map(|axis| span(axis).round().max(1.0)).product();
                cells as usize
            })
            .sum()
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
    fn a_loose_stone_costs_its_relief_and_a_coating_one_quad() {
        // This is the one piece of decoration in every biome, so its cost
        // is multiplied by the whole world. It was one quad for as long as
        // stones were stickers; it is the relief now ("сделай палки камни
        // и прочее 3д моделями"), and exactly the relief -- no second copy
        // of the flat quad under it, nothing per neighbour. The number
        // itself is bounded where it is made (`relief::tests::
        // what_the_things_on_the_ground_cost`). A coating is still the
        // quad: it has no thickness to give.
        use primitive_shared::types::{BLOCK_ASH, BLOCK_DIRT, BLOCK_PEBBLE};
        let sprites = |block| {
            let out = mesh_of(&cache_of(|x, y, z| match (x, y, z) {
                (0, 0, 0) => BLOCK_DIRT,
                (0, 1, 0) => block,
                _ => BLOCK_AIR,
            }));
            (out.sprite_end - out.leaf_end) as usize
        };
        let layers = FaceLayers::empty_for_test();
        let relief = layers.relief(BLOCK_PEBBLE).expect("a pebble has a thickness");
        assert_eq!(sprites(BLOCK_PEBBLE), relief.triangles() * 3, "a stone is drawn as more than its relief");
        assert_eq!(sprites(BLOCK_ASH), 6, "a coating should be one quad");
    }

    #[test]
    fn a_chunk_told_its_leaves_are_solid_keeps_only_the_outside_of_its_crowns_and_says_so() {
        // **The see-through canopy setting, at the mesher.** Past the line
        // (`lod::leaves_see_through_at`) a crown is drawn in the opaque pass,
        // and an opaque crown shows nothing of its inside: every face
        // between two of its own leaf cells is a triangle nobody sees, and
        // in a wood that is half the canopy. So the chunk is built as a
        // shell, and the mesh carries the flag the renderer draws by -- a
        // shell that was cut out would show sky through a hollow tree.
        //
        // And the other end of the setting must change nothing: a chunk
        // told its leaves are see-through is byte for byte the chunk
        // nobody told, which is what the game meshed before there was a
        // setting.
        let inside = |v: i32, lo: i32, hi: i32| (lo..hi).contains(&v);
        let crown = |x: i32, y: i32, z: i32| {
            if inside(x, 2, 6) && inside(y, 10, 14) && inside(z, 2, 6) {
                BLOCK_LEAVES
            } else {
                BLOCK_AIR
            }
        };
        let untold = mesh_of(&cache_of(crown));
        let mut cache = cache_of(crown);
        cache.draw_leaves_solid(false);
        let see_through = mesh_of(&cache);
        assert!(!see_through.leaves_solid, "a see-through canopy said it was solid");
        assert_eq!(see_through.indices, untold.indices, "see-through everywhere changed the mesh");
        assert_eq!(see_through.leaf_end, untold.leaf_end);
        assert_eq!(
            bytemuck::cast_slice::<Vertex, u8>(&see_through.vertices),
            bytemuck::cast_slice::<Vertex, u8>(&untold.vertices),
        );
        cache.draw_leaves_solid(true);
        let solid = mesh_of(&cache);
        assert!(solid.leaves_solid, "a shell of a canopy did not say so, and would be cut out");

        // A leaf quad is on the crown's outside when all four corners lie
        // on one of its six walls.
        let on_wall = |mesh: &MeshBuffers, quad: &[u32]| {
            let corners: Vec<[f32; 3]> = quad.iter().map(|&i| mesh.vertices[i as usize].position).collect();
            (0..3).any(|axis| {
                let (lo, hi) = if axis == 1 { (10.0, 14.0) } else { (2.0, 6.0) };
                [lo, hi].iter().any(|wall: &f32| corners.iter().all(|c| (c[axis] - wall).abs() < 0.01))
            })
        };
        let leaf_quads = |mesh: &MeshBuffers| {
            mesh.indices[mesh.solid_index_count as usize..mesh.leaf_end as usize].chunks(6).map(|q| q.to_vec()).collect::<Vec<_>>()
        };
        assert!(
            leaf_quads(&see_through).iter().any(|q| !on_wall(&see_through, q)),
            "the see-through crown has no inside to lose, so this test proves nothing"
        );
        let shell = leaf_quads(&solid);
        assert!(!shell.is_empty(), "the solid crown lost its outside too");
        for quad in &shell {
            assert!(on_wall(&solid, quad), "a solid crown still draws a face inside itself");
        }
        // ...and every wall is still there, all six of them.
        for (axis, wall) in [(0, 2.0), (0, 6.0), (1, 10.0), (1, 14.0), (2, 2.0), (2, 6.0)] {
            assert!(
                shell.iter().any(|q| q.iter().all(|&i| (solid.vertices[i as usize].position[axis] - wall).abs() < 0.01)),
                "the solid crown lost its wall at {axis} = {wall}"
            );
        }
    }

    #[test]
    fn a_chunk_told_its_stones_lie_flat_draws_each_as_one_quad() {
        // Past `lod::RELIEF_CHUNKS` the thickness is under a pixel and the
        // memory is not: a world given it everywhere held 132 MB more. So the
        // far chunk -- and any chunk `lod::coarsen` rewrote -- is the flat
        // quad again, one per stone.
        use primitive_shared::types::{BLOCK_DIRT, BLOCK_PEBBLE};
        let mut cache = cache_of(|x, y, z| match (x, y, z) {
            (0, 0, 0) => BLOCK_DIRT,
            (0, 1, 0) => BLOCK_PEBBLE,
            _ => BLOCK_AIR,
        });
        cache.lay_stones_flat(true);
        let out = mesh_of(&cache);
        assert_eq!(out.sprite_end - out.leaf_end, 6, "a far stone kept its thickness");
        cache.lay_stones_flat(false);
        cache.mark_coarse();
        let out = mesh_of(&cache);
        assert_eq!(out.sprite_end - out.leaf_end, 6, "a coarse chunk's stone kept its thickness");
    }

    #[test]
    fn a_stone_stands_on_the_ground_and_no_face_of_it_lies_in_the_grass() {
        // **Standing on the ground, not in it and not over it**: its sides
        // start at the floor of the cell, or it hovers with daylight under
        // its edge, or sinks and loses its rim.
        //
        // **And nothing of it lies in the ground's own plane**, which is the
        // flicker the flat quad was lifted a fiftieth for: two coplanar faces
        // twenty blocks off land on one depth value and trade places as the
        // camera moves. The lowest upward face of a stone is its rim's top,
        // a texel up -- more than twice that fiftieth.
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
        let lowest = stone.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let highest = stone.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        assert_eq!(lowest, 1.0, "the stone does not stand on the ground");
        // Two texels of a picture laid 0.74 of a block across: a lump the
        // height of a hand, not a brick.
        assert!(highest > 1.05 && highest < 1.1, "the stone is {} tall", highest - 1.0);
        for vertex in &stone {
            assert!((0.0..=1.0).contains(&vertex.position[0]));
            assert!((0.0..=1.0).contains(&vertex.position[2]));
        }
        for quad in out.indices[out.leaf_end as usize..out.sprite_end as usize].chunks_exact(6) {
            let heights = quad.iter().map(|&i| out.vertices[i as usize].position[1]);
            let (low, high) = heights.clone().fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(y), h.max(y)));
            if low == high {
                assert!(low >= 1.02, "a face of the stone lies in the grass, at {low}");
            }
        }
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
        // The first quad of each stone is the rim's top, the whole picture:
        // where in its cell the picture's corner went says which way round
        // the stone was laid. The stone is turned, not its picture on it --
        // turning only the picture would leave the sides where they were.
        let per_stone = FaceLayers::empty_for_test().relief(BLOCK_PEBBLE).expect("a pebble has a thickness").drawn.len() * 6;
        let corners: std::collections::HashSet<[i32; 2]> = out.indices
            [out.leaf_end as usize..out.sprite_end as usize]
            .chunks(per_stone)
            .map(|stone| {
                let p = out.vertices[stone[0] as usize].position;
                [(p[0].rem_euclid(1.0) * 4.0) as i32, (p[2].rem_euclid(1.0) * 4.0) as i32]
            })
            .collect();
        assert!(corners.len() > 1, "every stone was laid the same way round");
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
    fn a_rack_of_four_cells_is_drawn_once_and_not_as_four_racks() {
        // The near bottom draws the whole frame; the other three cells used
        // to draw a one-cell frame each on top of it (`RackColumns`).
        use primitive_shared::types::{rack_cells, Facing};
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let cells = rack_cells((6, 1, 6), facing);
            // In the air, with no floor: a model standing on a floor stops
            // the floor's faces merging round it, which is geometry that is
            // not the rack's and would muddy the count.
            let whole = mesh_of(|x, y, z| cells.iter().find(|(at, _)| *at == (x, y, z)).map_or(BLOCK_AIR, |&(_, id)| id));
            let (mut v, mut i) = (Vec::new(), Vec::new());
            rack_block([0.0; 3], cells[0].1, RackColumns::Whole(0, 0), &FaceLayers::empty_for_test(), 0xFF, &mut v, &mut i);
            assert_eq!(
                whole.vertices.len(),
                v.len(),
                "{facing:?}: four cells of one rack drew more than the one frame -- the other three are drawing racks of their own"
            );
        }
    }

    /// The goods hung on a whole rack of this facing, alone: what
    /// `rack_block` draws past the bare frame.
    fn hung_goods_of(facing: primitive_shared::types::Facing, near: u8, far: u8) -> Vec<Vertex> {
        use primitive_shared::types::rack_cells;
        let block = rack_cells((0, 0, 0), facing)[0].1;
        let layers = FaceLayers::empty_for_test();
        let (mut bare, mut i) = (Vec::new(), Vec::new());
        rack_block([0.0; 3], block, RackColumns::Whole(0, 0), &layers, 0xFF, &mut bare, &mut i);
        let (mut v, mut i) = (Vec::new(), Vec::new());
        rack_block([0.0; 3], block, RackColumns::Whole(near, far), &layers, 0xFF, &mut v, &mut i);
        v.split_off(bare.len())
    }

    #[test]
    fn every_face_of_a_hung_good_carries_the_direction_its_winding_points() {
        // **"в сушилке пусть будет не плоская хрень, а рыба или мясо".** The
        // slab under the ridge became strips, sods, hides and fish, and a fish
        // is its picture's silhouette stood on end, turned by whatever slant
        // the picture was drawn at, and mirrored into a solid
        // (`push_hung_silhouette`). A mirror turns a quad inside out: miss
        // the reversal and the pipeline culls the half of every fish facing
        // the player and draws the half behind it -- the rack's own
        // inside-out bug (`every_face_of_a_model_box_is_wound_to_face_outward`)
        // over again, one fish at a time. So: every quad's light word names
        // the axis nearest its winding, and every face of a silhouette across
        // the plane it hangs in points away from that plane. (A draped sheet
        // has faces turned in toward the ridge, rightly: only the
        // silhouettes are one solid either side of a plane.)
        use primitive_shared::types::{self as t, rack_far_step, Facing};
        let silhouette = |goods: u8| {
            primitive_shared::rack::HANGING[goods as usize].is_some_and(|item| {
                ![t::BLOCK_HIDE, t::BLOCK_LEATHER, t::BLOCK_RAW_MEAT, t::BLOCK_DRIED_MEAT, t::BLOCK_SALTED_MEAT, t::BLOCK_DRIED_SALTED_MEAT, t::BLOCK_PEAT, t::BLOCK_DRIED_PEAT]
                    .contains(&item)
            })
        };
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let (dx, dz) = rack_far_step(facing);
            // Across the ridge, in the world.
            let across = glam::Vec3::new(-dz as f32, 0.0, dx as f32);
            for goods in 1..primitive_shared::rack::HANGING.len() as u8 {
                let v = hung_goods_of(facing, goods, goods);
                assert!(!v.is_empty(), "{facing:?}: row {goods} hangs nothing");
                // The plane the goods hang in: the middle of all of them
                // across the ridge.
                let middle = v.iter().map(|p| glam::Vec3::from_array(p.position).dot(across)).sum::<f32>() / v.len() as f32;
                for quad in v.chunks_exact(4) {
                    let p = |k: usize| glam::Vec3::from_array(quad[k].position);
                    let wound = (p(1) - p(0)).cross(p(2) - p(1)).normalize();
                    let face = ((quad[0].light() >> 10) & 7) as u8;
                    assert_eq!(
                        face,
                        crate::engine::item_model::nearest_face(wound),
                        "{facing:?} row {goods}: a face wound toward {wound:?} carries face index {face}"
                    );
                    let centre = (p(0) + p(1) + p(2) + p(3)) * 0.25;
                    let side = centre.dot(across) - middle;
                    if silhouette(goods) && wound.dot(across).abs() > 0.99 && side.abs() > 0.01 {
                        assert!(
                            wound.dot(across) * side > 0.0,
                            "{facing:?} row {goods}: a face {side} off the plane the goods hang in points back into them"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn hung_goods_stay_in_their_own_column_between_the_poles_and_off_the_ground() {
        // Each column is hung between the poles crossing at its end (x 1.2 to
        // 4.4 and 27.6 to 30.8 of `misc/drying_rack_2x2.bbmodel`) and the
        // joint of the ridge at 16. The slab this replaced ran twelve
        // sixteenths from x 2 and 18 and so stood inside both far poles; a
        // fish long enough to reach the grass would stand in it.
        use primitive_shared::types::Facing;
        for goods in 1..primitive_shared::rack::HANGING.len() as u8 {
            for (near, far, span) in [(goods, 0, 4.4..16.0), (0, goods, 16.0..27.6)] {
                for vertex in hung_goods_of(Facing::North, near, far) {
                    let [x, y, _] = vertex.position.map(|c| c * 16.0);
                    assert!(span.contains(&x), "row {goods}: hung at x {x}, outside its column {span:?}");
                    assert!((4.0..28.0).contains(&y), "row {goods}: hung at y {y}, on the ground or over the ridge");
                }
            }
        }
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
/// cargo test --release -p primitive_client --lib \
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

/// What the padded neighbourhood is allowed to contain, now that filling
/// it stops at the skyline.
#[cfg(test)]
mod filling_tests {
    use super::*;
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::types::{ChunkPos, BLOCK_STONE, CHUNK_VOLUME};
    use primitive_shared::worldgen::WorldGen;

    /// Nine chunks of real generated terrain, lit.
    ///
    /// Generated rather than hand-built because the thing under test is
    /// what happens *above* the skyline, and a fixture with a flat roof
    /// has one skyline rather than 256 of them.
    fn terrain_around(pos: ChunkPos, seed: u32) -> (ChunkManager, LightMap) {
        let generator = WorldGen::new(seed);
        let mut chunks = ChunkManager::new(4);
        let mut light = LightMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks.insert(generator.generate_chunk(ChunkPos::new(pos.x + dx, pos.z + dz)));
            }
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                light.load_chunk(&chunks, ChunkPos::new(pos.x + dx, pos.z + dz));
            }
        }
        (chunks, light)
    }

    /// `Neighbourhood::fill` written the plain way: every padded column,
    /// every cell of it, addressed one at a time through `Chunk::index`.
    ///
    /// Slow and obviously right. The real one copies whole rows and stops
    /// at the skyline -- two arguments about *how much has to be touched*,
    /// and this is what makes them checkable rather than merely argued.
    fn plainly(pos: ChunkPos, blocks: &ChunkManager, light: &LightMap) -> (Vec<BlockId>, Vec<u8>, i32) {
        let mut out_blocks = vec![BLOCK_AIR; PADDED_X * CHUNK_SIZE_Y * PADDED_Z];
        let mut out_light = vec![0u8; PADDED_X * CHUNK_SIZE_Y * PADDED_Z];
        let mut ceiling = 0;
        let origin_x = pos.x * CHUNK_SIZE_X as i32;
        let origin_z = pos.z * CHUNK_SIZE_Z as i32;

        for pz in 0..PADDED_Z {
            for px in 0..PADDED_X {
                let is_ours = px >= PAD as usize
                    && pz >= PAD as usize
                    && px < PADDED_X - PAD as usize
                    && pz < PADDED_Z - PAD as usize;
                let gx = origin_x + px as i32 - PAD;
                let gz = origin_z + pz as i32 - PAD;
                let (cpos, lx, lz) = ChunkPos::from_global(gx, gz);
                for y in 0..CHUNK_SIZE_Y {
                    let idx = padded_index(px, y, pz);
                    let cell = Chunk::index(lx, y, lz);
                    out_blocks[idx] = match blocks.chunk_data(cpos) {
                        Some(data) => data[cell],
                        None => blocks.block_at(gx, y as i32, gz).unwrap_or(UNKNOWN_BLOCK),
                    };
                    if is_ours && out_blocks[idx] != BLOCK_AIR {
                        ceiling = ceiling.max(y as i32 + 1);
                    }
                    out_light[idx] = match light.chunk_light(cpos) {
                        Some(data) => data.get(cell),
                        None => 0x0F,
                    };
                }
            }
        }
        (out_blocks, out_light, ceiling)
    }

    #[test]
    fn copying_rows_fills_the_neighbourhood_the_same_as_copying_cells() {
        for seed in [1337u32, 7, 2024] {
            let pos = ChunkPos::new(3, -5);
            let (chunks, light) = terrain_around(pos, seed);
            let (want_blocks, want_light, want_ceiling) = plainly(pos, &chunks, &light);

            let mut cache = Neighbourhood::default();
            cache.fill(pos, &chunks, &light);
            assert_eq!(cache.ceiling(), want_ceiling, "seed {seed}: skyline");

            // Only up to the plane the face loop can reach. Above it the
            // fill deliberately leaves the array alone -- which is the
            // point of the next test.
            let planes = (want_ceiling as usize + 1).min(CHUNK_SIZE_Y);
            let reach = planes * PADDED_X * PADDED_Z;
            assert_eq!(&cache.blocks[..reach], &want_blocks[..reach], "seed {seed}: blocks");
            assert_eq!(&cache.light[..reach], &want_light[..reach], "seed {seed}: light");
        }
    }

    #[test]
    fn a_recycled_neighbourhood_meshes_the_same_as_a_fresh_one() {
        // The mesher keeps a pool of these and hands the same allocation
        // to chunk after chunk (see `mesher::Mesher::take_cache`), and
        // the fill now stops at the skyline instead of writing the sky.
        // So a chunk with a low skyline inherits whatever the last chunk
        // left above it, and the whole safety of that rests on the face
        // loop never reading up there. This is that claim, checked
        // against the geometry rather than against the argument.
        let pos = ChunkPos::new(3, -5);
        let (chunks, light) = terrain_around(pos, 1337);

        // Something tall enough to write every plane, so the recycled
        // neighbourhood is as dirty as it can be.
        let mut tall = ChunkManager::new(4);
        let filled = ChunkPos::new(900, 900);
        tall.insert(Chunk {
            pos: filled,
            blocks: vec![BLOCK_STONE; CHUNK_VOLUME],
        });
        let mut tall_light = LightMap::new();
        tall_light.load_chunk(&tall, filled);

        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let generator = WorldGen::new(1337);

        let mut recycled = Neighbourhood::default();
        recycled.fill(filled, &tall, &tall_light);
        assert_eq!(recycled.ceiling(), CHUNK_SIZE_Y as i32, "the dirtying fill must reach the roof");
        recycled.fill(pos, &chunks, &light);
        let mut from_recycled = MeshBuffers::default();
        build_mesh(pos, &recycled, &layers, &generator, &mut from_recycled);

        let mut fresh = Neighbourhood::default();
        fresh.fill(pos, &chunks, &light);
        let mut from_fresh = MeshBuffers::default();
        build_mesh(pos, &fresh, &layers, &generator, &mut from_fresh);

        assert!(!from_fresh.indices.is_empty(), "the fixture has to mesh to something");
        assert_eq!(
            bytemuck::cast_slice::<Vertex, u8>(&from_recycled.vertices),
            bytemuck::cast_slice::<Vertex, u8>(&from_fresh.vertices),
            "a chunk's vertices must not depend on what the pooled neighbourhood held before it"
        );
        assert_eq!(from_recycled.indices, from_fresh.indices, "...nor its indices");
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn bench_fill() {
        use std::time::Instant;
        const ROUNDS: usize = 400;
        const BATCHES: usize = 9;

        let pos = ChunkPos::new(3, -5);
        let (chunks, light) = terrain_around(pos, 1337);
        let mut cache = Neighbourhood::default();

        // The fastest batch, for the reason `bench_meshing` gives.
        let mut best = f64::MAX;
        for _ in 0..BATCHES {
            let started = Instant::now();
            for _ in 0..ROUNDS {
                cache.fill(pos, &chunks, &light);
                std::hint::black_box(&cache);
            }
            best = best.min(started.elapsed().as_secs_f64() * 1000.0 / ROUNDS as f64);
        }
        println!(
            "\nNeighbourhood::fill: {best:.4} ms per chunk (MAIN THREAD), skyline {}",
            cache.ceiling()
        );
    }
}

/// What merging coplanar faces is allowed to change, and what it is not.
#[cfg(test)]
mod merging_tests {
    use super::*;
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::types::{ChunkPos, BLOCK_STONE, CHUNK_VOLUME};
    use primitive_shared::worldgen::WorldGen;

    /// Which world axis each half of a face's texture coordinate runs
    /// along, read straight off `face_uv`'s six expressions by hand.
    ///
    /// Written out again rather than derived from `UV_SOURCE`, which is
    /// the point: a test that asked the table what the table says would
    /// pass however the table was wrong. Getting this mapping backwards
    /// is the mistake a merged mesh actually makes, and what it looks
    /// like is a texture stretched across a wall one way and squeezed
    /// the other -- which no single-cell face can ever show, so nothing
    /// else in the suite would catch it.
    fn uv_axes(face: usize) -> (usize, usize) {
        match face {
            0 | 1 => (0, 2), // +Y, -Y: u runs along x, v along z
            2 | 3 => (2, 1), // +X, -X: u runs along z, v along y
            _ => (0, 1),     // +Z, -Z: u runs along x, v along y
        }
    }

    fn normal_axis_of(face: usize) -> usize {
        match face {
            0 | 1 => 1,
            2 | 3 => 0,
            _ => 2,
        }
    }

    #[test]
    fn the_axis_table_agrees_with_the_face_uv_it_stands_in_for() {
        for face in 0..6 {
            for x in [0.0f32, 1.0] {
                for y in [0.0f32, 1.0] {
                    for z in [0.0f32, 1.0] {
                        let corner = [x, y, z];
                        assert_eq!(
                            spanning_uv(face, corner, [1.0; 3]),
                            face_uv(face, corner),
                            "face {face}, corner {corner:?}"
                        );
                    }
                }
            }
        }
    }

    /// One quad as these tests want to read it: which of the six
    /// directions it faces, where its four corners are, and what
    /// texture coordinate each of them asks for.
    type Quad = (usize, [[f32; 3]; 4], [[f32; 2]; 4]);

    /// The polygons in a run of indices, as `(base vertex, triangles)`.
    ///
    /// One quad is six indices; a rectangle with a T-junction closed on
    /// its edge is more. Either way a polygon's vertices are one run,
    /// its triangles are consecutive, its *first* triangle names its
    /// lowest vertex (every emitter in `build_mesh` promises that -- see
    /// the rotated diagonal there and `triangulate_edged_rect`), and
    /// every later triangle of it names some index at or above that
    /// base and at or below the highest index seen so far. A triangle
    /// that names something below the base, or something past the top,
    /// starts the next polygon -- whichever order the polygons' vertex
    /// runs come in, which after filing by direction is not the order
    /// of the indices.
    fn polygons_in(indices: &[u32]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut triangles = indices.chunks_exact(3).peekable();
        while let Some(first) = triangles.next() {
            let base = *first.iter().min().expect("three indices") as usize;
            let mut top = *first.iter().max().expect("three indices") as usize;
            let mut count = 1usize;
            while let Some(next) = triangles.peek() {
                let low = *next.iter().min().expect("three indices") as usize;
                if low < base || low > top {
                    break;
                }
                top = top.max(*next.iter().max().expect("three indices") as usize);
                triangles.next();
                count += 1;
            }
            out.push((base, count));
        }
        out
    }

    /// The index groups of the two passes a merge can touch: the seven
    /// solid groups, then the leaves.
    fn index_groups(mesh: &MeshBuffers) -> Vec<&[u32]> {
        let mut groups = Vec::new();
        let mut from = 0usize;
        for end in mesh.solid_groups {
            groups.push(&mesh.indices[from..end as usize]);
            from = end as usize;
        }
        groups.push(&mesh.indices[mesh.solid_index_count as usize..mesh.leaf_end as usize]);
        groups
    }

    /// Every quad in the two passes a merge can touch. A polygon with
    /// T-junction vertices on its edges is read as the rectangle its
    /// first four vertices span: see `emit_merged`.
    fn quads(mesh: &MeshBuffers) -> Vec<Quad> {
        let mut out = Vec::new();
        for group in index_groups(mesh) {
            for (base, _) in polygons_in(group) {
                let face = ((mesh.vertices[base].light() >> 10) & 7) as usize;
                let mut positions = [[0.0f32; 3]; 4];
                let mut uvs = [[0.0f32; 2]; 4];
                for step in 0..4 {
                    positions[step] = mesh.vertices[base + step].position;
                    uvs[step] = mesh.vertices[base + step].uv();
                }
                out.push((face, positions, uvs));
            }
        }
        out
    }

    /// The quads of [`quads`] with every corner on a whole block -- which is
    /// every quad the merge can have made, a rectangle being whole cells by
    /// construction.
    ///
    /// **Not every quad of a terrain chunk is a cell face any more.** The
    /// ordinary world grows its trees out of pieces (`types::is_branch`), and
    /// a piece is a box a few sixteenths across in the solid pass, its
    /// picture quantised to fine texels. Asked whether bark six sixteenths
    /// wide "covers exactly its cells", the merge tests went red on geometry
    /// the merge never touches (0.3775 against 0.375, and two pieces of one
    /// bough rounding to the same cell). Filtering by the grid rather than by
    /// block keeps them blind to the next model too, without hiding a single
    /// rectangle.
    fn grid_quads(mesh: &MeshBuffers) -> Vec<Quad> {
        quads(mesh)
            .into_iter()
            .filter(|(_, positions, _)| positions.iter().flatten().all(|c| c.fract() == 0.0))
            .collect()
    }

    fn span(values: impl Iterator<Item = f32>) -> f32 {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for value in values {
            lo = lo.min(value);
            hi = hi.max(value);
        }
        hi - lo
    }

    /// A merged rectangle covers exactly the cells it says it does.
    ///
    /// **Exactly**, and that word is load-bearing now. The mesh grew a
    /// hair past its own edge for one revision, to plug the seam the
    /// merge opens; that turned out to be the wrong place to do it (see
    /// the note above `MERGE_COPLANAR_FACES`), and it has been backed
    /// out. If this ever has to be loosened again, something has put
    /// geometry back off the grid.
    #[track_caller]
    fn covers(actual: f32, cells: f32) {
        assert_eq!(actual, cells, "a rectangle of {cells} cells spans {actual}");
    }

    /// Every face the world plainly shows is drawn exactly once.
    ///
    /// **The instrument that found the last fourteen specks.** With the
    /// merge on, a horizontal sweep painted by face direction kept
    /// showing single pixels of a *side* face inside a *top* face at the
    /// foot of walls; with the merge off it did not. Cutting more found
    /// nothing, drawing merged quads first found nothing, and shattering
    /// every rectangle back to single cells -- the same geometry as no
    /// merge at all, through the merge's own emitter -- found nothing
    /// either: still fourteen. Identical positions, identical
    /// attributes, identical winding, a different count. The one thing
    /// left that could differ was the *set* of cells drawn: a face the
    /// direct path emits and the merge path loses. A lost top face at
    /// middle distance is a few pixels wide and one or two tall, seen
    /// edge-on, and what shows through it is the wall behind -- which is
    /// the speck exactly.
    ///
    /// So this asks the question the pictures could not: for every cell
    /// that certainly has a face -- an opaque cube with air on that side,
    /// inside the chunk so no neighbour is needed -- how many quads of
    /// that direction cover it? The answer has to be one. Zero is a hole
    /// and two is a fight, and the test names the cell either way.
    #[test]
    fn every_face_the_world_shows_is_drawn_exactly_once() {
        use primitive_shared::blocks::{definition, Shape};
        use primitive_shared::types::{is_opaque, BLOCK_AIR};
        let face_defs = faces();
        for seed in [1337u32, 7, 2024] {
            // **Not always chunk zero.** The walk needs an opaque cell
            // with air beside it, and a chunk that is all sea has none:
            // seed 7's origin is ocean, and it only ever had faces to
            // check because a cave carved an air pocket under the
            // seabed. Narrowing the caves took that away and the test
            // started reporting "the walk is broken" about a world that
            // was fine -- a guard that measured the cave frequency
            // without meaning to. It walks outward until it finds land.
            let mut found = 0usize;
            for pos in [
                ChunkPos::new(0, 0),
                ChunkPos::new(1, 0),
                ChunkPos::new(0, 1),
                ChunkPos::new(2, 2),
                ChunkPos::new(-3, 1),
                ChunkPos::new(5, -4),
            ] {
            let generator = WorldGen::new(seed);
            let chunk = generator.generate_chunk(pos);
            let at = |x: i32, y: i32, z: i32| -> BlockId {
                // Below the world is stone and above it is air, exactly
                // as `Neighbourhood::block` answers -- so no face is
                // expected on the floor of the world.
                if y < 0 {
                    return primitive_shared::types::BLOCK_STONE;
                }
                if !(0..CHUNK_SIZE_X as i32).contains(&x)
                    || !(0..CHUNK_SIZE_Y as i32).contains(&y)
                    || !(0..CHUNK_SIZE_Z as i32).contains(&z)
                {
                    return BLOCK_AIR;
                }
                chunk.blocks[Chunk::index(x as usize, y as usize, z as usize)]
            };
            let mesh = terrain_mesh(seed, pos);

            // How many quads of each direction cover each cell.
            let mut cover = vec![0u8; 6 * CHUNK_VOLUME];
            let cell_index = |face: usize, x: usize, y: usize, z: usize| {
                face * CHUNK_VOLUME + Chunk::index(x, y, z)
            };
            for (face, positions, _) in quads(&mesh) {
                let n = face_defs[face].normal_axis;
                let (a, b) = other_axes(n);
                let lo = |axis: usize| positions.iter().map(|p| p[axis]).fold(f32::MAX, f32::min);
                let hi = |axis: usize| positions.iter().map(|p| p[axis]).fold(f32::MIN, f32::max);
                // The cell a face belongs to sits behind it along its
                // normal: a +face at plane p is the top of cell p-1.
                // A face off the block grid is a partial block's -- a lip on
                // a generated slope (`worldgen::lips`) -- and this counts the
                // faces of whole cubes. Floored onto the grid it was counted
                // as a face of a cell it does not belong to.
                if lo(n).fract() != 0.0 {
                    continue;
                }
                let plane = lo(n) as i32 - face_defs[face].corners[0][n] as i32;
                if !(0..CHUNK_SIZE_Y.max(CHUNK_SIZE_X) as i32).contains(&plane) {
                    continue;
                }
                for ca in lo(a) as i32..hi(a) as i32 {
                    for cb in lo(b) as i32..hi(b) as i32 {
                        let mut c = [0i32; 3];
                        c[n] = plane;
                        c[a] = ca;
                        c[b] = cb;
                        if c[0] < 0 || c[1] < 0 || c[2] < 0
                            || c[0] >= CHUNK_SIZE_X as i32
                            || c[1] >= CHUNK_SIZE_Y as i32
                            || c[2] >= CHUNK_SIZE_Z as i32
                        {
                            continue;
                        }
                        cover[cell_index(face, c[0] as usize, c[1] as usize, c[2] as usize)] += 1;
                    }
                }
            }

            let mut checked = 0usize;
            for y in 0..CHUNK_SIZE_Y as i32 - 1 {
                for z in 1..CHUNK_SIZE_Z as i32 - 1 {
                    for x in 1..CHUNK_SIZE_X as i32 - 1 {
                        let id = at(x, y, z);
                        if !is_opaque(id) || definition(id).shape != Shape::Cube {
                            continue;
                        }
                        for (face, def) in face_defs.iter().enumerate() {
                            let nb = def.neighbor;
                            if at(x + nb[0], y + nb[1], z + nb[2]) != BLOCK_AIR {
                                continue;
                            }
                            checked += 1;
                            let n = cover[cell_index(face, x as usize, y as usize, z as usize)];
                            assert_eq!(
                                n, 1,
                                "seed {seed}: face {face} of {} at ({x},{y},{z}) is drawn {n} times",
                                primitive_shared::types::block_name(id)
                            );
                        }
                    }
                }
            }
            found = found.max(checked);
            if found > 50 {
                println!(
                    "seed {seed}: {checked} faces at chunk {},{}, each drawn exactly once",
                    pos.x, pos.z
                );
                break;
            }
            }
            assert!(found > 50, "seed {seed}: no chunk with land in six tries");
        }
    }

    /// The outward table and the face table agree, face by face.
    #[test]
    fn the_outward_table_is_the_face_table() {
        for (face, def) in faces().iter().enumerate() {
            let (axis, sign) = FACE_OUTWARD[face];
            assert_eq!(def.normal_axis, axis, "face {face}: axis");
            assert_eq!(def.neighbor[axis], sign, "face {face}: sign");
            assert!(def.neighbor.iter().filter(|c| **c != 0).count() == 1, "face {face}: one axis");
        }
    }

    /// Every cube face in the solid range sits in the group of the
    /// direction it looks, and the groups tile the range in order.
    ///
    /// The renderer leaves out whole groups by direction; a face filed
    /// in the wrong group would vanish from one side of the world and
    /// be shaded uselessly from the other. Both paths -- the merged
    /// rectangles and the faces drawn as themselves -- have to file
    /// correctly, so a generated chunk with both is what is checked.
    #[test]
    fn every_solid_face_is_filed_under_the_direction_it_looks() {
        for seed in [1337u32, 7, 2024] {
            // Chunk one for seed 7: its origin is open sea, which draws
            // a handful of polygons and says nothing about filing. See
            // `every_face_the_world_shows_is_drawn_exactly_once`, which
            // learned the same thing when the caves were narrowed.
            let at = if seed == 7 { ChunkPos::new(1, 0) } else { ChunkPos::new(0, 0) };
            let mesh = terrain_mesh(seed, at);
            let groups = mesh.solid_groups;
            assert!(groups.windows(2).all(|w| w[0] <= w[1]), "seed {seed}: groups out of order {groups:?}");
            assert_eq!(groups[6], mesh.solid_index_count, "seed {seed}: the last group ends the solid range");
            let mut filed = 0usize;
            for face in 0..6 {
                let range = &mesh.indices[groups[face] as usize..groups[face + 1] as usize];
                for (base, _) in polygons_in(range) {
                    let looks = ((mesh.vertices[base].light() >> 10) & 7) as usize;
                    assert_eq!(looks, face, "seed {seed}: a face looking {looks} filed under {face}");
                    filed += 1;
                }
            }
            // Polygons, not cells: a merged chunk is a few hundred of
            // them, and the sparsest of the three is comfortably over
            // this.
            assert!(filed > 100, "seed {seed}: only {filed} faces filed -- the walk is broken");
        }
    }

    /// The corners of a unit rectangle in (u, v), for the triangulation
    /// tests: A, B, C, D go round counter-clockwise.
    fn rect_uv(w: u32, h: u32, points: &EdgePoints) -> Vec<(f32, f32)> {
        let mut uv = vec![(0.0, 0.0), (w as f32, 0.0), (w as f32, h as f32), (0.0, h as f32)];
        uv.extend(points.bottom.iter().map(|u| (*u as f32, 0.0)));
        uv.extend(points.top.iter().map(|u| (*u as f32, h as f32)));
        uv.extend(points.left.iter().map(|v| (0.0, *v as f32)));
        uv.extend(points.right.iter().map(|v| (w as f32, *v as f32)));
        uv
    }

    fn twice_signed_area(uv: &[(f32, f32)], tri: &[u32]) -> f32 {
        let (p, q, r) = (uv[tri[0] as usize], uv[tri[1] as usize], uv[tri[2] as usize]);
        (q.0 - p.0) * (r.1 - p.1) - (r.0 - p.0) * (q.1 - p.1)
    }

    /// A rectangle with points on its edges is exactly `points + 2`
    /// triangles, every one of them with area, every one wound the
    /// same way, together covering the rectangle once, and every
    /// point a vertex of at least one of them.
    ///
    /// Tried on every way of scattering points over the four edges of
    /// small rectangles, including none on some edges and all on one,
    /// because the construction has a special case for each of those
    /// and the fan-from-a-corner it replaced fails precisely on the
    /// points that share an edge with the apex.
    #[test]
    fn a_rectangle_with_corners_on_its_edges_is_points_plus_two_whole_triangles() {
        let mut checked = 0usize;
        for (w, h) in [(2u32, 1u32), (1, 2), (3, 3), (5, 2), (2, 5), (4, 4)] {
            // Every subset of the interior lattice points of each edge.
            let along_u: Vec<u32> = (1..w).collect();
            let along_v: Vec<u32> = (1..h).collect();
            let subsets = |of: &Vec<u32>| -> Vec<Vec<u32>> {
                (0..1u32 << of.len())
                    .map(|mask| of.iter().enumerate().filter(|(i, _)| mask >> i & 1 == 1).map(|(_, x)| *x).collect())
                    .collect()
            };
            for bottom in subsets(&along_u) {
                for top in subsets(&along_u) {
                    for left in subsets(&along_v) {
                        for right in subsets(&along_v) {
                            let points = EdgePoints {
                                bottom: bottom.clone(),
                                top: top.clone(),
                                left: left.clone(),
                                right: right.clone(),
                            };
                            let uv = rect_uv(w, h, &points);
                            for ccw in [true, false] {
                                let mut out = Vec::new();
                                triangulate_edged_rect(&points, [0, 1, 2, 3], 4, ccw, &mut out);
                                let n = points.len();
                                assert_eq!(out.len(), 3 * (n + 2), "{w}x{h} {points:?}: triangle count");
                                assert!(out[..3].contains(&0), "{w}x{h} {points:?}: the first triangle names the base");
                                let mut covered = 0.0f32;
                                let mut used = vec![false; uv.len()];
                                for tri in out.chunks_exact(3) {
                                    let area = twice_signed_area(&uv, tri);
                                    assert!(
                                        if ccw { area > 0.0 } else { area < 0.0 },
                                        "{w}x{h} {points:?} ccw={ccw}: triangle {tri:?} has area {area}"
                                    );
                                    covered += area.abs();
                                    for i in tri {
                                        used[*i as usize] = true;
                                    }
                                }
                                assert_eq!(covered, 2.0 * (w * h) as f32, "{w}x{h} {points:?}: covers the rectangle once");
                                assert!(used.iter().all(|u| *u), "{w}x{h} {points:?}: a point is not a vertex of anything");
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1000, "only {checked} cases -- the enumeration is broken");
    }

    /// Closing the T-junctions never draws a triangle without area: a
    /// generated chunk's solid and leaf ranges hold no flat triangle.
    ///
    /// A flat triangle is what a fan from the wrong apex produces, and
    /// it is invisible on screen -- the crack it was meant to close
    /// simply stays open. So the property is checked on the real mesh,
    /// not only on the synthetic rectangles above.
    #[test]
    fn closing_the_t_junctions_never_draws_a_flat_triangle() {
        for seed in [1337u32, 7, 2024] {
            let mesh = terrain_mesh(seed, ChunkPos::new(0, 0));
            let indices = &mesh.indices[..mesh.leaf_end as usize];
            let mut with_points = 0usize;
            for tri in indices.chunks_exact(3) {
                let p = |i: usize| glam::Vec3::from(mesh.vertices[tri[i] as usize].position);
                let area = (p(1) - p(0)).cross(p(2) - p(0)).length();
                assert!(area > 1e-6, "seed {seed}: flat triangle {tri:?}");
            }
            // ...and the polygons are there to be checked: at least
            // one rectangle in the chunk took a point on its edge.
            for group in index_groups(&mesh) {
                with_points += polygons_in(group).iter().filter(|(_, count)| *count > 2).count();
            }
            assert!(with_points > 0, "seed {seed}: no rectangle had a corner on its edge -- the split is not running");
        }
    }

    /// Every polygon of a mesh as `(face, plane, vertices)`, the vertices
    /// being every distinct one its triangles name, in world space plus
    /// `shift`.
    fn polygon_vertices(mesh: &MeshBuffers, shift: [f32; 3]) -> Vec<(usize, f32, Vec<[f32; 3]>)> {
        let face_defs = faces();
        let mut out = Vec::new();
        for group in index_groups(mesh) {
            let mut triangles = group.chunks_exact(3).peekable();
            while let Some(first) = triangles.next() {
                let mut base = *first.iter().min().unwrap() as usize;
                let mut top = *first.iter().max().unwrap() as usize;
                let mut named: std::collections::BTreeSet<usize> =
                    first.iter().map(|i| *i as usize).collect();
                while let Some(next) = triangles.peek() {
                    let low = *next.iter().min().unwrap() as usize;
                    if low < base || low > top {
                        break;
                    }
                    base = base.min(low);
                    top = top.max(*next.iter().max().unwrap() as usize);
                    named.extend(next.iter().map(|i| *i as usize));
                    triangles.next();
                }
                let face = ((mesh.vertices[base].light() >> 10) & 7) as usize;
                let n = face_defs[face].normal_axis;
                let verts: Vec<[f32; 3]> = named
                    .iter()
                    .map(|&i| {
                        let p = mesh.vertices[i].position;
                        [p[0] + shift[0], p[1] + shift[1], p[2] + shift[2]]
                    })
                    .collect();
                let plane = verts[0][n];
                out.push((face, plane, verts));
            }
        }
        out
    }

    /// The T-junctions between the polygons of `own` and the vertices of
    /// `pool` (which should include `own`): a vertex of one polygon lying
    /// strictly inside an edge of another on the same face and plane.
    /// Each is reported as the edge and the vertex, in world space.
    fn t_junctions(
        own: &[(usize, f32, Vec<[f32; 3]>)],
        pool: &[(usize, f32, Vec<[f32; 3]>)],
    ) -> Vec<String> {
        let face_defs = faces();
        let mut found = Vec::new();
        for (face, plane, verts) in own {
            let n = face_defs[*face].normal_axis;
            let (a, b) = other_axes(n);
            let lo = |axis: usize| verts.iter().map(|v| v[axis]).fold(f32::MAX, f32::min);
            let hi = |axis: usize| verts.iter().map(|v| v[axis]).fold(f32::MIN, f32::max);
            let (a0, a1, b0, b1) = (lo(a), hi(a), lo(b), hi(b));
            // The four sides, each as the sorted run of this polygon's
            // vertices lying on it; consecutive pairs are the edges.
            let mut edges: Vec<([f32; 3], [f32; 3])> = Vec::new();
            for (fixed_axis, fixed, run_axis) in [(b, b0, a), (b, b1, a), (a, a0, b), (a, a1, b)] {
                let mut on: Vec<[f32; 3]> =
                    verts.iter().copied().filter(|v| (v[fixed_axis] - fixed).abs() < 1e-5).collect();
                on.sort_by(|p, q| p[run_axis].total_cmp(&q[run_axis]));
                for pair in on.windows(2) {
                    edges.push((pair[0], pair[1]));
                }
            }
            for (other_face, other_plane, other_verts) in pool {
                if other_face != face || (other_plane - plane).abs() > 1e-5 {
                    continue;
                }
                if std::ptr::eq(other_verts, verts) {
                    continue;
                }
                for v in other_verts {
                    // Outside this rectangle's box entirely: no edge to sit on.
                    if v[a] < a0 - 1e-5 || v[a] > a1 + 1e-5 || v[b] < b0 - 1e-5 || v[b] > b1 + 1e-5 {
                        continue;
                    }
                    for (p, q) in &edges {
                        let run = if (p[a] - q[a]).abs() > 1e-5 { a } else { b };
                        let fixed = if run == a { b } else { a };
                        if (v[fixed] - p[fixed]).abs() > 1e-5 {
                            continue;
                        }
                        let (s0, s1) = (p[run].min(q[run]), q[run].max(p[run]));
                        if v[run] > s0 + 1e-5 && v[run] < s1 - 1e-5 {
                            found.push(format!(
                                "face {face} plane {plane}: vertex {v:?} inside edge {p:?}..{q:?}"
                            ));
                        }
                    }
                }
            }
        }
        found
    }

    /// The mesh of a showcase chunk, built with its real neighbours.
    fn showcase_mesh(cx: i32, cz: i32) -> MeshBuffers {
        use crate::logic::chunk_manager::ChunkManager;
        use primitive_shared::lighting::LightMap;
        let mut chunks = ChunkManager::new(9);
        for dx in -1..=1 {
            for dz in -1..=1 {
                chunks.insert(primitive_shared::showcase::generate_chunk(ChunkPos::new(cx + dx, cz + dz)));
            }
        }
        let mut light = LightMap::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                light.load_chunk(&chunks, ChunkPos::new(cx + dx, cz + dz));
            }
        }
        let pos = ChunkPos::new(cx, cz);
        let mut cache = Neighbourhood::default();
        cache.fill(pos, &chunks, &light);
        let mut out = MeshBuffers::default();
        build_mesh(pos, &cache, &FaceLayers::empty_for_test(), &primitive_shared::worldgen::WorldGen::new(0), &mut out);
        out
    }

    /// **No vertex of any polygon lies inside an edge of another**, on
    /// the same face and plane -- within a chunk, and across the seam to
    /// the chunk east and the chunk north. This is the T-junction
    /// itself, found in the geometry rather than in a photograph: the
    /// photograph counts the pixels a T-junction happens to leak, and
    /// this names the edge.
    ///
    /// Run over the test world, whose plots are one material each and
    /// meet on the chunk seams -- the world the player photographed
    /// dotted lines in -- and over natural terrain.
    ///
    /// Only the polygons on the block lattice are looked at. A model --
    /// a drying rack, a carcass, a tuft of grass -- is boxes at
    /// sixteenths of a block, and its own faces meet each other in
    /// T-junctions by construction (a post standing on a rail); they are
    /// a few pixels wide, they never merge, and closing them is not what
    /// the greedy merge is for. A terrain rectangle's corners, T-points
    /// included, are whole blocks, so the sieve is exact.
    #[test]
    fn no_polygon_has_a_foreign_vertex_inside_its_edge() {
        let size_x = CHUNK_SIZE_X as f32;
        let size_z = CHUNK_SIZE_Z as f32;
        let on_lattice = |mesh: &MeshBuffers, shift: [f32; 3]| -> Vec<(usize, f32, Vec<[f32; 3]>)> {
            polygon_vertices(mesh, shift)
                .into_iter()
                .filter(|(_, _, verts)| verts.iter().all(|v| v.iter().all(|c| c.fract() == 0.0)))
                .collect()
        };
        let mut report = Vec::new();
        for (cx, cz) in [(2, 0), (2, 2), (1, 0), (0, 0), (-2, 2), (-1, 0)] {
            let own_mesh = showcase_mesh(cx, cz);
            let east = showcase_mesh(cx + 1, cz);
            let north = showcase_mesh(cx, cz + 1);
            let own = on_lattice(&own_mesh, [0.0, 0.0, 0.0]);
            let mut pool = own.clone();
            pool.extend(on_lattice(&east, [size_x, 0.0, 0.0]));
            pool.extend(on_lattice(&north, [0.0, 0.0, size_z]));
            let mut found = t_junctions(&own, &pool);
            found.truncate(6);
            if !found.is_empty() {
                report.push(format!("showcase chunk ({cx}, {cz}):\n  {}", found.join("\n  ")));
            }
        }
        // **Natural terrain, and against its neighbours.** This used
        // to hand `t_junctions` one chunk and itself, which asks only
        // half the question: `mark_corners` fills a lattice seventeen
        // cells wide -- a chunk's own -- so a rectangle that stops at
        // the seam cannot see the corner the neighbour puts on its
        // edge. Chosen because a bank crossing a chunk boundary is
        // exactly what a player standing under one is looking at, and
        // the sky behind it is what a crack there would show.
        for seed in [1337u32, 7] {
            for (cx, cz) in [(0, 0), (1, 0), (0, 1), (-1, 2), (3, -2)] {
                let own_mesh = terrain_mesh(seed, ChunkPos::new(cx, cz));
                let east = terrain_mesh(seed, ChunkPos::new(cx + 1, cz));
                let north = terrain_mesh(seed, ChunkPos::new(cx, cz + 1));
                let own = on_lattice(&own_mesh, [0.0, 0.0, 0.0]);
                let mut pool = own.clone();
                pool.extend(on_lattice(&east, [size_x, 0.0, 0.0]));
                pool.extend(on_lattice(&north, [0.0, 0.0, size_z]));
                let mut found = t_junctions(&own, &pool);
                let total = found.len();
                found.truncate(6);
                if !found.is_empty() {
                    report.push(format!(
                        "terrain seed {seed} chunk ({cx}, {cz}), {total} of them:\n  {}",
                        found.join("\n  ")
                    ));
                }
            }
        }
        assert!(report.is_empty(), "T-junctions left in the mesh:\n{}", report.join("\n"));
    }

    /// **A block that does not fill its cell puts no corner inside the
    /// edge of another face** -- on any face, in any plane.
    ///
    /// The dots the T-junction cure closed on whole cubes came back on
    /// blocks that are not whole. A campfire is a quarter of a block
    /// tall, so the top corners of its sides sit at y + 0.25 on the
    /// lattice line where it meets the wall beside it, strictly inside
    /// that wall's vertical edge; a dead player's pack does the same at
    /// y + 0.5. That is a T-junction exactly like the ones the merge used
    /// to make, and two things kept the cure from seeing it: `mark_corners`
    /// is a lattice of whole blocks, and it only ever puts vertices into
    /// *merged* rectangles -- a single wall face beside a fire is never
    /// touched. Behind the crack is the fire's own floor, whose top face
    /// the fire culls, so what shows through is the sky. Before `reach`
    /// grew the two off the lattice this room had twenty such corners.
    ///
    /// Counted over polygons whose corners all sit on the eighths of a
    /// block, which is every face of the block lattice and every face a
    /// part-height cube could put on it. A model's boxes are grown off
    /// that grid by `BITE` and overlap what they touch rather than meet
    /// it, which is what keeps them out of this and out of the report.
    #[test]
    fn a_part_height_block_puts_no_corner_inside_another_faces_edge() {
        use primitive_shared::types::{
            BLOCK_AIR, BLOCK_BACKPACK, BLOCK_BED, BLOCK_CAMPFIRE, BLOCK_PLANKS, BLOCK_STOOL,
            BLOCK_STRAW_BED, BLOCK_TABLE,
        };
        // A stone floor, a plank wall two blocks high along z = 8, and a
        // row against it: a hearth and a pack between plank blocks and
        // the furniture a room has in it (models, and the neighbours a
        // fire is really built beside), plus one table on its own.
        let row = [
            BLOCK_TABLE, BLOCK_PLANKS, BLOCK_BED, BLOCK_STOOL, BLOCK_STOOL, BLOCK_CAMPFIRE,
            BLOCK_PLANKS, BLOCK_BACKPACK, BLOCK_STRAW_BED, BLOCK_TABLE,
        ];
        let cache = super::plant_tests::cache_of(|x, y, z| {
            if y == 0 {
                return BLOCK_STONE;
            }
            if z == 8 && (1..=2).contains(&y) && (0..16).contains(&x) {
                return BLOCK_PLANKS;
            }
            if y == 1 && z == 7 && (3..3 + row.len() as i32).contains(&x) {
                return row[(x - 3) as usize];
            }
            if (x, y, z) == (6, 1, 4) {
                return BLOCK_TABLE;
            }
            BLOCK_AIR
        });
        let mesh = super::plant_tests::mesh_of(&cache);
        // The hearth and the pack are in the mesh at all -- their tops are
        // the one face at y 1.25 and the one at 1.5 -- or the count below
        // would pass on a room with nothing part-height left in it.
        let tops_at = |height: f32| {
            polygon_vertices(&mesh, [0.0; 3])
                .iter()
                .filter(|(face, _, verts)| *face == 0 && verts.iter().all(|v| v[1] == height))
                .count()
        };
        assert_eq!(tops_at(1.25), 1, "the fixture's campfire was not drawn");
        assert_eq!(tops_at(1.5), 1, "the fixture's pack was not drawn");
        let on_grid = |c: f32| (c * 8.0).fract() == 0.0;
        let polygons: Vec<_> = polygon_vertices(&mesh, [0.0; 3])
            .into_iter()
            .filter(|(_, _, verts)| verts.iter().all(|v| v.iter().all(|c| on_grid(*c))))
            .collect();
        let face_defs = faces();
        let mut found = Vec::new();
        for (index, (face, _, verts)) in polygons.iter().enumerate() {
            let (a, b) = other_axes(face_defs[*face].normal_axis);
            let lo = |axis: usize| verts.iter().map(|v| v[axis]).fold(f32::MAX, f32::min);
            let hi = |axis: usize| verts.iter().map(|v| v[axis]).fold(f32::MIN, f32::max);
            let mut edges: Vec<([f32; 3], [f32; 3], usize)> = Vec::new();
            for (fixed_axis, fixed, run_axis) in [(b, lo(b), a), (b, hi(b), a), (a, lo(a), b), (a, hi(a), b)] {
                let mut on: Vec<[f32; 3]> =
                    verts.iter().copied().filter(|v| (v[fixed_axis] - fixed).abs() < 1e-5).collect();
                on.sort_by(|p, q| p[run_axis].total_cmp(&q[run_axis]));
                for pair in on.windows(2) {
                    edges.push((pair[0], pair[1], run_axis));
                }
            }
            for (other, (other_face, _, other_verts)) in polygons.iter().enumerate() {
                if other == index {
                    continue;
                }
                for v in other_verts {
                    for (p, q, run) in &edges {
                        let beside = (0..3).filter(|axis| axis != run).all(|axis| (v[axis] - p[axis]).abs() < 1e-5);
                        let (s0, s1) = (p[*run].min(q[*run]), p[*run].max(q[*run]));
                        if beside && v[*run] > s0 + 1e-5 && v[*run] < s1 - 1e-5 {
                            found.push(format!(
                                "a corner of a face {other_face} polygon at {v:?} lies inside the edge {p:?}..{q:?} of a face {face} polygon"
                            ));
                        }
                    }
                }
            }
        }
        let total = found.len();
        found.sort();
        found.dedup();
        found.truncate(8);
        assert!(
            found.is_empty(),
            "{total} corner(s) inside another face's edge, first of them:\n  {}",
            found.join("\n  ")
        );
    }

    /// What the direction groups are worth, and what knowing where the
    /// faces actually are is worth on top of that.
    ///
    /// ```text
    /// cargo test -p primitive_client --lib what_the_face_groups_cost -- --ignored --nocapture
    /// ```
    ///
    /// `renderer::solid_ranges_facing` drops a group when the eye is on
    /// the blind side of the slab it lives in. For x and z that slab is
    /// the chunk's real sixteen blocks; for y it used to be the whole
    /// sixty-four-block column of the world, so the two vertical groups
    /// were sent from everywhere. This is the measurement that says how
    /// much that cost -- and it is the one to re-run before touching
    /// any of it, because the answer is a property of the terrain and
    /// not of the code.
    #[test]
    #[ignore]
    fn what_the_face_groups_cost() {
        use primitive_shared::types::CHUNK_SIZE_Y;
        let seed = 4242;
        let names = ["other", "+Y", "-Y", "+X", "-X", "+Z", "-Z"];
        let mut totals = [0u64; 7];
        let (mut sent_column, mut sent_measured, mut all) = (0u64, 0u64, 0u64);
        // A ring of chunks around the eye, the way a frame sees them,
        // with the eye at head height over the middle of it.
        let eye = [8.0f32, 46.0, 8.0];
        for cz in -3..=3 {
            for cx in -3..=3 {
                let pos = ChunkPos::new(cx, cz);
                let mesh = terrain_mesh(seed, pos);
                let mut from = 0u32;
                for (group, &end) in mesh.solid_groups.iter().enumerate() {
                    totals[group] += (end - from) as u64;
                    from = end;
                }
                all += mesh.solid_index_count as u64;
                let min = [cx as f32 * 16.0, 0.0, cz as f32 * 16.0];
                let count = |vertical: (f32, f32)| -> u64 {
                    crate::engine::renderer::solid_ranges_facing(
                        eye,
                        min,
                        &mesh.solid_groups,
                        vertical,
                        None,
                    )
                    .iter()
                    .map(|(a, b)| (b - a) as u64)
                    .sum()
                };
                sent_column += count((0.0, CHUNK_SIZE_Y as f32));
                sent_measured += count((mesh.up_faces_from, mesh.down_faces_to));
            }
        }
        println!("total solid indices {all}");
        for (name, count) in names.iter().zip(totals) {
            println!("  {name:5} {count:8}  {:5.1}%", count as f64 * 100.0 / all as f64);
        }
        println!(
            "sent with y tested against the whole world column {sent_column} ({:.1}%)",
            sent_column as f64 * 100.0 / all as f64
        );
        println!(
            "sent with y tested against the faces themselves  {sent_measured} ({:.1}%)",
            sent_measured as f64 * 100.0 / all as f64
        );
    }

    fn terrain_mesh(seed: u32, pos: ChunkPos) -> MeshBuffers {
        let generator = WorldGen::new(seed);
        let mut chunks = ChunkManager::new(4);
        let mut light = LightMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks.insert(generator.generate_chunk(ChunkPos::new(pos.x + dx, pos.z + dz)));
            }
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                light.load_chunk(&chunks, ChunkPos::new(pos.x + dx, pos.z + dz));
            }
        }
        let mut cache = Neighbourhood::default();
        cache.fill(pos, &chunks, &light);
        let mut out = MeshBuffers::default();
        build_mesh(
            pos,
            &cache,
            &crate::engine::texture::FaceLayers::empty_for_test(),
            &generator,
            &mut out,
        );
        out
    }

    #[test]
    fn a_merged_rectangle_tiles_its_texture_one_cell_at_a_time() {
        // The whole risk of the merge, in one assertion. A rectangle
        // four cells wide has to ask for four repeats of its picture,
        // along the axis it is actually four cells wide in. The sampler
        // is already `AddressMode::Repeat`, so the tiling itself is free
        // -- what is not free is asking for it on the right axis.
        let mut merged_anywhere = false;
        for seed in [1337u32, 7, 2024] {
            let mesh = terrain_mesh(seed, ChunkPos::new(0, 0));
            let mut widest = 1.0f32;
            for (face, positions, uvs) in grid_quads(&mesh) {
                let (u_axis, v_axis) = uv_axes(face);
                let du = span(uvs.iter().map(|uv| uv[0]));
                let dv = span(uvs.iter().map(|uv| uv[1]));
                covers(span(positions.iter().map(|p| p[u_axis])), du);
                covers(span(positions.iter().map(|p| p[v_axis])), dv);
                widest = widest.max(du).max(dv);
            }
            // **The merge is switched off** (see `MERGE_COPLANAR_FACES`,
            // which carries the measurement that switched it), so what this
            // asserts is the other half of that switch: without it every
            // face is one cell and the picture is mapped corner to corner.
            // Kept live rather than ignored, because the merging code is
            // still here waiting on the T-junction split and a test that
            // stops running is a test that stops being true.
            if MERGE_COPLANAR_FACES {
                // **Per seed this can legitimately be one.** The cutting
                // pass in `split_t_junctions` takes a rectangle apart
                // wherever an unevenly lit face beside it puts a corner
                // on its edge, and on a seed that is all crease and no
                // plain -- 2024 is one -- that is every rectangle. The
                // sanity check that the merge still *runs* is therefore
                // asked across the three seeds together, not of each.
                merged_anywhere |= widest > 1.0;
            } else {
                assert_eq!(
                    widest, 1.0,
                    "seed {seed}: a quad spans {widest} cells with the merge switched off"
                );
            }
        }
        // Whether the merge still *runs* is the flat roof's question --
        // `a_flat_roof_of_stone_is_one_rectangle_rather_than_two_hundred_and_fifty_six`
        // -- and not this one's: on natural terrain the cutting pass can
        // legitimately leave nothing merged on all three seeds, and this
        // test is about the texture on whatever did merge.
        let _ = merged_anywhere;
    }

    /// A rectangle may never be longer than the number it has to carry.
    ///
    /// **The one place the mesher can outgrow its own vertex.** Terrain
    /// generated from a seed never shows this: the ground merges along
    /// x and z, and a chunk is only sixteen cells across either way. The
    /// four sideways face directions grow their rectangles along *y*,
    /// where a chunk is sixty-four, and `Vertex::uv` holds five bits.
    /// A shaft cut through solid rock offers a sixty-cell run, and
    /// before `MAX_RUN` existed it was emitted whole and then silently
    /// clamped to thirty-one -- a wall wearing thirty-one tiles of
    /// picture stretched over sixty blocks, and out of step with every
    /// unmerged face beside it.
    #[test]
    fn a_shaft_through_solid_rock_is_split_where_the_vertex_runs_out_of_bits() {
        let pos = ChunkPos::new(0, 0);
        let mut chunks = ChunkManager::new(4);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let mut blocks = vec![BLOCK_STONE; CHUNK_VOLUME];
                // Sealed top and bottom, so the shaft is dark and every
                // cell of its wall agrees about light -- which is what
                // lets the run get long enough to be the point.
                if (dx, dz) == (0, 0) {
                    for y in 1..CHUNK_SIZE_Y - 1 {
                        blocks[Chunk::index(8, y, 8)] = BLOCK_AIR;
                    }
                }
                chunks.insert(Chunk {
                    pos: ChunkPos::new(pos.x + dx, pos.z + dz),
                    blocks,
                });
            }
        }
        let mut light = LightMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                light.load_chunk(&chunks, ChunkPos::new(pos.x + dx, pos.z + dz));
            }
        }
        let mut cache = Neighbourhood::default();
        cache.fill(pos, &chunks, &light);
        let mut mesh = MeshBuffers::default();
        build_mesh(
            pos,
            &cache,
            &crate::engine::texture::FaceLayers::empty_for_test(),
            &WorldGen::new(1),
            &mut mesh,
        );

        let mut tallest = 0.0f32;
        for (face, positions, uvs) in quads(&mesh) {
            let (u_axis, v_axis) = uv_axes(face);
            let du = span(uvs.iter().map(|uv| uv[0]));
            let dv = span(uvs.iter().map(|uv| uv[1]));
            assert!(
                du <= MAX_RUN as f32 && dv <= MAX_RUN as f32,
                "face {face} asks for {du} x {dv} cells, which does not fit in five bits"
            );
            // ...and the coordinate still covers exactly the blocks the
            // quad is drawn over. A cap that shortened the picture
            // instead of the rectangle would pass the line above.
            covers(span(positions.iter().map(|p| p[u_axis])), du);
            covers(span(positions.iter().map(|p| p[v_axis])), dv);
            if face >= 2 {
                tallest = tallest.max(span(positions.iter().map(|p| p[1])));
            }
        }
        // **The merge is switched off** (see `MERGE_COPLANAR_FACES`,
        // which carries the measurement that switched it), so what this
        // asserts is the other half of that switch: without it every
        // face is one cell and the picture is mapped corner to corner.
        // Kept live rather than ignored, because the merging code is
        // still here waiting on the T-junction split and a test that
        // stops running is a test that stops being true.
        if MERGE_COPLANAR_FACES {
            covers(tallest, MAX_RUN as f32);
            assert!(
                tallest >= MAX_RUN as f32,
                "the shaft wall never reached the cap, so nothing was tested"
            );
        } else {
            assert_eq!(
                tallest, 1.0,
                "a shaft wall is {tallest} cells tall with the merge switched off"
            );
        }
    }

    /// The merge and the sampler live in different files and only work
    /// together.
    ///
    /// A merged rectangle's coordinate runs past 1.0 and means *tile the
    /// picture this many times*; under `ClampToEdge` the same number
    /// means *show the last texel column for the rest of the quad*. That
    /// is not a subtle difference -- it is the dark bands across a
    /// meadow a player sent a photograph of.
    #[test]
    fn merging_faces_is_only_legible_because_the_block_sampler_repeats() {
        assert!(
            !MERGE_COPLANAR_FACES
                || crate::engine::texture::BLOCK_ADDRESS_MODE == wgpu::AddressMode::Repeat,
            "coplanar faces are merged, so the block sampler has to tile past 1.0"
        );
    }

    #[test]
    fn no_two_rectangles_cover_the_same_cell() {
        // The greedy pass clears every cell of a rectangle as it emits
        // it. If it ever stopped, the overlap would be two coplanar
        // depth-writing quads in the same place, which is the flicker
        // that reads as a driver fault rather than as a mesher one.
        for seed in [1337u32, 7, 2024] {
            let mesh = terrain_mesh(seed, ChunkPos::new(0, 0));
            let mut seen = std::collections::HashSet::new();
            for (face, positions, _) in grid_quads(&mesh) {
                let normal_axis = normal_axis_of(face);
                let (axis_a, axis_b) = other_axes(normal_axis);
                let lowest = |axis: usize| {
                    positions.iter().map(|p| p[axis]).fold(f32::MAX, f32::min).round() as i32
                };
                let plane = lowest(normal_axis);
                let (a0, b0) = (lowest(axis_a), lowest(axis_b));
                let width = span(positions.iter().map(|p| p[axis_a])).round().max(1.0) as i32;
                let height = span(positions.iter().map(|p| p[axis_b])).round().max(1.0) as i32;
                for da in 0..width {
                    for db in 0..height {
                        assert!(
                            seen.insert((face, plane, a0 + da, b0 + db)),
                            "seed {seed}: two quads cover face {face} plane {plane} cell ({}, {})",
                            a0 + da,
                            b0 + db
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_flat_roof_of_stone_is_one_rectangle_rather_than_two_hundred_and_fifty_six() {
        // The best case, stated as a number. If this comes back as 256
        // the merge has stopped running; if it comes back as sixteen the
        // greedy pass has stopped growing rectangles along its second
        // axis, which is the half of it that is easy to lose.
        let pos = ChunkPos::new(0, 0);
        let mut chunks = ChunkManager::new(4);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
                for y in 0..=20 {
                    for lz in 0..CHUNK_SIZE_Z {
                        for lx in 0..CHUNK_SIZE_X {
                            blocks[Chunk::index(lx, y, lz)] = BLOCK_STONE;
                        }
                    }
                }
                chunks.insert(Chunk {
                    pos: ChunkPos::new(pos.x + dx, pos.z + dz),
                    blocks,
                });
            }
        }
        let mut light = LightMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                light.load_chunk(&chunks, ChunkPos::new(pos.x + dx, pos.z + dz));
            }
        }
        let mut cache = Neighbourhood::default();
        cache.fill(pos, &chunks, &light);
        let mut mesh = MeshBuffers::default();
        build_mesh(
            pos,
            &cache,
            &crate::engine::texture::FaceLayers::empty_for_test(),
            &WorldGen::new(1),
            &mut mesh,
        );

        // Both halves of the switch are asserted, so that flipping it
        // for a measurement cannot pass silently: with the merge off
        // every face is one cell and the picture is mapped corner to
        // corner.
        let tops: Vec<_> = quads(&mesh).into_iter().filter(|(face, _, _)| *face == 0).collect();
        if !MERGE_COPLANAR_FACES {
            assert_eq!(
                tops.len(),
                CHUNK_SIZE_X * CHUNK_SIZE_Z,
                "with the merge off a flat roof is one quad per cell"
            );
            return;
        }
        // One rectangle: nothing on a flat stone roof with flat stone
        // neighbours is lit unevenly, so nothing is drawn outside the
        // merge and there is no corner anywhere for the cutting pass to
        // find. Sixteen coming back here means the greedy pass has
        // stopped growing along its second axis; two hundred and
        // fifty-six means the merge has stopped running.
        assert_eq!(tops.len(), 1, "the roof should be one rectangle");
        let (_, positions, uvs) = tops[0];
        covers(span(positions.iter().map(|p| p[0])), CHUNK_SIZE_X as f32);
        covers(span(positions.iter().map(|p| p[2])), CHUNK_SIZE_Z as f32);
        assert_eq!(span(uvs.iter().map(|uv| uv[0])), CHUNK_SIZE_X as f32);
        assert_eq!(span(uvs.iter().map(|uv| uv[1])), CHUNK_SIZE_Z as f32);
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn bench_merge_reduction() {
        for seed in [1337u32, 7, 2024] {
            let mesh = terrain_mesh(seed, ChunkPos::new(0, 0));
            let drawn = quads(&mesh);
            let cells: usize = drawn
                .iter()
                .map(|(face, positions, _)| {
                    let (axis_a, axis_b) = other_axes(normal_axis_of(*face));
                    let width = span(positions.iter().map(|p| p[axis_a])).round().max(1.0);
                    let height = span(positions.iter().map(|p| p[axis_b])).round().max(1.0);
                    (width * height) as usize
                })
                .sum();
            println!(
                "seed {seed}: {cells} faces drawn as {} quads ({:+.0}%), {} vertices",
                drawn.len(),
                (drawn.len() as f32 / cells.max(1) as f32 - 1.0) * 100.0,
                mesh.vertices.len(),
            );
        }
    }
}


/// Leaves in water, ice against everything, and a model bigger than its
/// cell -- each held to the mechanism a photograph found, not to the
/// photograph. `water_repro::what_leaves_and_ice_look_like` and
/// `rack_repro::what_a_whole_rack_looks_like` are the pictures.
#[cfg(test)]
mod leaf_ice_rack_tests {
    use super::transparency_tests::{cache_of, mesh_of};
    use super::*;
    use primitive_shared::types::{BLOCK_ICE, BLOCK_LEAVES, BLOCK_STONE, BLOCK_WATER};

    /// The blended range as triangles of positions.
    fn water_triangles(mesh: &MeshBuffers) -> Vec<[[f32; 3]; 3]> {
        mesh.indices[mesh.sprite_end as usize..]
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]].map(|i| mesh.vertices[i as usize].position))
            .collect()
    }

    /// Is the point (x, z) under one of these triangles, all of whose corners
    /// are at `height`?
    fn covered_at(triangles: &[[[f32; 3]; 3]], height: f32, (x, z): (f32, f32)) -> bool {
        triangles.iter().any(|t| {
            if t.iter().any(|p| (p[1] - height).abs() > 1e-4) {
                return false;
            }
            let side = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0]);
            let (d0, d1, d2) = (side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0]));
            (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
        })
    }

    #[test]
    fn a_bush_sunk_to_the_brim_leaves_no_hole_in_the_surface_over_it() {
        // **"у листвы проблемы с рендером".** A bush three by three sunk in a
        // pond with its top at the surface. Two things opened the pond over
        // it. The surface of a cell averages its corners with the water round
        // them, and a flooded crown is leaves by its id, so the corner where
        // four cells of the bush met counted nothing: 0/0, NaN, and the GPU
        // dropped every triangle touching it. And the middle cell has leaves
        // on four sides and air over it, so it was dry and drew no surface
        // at all. From above: a hole in the water with the bush in it. From
        // under the surface: the sky through the leaves.
        let fill = |x: i32, y: i32, z: i32| match y {
            _ if (7..=9).contains(&x) && (7..=9).contains(&z) && (4..=5).contains(&y) => BLOCK_LEAVES,
            0..=3 => BLOCK_STONE,
            4..=5 => BLOCK_WATER,
            _ => BLOCK_AIR,
        };
        let mesh = mesh_of(&cache_of(fill));
        let triangles = water_triangles(&mesh);
        for t in &triangles {
            assert!(t.iter().flatten().all(|c| c.is_finite()), "a water corner the mesher could not put a height on: {t:?}");
        }
        let surface = 5.0 + primitive_shared::fluid::surface_height(BLOCK_WATER);
        for x in 6..=10 {
            for z in 6..=10 {
                assert!(
                    covered_at(&triangles, surface, (x as f32 + 0.5, z as f32 + 0.5)),
                    "the pond is open over ({x}, {z}), in or beside the bush"
                );
            }
        }
    }

    #[test]
    fn the_water_in_a_flooded_crown_walls_off_a_dry_crown_beside_it() {
        // A bush five wide, sunk to its top layer, which stands out of the
        // water. Its middle is two cells from the pond on every side, which
        // is past what the mesher reads (`wet_through_the_leaves`), so it
        // stays dry -- and the water round it has to wall it off, or it is
        // a hole through the pond that the sky shows through
        // (`dry_beside_a_flooded_crown`).
        let fill = |x: i32, y: i32, z: i32| match y {
            _ if (6..=10).contains(&x) && (6..=10).contains(&z) && (4..=6).contains(&y) => BLOCK_LEAVES,
            0..=3 => BLOCK_STONE,
            4..=5 => BLOCK_WATER,
            _ => BLOCK_AIR,
        };
        let mesh = mesh_of(&cache_of(fill));
        let walls = water_triangles(&mesh)
            .into_iter()
            .filter(|t| {
                t.iter().all(|p| (p[0] - 8.0).abs() < 1e-4 && (8.0..=9.0).contains(&p[2]) && (4.8..=6.0).contains(&p[1]))
            })
            .count();
        assert!(walls > 0, "nothing closes the dry middle of the bush off from the water beside it");
    }

    #[test]
    fn a_crown_in_the_air_is_not_filled_on_its_neighbours_word() {
        // The other side of `wet_through_the_leaves`: a crown walled in by
        // crowns is only filled when one of them has water beside it. A
        // tree's crown over dry ground has none, and must draw no water.
        let fill = |x: i32, y: i32, z: i32| match y {
            _ if (6..=10).contains(&x) && (6..=10).contains(&z) && (6..=8).contains(&y) => BLOCK_LEAVES,
            0..=3 => BLOCK_STONE,
            _ => BLOCK_AIR,
        };
        let mesh = mesh_of(&cache_of(fill));
        assert!(water_triangles(&mesh).is_empty(), "a crown in the air drew water in it");
    }

    #[test]
    fn ice_hides_its_cell_as_stone_does() {
        // **Light passes ice; sight does not** (`hides_its_cell_from_sight`).
        // Read as a leaf, a sheet of ice drew every face between two of its
        // cells, and a leaf lying on it drew its underside in the plane of
        // the ice's top -- two depth-writing quads in one place.
        let fill = |x: i32, y: i32, z: i32| match y {
            _ if (x, y, z) == (8, 6, 8) => BLOCK_LEAVES,
            _ if (7..=9).contains(&x) && (7..=9).contains(&z) && y == 5 => BLOCK_ICE,
            0..=4 => BLOCK_STONE,
            _ => BLOCK_AIR,
        };
        let mesh = mesh_of(&cache_of(fill));
        let triangles: Vec<[[f32; 3]; 3]> = mesh
            .indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]].map(|i| mesh.vertices[i as usize].position))
            .collect();
        let inside = triangles
            .iter()
            .filter(|t| {
                let c = [0, 1, 2].map(|a| t.iter().map(|p| p[a]).sum::<f32>() / 3.0);
                (7.01..9.99).contains(&c[0]) && (7.01..9.99).contains(&c[2]) && (5.01..5.99).contains(&c[1])
            })
            .count();
        assert_eq!(inside, 0, "faces drawn between two cells of ice, inside the sheet");
        // Under the leaf, in the ice's top plane: one quad's two triangles.
        let on_the_shared_plane = triangles.iter().filter(|t| covered_at(&[**t], 6.0, (8.5, 8.4))).count();
        assert_eq!(on_the_shared_plane, 1, "the leaf and the ice under it both drew the plane they share");
    }

    #[test]
    fn ice_is_lit_as_the_ground_is_and_not_as_a_canopy() {
        // Ice is in the cutout pass because light passes it, and the cube
        // path used to read "cutout" as "leaf": the ice was lit flat from the
        // cell in front and two levels were taken off for a canopy it is not
        // part of. A frozen bay was a grey sheet darker than its shore, and a
        // torch on it lit it in squares. The top of a cell of ice has to carry
        // exactly the light a cell of stone in its place would.
        // Over the whole floor, not the one cell: the stone's top is merged
        // into quads far wider than a cell, and they carry the same light
        // word as every cell under them.
        let top_light = |middle: BlockId, near: bool| {
            let mesh = mesh_of(&cache_of(move |x, y, z| match y {
                _ if (x, y, z) == (8, 4, 8) => middle,
                0..=4 => BLOCK_STONE,
                _ => BLOCK_AIR,
            }));
            let mut lights: Vec<u32> = mesh
                .vertices
                .iter()
                .filter(|v| {
                    let p = v.position;
                    (p[1] - 5.0).abs() < 1e-4
                        && (!near || ((8.0..=9.0).contains(&p[0]) && (8.0..=9.0).contains(&p[2])))
                })
                .map(|v| v.light())
                .collect();
            lights.sort_unstable();
            lights.dedup();
            lights
        };
        let ice = top_light(BLOCK_ICE, true);
        assert!(!ice.is_empty(), "the ice drew no top");
        assert_eq!(ice, top_light(BLOCK_STONE, false), "ice is lit differently from the stone it stands in for");
    }

    #[test]
    fn a_model_bigger_than_its_cell_wears_its_picture_on_every_face() {
        // **"текстура сушилки 2x2 -- месиво".** The whole rack is written to
        // thirty-two sixteenths, and a side reads its picture's `v` as
        // `1 - y`: at y 2 that is -1, which the unsigned fine coordinate took
        // as nought. The upper cell of every pole wore one row of bark
        // stretched the length of it. A face is cut from its picture at one
        // texel a sixteenth, so on every face the picture has to run exactly
        // as far as the face does, along both of its sides.
        use primitive_shared::types::{rack_cells, Facing};
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let block = rack_cells((0, 0, 0), facing)[0].1;
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            rack_block([0.0; 3], block, RackColumns::Whole(0, 0), &FaceLayers::empty_for_test(), 0xFF, &mut vertices, &mut indices);
            assert!(!vertices.is_empty(), "{facing:?}: the whole rack drew nothing");
            for quad in vertices.chunks_exact(4) {
                let span = |values: [f32; 4]| {
                    values.iter().cloned().fold(f32::MIN, f32::max) - values.iter().cloned().fold(f32::MAX, f32::min)
                };
                // The lengths of the quad's own edges, not its extent along
                // the world's axes: a pole is a tilted box, and a face along
                // it is longer than the height it spans.
                let edge = |a: usize, b: usize| {
                    let (p, q) = (quad[a].position, quad[b].position);
                    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
                };
                let mut sides = [edge(0, 1), edge(1, 2)];
                sides.sort_by(|a, b| b.total_cmp(a));
                let mut worn = [0, 1].map(|a| span([0, 1, 2, 3].map(|k| quad[k].uv()[a])));
                worn.sort_by(|a, b| b.total_cmp(a));
                for (side, picture) in sides.iter().zip(worn) {
                    assert!(
                        (side - picture).abs() < 2.0 / FINE_UNITS,
                        "{facing:?}: a face {side} blocks long wears {picture} of its picture -- {:?}",
                        quad.iter().map(|v| (v.position, v.uv())).collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}
