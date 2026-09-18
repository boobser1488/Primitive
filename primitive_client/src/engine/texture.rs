//! Block textures, now **per face**.
//!
//! `assets/textures/blocks.toml` accepts either form:
//!
//! ```toml
//! stone = "terrain/stone.png"                                   # same on all 6 faces
//! grass = { top = "terrain/grass_top.png", side = "terrain/grass_side.png", bottom = "terrain/dirt.png" }
//! log   = { top = "terrain/log_top.png", side = "terrain/log_side.png" }
//! chest = { north = "chest_front.png", side = "terrain/chest_side.png", all = "terrain/chest_side.png" }
//! ```
//!
//! Resolution order for a face, first match wins:
//! its own name (`north`/`south`/`east`/`west`/`top`/`bottom`) ->
//! `side` (the four vertical faces) -> `all` -> the missing-texture
//! placeholder. So the common cases stay one line, and a block that
//! needs a distinct front face can have one without listing all six.
//!
//! Implementation notes:
//!
//! * Still a wgpu texture *array*, one layer per distinct **image**, not
//!   per (block, face) pair. Identical filenames are deduplicated, so
//!   grass's bottom and plain dirt share a layer instead of uploading
//!   the same pixels twice. With six faces per block that dedup matters:
//!   naively it would be 60 layers for 10 blocks, here it's 14.
//! * The lookup the mesher uses is a flat `Vec<u32>` indexed by
//!   `block_id * 6 + face`, not a `HashMap` -- it's called once per
//!   emitted face, several hundred thousand times per chunk batch, and
//!   a hash there is pure overhead.
//! * A missing or unreadable file still falls back to a magenta/black
//!   checkerboard rather than refusing to start.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::{imageops::FilterType, GenericImageView, RgbaImage};
use serde::Deserialize;

use primitive_shared::types::{is_cross, is_flat, is_item, BlockId, ALL_BLOCK_IDS};

use crate::engine::item_model::ItemModel;
use crate::engine::relief::{self, Hung, Relief};

/// How many stages of cracks the breaking overlay has: `break.0.png`
/// through `break.4.png`.
pub const BREAK_STAGES: usize = 5;

/// Face order must match `mesh::faces()`.
pub const FACE_TOP: usize = 0;
pub const FACE_BOTTOM: usize = 1;
pub const FACE_EAST: usize = 2;
pub const FACE_WEST: usize = 3;
pub const FACE_SOUTH: usize = 4;
#[allow(dead_code)] // completes the face-name set; used by tests and configs
pub const FACE_NORTH: usize = 5;
pub const FACES: usize = 6;

/// A seventh slot beside a block's six faces: what it looks like as a
/// *thing you are carrying* rather than as a thing in the world.
///
/// Most blocks need no such picture -- a cobblestone in the pack is a
/// cobblestone, and showing one of its faces says so. Some are the same
/// stuff in two shapes, and the block face is the wrong one of them: a
/// tile of ash tells you what a floor of ash looks like and nothing at
/// all about the handful in your pocket. Rather than a second texture
/// system for icons, that is one more entry in the table this one
/// already keeps.
///
/// Zero means "not configured", which is the same thing the placeholder
/// layer means everywhere else -- so the fallback is a face, and a block
/// that does not want an icon of its own costs one `u32`.
pub const ITEM_SLOT: usize = FACES;
/// Faces plus the item picture: the stride of the layer table.
pub const SLOTS: usize = FACES + 1;

#[derive(Debug, Deserialize)]
struct BlocksToml {
    #[serde(default = "default_resolution")]
    resolution: u32,
    #[serde(default)]
    textures: HashMap<String, TextureSpec>,
}

fn default_resolution() -> u32 {
    16
}

/// Either one filename for the whole block, or a per-face table.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TextureSpec {
    Single(String),
    Faces(FaceTextures),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaceTextures {
    all: Option<String>,
    /// What this looks like in the pack and on the hotbar. See
    /// `ITEM_SLOT`. Never falls back to `all`: an icon is opt-in, and a
    /// block that has not asked for one shows a face.
    item: Option<String>,
    top: Option<String>,
    bottom: Option<String>,
    /// The four vertical faces at once.
    side: Option<String>,
    north: Option<String>,
    south: Option<String>,
    east: Option<String>,
    west: Option<String>,
}

impl TextureSpec {
    /// Filename for the carried picture, or `None` to use a face.
    fn for_item(&self) -> Option<&str> {
        match self {
            TextureSpec::Single(_) => None,
            TextureSpec::Faces(f) => f.item.as_deref(),
        }
    }

    /// Filename for one face, following the fallback chain described in
    /// the module docs. `None` means "nothing configured" -> placeholder.
    fn for_face(&self, face: usize) -> Option<&str> {
        match self {
            TextureSpec::Single(name) => Some(name.as_str()),
            TextureSpec::Faces(f) => {
                let specific = match face {
                    FACE_TOP => f.top.as_ref(),
                    FACE_BOTTOM => f.bottom.as_ref(),
                    FACE_EAST => f.east.as_ref(),
                    FACE_WEST => f.west.as_ref(),
                    FACE_SOUTH => f.south.as_ref(),
                    _ => f.north.as_ref(),
                };
                let sided = if face == FACE_TOP || face == FACE_BOTTOM {
                    None
                } else {
                    f.side.as_ref()
                };
                specific
                    .or(sided)
                    .or(f.all.as_ref())
                    .map(|s| s.as_str())
            }
        }
    }
}

/// The block-face -> texture-layer table on its own, cheap to clone and
/// safe to send to another thread.
///
/// Meshing runs on worker threads, and they need this lookup but must
/// not touch the `TextureManager` (which owns GPU resources). Splitting
/// the plain data out keeps the GPU handles on the main thread where
/// they belong.
#[derive(Clone)]
pub struct FaceLayers {
    layers: std::sync::Arc<[u32]>,
    max_block_id: BlockId,
    /// Layers for the pictures that are not block faces, in the order
    /// of `EXTRA_TEXTURES`.
    extra: std::sync::Arc<[u32]>,
    /// One row of `SHEET_SLOTS` per animal, in `ANIMAL_SHEETS` order.
    /// See `animal`.
    animals: std::sync::Arc<[u32]>,
    /// The thickness of the things lying on the ground, by block kind.
    /// See `relief`.
    reliefs: Reliefs,
    /// The things a drying rack hangs, read off their pictures. See `hung`.
    hung: HungTable,
    /// One layer per block kind that can wear moss, zero for the rest.
    /// See `mossy`.
    mossy: std::sync::Arc<[u32]>,
}

/// Pictures that belong to no block.
///
/// Three animal skins and two kinds of falling weather. They are loaded
/// exactly like `break.N.png` -- by name, with no row in `blocks.toml`
/// -- because there is no block for them to be a face of: an animal is
/// an entity wearing one texture, and rain is a quad standing in front
/// of the camera.
///
/// Order is the index, so the constants below are the only names
/// anything outside this file uses.
/// One sheet per animal, in **`Species::ALL` order**, and that order is
/// load-bearing: a species is looked up by its position in that list.
///
/// **One picture per animal rather than nine.** A wolf used to be a
/// pelt, an eye, the flip of the eye, a muzzle, a nose, an ear, a paw
/// and a coarse weave -- eight files, eight lines in three tables, and
/// eight chances to name one of them wrong. It is one sheet now, cut
/// into tiles when it is loaded (see `SHEET_COLUMNS` and `sheet_tiles`),
/// which is also the form anything outside the game wants: a model
/// exported to `.obj` gets one material and one image instead of ten.
///
/// A tile that is left blank is not a picture at all: the animal has no
/// such part, and every face that asked for it wears the hide instead.
/// So *drawing* a wolf's tusks is drawing them, with no code anywhere.
pub const ANIMAL_SHEETS: &[&str] = &[
    "animals/hare.png",
    "animals/deer.png",
    "animals/boar.png",
    "animals/wolf.png",
    "animals/sheep.png",
    // **In `Species::ALL` order, and that is load-bearing**: the sheet
    // for a species is looked up by its *index* in that list (see
    // `obj_export::sheet_of` and `FaceLayers::animal`), so a row out of
    // order here dresses a bear in a sheep.
    "animals/bear.png",
    // **The bird, and it went five versions without one.** `sheet_index`
    // answered zero for a fowl -- the hare's sheet -- so every bird in
    // the world was a small brown hare-coloured thing perched in a
    // canopy, which is why a player said they could not see any. It is
    // not that they were rare; it is that nothing about them said bird.
    "animals/fowl.png",
    // The savanna's three, appended like the species themselves. Each is
    // derived from the deer's sheet and redrawn -- a striped hide, a golden
    // coat with a white belly in the fur slot and a ringed horn in the horn
    // slot, a sand coat with a dark mane -- and the tiles an animal has no
    // part for are left blank, so they cost no layer (see `sheet_tiles`).
    "animals/zebra.png",
    "animals/antelope.png",
    "animals/lion.png",
    // The two that swim, appended with the species. A fish sheet draws the
    // hide, the eyed sides, the face, the lips and the mouth, and the fins
    // in the fur slot; the ear, hoof, tusk, antler and horn tiles are blank
    // and cost nothing (`sheet_tiles`).
    "animals/fish.png",
    "animals/cod.png",
    // The gull, recoloured tile by tile from the fowl's sheet so the grain
    // and the eye are the bird's own: white hide and head, a yellow bill
    // with the red spot on its tip, pink feet, the grey mantle in the fur
    // slot and -- in the ear slot, which a bird has no ear for -- the black
    // wingtip with its two white mirrors.
    "animals/gull.png",
    // The three that fill in the rest of the water, appended with the
    // species. Each is the plain fish's hide tile run through another ramp
    // -- an olive, spotted trout, a weed-green pike flecked with pale, a
    // silver herring -- and every other tile is left blank, which is one
    // layer apiece and not twelve (see `sheet_tiles`: a blank tile is an
    // alias for the hide).
    "animals/trout.png",
    "animals/pike.png",
    "animals/herring.png",
    // The rat, recoloured from the hare's sheet tile by tile: the same
    // grain and the same shading through a grey-brown ramp, and the bare
    // parts -- snout, ears, feet and the tail, which wears the snout's
    // tile -- through a skin one. Derived rather than drawn for the
    // reason the savanna's three were: what makes an animal in this game
    // look like it belongs is the *grain*, and a fresh 16x16 never has
    // the same one. Its eye is a black bead with no white in it, which
    // is the one thing a rat's face has that a hare's does not.
    "animals/rat.png",
    "animals/horse.png",
];

/// Which sheet a species wears, as a row of `ANIMAL_SHEETS`.
///
/// A function rather than the species' own position in `Species::ALL`, so
/// that two species *could* wear one sheet -- a cousin in the same coat is
/// one line here and no picture. **None does.** This note used to say the
/// bird wore the hare's, and that sharing was the bug rather than the
/// saving: every bird in the world was a small brown hare-coloured thing
/// and a player said there were no birds (`no_two_animals_share_a_coat`).
/// The savanna's three have sheets of their own for the same reason: a
/// zebra in the deer's coat is a deer.
pub fn sheet_index(species: primitive_shared::animals::Species) -> usize {
    use primitive_shared::animals::Species;
    match species {
        Species::Hare => 0,
        Species::Deer => 1,
        Species::Boar => 2,
        Species::Wolf => 3,
        Species::Sheep => 4,
        Species::Bear => 5,
        Species::Fowl => 6,
        Species::Zebra => 7,
        Species::Antelope => 8,
        Species::Lion => 9,
        Species::Fish => 10,
        Species::Cod => 11,
        Species::Gull => 12,
        Species::Trout => 13,
        Species::Pike => 14,
        Species::Herring => 15,
        Species::Rat => 16,
        Species::Horse => 17,
    }
}

/// How the tiles of an animal sheet are laid out.
///
/// Twelve slots in a four-by-three grid, which at the stock resolution
/// is a 64x48 picture. Which slot is which is `animal_model::Skin::slot`
/// -- the one place that decides, so the sheet and the model cannot
/// disagree about where an ear is.
pub const SHEET_COLUMNS: u32 = 4;
pub const SHEET_ROWS: u32 = 3;
/// How many pictures one animal can have.
pub const SHEET_SLOTS: usize = (SHEET_COLUMNS * SHEET_ROWS) as usize;

/// The pictures that belong to no block and no animal.
pub const EXTRA_TEXTURES: &[&str] = &[
    "effects/rain.png",
    "effects/snow_fall.png",
    // **One drawn sheet of `FLAME_FRAMES` frames, and the order is the
    // mechanism.** The fire is drawn the way a tuft of grass is -- two
    // quads crossing on the cell's diagonals -- and the two quads must
    // not wear the same picture at the same moment, or the flame is one
    // silhouette at right angles to itself and reads as a cardboard X.
    // The second sheet is *not* in this list: `load` derives it from
    // these, mirrored and half a loop late. See `FLAME_SHEETS`, and
    // `animated` in shader.wgsl, which finds a layer's sheet by
    // dividing -- so the two sheets have to stay contiguous, this one
    // first.
    "effects/flame.0.png",
    "effects/flame.1.png",
    "effects/flame.2.png",
    "effects/flame.3.png",
    "effects/flame.4.png",
    "effects/flame.5.png",
    // The skin on a loaded drying rack. Its own picture rather than a
    // crop of an animal's hide tile: the slab is the one face of the
    // model a player actually looks at, and a featureless crop of coat
    // read as a sheet of cardboard. This one is *drawn for the slab* --
    // laced edges and all -- so it wears the whole picture, uncropped.
    "hide/stretched_hide.png",
    // **The torch's own fire, and it is a second drawing on purpose.**
    // The hearth's fire above is drawn to the full width of its bottom
    // row, because in the world it fills the cell inside a ring of
    // stones; a torch head is four texels across, and no size of quad
    // can both swallow that head and keep a full-width base from
    // hanging out in the air beside the stick. Six more layers is what
    // that costs, out of sixty-odd spare -- see `generate_torch_flame`,
    // which carries the measurement.
    //
    // One sheet and not two: the hearth needs a second because its fire
    // is two quads crossing on the cell's diagonals, and this is a
    // single billboard in the hand with no second silhouette to
    // disagree with.
    "effects/torch_flame.0.png",
    "effects/torch_flame.1.png",
    "effects/torch_flame.2.png",
    "effects/torch_flame.3.png",
    "effects/torch_flame.4.png",
    "effects/torch_flame.5.png",
    // **Furniture's own materials** (`mesh::Material`): planed boards with
    // the grain along them, a squared post with the grain up it, a wrought
    // strap with its rivets, and a skin with the hair on for a blanket. A
    // player said of every stool, chair and bed that it was "just wood",
    // and it was: the boards were the wall's own planks, seams, nail heads
    // and all, and the legs were a log's bark. Here and not in
    // `blocks.toml` because no block is these -- they are what a model's
    // boxes are cut from, and a block row naming them would be a block
    // nobody can place standing in for a picture.
    // The moss that grows on a north face and on the top of a stone: a skin
    // of patches with holes in it, laid over whatever it grows on when the
    // atlas is built (`mossy_over`). It used to be a *tint* -- the whole face
    // pushed toward green -- and a player asked for moss to be a picture.
    "plants/moss_patch.png",
    "furniture/timber.png",
    "furniture/post.png",
    "furniture/iron.png",
    "furniture/fur.png",
    // **Furniture in every wood but the oak** (`types::furniture_wood`):
    // each wood's run of `WOOD_PIECES`, in `wood::WOODS` order from the
    // birch. The oak wears the pictures above and in `blocks.toml`.
    // Recoloured from those to each wood's boards, wood-brown pixels only,
    // so iron and fur stay iron and fur; see `extra_in_wood`.
    "furniture/birch/timber.png",
    "furniture/birch/post.png",
    "furniture/birch/chest_side.png",
    "furniture/birch/door.png",
    "furniture/birch/door_top.png",
    "furniture/birch/stool_item.png",
    "furniture/birch/chair_item.png",
    "furniture/birch/table_item.png",
    "furniture/birch/bed_item.png",
    "furniture/birch/chest_item.png",
    "furniture/birch/door_item.png",
    "plants/birch/leaf_litter.png",
    "plants/birch/leaf_handful.png",
    "furniture/fir/timber.png",
    "furniture/fir/post.png",
    "furniture/fir/chest_side.png",
    "furniture/fir/door.png",
    "furniture/fir/door_top.png",
    "furniture/fir/stool_item.png",
    "furniture/fir/chair_item.png",
    "furniture/fir/table_item.png",
    "furniture/fir/bed_item.png",
    "furniture/fir/chest_item.png",
    "furniture/fir/door_item.png",
    "plants/fir/leaf_litter.png",
    "plants/fir/leaf_handful.png",
    "furniture/saxaul/timber.png",
    "furniture/saxaul/post.png",
    "furniture/saxaul/chest_side.png",
    "furniture/saxaul/door.png",
    "furniture/saxaul/door_top.png",
    "furniture/saxaul/stool_item.png",
    "furniture/saxaul/chair_item.png",
    "furniture/saxaul/table_item.png",
    "furniture/saxaul/bed_item.png",
    "furniture/saxaul/chest_item.png",
    "furniture/saxaul/door_item.png",
    "plants/saxaul/leaf_litter.png",
    "plants/saxaul/leaf_handful.png",
    "furniture/pine/timber.png",
    "furniture/pine/post.png",
    "furniture/pine/chest_side.png",
    "furniture/pine/door.png",
    "furniture/pine/door_top.png",
    "furniture/pine/stool_item.png",
    "furniture/pine/chair_item.png",
    "furniture/pine/table_item.png",
    "furniture/pine/bed_item.png",
    "furniture/pine/chest_item.png",
    "furniture/pine/door_item.png",
    "plants/pine/leaf_litter.png",
    "plants/pine/leaf_handful.png",
    "furniture/willow/timber.png",
    "furniture/willow/post.png",
    "furniture/willow/chest_side.png",
    "furniture/willow/door.png",
    "furniture/willow/door_top.png",
    "furniture/willow/stool_item.png",
    "furniture/willow/chair_item.png",
    "furniture/willow/table_item.png",
    "furniture/willow/bed_item.png",
    "furniture/willow/chest_item.png",
    "furniture/willow/door_item.png",
    "plants/willow/leaf_litter.png",
    "plants/willow/leaf_handful.png",
    // **The same skin cured**, laced in a hide frame once it has dried
    // (`types::HIDE_CURED`): the stretched hide above in leather's browns,
    // drawn for the same slab and so worn whole as well. Last, because every
    // index above is arithmetic on its neighbours (`extra_in_wood`) and a
    // picture put among them would move a wood's run by one.
    "hide/stretched_leather.png",
];

/// How many sheets the fire is drawn from: one per crossing quad.
///
/// **Only the first of them is on disk.** There used to be two hand-made
/// sets, `flame_a.*` and `flame_b.*`, because the fire was generated and
/// a second set cost nothing but a different seed. The fire is a
/// drawing now, and one drawing is what there is -- so the second sheet
/// is made in `load` out of the first: mirrored, and `FLAME_SHEET_LAG`
/// frames late.
///
/// The rejected alternative was a second set of files, copied from the
/// first by hand. It costs the same layers and adds the failure this
/// codebase hates most: redraw one frame, forget its copy, and the fire
/// is subtly wrong from one direction only, with nothing to say so.
/// Deriving it means there is exactly one place the fire is drawn.
pub const FLAME_SHEETS: u32 = 2;

/// How many frames the fire has, and where the first one is.
///
/// The count is here rather than in the shader because the shader is
/// told: the renderer passes the base layer and the count in `globals`,
/// so a seventh frame is a line in the table above and a number here.
///
/// **Six, and the drawing arrived as seven files.** It was four while
/// the fire was generated. The hand-drawn set is a tongue that rises
/// over four pictures and falls back over two -- and then a seventh
/// file that is byte-for-byte the first one again, which is how a loop
/// is *drawn* and not how it is played: a frame that closes the cycle
/// by repeating its start is that picture held for two ticks. At eight
/// pictures a second, on a fire a player is standing next to, that
/// stumble is visible, and the loop is short enough to see several
/// times a breath. So the repeat is not shipped, and nothing is lost --
/// there were six drawings in the seven files.
///
/// The same argument settles padding in the other direction: an eighth
/// frame duplicated to make a rounder number would buy nothing and cost
/// the same hitch.
pub const FLAME_FRAMES: u32 = 6;

/// How far behind the drawn sheet the derived one runs, in frames.
///
/// Three of six: half the loop, which is the furthest apart two phases
/// of it can be. The point is that the two crossing quads never pulse
/// together: this animation swells and subsides as a whole, and both
/// quads at the same moment of it would make the fire breathe like a
/// bellows, which is exactly what a campfire does not do. Offset, one
/// quad is settling while the other lifts, and the fire only flickers.
pub const FLAME_SHEET_LAG: u32 = 3;

/// A lag of none, or of the whole loop, is no lag at all -- both quads
/// would then swell together, which is the thing the offset exists to
/// prevent. Checked here rather than in a test because it is a property
/// of two numbers written above and can be settled before the build.
const _: () = assert!(
    FLAME_SHEET_LAG > 0 && FLAME_SHEET_LAG < FLAME_FRAMES,
    "the fire's second sheet must lag the first by part of a loop"
);

/// How many times a second the fire changes picture.
///
/// Eight. Slower reads as a slideshow and faster reads as noise; eight
/// is about the rate a real flame changes shape at the scale a campfire
/// is drawn.
pub const FLAME_FPS: f32 = 8.0;

/// Where each of them sits in `EXTRA_TEXTURES`.
///
/// The animals are indexed by `Species` in the order `Species::ALL`
/// gives them, and a test pins that: a mismatch here would dress every
/// deer in a boar's hide, which is exactly the kind of thing that is
/// obvious in a screenshot and invisible in a review.
pub const EXTRA_RAIN: usize = 0;
pub const EXTRA_SNOW: usize = 1;
/// The first of `FLAME_FRAMES` pictures of fire. The rest follow it.
pub const EXTRA_FLAME: usize = 2;
/// A skin stretched and laced on a frame: what the drying rack's slab
/// wears. Appended after the flame run, which must stay contiguous.
///
/// An index into `EXTRA_TEXTURES`, not a layer: the derived second
/// sheet of fire takes `FLAME_FRAMES` layers between the two and no
/// entry in the list at all, which is why this is eight and not
/// fourteen.
pub const EXTRA_STRETCHED_HIDE: usize = 8;
/// A skin laced in a frame and cured: what a hide frame's slab wears once it has
/// dried. The last entry of `EXTRA_TEXTURES`.
pub const EXTRA_STRETCHED_LEATHER: usize = EXTRA_TEXTURES.len() - 1;
/// The first of `FLAME_FRAMES` pictures of the fire on a torch. The
/// rest follow it, and they are a run in the array the same way the
/// hearth's are -- but no second sheet is derived from them, so unlike
/// `EXTRA_FLAME` this index and its layer differ only by the six the
/// hearth's derived sheet took.
pub const EXTRA_TORCH_FLAME: usize = 9;
/// The first of furniture's four materials, in `EXTRA_TEXTURES` order:
/// timber, post, iron, fur. After the torch's run, which is where the list
/// ends; see `mesh::Material::layer`.
/// The moss overlay, before the furniture's materials in `EXTRA_TEXTURES`.
pub const EXTRA_MOSS: usize = EXTRA_TORCH_FLAME + FLAME_FRAMES as usize;
pub const EXTRA_FURNITURE: usize = EXTRA_MOSS + 1;

/// What a wood's run of furniture pictures holds, in order. See
/// `extra_in_wood`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WoodPiece {
    Timber,
    Post,
    ChestSide,
    Door,
    DoorTop,
    StoolIcon,
    ChairIcon,
    TableIcon,
    BedIcon,
    ChestIcon,
    DoorIcon,
    /// The leaves of this wood fallen on the ground, and a fistful of them:
    /// the two things that carry a wood without being furniture
    /// (`types::carries_wood`).
    LeafLitter,
    LeafHandful,
}

/// How many pictures one wood's run is.
pub const WOOD_PIECES: usize = 13;

/// The index in `EXTRA_TEXTURES` of a piece in a wood, or `None` for the oak,
/// whose pictures are the furniture's own. After the four furniture materials.
pub fn extra_in_wood(wood: usize, piece: WoodPiece) -> Option<usize> {
    (1..primitive_shared::wood::WOODS.len())
        .contains(&wood)
        .then(|| EXTRA_FURNITURE + 4 + (wood - 1) * WOOD_PIECES + piece as usize)
}

/// The icon a piece of furniture wears in a wood other than oak.
fn wood_icon(block_id: BlockId) -> Option<usize> {
    use primitive_shared::types as t;
    let piece = match t::block_kind(block_id) {
        t::BLOCK_LEAF_LITTER => WoodPiece::LeafLitter,
        t::BLOCK_LEAF_HANDFUL => WoodPiece::LeafHandful,
        t::BLOCK_STOOL => WoodPiece::StoolIcon,
        t::BLOCK_CHAIR => WoodPiece::ChairIcon,
        t::BLOCK_TABLE => WoodPiece::TableIcon,
        t::BLOCK_BED => WoodPiece::BedIcon,
        t::BLOCK_CHEST => WoodPiece::ChestIcon,
        t::BLOCK_DOOR | t::BLOCK_DOOR_TOP => WoodPiece::DoorIcon,
        _ => return None,
    };
    extra_in_wood(t::furniture_wood(block_id), piece)
}

/// The fire's second sheet, made out of its first.
///
/// Each frame is a drawn frame `FLAME_SHEET_LAG` further round the loop,
/// flipped left to right. Both halves of that matter and they answer
/// different complaints: the flip is what stops the two crossing quads
/// from being one silhouette at right angles to itself, and the lag is
/// what stops them from swelling and subsiding together. See
/// `FLAME_SHEETS` for why this is code and not seven more files.
///
/// A separate function because it is the one part of building the atlas
/// that is a pure picture-to-picture rule, and a test can hold it to
/// that without a graphics card.
fn mirrored_flame_sheet(drawn: &[RgbaImage]) -> Vec<RgbaImage> {
    if drawn.is_empty() {
        return Vec::new();
    }
    (0..drawn.len())
        .map(|frame| image::imageops::flip_horizontal(&drawn[(frame + FLAME_SHEET_LAG as usize) % drawn.len()]))
        .collect()
}

/// One layer number per entry of `EXTRA_TEXTURES`, counting from
/// `base`, **with the gap the fire's derived sheet occupies**.
///
/// The fixtures below number their pictures by hand so that a test can
/// ask "are these two the same picture" and get a truthful answer. That
/// stops being truthful the moment a layer is not a file: `flame(1)` is
/// the first drawn frame plus `FLAME_FRAMES`, and without this gap that
/// arithmetic lands on whatever picture follows the fire in the list --
/// so the drying rack's hide and the fire's second sheet come out as one
/// layer, and a test written to catch exactly that kind of collision
/// stops being able to see it.
#[cfg_attr(not(test), allow(dead_code))]
fn extra_layers_for_test(base: u32) -> Vec<u32> {
    (0..EXTRA_TEXTURES.len() as u32)
        .map(|index| {
            let past_the_fire = index as usize >= EXTRA_FLAME + FLAME_FRAMES as usize;
            base + index + if past_the_fire { FLAME_FRAMES } else { 0 }
        })
        .collect()
}

/// How many layers the extras take up, which is more than there are
/// files. See `extra_layers_for_test`.
#[cfg_attr(not(test), allow(dead_code))]
const EXTRA_LAYERS_FOR_TEST: u32 = EXTRA_TEXTURES.len() as u32 + FLAME_FRAMES;

/// One thickness per block kind, `None` for everything not lying on the
/// ground. Shared rather than copied: `TextureManager::face_layers` hands
/// one of these to every mesher thread.
pub type Reliefs = std::sync::Arc<[Option<Relief>]>;

/// The reliefs of the pictures built into the game, for the tables that
/// load no pictures.
///
/// **Real shapes, not none**: `numbered_for_test` is what the mesher's
/// measurements run on (`arena::world_cost`), and a table with no stones
/// in it would report a forest floor as costing what it cost before it
/// had any thickness. Decoded once for the process.
pub(crate) fn embedded_reliefs() -> Reliefs {
    static EMBEDDED: std::sync::OnceLock<Reliefs> = std::sync::OnceLock::new();
    EMBEDDED
        .get_or_init(|| {
            let config: BlocksToml =
                toml::from_str(crate::embedded::BLOCKS_TOML).expect("the built-in blocks.toml parses");
            let resolution = config.resolution.clamp(1, 512);
            let max = ALL_BLOCK_IDS.iter().map(|&(id, _)| id).max().unwrap_or(0);
            let mut reliefs: Vec<Option<Relief>> = vec![None; max as usize + 1];
            for &(block_id, name) in ALL_BLOCK_IDS {
                if !relief::has_relief(block_id) {
                    continue;
                }
                let picture = config
                    .textures
                    .get(name)
                    .and_then(|spec| spec.for_face(FACE_TOP))
                    .and_then(crate::embedded::texture)
                    .and_then(|bytes| decode(bytes, resolution).ok());
                reliefs[block_id as usize] = picture.map(|image| Relief::from_image(&image));
            }
            std::sync::Arc::from(reliefs)
        })
        .clone()
}

/// One [`Hung`] per block kind, `None` for everything a rack does not hang.
/// Shared like [`Reliefs`], and for the same reason.
pub type HungTable = std::sync::Arc<[Option<Hung>]>;

/// Whether a rack ever hangs this block, and so whether its picture is read
/// into a [`Hung`]: every good a ridge can show (`rack::HANGING`) and the
/// cord it is tied on with.
fn is_hung(block_id: BlockId) -> bool {
    block_id == primitive_shared::types::BLOCK_CORD
        || primitive_shared::rack::HANGING.contains(&Some(block_id))
}

/// The hung goods of the pictures built into the game, for the tables that
/// load no pictures -- so a mesh test of a loaded rack draws the shapes the
/// game draws. Read from the picture the mesher dresses them in: the carried
/// one where there is one, as `mesh::hang_goods` asks for it.
pub(crate) fn embedded_hung() -> HungTable {
    static EMBEDDED: std::sync::OnceLock<HungTable> = std::sync::OnceLock::new();
    EMBEDDED
        .get_or_init(|| {
            let config: BlocksToml =
                toml::from_str(crate::embedded::BLOCKS_TOML).expect("the built-in blocks.toml parses");
            let resolution = config.resolution.clamp(1, 512);
            let max = ALL_BLOCK_IDS.iter().map(|&(id, _)| id).max().unwrap_or(0);
            let mut hung: Vec<Option<Hung>> = vec![None; max as usize + 1];
            for &(block_id, name) in ALL_BLOCK_IDS {
                if !is_hung(block_id) {
                    continue;
                }
                let picture = config
                    .textures
                    .get(name)
                    .and_then(|spec| spec.for_item().or_else(|| spec.for_face(FACE_TOP)))
                    .and_then(crate::embedded::texture)
                    .and_then(|bytes| decode(bytes, resolution).ok());
                hung[block_id as usize] = picture.map(|image| Hung::from_image(&image));
            }
            std::sync::Arc::from(hung)
        })
        .clone()
}

impl FaceLayers {
    /// A thing a drying rack hangs, as its picture shapes it, if the rack
    /// hangs it at all. See `relief::Hung`.
    #[inline]
    pub fn hung(&self, block_id: BlockId) -> Option<&Hung> {
        let kind = primitive_shared::types::block_kind(block_id);
        self.hung.get(kind as usize)?.as_ref()
    }

    /// How a thing lying on the ground stands up from it, if it does.
    /// See `relief`.
    #[inline]
    pub fn relief(&self, block_id: BlockId) -> Option<&Relief> {
        let kind = primitive_shared::types::block_kind(block_id);
        self.reliefs.get(kind as usize)?.as_ref()
    }

    /// The same table with every thing on the ground lying flat again, as
    /// it was drawn before `relief`: the "before" of a comparison made in one
    /// binary, which is the only kind whose difference is the change.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn without_reliefs(mut self) -> Self {
        self.reliefs = std::sync::Arc::from(Vec::new());
        self
    }

    /// The same table with every stone drawn the way it shipped before the
    /// seams on it were found (`Relief::as_it_shipped`): the silhouette left
    /// to the cut-out. The "before" of `relief_repro`'s seam photographs, in
    /// this binary.
    #[cfg(test)]
    pub fn with_reliefs_as_they_shipped(mut self) -> Self {
        let old: Vec<Option<crate::engine::relief::Relief>> =
            self.reliefs.iter().map(|relief| relief.as_ref().map(|r| r.as_it_shipped())).collect();
        self.reliefs = std::sync::Arc::from(old);
        self
    }

    /// Every picture numbered, and every block in the table.
    ///
    /// `empty_for_test` covers sixty-four ids and answers zero for all
    /// of them, which is right for a test asking "are these the same
    /// picture" and useless for one that has to know *which* picture: a
    /// preview that draws a model has to map a layer back to a file. See
    /// `ui::snapshot::block_models`.
    #[cfg(test)]
    pub fn numbered_for_test() -> Self {
        const IDS: usize = 512;
        // Wrapped into what a vertex can carry. 512 ids at seven slots is
        // 3584 numbers, past the eleven bits `mesh::Vertex` asserts on, so
        // a mesh test of any block from id 293 up (the palm trunk is 356)
        // tripped that assertion on a number no real atlas would hand out.
        // Wrapping keeps neighbouring blocks distinct, which is all these
        // tests compare; a test that needs two far-apart ids apart says so.
        let wrap = |i: u32| i % (crate::engine::mesh::MAX_TEXTURE_LAYERS - 1) + 1;
        Self {
            layers: std::sync::Arc::from(
                (0..(IDS * SLOTS) as u32).map(wrap).collect::<Vec<_>>(),
            ),
            max_block_id: IDS as BlockId - 1,
            extra: std::sync::Arc::from(extra_layers_for_test((IDS * SLOTS) as u32 + 1)),
            animals: std::sync::Arc::from(
                (0..(ANIMAL_SHEETS.len() * SHEET_SLOTS) as u32)
                    .map(|i| i + (IDS * SLOTS) as u32 + EXTRA_LAYERS_FOR_TEST + 1)
                    .collect::<Vec<_>>(),
            ),
            reliefs: embedded_reliefs(),
            hung: embedded_hung(),
            mossy: std::sync::Arc::from(Vec::new()),
        }
    }

    /// A table whose every face layer *is* its block kind plus one, and
    /// whose extras and animal sheets sit above all of them -- so a mesh
    /// can be read back as "whose triangles are these". `numbered_for_test`
    /// wraps ids into seven slots each and cannot answer that; the scene
    /// census in `arena` needs it answered.
    #[cfg(test)]
    pub fn by_kind_for_test() -> Self {
        const IDS: usize = 1024;
        const EXTRA_BASE: u32 = IDS as u32 + 1;
        Self {
            layers: std::sync::Arc::from((0..(IDS * SLOTS) as u32).map(|i| i / SLOTS as u32 + 1).collect::<Vec<_>>()),
            max_block_id: IDS as BlockId - 1,
            extra: std::sync::Arc::from(extra_layers_for_test(EXTRA_BASE)),
            animals: std::sync::Arc::from(
                (0..(ANIMAL_SHEETS.len() * SHEET_SLOTS) as u32)
                    .map(|i| EXTRA_BASE + EXTRA_LAYERS_FOR_TEST + 1 + i.min(64))
                    .collect::<Vec<_>>(),
            ),
            reliefs: embedded_reliefs(),
            hung: embedded_hung(),
            mossy: std::sync::Arc::from(Vec::new()),
        }
    }

    /// An all-placeholder table, for tests that exercise the meshing
    /// pipeline without a GPU -- and for `ui::snapshot`, which draws the
    /// interface without one.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn empty_for_test() -> Self {
        Self {
            layers: std::sync::Arc::from(vec![0u32; 64 * SLOTS]),
            max_block_id: 63,
            // Numbered rather than all zero, so a test can tell one
            // extra from another -- which is the whole question about
            // the fire, whose two crossing quads must not come out
            // wearing the same picture.
            extra: std::sync::Arc::from(extra_layers_for_test(1)),
            // Numbered too, and offset past the extras so that no two
            // pictures in a test share a layer: the questions these
            // tests ask are all "are these two the same picture", and a
            // fixture full of zeroes answers yes to every one of them.
            animals: std::sync::Arc::from(
                (0..(ANIMAL_SHEETS.len() * SHEET_SLOTS) as u32)
                    .map(|i| i + EXTRA_LAYERS_FOR_TEST + 1)
                    .collect::<Vec<_>>(),
            ),
            reliefs: embedded_reliefs(),
            hung: embedded_hung(),
            mossy: std::sync::Arc::from(Vec::new()),
        }
    }

    /// The texture for one *world* face of a block, after its
    /// orientation has been taken into account.
    ///
    /// The table is built per block kind and indexed by the block's own
    /// faces -- a log's top is its cut end, wherever the log happens to
    /// be pointing. Turning a block moves which world face shows which
    /// of its own, so the lookup rotates the face index on the way in
    /// and the table stays one entry per kind.
    #[inline]
    pub fn layer_for_face(&self, block_id: BlockId, face: usize) -> u32 {
        // **Fallen leaves wear their own wood's picture** on the ground as
        // well as in the pack: a birch wood's floor is pale and a fir's is
        // rust (`wood_icon`, and `types::carries_wood` for why the wood is
        // in the id at all).
        if let Some(index) = wood_icon(block_id).filter(|_| {
            matches!(
                primitive_shared::types::block_kind(block_id),
                primitive_shared::types::BLOCK_LEAF_LITTER | primitive_shared::types::BLOCK_LEAF_HANDFUL
            )
        }) {
            return self.extra(index);
        }
        let kind = primitive_shared::types::block_kind(block_id);
        if kind > self.max_block_id || face >= FACES {
            return 0;
        }
        let face = local_face(face, primitive_shared::types::block_axis(block_id));
        // ...and then turned to whichever way it is looking, for the
        // handful of blocks that have a front. See `types::Facing`.
        let face = faced_face(face, primitive_shared::types::block_facing(block_id));
        self.layers[kind as usize * SLOTS + face]
    }

    /// The first frame of one of the fire's two sheets.
    ///
    /// Which sheet a quad gets is the mesher's business -- the two
    /// crossing quads must not wear the same picture -- and *which
    /// frame* is the shader's. See `EXTRA_FLAME`.
    ///
    /// Arithmetic on the layer rather than a lookup by name, because the
    /// second sheet has no name: it is `FLAME_FRAMES` layers derived in
    /// `load` and laid straight after the drawn ones. Contiguity is what
    /// both this and `animated` in the shader rest on.
    pub fn flame(&self, sheet: u32) -> u32 {
        self.extra(EXTRA_FLAME) + (sheet % FLAME_SHEETS) * FLAME_FRAMES
    }

    /// The skin on a loaded drying rack. See `EXTRA_STRETCHED_HIDE`.
    pub fn stretched_hide(&self) -> u32 {
        self.extra(EXTRA_STRETCHED_HIDE)
    }

    /// The cured skin on a hide frame. See `EXTRA_STRETCHED_LEATHER`.
    pub fn stretched_leather(&self) -> u32 {
        self.extra(EXTRA_STRETCHED_LEATHER)
    }

    /// The layer of one of the pictures that belong to no block. See
    /// `EXTRA_TEXTURES`.
    ///
    /// Out of range is the placeholder rather than a panic, for the same
    /// reason an unknown block id is: this is reached from the entity
    /// path, and an entity kind arrives over a socket.
    #[inline]
    pub fn extra(&self, index: usize) -> u32 {
        self.extra.get(index).copied().unwrap_or(0)
    }

    /// The block's picture with moss grown over it, if this is a block that
    /// can wear moss (`ground::may_grow_moss`). `None` for everything else,
    /// and for a pack whose atlas was built before this existed.
    #[inline]
    pub fn mossy(&self, block_id: BlockId) -> Option<u32> {
        let kind = primitive_shared::types::block_kind(block_id) as usize;
        match self.mossy.get(kind).copied().unwrap_or(0) {
            0 => None,
            layer => Some(layer),
        }
    }

    /// One picture off one animal's sheet.
    ///
    /// By the species' place in `Species::ALL`, which is what keeps this
    /// from being a second list that has to agree with the first one,
    /// and by the slot `animal_model::Skin::slot` gives -- the one place
    /// that decides where on a sheet an ear is.
    ///
    /// A slot the sheet left blank comes back as the animal's hide, so
    /// a species with no tusks needs no entry anywhere: the loader
    /// aliased the empty tile when it cut the sheet up.
    #[inline]
    pub fn animal(&self, species: primitive_shared::animals::Species, slot: usize) -> u32 {
        let index = sheet_index(species);
        self.animals
            .get(index * SHEET_SLOTS + slot.min(SHEET_SLOTS - 1))
            .copied()
            .unwrap_or(0)
    }

    /// The picture for this block *in the pack*, if it has one of its
    /// own. See `ITEM_SLOT`.
    #[inline]
    pub fn layer_for_item(&self, block_id: BlockId) -> Option<u32> {
        if let Some(index) = wood_icon(block_id) {
            return Some(self.extra(index));
        }
        let kind = primitive_shared::types::block_kind(block_id);
        if kind > self.max_block_id {
            return None;
        }
        match self.layers[kind as usize * SLOTS + ITEM_SLOT] {
            0 => None,
            layer => Some(layer),
        }
    }
}

/// Which of a block's own faces is showing at a given world face.
///
/// Face order is the mesher's: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
///
/// A block lying along X has been turned a quarter turn about Z, so its
/// own top points at world +X; one lying along Z has been turned about
/// X, so its top points at world +Z. The two tables are those rotations
/// written out -- six entries each is cheaper to read, and to be sure
/// of, than the matrix that would generate them.
#[inline]
fn local_face(world_face: usize, axis: primitive_shared::types::Axis) -> usize {
    use primitive_shared::types::Axis;
    const ALONG_X: [usize; FACES] = [3, 2, 0, 1, 4, 5];
    // World +Z shows the local top, so the local +Z face -- turned to
    // world -Y by the same quarter turn -- is what the *bottom* shows,
    // and -Z is what the top shows. Swapping the first two entries
    // (as this table once did) is not a different rotation: it is a
    // reflection, which no turned block can produce, and it put each
    // end's texture on the opposite end.
    const ALONG_Z: [usize; FACES] = [5, 4, 2, 3, 0, 1];
    match axis {
        Axis::Y => world_face,
        Axis::X => ALONG_X[world_face],
        Axis::Z => ALONG_Z[world_face],
    }
}

/// Which of a block's own side textures a world face shows, once the
/// block has been turned to face somewhere.
///
/// **A quarter turn per step, and only the four sides move.** The top of
/// a kiln is its top whichever way the mouth points, and the bottom is
/// the bottom; what rotates is which wall the player is looking at.
///
/// The face order is +Y, -Y, +X, -X, +Z, -Z (see the constants above),
/// and a block facing north shows its own north picture on world -Z.
///
/// **The ring is in the order `Facing` counts -- north, east, south, west
/// -- and it used to run the other way round.** It was east, north, west,
/// south, which is a turn in the opposite sense, so the step and the
/// picture disagreed about every facing that is not its own reverse:
/// north and south came out right, and a chest, kiln or bloomery put down
/// by a player looking along the x axis showed that player its *back*,
/// with the mouth on the far side. Half of every camp faced the wrong
/// way, and "blocks cannot be placed in different directions" is what a
/// player makes of that. Held by
/// `a_chest_placed_from_each_side_shows_its_front_to_whoever_placed_it`.
fn faced_face(world_face: usize, facing: primitive_shared::types::Facing) -> usize {
    if world_face == FACE_TOP || world_face == FACE_BOTTOM {
        return world_face;
    }
    // Index `i` is the world side a block facing `Facing` number `i`
    // points its front at (`Facing::step`), so a block turned `q`
    // quarters shows at side `i` the picture drawn for side `i - q`.
    const RING: [usize; 4] = [FACE_NORTH, FACE_EAST, FACE_SOUTH, FACE_WEST];
    let at = RING.iter().position(|&f| f == world_face).unwrap_or(0);
    let turned = (at + 4 - facing.quarters() as usize) % 4;
    RING[turned]
}

/// Every character with a layer of its own, in layer order.
///
/// **The font's own order, not a second copy of it.** This used to be
/// the string itself, written out here, while `font::glyph` had its own
/// list of the characters it could draw -- two lists that had to agree
/// and nothing to make them. The font is a picture now
/// (`assets/fonts/primitive.png`) and the order of its cells is the
/// order of these layers, so there is one list and it lives beside the
/// font: see `font::ORDER`.
///
/// The name stays because half the client asks `texture::GLYPHS` and
/// `texture::has_glyph` what it may draw.
pub const GLYPHS: &str = crate::engine::font::ORDER;
// The font used to start at layer 1, immediately after the placeholder,
// and everything else was pushed after it. That order is now reversed --
// blocks first, glyphs last -- and `FontAtlas::base` is where they
// actually landed. See `MAX_TERRAIN_LAYERS`.

/// Whether the font can draw this character.
///
/// What every text field asks before accepting a keystroke. The test
/// used to be `is_ascii_graphic`, which was right when the font was
/// ASCII and wrong ever since it learned Cyrillic and Polish: the
/// interface could *say* things in four languages while the player
/// could type in one. A world named "Дом" costs nothing the font does
/// not already have.
#[inline]
pub fn has_glyph(c: char) -> bool {
    GLYPHS.contains(c)
}

/// Where the font lives in the texture array, and where in a layer each
/// glyph sits.
///
/// ## Why a glyph is not a layer any more
///
/// It was, and that was the point of the whole arrangement: one glyph
/// per array layer makes a character one textured quad instead of the
/// dozen a per-pixel renderer needs. What it cost was **179 layers**,
/// one per entry in `GLYPHS` -- against an array that wgpu's default
/// limits cap at 256. 1.5 added twenty-odd block textures, the array
/// came to 260, and the game refused to create it at all.
///
/// That cap turned out to be wgpu's portability floor rather than the
/// card's answer -- the card offers 2048, and the game now asks for it
/// (see `renderer::terrain_limits`). The packing stays regardless: it is
/// what makes a font cost fifteen layers instead of a hundred and
/// seventy-nine, and layers spent on glyphs are layers not spent on the
/// world.
///
/// So the glyphs are *packed*: as many per layer as the resolution has
/// room for, laid out in a grid, and this carries where each one landed.
/// At the stock 16x16 that is two per layer -- 179 glyphs in 90 layers
/// instead of 179 -- and at a 32x32 pack it is fifteen, which is twelve
/// layers. Nothing about the drawing changes except that a glyph quad
/// now starts at an offset rather than at the corner.
///
/// The cells are strided rather than packed edge to edge (a 6-wide glyph
/// in an 8-wide cell at 16x16), which leaves a gap of empty texels
/// between neighbours. That is not tidiness: the array has a mip chain,
/// and without the gap a glyph's ink bleeds into the one beside it at
/// every level below the first.
///
/// `Copy` and small, so it is passed by value to everything that draws
/// text rather than reached for through the renderer.
#[derive(Debug, Clone, Copy)]
pub struct FontAtlas {
    /// Where the glyphs start in the texture array.
    ///
    /// A field rather than the constant it was, because the font moved
    /// to the **end** of the array in 1.5 and the end is not a fixed
    /// place -- it is one past however many block images the pack turned
    /// out to have.
    pub base: u32,
    /// Fraction of the layer one glyph covers: `6/resolution` by
    /// `9/resolution` at the stock size. Measured from wherever the
    /// glyph's cell starts -- see `place`.
    pub u_max: f32,
    pub v_max: f32,
    /// How many glyphs share a layer, and how many of them across.
    per_layer: u32,
    columns: u32,
    /// The size of one cell as a fraction of the layer, which is the
    /// step from one glyph's corner to the next one's.
    stride_u: f32,
    stride_v: f32,
}

impl FontAtlas {
    /// A font atlas for a stated resolution, with the glyphs where
    /// `load` would have put them.
    ///
    /// For the tests, and for the screen snapshotter -- see the
    /// `ui_snapshot` example, which draws the interface into a PNG so it
    /// can be *looked at* without a graphics card. An interface nobody
    /// can see is an interface nobody can review.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn for_size(resolution: u32, base: u32) -> Self {
        let (columns, rows) = Self::grid(resolution);
        Self {
            base,
            u_max: crate::engine::font::GLYPH_WIDTH as f32 / resolution as f32,
            v_max: crate::engine::font::GLYPH_HEIGHT as f32 / resolution as f32,
            per_layer: (columns * rows).max(1),
            columns,
            stride_u: crate::engine::font::CELL_WIDTH as f32 / resolution as f32,
            stride_v: crate::engine::font::CELL_HEIGHT as f32 / resolution as f32,
        }
    }

    /// How the glyphs are laid out at a given texture resolution.
    ///
    /// Answers `(columns, rows)`, at least one of each: a pack whose
    /// tiles are smaller than a glyph gets one glyph per layer and a
    /// clipped one, which is what it asked for.
    fn grid(resolution: u32) -> (u32, u32) {
        use crate::engine::font::{CELL_HEIGHT, CELL_WIDTH};
        (
            (resolution / CELL_WIDTH as u32).max(1),
            (resolution / CELL_HEIGHT as u32).max(1),
        )
    }

    /// The layer holding a character, and where in it the glyph starts.
    ///
    /// Layer 0 with no offset for anything outside `GLYPHS`, which draws
    /// the same visible box a missing glyph would.
    #[inline]
    pub fn place(&self, c: char) -> (u32, f32, f32) {
        let Some(index) = GLYPHS.chars().position(|g| g == c) else {
            return (0, 0.0, 0.0);
        };
        let index = index as u32;
        let slot = index % self.per_layer;
        (
            self.base + index / self.per_layer,
            (slot % self.columns) as f32 * self.stride_u,
            (slot / self.columns) as f32 * self.stride_v,
        )
    }

    /// A stand-in for tests, which lay text out without a GPU.
    ///
    /// One glyph per layer and a whole layer each, so a test that maps a
    /// quad back to the character it stands for can do it by layer alone
    /// -- see `widgets::dump_to_png`.
    pub fn for_test() -> Self {
        Self {
            base: 1,
            u_max: 1.0,
            v_max: 1.0,
            per_layer: 1,
            columns: 1,
            stride_u: 1.0,
            stride_v: 1.0,
        }
    }
}

/// One layer of the font: as many glyphs as fit, drawn
/// white-on-transparent at their native size in a grid.
///
/// Native size, not stretched to fill: a 6x9 bitmap scaled by a
/// non-integer factor gives pixels of uneven width, which is the one
/// thing a pixel font must not do. Anything that does not fit in the
/// cell is clipped rather than squashed, for the same reason.
fn glyph_sheet(glyphs: &[char], resolution: u32) -> RgbaImage {
    use crate::engine::font::{GLYPH_HEIGHT, GLYPH_WIDTH};

    use crate::engine::font::{CELL_HEIGHT, CELL_WIDTH};

    let (columns, _) = FontAtlas::grid(resolution);
    // The same cell `FontAtlas::place` reads by, or a glyph is drawn in
    // one square and read out of another.
    let (stride_x, stride_y) = (CELL_WIDTH as u32, CELL_HEIGHT as u32);
    let mut img = RgbaImage::new(resolution, resolution);

    for (slot, &c) in glyphs.iter().enumerate() {
        let slot = slot as u32;
        let (origin_x, origin_y) = (
            (slot % columns) * stride_x,
            (slot / columns) * stride_y,
        );
        for (row_index, row) in crate::engine::font::glyph(c).iter().enumerate() {
            let y = origin_y + row_index as u32;
            if row_index >= GLYPH_HEIGHT || y >= resolution {
                break;
            }
            for column in 0..GLYPH_WIDTH {
                let x = origin_x + column as u32;
                if x >= resolution {
                    break;
                }
                // Most significant bit is the leftmost pixel, the same
                // way `Painter::text` used to read it.
                if row & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                    continue;
                }
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
    }
    img
}

/// The fewest layers an array may be split at: what GLES promises.
pub const MIN_PER_ARRAY: u32 = 256;
/// The most layers one array is ever asked to hold: what Vulkan and D3D12
/// promise, and what a block vertex can name (`mesh::MAX_TEXTURE_LAYERS`).
pub const MAX_PER_ARRAY: u32 = 2048;
/// How many arrays the atlas may be split into.
///
/// **Eight, because eight arrays of GLES's 256 is exactly what a vertex can
/// name**, and because a fragment stage has only so many texture units to
/// give. GLES 3.0 promises sixteen, and wgpu's own default
/// `max_sampled_textures_per_shader_stage` is the same sixteen; the terrain
/// shader already binds three textures that are not the atlas (the sun's
/// shadow map, the lamp cells and the column heights), so eight arrays is
/// eleven of sixteen and a ninth array would buy layers no vertex can
/// address. `a_phone_with_the_gles_minimum_can_bind_every_array` counts the
/// other textures out of `shader.wgsl` rather than trusting this paragraph.
pub const MAX_ARRAYS: u32 = 8;
// ...and those eight, at GLES's least, reach what a vertex names. Checked
// by the compiler, since both sides are constants.
const _: () = assert!(
    MIN_PER_ARRAY * MAX_ARRAYS >= crate::engine::mesh::MAX_TEXTURE_LAYERS,
    "eight arrays of 256 do not reach what a vertex can name"
);

/// **How the atlas is laid across texture arrays**, and the one place the
/// arithmetic between a layer and an array lives.
///
/// A layer is still one number everywhere above the shader -- in a vertex,
/// in `FaceLayers`, in the fire's animated run -- and it means "the n-th
/// picture of the atlas". Only the fragment shader splits it:
/// `array = layer / per_array`, `slot = layer % per_array`, and samples
/// `slot` of that array.
///
/// **Why it exists.** One array was enough while a device granted as many
/// layers as the pack had: 2048 on Vulkan and D3D12. A GLES driver is only
/// obliged to offer 256, the atlas is past 500, and a phone with no Vulkan
/// driver could not start at all -- `TextureManager::load` said so in words
/// and that was the whole of the support. Split at what the device grants,
/// the same pack fits in three arrays there and in one everywhere else.
///
/// Rejected, in the order they were weighed:
///
/// * **A draw per array**: sorting the terrain by which array its pictures
///   are in. A chunk is one draw today, and a chunk with grass (array 0) and
///   a bronze tool (array 1) would become two -- on exactly the GLES phones
///   where a draw call costs most. Rejected on sight.
/// * **`binding_array<texture_2d_array>`**, one binding holding all of them:
///   the clean spelling, and a feature (`TEXTURE_BINDING_ARRAY`) GLES does
///   not have. The device that needs the split is the one that cannot use it.
/// * **Several arrays, chosen per fragment** -- this. A branch on a flat
///   layer and one fetch; draws unchanged, the vertex unchanged in size.
///
/// **Where one array is enough nothing changes at all.** `arrays == 1`
/// leaves every shader exactly the text in its file (`specialise` hands it
/// back borrowed), the bind group layout is the two entries it always was,
/// and the frame is byte for byte the frame before the split existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasSplit {
    /// Layers in every array but the last. Only meaningful when `arrays` is
    /// above one; a single array is exactly as deep as the atlas.
    pub per_array: u32,
    pub arrays: u32,
}

impl AtlasSplit {
    /// One array, the shaders as written: what every repro that builds its
    /// own two-entry layout is drawing with.
    #[cfg(test)]
    pub const ONE: Self = Self { per_array: MAX_PER_ARRAY, arrays: 1 };

    /// `layers` pictures on a device whose arrays hold `device_ceiling`.
    ///
    /// Held between GLES's floor and Vulkan's promise: below 256 is not a
    /// conforming device, and above 2048 buys nothing a vertex can name.
    pub fn new(layers: u32, device_ceiling: u32) -> Self {
        let per_array = device_ceiling.clamp(MIN_PER_ARRAY, MAX_PER_ARRAY);
        Self { per_array, arrays: layers.max(1).div_ceil(per_array) }
    }

    /// Whether the split can be bound: few enough arrays for a fragment
    /// stage, few enough layers for a vertex.
    pub fn addresses(self, layers: u32) -> bool {
        self.arrays <= MAX_ARRAYS
            && layers <= crate::engine::mesh::MAX_TEXTURE_LAYERS
            && layers <= self.per_array.saturating_mul(self.arrays)
    }

    /// Which array holds `layer`, and where in it. What the shader does.
    pub fn locate(self, layer: u32) -> (u32, u32) {
        if self.arrays == 1 {
            return (0, layer);
        }
        (layer / self.per_array, layer % self.per_array)
    }

    /// The inverse of `locate`.
    #[cfg(test)]
    pub fn layer(self, array: u32, slot: u32) -> u32 {
        if self.arrays == 1 {
            return slot;
        }
        array * self.per_array + slot
    }

    /// How deep array `array` is made, for an atlas of `layers`: full but
    /// for the last, which holds what is left.
    pub fn depth(self, array: u32, layers: u32) -> u32 {
        if self.arrays == 1 {
            return layers.max(1);
        }
        layers.saturating_sub(array * self.per_array).min(self.per_array)
    }

    /// The bind group layout the terrain, the interface and the particles
    /// share: the arrays and the sampler, at the bindings `specialise`
    /// declares.
    pub fn layout_entries(self) -> Vec<wgpu::BindGroupLayoutEntry> {
        let array = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let mut entries = vec![
            array(0),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ];
        entries.extend((1..self.arrays).map(|i| array(Self::binding(i))));
        entries
    }

    /// The entries of a bind group for `layout_entries`: `views` one per
    /// array, in order. The player's skin hands its one picture for every
    /// array, which is why this takes a list rather than a `TextureManager`.
    pub fn bind_entries<'a>(
        self,
        views: &[&'a wgpu::TextureView],
        sampler: &'a wgpu::Sampler,
    ) -> Vec<wgpu::BindGroupEntry<'a>> {
        assert_eq!(views.len() as u32, self.arrays, "one view per array of the atlas");
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(sampler),
        }];
        entries.extend(views.iter().enumerate().map(|(i, view)| wgpu::BindGroupEntry {
            binding: Self::binding(i as u32),
            resource: wgpu::BindingResource::TextureView(view),
        }));
        entries
    }

    /// Where array `i` is bound: 0 for the first, as it always was, and the
    /// rest after the sampler.
    fn binding(i: u32) -> u32 {
        if i == 0 {
            0
        } else {
            i + 1
        }
    }

    /// The shader, rewritten to read a split atlas.
    ///
    /// **A rewrite and not a second copy of every sampling site**, because
    /// the files are also compiled as written: by `shaders_compile`, by every
    /// repro that builds a two-entry layout, and by the tests that find a
    /// line of `sample_block` and replace it. With one array this returns
    /// the text it was handed; with more, every
    /// `textureSample*(block_textures, block_sampler, ...)` and
    /// `textureLoad(block_textures, ...)` becomes the `atlas_*` function of
    /// the same arguments, and those functions -- and the other arrays'
    /// bindings -- are appended.
    ///
    /// **Inside a branch, and legal for a reason worth writing down.**
    /// `textureSample` takes its derivatives from the 2x2 quad of pixels, and
    /// the language forbids it under a branch on a varying because the
    /// neighbours might take the other path. They cannot here: the branch is
    /// on the *layer*, which is `@interpolate(flat)` -- one value for the
    /// whole primitive -- and the quad's helper pixels belong to that same
    /// primitive. naga does not enforce the rule for fragment shaders
    /// (`DISABLE_UNIFORMITY_REQ_FOR_FRAGMENT_STAGE`), and the alternative,
    /// `textureSampleGrad` everywhere, is the instruction `sample_block`
    /// measured as the expensive one.
    pub fn specialise<'a>(self, source: std::borrow::Cow<'a, str>) -> std::borrow::Cow<'a, str> {
        if self.arrays <= 1 {
            return source;
        }
        let mut text = source.into_owned();
        for (call, replacement) in [
            ("textureSampleGrad(", "atlas_sample_grad("),
            ("textureSampleLevel(", "atlas_sample_level("),
            ("textureSample(", "atlas_sample("),
            ("textureLoad(", "atlas_load("),
        ] {
            text = rewrite_calls(&text, call, replacement);
        }
        text.push_str(&self.dispatch());
        std::borrow::Cow::Owned(text)
    }

    /// The bindings and the four `atlas_*` functions `specialise` appends.
    fn dispatch(self) -> String {
        use std::fmt::Write;
        let name = |i: u32| if i == 0 { "block_textures".to_string() } else { format!("block_textures_{i}") };
        let mut out = String::new();
        let _ = writeln!(
            out,
            "\n// ---- the atlas in {} arrays of {} (`texture::AtlasSplit`) ----",
            self.arrays, self.per_array
        );
        let _ = writeln!(out, "const ATLAS_PER_ARRAY: u32 = {}u;", self.per_array);
        for i in 1..self.arrays {
            let _ = writeln!(
                out,
                "@group(1) @binding({})\nvar {}: texture_2d_array<f32>;",
                Self::binding(i),
                name(i)
            );
        }
        for (function, params, returns, call) in [
            ("atlas_sample", "uv: vec2<f32>, layer: i32", "vec4<f32>", "textureSample({t}, block_sampler, uv, slot)"),
            (
                "atlas_sample_grad",
                "uv: vec2<f32>, layer: i32, ddx: vec2<f32>, ddy: vec2<f32>",
                "vec4<f32>",
                "textureSampleGrad({t}, block_sampler, uv, slot, ddx, ddy)",
            ),
            (
                "atlas_sample_level",
                "uv: vec2<f32>, layer: i32, level: f32",
                "vec4<f32>",
                "textureSampleLevel({t}, block_sampler, uv, slot, level)",
            ),
            ("atlas_load", "texel: vec2<i32>, layer: i32, level: i32", "vec4<f32>", "textureLoad({t}, texel, slot, level)"),
        ] {
            let _ = writeln!(out, "fn {function}({params}) -> {returns} {{");
            let _ = writeln!(out, "    let which = u32(layer) / ATLAS_PER_ARRAY;");
            let _ = writeln!(out, "    let slot = i32(u32(layer) % ATLAS_PER_ARRAY);");
            // The last array also takes anything past the end, so a layer
            // out of range is clamped by the sampler as it always was
            // rather than falling back into the first array.
            for i in (1..self.arrays).rev() {
                let test = if i == self.arrays - 1 { ">=" } else { "==" };
                let _ = writeln!(
                    out,
                    "    if (which {test} {i}u) {{ return {}; }}",
                    call.replace("{t}", &name(i))
                );
            }
            let _ = writeln!(out, "    return {};\n}}", call.replace("{t}", &name(0)));
        }
        out
    }
}

/// Every `call` whose first argument is `block_textures` (and, for the
/// samplers, whose second is `block_sampler`), renamed to `replacement` with
/// those arguments dropped. Whitespace between them is whatever the file has:
/// `sample_cutout` spells its `textureLoad` over several lines.
fn rewrite_calls(text: &str, call: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(call) {
        // `textureSample(` must not match inside `atlas_textureSample(`
        // or a longer name; the ones this is used on are all builtins.
        let preceded = rest[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
        out.push_str(&rest[..at]);
        let after = &rest[at + call.len()..];
        let args = after.trim_start();
        let Some(args) = args.strip_prefix("block_textures") else {
            out.push_str(call);
            rest = after;
            continue;
        };
        let args = args.trim_start();
        let Some(mut args) = args.strip_prefix(',') else {
            out.push_str(call);
            rest = after;
            continue;
        };
        if preceded {
            out.push_str(call);
            rest = after;
            continue;
        }
        if call != "textureLoad(" {
            let Some(past) = args.trim_start().strip_prefix("block_sampler").and_then(|a| a.trim_start().strip_prefix(',')) else {
                out.push_str(call);
                rest = after;
                continue;
            };
            args = past;
        }
        out.push_str(replacement);
        rest = args.trim_start();
    }
    out.push_str(rest);
    out
}

pub struct TextureManager {
    /// The first array of the atlas -- and, wherever one array holds it all,
    /// the whole of it. See `AtlasSplit`.
    pub texture_view: wgpu::TextureView,
    /// The arrays after the first, in order; empty unless the atlas is
    /// split. Bound by `bind_entries`.
    pub more_views: Vec<wgpu::TextureView>,
    /// How the atlas is laid across `texture_view` and `more_views`.
    pub split: AtlasSplit,
    pub sampler: wgpu::Sampler,
    /// Always nearest; see `build_ui_sampler`.
    pub ui_sampler: wgpu::Sampler,
    /// The picture the cloud layer is drawn from.
    clouds: CloudTexture,
    /// Side of one texture layer, in texels. The terrain shader needs it
    /// to snap UVs to texel centres.
    pub resolution: u32,
    /// Number of distinct images uploaded (array layers).
    pub layer_count: u32,
    /// Where the font sits in that array.
    pub font: FontAtlas,
    /// Flat table: `block_id * SLOTS + face` -> array layer, with the
    /// carried picture in the slot past the six faces.
    face_layers: Vec<u32>,
    max_block_id: BlockId,
    /// Where the five breaking-overlay stages sit in the array.
    break_layers: [u32; BREAK_STAGES],
    /// ...and where the pictures that belong to no block sit. See
    /// `EXTRA_TEXTURES`.
    extra_layers: Vec<u32>,
    /// One row of `SHEET_SLOTS` per animal. See `FaceLayers::animal`.
    animal_layers: Vec<u32>,
    /// Shapes for the blocks that are not cubes, by block kind.
    item_models: HashMap<BlockId, ItemModel>,
    /// Thicknesses for the things lying on the ground, by block kind.
    reliefs: Reliefs,
    /// The things a drying rack hangs, by block kind.
    hung: HungTable,
    /// One layer per block kind that can wear moss; zero for the rest. See
    /// `FaceLayers::mossy`.
    mossy_layers: Vec<u32>,
}

impl TextureManager {
    pub fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets_dir: &Path,
        anisotropy: u16,
    ) -> anyhow::Result<Self> {
        let ceiling = device.limits().max_texture_array_layers;
        Self::load_split_at(device, queue, assets_dir, anisotropy, ceiling)
    }

    /// Every array of the atlas, for `AtlasSplit::bind_entries`.
    pub fn views(&self) -> Vec<&wgpu::TextureView> {
        std::iter::once(&self.texture_view).chain(&self.more_views).collect()
    }

    /// The bind group entries for the atlas with `sampler`.
    pub fn bind_entries<'a>(&'a self, sampler: &'a wgpu::Sampler) -> Vec<wgpu::BindGroupEntry<'a>> {
        self.split.bind_entries(&self.views(), sampler)
    }

    /// `load`, splitting the atlas as though the device's arrays held
    /// `ceiling` layers -- which is how a desktop is made to draw what a
    /// GLES phone draws (`atlas_split_repro`).
    pub fn load_split_at(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets_dir: &Path,
        anisotropy: u16,
        ceiling: u32,
    ) -> anyhow::Result<Self> {
        let textures_dir = assets_dir.join("textures");
        let config_path = textures_dir.join("blocks.toml");

        // **Before anything asks for a letter.** The font is a picture
        // in `assets/fonts`, and a copy of it beside the executable
        // replaces the built-in one the same way a block texture does --
        // but `font::glyph` is a free function with nowhere to hang a
        // path, so this is where the path arrives. It has to be here
        // rather than further down: the first call to `glyph` freezes
        // the table for the life of the process, and the glyph sheets a
        // few hundred lines below are that first call.
        crate::engine::font::use_assets_dir(assets_dir);

        // A file on disk wins over the built-in copy, so a folder of
        // replacement textures next to the executable works. Without
        // one, the embedded assets mean the game is a single file rather
        // than an executable that starts and renders every block as a
        // magenta checkerboard.
        let config: BlocksToml = match std::fs::read_to_string(&config_path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| anyhow::anyhow!("{} is invalid: {e}", config_path.display()))?,
            Err(_) => {
                println!("using built-in textures ({} not found)", config_path.display());
                toml::from_str(crate::embedded::BLOCKS_TOML)
                    .map_err(|e| anyhow::anyhow!("the built-in blocks.toml is invalid: {e}"))?
            }
        };

        let resolution = config.resolution.clamp(1, 512);

        let max_block_id = ALL_BLOCK_IDS
            .iter()
            .map(|&(id, _)| id)
            .max()
            .unwrap_or(0);

        let mut images: Vec<RgbaImage> = Vec::new();
        let mut layer_of_file: HashMap<String, u32> = HashMap::new();
        let mut face_layers = vec![0u32; (max_block_id as usize + 1) * SLOTS];

        // Layer 0 is always the placeholder, so an unconfigured block or
        // an out-of-range id resolves to something obviously wrong rather
        // than silently to grass.
        images.push(placeholder_texture(resolution));

        // Shapes for the things that are not cubes. See
        // `engine::item_model` for what one is and why.
        let mut item_models: HashMap<BlockId, ItemModel> = HashMap::new();

        for &(block_id, name) in ALL_BLOCK_IDS {
            let spec = config.textures.get(name);
            if spec.is_none() {
                eprintln!(
                    "warning: no texture configured for block '{name}' in {}; using placeholder",
                    config_path.display()
                );
            }
            for face in 0..FACES {
                let layer = match spec.and_then(|s| s.for_face(face)) {
                    Some(filename) => *layer_of_file
                        .entry(filename.to_string())
                        .or_insert_with(|| {
                            let img = load_or_placeholder(
                                &textures_dir.join(filename),
                                filename,
                                resolution,
                            );
                            images.push(img);
                            (images.len() - 1) as u32
                        }),
                    None => 0,
                };
                face_layers[block_id as usize * SLOTS + face] = layer;
            }

            // ...and the carried picture, if this block asked for one.
            if let Some(filename) = spec.and_then(|s| s.for_item()) {
                let layer = *layer_of_file
                    .entry(filename.to_string())
                    .or_insert_with(|| {
                        let img = load_or_placeholder(
                            &textures_dir.join(filename),
                            filename,
                            resolution,
                        );
                        images.push(img);
                        (images.len() - 1) as u32
                    });
                face_layers[block_id as usize * SLOTS + ITEM_SLOT] = layer;
            }

            // Anything that is not a cube gets a model cut from its
            // picture. Which ones those are is the block's business, not
            // the texture system's -- an item has no cell in the world,
            // a cross-shaped plant is a sprite standing in one, and a
            // stone lies flat in one. All three are pictures rather than
            // boxes, and a dropped one drawn as a cube shows its
            // transparent corners as whatever was behind them.
            // ...or that asked for a picture of its own. A block with an
            // `item` texture has said, in the only way this file can,
            // that a face of it is the wrong picture for a carried one
            // -- and a dropped stack is exactly a carried one lying on
            // the ground. Ash is the case that named the key: a tile of
            // it says what a floor of ash looks like, and a shovelful
            // dropped in the grass is a handful.
            let carried = face_layers[block_id as usize * SLOTS + ITEM_SLOT];
            if is_item(block_id) || is_cross(block_id) || is_flat(block_id) || carried != 0 {
                // The carried picture if there is one, else the face
                // that stands in for it. Taking the face regardless is
                // what made a dropped handful of ash a paving slab.
                let layer = if carried != 0 {
                    carried as usize
                } else {
                    face_layers[block_id as usize * SLOTS] as usize
                };
                let model = ItemModel::from_image(&images[layer]);
                if model.quads.is_empty() {
                    eprintln!(
                        "warning: '{name}' has no opaque texels, so a dropped one                          would be invisible; drawing it as a cube instead"
                    );
                } else {
                    item_models.insert(block_id, model);
                }
            }
        }

        // The things lying on the ground, cut from the picture the mesher
        // lays on them -- face 0, not the carried one: what is on the ground
        // is what the flat quad showed, and the two have to agree or a stone
        // stands up out of a silhouette it is not wearing.
        let mut reliefs: Vec<Option<Relief>> = vec![None; max_block_id as usize + 1];
        for &(block_id, _) in ALL_BLOCK_IDS {
            if relief::has_relief(block_id) {
                let layer = face_layers[block_id as usize * SLOTS] as usize;
                reliefs[block_id as usize] = Some(Relief::from_image(&images[layer]));
            }
        }
        // The things a rack hangs, cut from the picture the rack dresses
        // them in -- the carried one where there is one, as a slab of the
        // good wore before (`mesh::hang_goods`) -- so the swatch a strip
        // wears is a piece of the picture it samples.
        let mut hung: Vec<Option<Hung>> = vec![None; max_block_id as usize + 1];
        for &(block_id, _) in ALL_BLOCK_IDS {
            if is_hung(block_id) {
                let item = face_layers[block_id as usize * SLOTS + ITEM_SLOT];
                let layer = if item != 0 { item } else { face_layers[block_id as usize * SLOTS] } as usize;
                hung[block_id as usize] = images.get(layer).map(Hung::from_image);
            }
        }

        // The breaking overlay, last: five stages of cracks laid over
        // whatever block is being mined. They live in the same array as
        // everything else, so drawing them needs no second texture, no
        // second bind group and no shader of their own -- the terrain
        // shader already samples this array by layer index, and the
        // cracks are a quad with a layer like any other.
        let mut break_layers = [0u32; BREAK_STAGES];
        for (stage, layer) in break_layers.iter_mut().enumerate() {
            let filename = format!("effects/break.{stage}.png");
            images.push(load_or_placeholder(
                &textures_dir.join(&filename),
                &filename,
                resolution,
            ));
            *layer = (images.len() - 1) as u32;
        }

        // The pictures that belong to no block -- animal skins and
        // falling weather. Here rather than in `blocks.toml`, for the
        // reason `EXTRA_TEXTURES` gives, and *before* the font for the
        // reason everything else is: a terrain vertex has to be able to
        // name them, and an animal is drawn in the terrain format.
        let mut extra_layers = Vec::with_capacity(EXTRA_TEXTURES.len());
        for (index, filename) in EXTRA_TEXTURES.iter().enumerate() {
            images.push(load_or_placeholder(
                &textures_dir.join(filename),
                filename,
                resolution,
            ));
            extra_layers.push((images.len() - 1) as u32);

            // **The fire's second sheet, which is not a file.**
            //
            // The last drawn frame has just gone in, so the layers that
            // follow are the ones `FaceLayers::flame(1)` and `animated`
            // in the shader both count on -- a second run of
            // `FLAME_FRAMES`, immediately after the first. It gets no
            // entry in `extra_layers`: nothing names it, everything
            // reaches it by adding `FLAME_FRAMES` to the first frame,
            // which is why `EXTRA_STRETCHED_HIDE` is still an index into
            // `EXTRA_TEXTURES` and not into the array.
            //
            // Mirrored and `FLAME_SHEET_LAG` frames late, for the reason
            // `FLAME_SHEETS` gives: two quads crossing at right angles
            // wearing one picture read as a flat X, and two quads
            // swelling in step read as a bellows.
            if index == EXTRA_FLAME + FLAME_FRAMES as usize - 1 {
                // The drawn frames went in one after another, so they
                // are a run in `images` as well as in the array.
                let first = extra_layers[EXTRA_FLAME] as usize;
                let derived =
                    mirrored_flame_sheet(&images[first..first + FLAME_FRAMES as usize]);
                images.extend(derived);
            }
        }

        // **The mossy faces**, one picture per block that can wear moss
        // (`ground::may_grow_moss`): the block's own side with the overlay
        // laid over it. Six woods and fifteen rocks in two forms is a few
        // dozen layers out of the hundreds spare, and it buys moss that is a
        // picture rather than a green wash over the stone's own.
        //
        // Rejected: a second quad over the face. That is a quad per mossy
        // block in the densest loop the client has, for a surface that never
        // moves -- and it has to be lit, sorted and culled like any other.
        let moss_overlay = images[extra_layers[EXTRA_MOSS] as usize].clone();
        let mut mossy_layers = vec![0u32; max_block_id as usize + 1];
        for &(block_id, _) in ALL_BLOCK_IDS {
            if !primitive_shared::ground::may_grow_moss(block_id) {
                continue;
            }
            let base = face_layers[block_id as usize * SLOTS + 2] as usize;
            let mut grown = images[base].clone();
            image::imageops::overlay(&mut grown, &moss_overlay, 0, 0);
            images.push(grown);
            mossy_layers[block_id as usize] = (images.len() - 1) as u32;
        }

        // The animals, one sheet each, cut into tiles.
        //
        // **A blank tile is not a picture, it is a "no".** The grid has
        // room for a tusk and a set of antlers whether or not this
        // animal has either, and an empty one aliases the hide -- which
        // is what makes drawing a wolf's tusks a matter of drawing them.
        // It also costs nothing: an alias is the same layer number
        // twice, not a second copy of the picture.
        let mut animal_layers = vec![0u32; ANIMAL_SHEETS.len() * SHEET_SLOTS];
        for (index, filename) in ANIMAL_SHEETS.iter().enumerate() {
            let sheet = load_sheet(&textures_dir.join(filename), filename);
            let tiles = sheet_tiles(&sheet, resolution);
            // Slot 0 is the hide, and it is what everything blank falls
            // back to -- so it is pushed first and unconditionally, even
            // for a sheet that is somehow empty.
            let hide_layer = {
                images.push(tiles[0].clone());
                (images.len() - 1) as u32
            };
            for slot in 0..SHEET_SLOTS {
                // Slot 0 *is* the hide, and a blank slot falls back to
                // it -- one branch, because they are the same answer.
                animal_layers[index * SHEET_SLOTS + slot] =
                    if slot == 0 || is_blank(&tiles[slot]) {
                        hide_layer
                    } else {
                        images.push(tiles[slot].clone());
                        (images.len() - 1) as u32
                    };
            }
        }

        // **Everything a terrain vertex can name is behind us here**,
        // and that is why the check happens at this line rather than at
        // the end of the function.
        //
        // A terrain vertex carries its layer in eleven bits, so a pack
        // with more *block* images than that would silently wrap and
        // dress half the world in glyphs. Refusing to start says so
        // instead.
        let terrain_layers = images.len() as u32;
        anyhow::ensure!(
            terrain_layers <= crate::engine::mesh::MAX_TEXTURE_LAYERS,
            "{terrain_layers} block textures configured, but a block vertex can only \
             address {}; use fewer distinct images",
            crate::engine::mesh::MAX_TEXTURE_LAYERS
        );

        // The font, one glyph per layer, **last**.
        //
        // Text used to be drawn as one quad per lit font pixel -- about
        // twelve quads, seventy-five vertices, per character. A screen of
        // menu text or the F3 panel came to forty thousand vertices and
        // one and a half megabytes, rebuilt and uploaded *every frame*;
        // at a few hundred frames a second that is gigabytes a second
        // spent on the interface, and it showed up exactly where you
        // would expect -- opening a menu or the debug panel cost most of
        // the frame rate.
        //
        // A glyph per layer makes a character one textured quad, and the
        // layers live in this same array so it needs no second texture,
        // no second sampler, no second bind group and no shader change:
        // the UI shader already samples this array by layer index, and
        // its vertex carries a whole `u32` to name one with.
        //
        // What changed in 1.5 is the *order* and the *packing*, and the
        // second one was not optional. A glyph used to have a layer to
        // itself, which is 179 of them -- and a texture array is capped
        // at 256 layers by the graphics API, not by this file. Adding
        // twenty-odd block textures took the array to 260 and the game
        // would not start at all. Packed two to a layer at the stock
        // resolution, the font is 90 layers instead of 179.
        //
        // Putting it *last* is the other half, and that one is about the
        // eight bits a terrain vertex carries: everything a block face
        // can name has to fit below 256, and the font is the one thing
        // in the array no terrain vertex has ever sampled.
        let font_base = images.len() as u32;
        let (columns, rows) = FontAtlas::grid(resolution);
        let per_layer = (columns * rows).max(1) as usize;
        let glyphs: Vec<char> = GLYPHS.chars().collect();
        for sheet in glyphs.chunks(per_layer) {
            images.push(glyph_sheet(sheet, resolution));
        }

        let layer_count = images.len() as u32;

        // **What this device actually granted, not what the file hoped
        // for.** The array ceiling is asked of the adapter (see
        // `renderer::terrain_limits`): thousands on Vulkan or D3D12, and
        // on a GLES driver as little as 256. It used to be refused here
        // when the pack was deeper than that, and a phone without a
        // Vulkan driver could not start; now the atlas is split across
        // as many arrays as it takes (`AtlasSplit`), and what is left to
        // refuse is a pack past what eight arrays or a vertex can name.
        let split = AtlasSplit::new(layer_count, ceiling);
        anyhow::ensure!(
            split.addresses(layer_count),
            "the texture pack needs {layer_count} array layers; this device's arrays hold              {ceiling}, which is {} arrays, and a fragment stage is given at most {MAX_ARRAYS}              of them and a vertex names at most {} layers. Ship fewer distinct pictures, or a              pack at a resolution whose font packs tighter.",
            split.arrays,
            crate::engine::mesh::MAX_TEXTURE_LAYERS
        );

        // Mip levels down to 1x1.
        //
        // Needed for two things at once. Anisotropic filtering is only
        // legal in wgpu when minification and mipmapping are both
        // linear, so without a mip chain the setting cannot be offered
        // at all. And the shimmer it is meant to cure -- distant block
        // faces crawling as the camera moves -- is minification
        // aliasing, which is exactly what mips exist for.
        //
        // Generated on the CPU: the images are 16x16, so the whole chain
        // is a handful of box filters done once at startup. A GPU blit
        // chain would need a render pass per level per layer.
        let mip_levels = (resolution.max(1) as f32).log2().floor() as u32 + 1;

        // One array where the device has room for the atlas, several
        // where it does not; see `AtlasSplit`.
        let arrays: Vec<wgpu::Texture> = (0..split.arrays)
            .map(|array| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("block texture array"),
                    size: wgpu::Extent3d {
                        width: resolution,
                        height: resolution,
                        depth_or_array_layers: split.depth(array, layer_count),
                    },
                    mip_level_count: mip_levels,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            })
            .collect();

        for (layer, img) in images.iter().enumerate() {
            let (array, slot) = split.locate(layer as u32);
            // The colour under the holes, spread outwards before
            // anything averages it -- see `bleed_into_transparency` for
            // the canopy this is about.
            //
            // The font is left out on purpose. Its layers are drawn by
            // the interface through `build_ui_sampler`, which filters
            // nothing and reads no mip, so no glyph edge is ever a blend
            // of two texels and there is nothing for a bleed to fix; and
            // a glyph is ink on an empty cell, where "the nearest
            // visible colour" would mean smearing one letter's ink
            // across the space its neighbour is supposed to occupy.
            let mut level = if (layer as u32) < terrain_layers {
                bleed_into_transparency(img)
            } else {
                img.clone()
            };
            let mut size = resolution;
            // What share of this picture a cutout keeps, if it is a
            // cutout picture at all. See `mask_coverage`. Asked of the
            // picture as it was loaded, which is the same question: the
            // bleed above moves colour and never alpha.
            let coverage = mask_coverage(img);
            for mip in 0..mip_levels {
                // The level as it goes to the card. Every level below
                // the first has its alpha stretched so the threshold
                // keeps the same share of it -- see `held_to_coverage`
                // for what goes wrong without that, which is a leaf
                // canopy that fills in solid and a meadow that goes
                // bald.
                let uploaded = match coverage {
                    Some(share) if mip > 0 => {
                        std::borrow::Cow::Owned(held_to_coverage(&level, share))
                    }
                    _ => std::borrow::Cow::Borrowed(&level),
                };
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &arrays[array as usize],
                        mip_level: mip,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: slot,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    uploaded.as_raw(),
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * size),
                        rows_per_image: Some(size),
                    },
                    wgpu::Extent3d {
                        width: size,
                        height: size,
                        depth_or_array_layers: 1,
                    },
                );
                if mip + 1 < mip_levels {
                    size = (size / 2).max(1);
                    level = downsample(&level, size);
                }
            }
        }

        let mut views = arrays.iter().map(|array| {
            array.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        });
        let texture_view = views.next().expect("an atlas has at least one array");
        let more_views: Vec<wgpu::TextureView> = views.collect();

        let sampler = build_sampler(device, anisotropy);
        let ui_sampler = build_ui_sampler(device);
        let clouds = CloudTexture::load(device, queue, &textures_dir);

        println!(
            "textures: {} block(s), {terrain_layers} of {} block layer(s), {layer_count} total \
             at {resolution}x{resolution}, in {} array(s) of {}",
            ALL_BLOCK_IDS.len(),
            crate::engine::mesh::MAX_TEXTURE_LAYERS,
            split.arrays,
            if split.arrays == 1 { layer_count } else { split.per_array },
        );

        Ok(Self {
            texture_view,
            more_views,
            split,
            sampler,
            ui_sampler,
            clouds,
            resolution,
            layer_count,
            font: FontAtlas {
                base: font_base,
                u_max: crate::engine::font::GLYPH_WIDTH as f32 / resolution as f32,
                v_max: crate::engine::font::GLYPH_HEIGHT as f32 / resolution as f32,
                per_layer: per_layer as u32,
                columns,
                // The *cell*, not the glyph and not the layer divided by
                // the count: the gap between ink and cell is what keeps
                // a glyph out of its neighbour at every mip level below
                // the first, and dividing the layer instead put the two
                // readings of this table a texel apart at every
                // resolution but sixteen. See `font::CELL_WIDTH`.
                stride_u: crate::engine::font::CELL_WIDTH as f32 / resolution as f32,
                stride_v: crate::engine::font::CELL_HEIGHT as f32 / resolution as f32,
            },
            face_layers,
            max_block_id,
            break_layers,
            extra_layers,
            animal_layers,
            item_models,
            reliefs: std::sync::Arc::from(reliefs),
            hung: std::sync::Arc::from(hung),
            mossy_layers,
        })
    }

    /// The picture the sky's cloud layer is drawn from.
    pub fn clouds(&self) -> &CloudTexture {
        &self.clouds
    }

    /// The shape a dropped one of these has, if it is not a cube.
    pub fn item_model(&self, block: BlockId) -> Option<&ItemModel> {
        self.item_models
            .get(&primitive_shared::types::block_kind(block))
    }

    /// The crack overlay for a given stage of breaking, 0 (barely
    /// scratched) to `BREAK_STAGES - 1` (about to give).
    pub fn break_layer(&self, stage: usize) -> u32 {
        self.break_layers[stage.min(BREAK_STAGES - 1)]
    }

    /// Where the animated run begins, for the renderer to hand to the
    /// shader. See `EXTRA_FLAME`.
    pub fn flame_layer(&self) -> u32 {
        self.extra_layers
            .get(EXTRA_FLAME)
            .copied()
            .unwrap_or(0)
    }

    /// The first of the six pictures of the fire on a torch.
    ///
    /// Its own run of layers rather than the hearth's: see
    /// `EXTRA_TORCH_FLAME`, and `generate_torch_flame` for the
    /// measurement that made two drawings necessary.
    pub fn torch_flame_layer(&self) -> u32 {
        self.extra_layers
            .get(EXTRA_TORCH_FLAME)
            .copied()
            .unwrap_or_else(|| self.flame_layer())
    }

    /// A sendable copy of the face lookup, for the mesher threads.
    pub fn face_layers(&self) -> FaceLayers {
        FaceLayers {
            layers: self.face_layers.clone().into(),
            max_block_id: self.max_block_id,
            extra: self.extra_layers.clone().into(),
            animals: self.animal_layers.clone().into(),
            reliefs: self.reliefs.clone(),
            hung: self.hung.clone(),
            mossy: self.mossy_layers.clone().into(),
        }
    }

    /// Array layer for one face of one block.
    ///
    /// By *kind*, like the meshing copy of this table (`FaceLayers`):
    /// an id carries how the block lies and how deep it is as well as
    /// what it is, and this table has one entry per material. Without
    /// the strip, a layer of snow or a sideways log falls off the end
    /// of the table and comes back as layer 0 -- which is not an error
    /// anyone sees, it is grass drawn on a snowdrift.
    #[inline]
    pub fn layer_for_face(&self, block_id: BlockId, face: usize) -> u32 {
        // **Fallen leaves wear their own wood's picture** on the ground as
        // well as in the pack: a birch wood's floor is pale and a fir's is
        // rust (`wood_icon`, and `types::carries_wood` for why the wood is
        // in the id at all).
        if let Some(index) = wood_icon(block_id).filter(|_| {
            matches!(
                primitive_shared::types::block_kind(block_id),
                primitive_shared::types::BLOCK_LEAF_LITTER | primitive_shared::types::BLOCK_LEAF_HANDFUL
            )
        }) {
            return self.extra_layers.get(index).copied().unwrap_or(0);
        }
        let kind = primitive_shared::types::block_kind(block_id);
        if kind > self.max_block_id || face >= FACES {
            return 0;
        }
        self.face_layers[kind as usize * SLOTS + face]
    }

    /// The picture for this block *in the pack*, if it has one of its
    /// own. The same answer `FaceLayers::layer_for_item` gives, read
    /// straight from this table -- the UI asks per icon per frame, and
    /// going through `face_layers()` for it cloned the whole table
    /// every time.
    #[inline]
    pub fn layer_for_item(&self, block_id: BlockId) -> Option<u32> {
        if let Some(index) = wood_icon(block_id) {
            return self.extra_layers.get(index).copied();
        }
        let kind = primitive_shared::types::block_kind(block_id);
        if kind > self.max_block_id {
            return None;
        }
        match self.face_layers[kind as usize * SLOTS + ITEM_SLOT] {
            0 => None,
            layer => Some(layer),
        }
    }
}


/// The cloud field, as a texture the sky shader samples.
///
/// **Not a layer of the block array**, which is where the font and the
/// crack stages live. Every layer of an array is one size, and the block
/// array is whatever `resolution` says -- sixteen texels, in the stock
/// pack. A sixteen-texel cloud field is four clouds. So the sky gets a
/// texture of its own at a size of its own, and that is the whole reason
/// for a second binding.
pub struct CloudTexture {
    pub view: wgpu::TextureView,
    /// Linear and repeating. Repeating because the field is tiled across
    /// the sky and has to wrap; linear because it is magnified
    /// enormously -- one texel covers several blocks of the world -- and
    /// the shader does its own snapping to the cloud grid on top.
    pub sampler: wgpu::Sampler,
}

const CLOUD_FILE: &str = "effects/sky_clouds.png";

/// Must match `CLOUD_RESOLUTION` in the texture generator: the file is
/// resized to this on load, so a mismatch silently blurs the field
/// rather than failing.
const CLOUD_RESOLUTION: u32 = 512;

impl CloudTexture {
    fn load(device: &wgpu::Device, queue: &wgpu::Queue, textures_dir: &Path) -> Self {
        let img = load_or_placeholder(
            &textures_dir.join(CLOUD_FILE),
            CLOUD_FILE,
            CLOUD_RESOLUTION,
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cloud field"),
            size: wgpu::Extent3d {
                width: CLOUD_RESOLUTION,
                height: CLOUD_RESOLUTION,
                depth_or_array_layers: 1,
            },
            // No mips: this is magnified everywhere it is used, so a mip
            // chain would be built and never read.
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // **Not sRGB.** The block textures are colours and want the
            // curve; this is three numbers a threshold is applied to,
            // and putting them through a gamma ramp would move every
            // cut in `fs_sky` somewhere the generator did not intend.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            img.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * CLOUD_RESOLUTION),
                rows_per_image: Some(CLOUD_RESOLUTION),
            },
            wgpu::Extent3d {
                width: CLOUD_RESOLUTION,
                height: CLOUD_RESOLUTION,
                depth_or_array_layers: 1,
            },
        );

        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("cloud sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            }),
        }
    }

    /// The layout both sky pipelines bind at group 1.
    pub fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cloud layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    pub fn bind_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cloud field"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

/// Loads the window icon from `textures/workbench_side.png`.
///
/// Upscaled with nearest-neighbour rather than handed over at 16x16.
/// Windows scales an undersized icon itself, smoothly, and a smoothly
/// scaled 16x16 pixel-art tile is a brown smudge; multiplying it up
/// first keeps the pixels square in the taskbar.
///
/// Returns `None` on any failure. A missing icon is a cosmetic loss and
/// must not be a reason the game won't start.
/// Where the cutout stands, as the shader compares it: `ALPHA_CUTOFF`
/// is 0.5 and alpha arrives as a unorm, so 128 is the first byte that
/// survives.
const CUTOUT_BYTE: u8 = 128;

/// What share of this picture a cutout keeps -- and `None` when the
/// picture is not a cutout picture at all.
///
/// **Why the question is asked of the alpha rather than of the block.**
/// The texture array does not know which layer is a leaf: it is a list
/// of pictures, and which of them `fs_cutout` will draw is decided by
/// the mesher, three files away. What it *can* see is the shape of the
/// alpha, and a mask is unmistakable -- every texel either fully there
/// or fully absent, which is what pixel art drawn against transparency
/// looks like and what a threshold at a half is meaningful against.
///
/// That test excludes exactly the pictures it must. The crack sheets
/// (`fs_crack`) and the rain and snow sprites carry *soft* alpha,
/// because for them alpha is a blend weight and not a mask -- stretching
/// it would darken a distant crack rather than preserve anything. They
/// come back `None` here on their own, without a list of filenames that
/// somebody would have to remember to add to.
///
/// A fully opaque picture -- every block texture in the game -- is
/// `None` as well: there is no coverage to preserve, and the whole of
/// this costs it nothing.
fn mask_coverage(image: &RgbaImage) -> Option<f32> {
    let mut kept = 0usize;
    for pixel in image.pixels() {
        let a = pixel.0[3];
        if a != 0 && a != 255 {
            return None;
        }
        if a >= CUTOUT_BYTE {
            kept += 1;
        }
    }
    let total = image.pixels().len();
    if total == 0 || kept == 0 || kept == total {
        return None;
    }
    Some(kept as f32 / total as f32)
}

/// A mip level with its alpha stretched so that the cutout keeps the
/// same share of it as the full-size picture did.
///
/// **The bug this is the whole of.** A mip is a box average, so a leaf
/// texel and the gap beside it average to a half-transparent texel --
/// and then a threshold at 0.5 has to call it one thing or the other.
/// Which way it falls is not neutral, and it is not even the same way
/// for two different pictures. Measured on this game's own art, by the
/// share of texels a cutout keeps at each level:
///
/// ```text
/// plants/leaves.png        0.742 -> 0.984 -> 1.000 -> 1.000
/// plants/grass_mesh.png    0.207 -> 0.219 -> 0.250 -> 0.000
/// ```
///
/// A canopy that is three-quarters leaf fills in solid, because most of
/// its averages land above the half; a meadow that is a fifth blade
/// disappears, because most of its averages land below. Both are the
/// same arithmetic, and between the two levels the fragments that sit
/// near the threshold flip according to where the camera happens to be
/// standing -- which is the grain that flickers when the player turns.
///
/// **The fix is the alpha, not the threshold.** Scaling this level's
/// alpha until the same share of it survives the cutoff costs nothing
/// at all in the frame: it happens once, at load, on pictures that are
/// at most a hundred and twenty-eight texels square. Three alternatives
/// were rejected. Lowering `ALPHA_CUTOFF` fixes the meadow and makes
/// the canopy worse, because the two failures are in opposite
/// directions. Alpha-to-coverage needs multisampling, which this
/// renderer does not have and would be a large price for foliage.
/// Hashed or dithered cutout replaces one grain with another and is
/// paid per pixel, on a phone, every frame.
///
/// **The scale is chosen exactly rather than searched for.** The usual
/// recipe bisects for a multiplier; but the achievable coverages are a
/// short, known list -- sort the alphas, and keeping the `k` largest is
/// what every threshold between two of them does -- so the right `k` is
/// picked by looking, and the multiplier follows from the smallest
/// alpha that has to survive. That is exact where a bisection is only
/// close, it needs no iteration count, and at the bottom of the chain,
/// where a sprite is four texels and the coverages are 0, 1/4, 1/2, 3/4
/// and 1, "closest" is a real decision rather than a rounding error:
/// rounding up would turn every distant tuft into a solid block, and
/// rounding down is how the meadow vanished in the first place.
fn held_to_coverage(level: &RgbaImage, wanted: f32) -> RgbaImage {
    let mut alphas: Vec<u8> = level.pixels().map(|p| p.0[3]).collect();
    let total = alphas.len();
    if total == 0 {
        return level.clone();
    }
    alphas.sort_unstable_by(|a, b| b.cmp(a));

    // How many texels to keep: the count whose share is nearest the one
    // the full-size picture had.
    let keep = (0..=total)
        .min_by(|a, b| {
            let error = |k: &usize| (*k as f32 / total as f32 - wanted).abs();
            error(a).total_cmp(&error(b))
        })
        .unwrap_or(0);

    let mut out = level.clone();
    if keep == 0 {
        for pixel in out.pixels_mut() {
            pixel.0[3] = 0;
        }
        return out;
    }
    // The dimmest texel that has to survive -- and the two ways of
    // treating the ones exactly as dim as it.
    //
    // A threshold cannot separate equals, and a box-filtered mip is full
    // of equals: at eight texels square a picture of wheat has a handful
    // of distinct alphas and a dozen texels wearing each. So `keep` is
    // usually not achievable, and the two counts that are -- everything
    // at least as opaque as the dimmest survivor, or everything strictly
    // more opaque than it -- can sit either side of it. Taking the first
    // without looking is what makes this go *wrong*: on a picture whose
    // values are that lumpy it can keep twice what was asked for, which
    // is worse than the plain mip it was supposed to improve on.
    //
    // Both are looked at, and the nearer wins. That is also what makes
    // the guarantee in `a_mip_of_a_cutout_keeps_as_much_of_it_as_the_
    // picture_had` true: the plain mip's own coverage is a threshold
    // too, so choosing the nearest achievable one can never land further
    // from the picture than doing nothing would.
    let dimmest = alphas[keep - 1];
    let with_equals = alphas.iter().filter(|a| **a >= dimmest).count();
    let without_equals = alphas.iter().filter(|a| **a > dimmest).count();
    let share = |count: usize| (count as f32 / total as f32 - wanted).abs();
    let floor = if share(without_equals) < share(with_equals) {
        dimmest.saturating_add(1)
    } else {
        dimmest
    }
    .max(1);

    // **Where the cutoff is put, and why it is not put on `floor`.**
    //
    // The obvious ending here is one multiply -- scale every alpha by
    // `CUTOUT_BYTE / floor`, so the dimmest survivor lands exactly on
    // the cutoff. That is correct arithmetic for a *point* sample and
    // it was ruinous on screen, because nothing in this renderer point
    // samples a mip. It put every surviving texel of every level below
    // the first at alpha 128, which is 0.502 against a cutoff of 0.5:
    // a margin of two parts in a thousand. The card then filters --
    // bilinearly between texels, trilinearly between levels, and up to
    // sixteen anisotropic taps across the footprint -- and any of those
    // blends drags a 0.502 under the half the moment a hole is anywhere
    // near it. The canopy did not thin evenly; it broke into a sparse
    // dither of the few pixels that happened to land dead centre of a
    // texel, and a tree fifty blocks off read as a dark scribble with
    // the trunk showing through it. Measured on the shipped art, the
    // whole chain came out `[0, 128]` at level 1 and `[42, 85, 128,
    // 171]` at level 2 -- survivors sitting *on* the knife.
    //
    // So the split is put in the **middle of the gap** the threshold
    // has to fall in: between the brightest alpha that must be
    // discarded and the dimmest that must survive. Everything below it
    // is stretched into `0..CUTOUT_BYTE` and everything above into
    // `CUTOUT_BYTE..255`, which is monotone -- so the set of texels
    // that survive a point sample is exactly the one chosen above, and
    // the coverage guarantee this function exists for is untouched --
    // while a survivor now sits as far above the cutoff as the data
    // allows. On a level that already has the wanted coverage the two
    // ends of the gap are 0 and 255, the split is the midpoint, and the
    // remap is the identity: a picture that was right is left alone,
    // where the multiply used to halve it for nothing.
    let brightest_rejected = alphas.iter().copied().filter(|a| *a < floor).max().unwrap_or(0);
    let dimmest_kept = alphas.iter().copied().filter(|a| *a >= floor).min().unwrap_or(255);
    // Half-integers matter: when the two ends are adjacent bytes there
    // is no whole number between them, and rounding the split to one of
    // them would move a texel across the cutoff and change the
    // coverage.
    let split = (f32::from(brightest_rejected) + f32::from(dimmest_kept)) / 2.0;
    let split = split.clamp(0.5, 254.5);
    let cut = f32::from(CUTOUT_BYTE);
    for pixel in out.pixels_mut() {
        let a = f32::from(pixel.0[3]);
        // Clamped to its own side of the cutoff after rounding, and
        // that is not belt and braces: when the gap is one byte wide
        // the split is a half-integer, and a rejected texel one below
        // it rounds straight back up onto the cutoff -- which would
        // hand the coverage guarantee back a texel it had decided
        // against.
        let mapped = if a >= split {
            (cut + (a - split) * (255.0 - cut) / (255.0 - split)).max(cut)
        } else {
            (a * cut / split).min(cut - 1.0)
        };
        pixel.0[3] = mapped.round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Spreads the colour of the visible texels into the invisible ones.
///
/// **The bug this is the whole of: a canopy that goes black at forty
/// blocks.** A leaf picture is drawn on transparency, and what a paint
/// program leaves under a transparent texel is `#000000`. Nothing ever
/// draws that texel -- `fs_cutout` discards it -- but plenty of things
/// *average* it. `downsample` below is a box filter over all four
/// channels, so one leaf texel next to one hole makes a mip texel that
/// is half black; the linear and anisotropic filters on the card do the
/// same thing between texels at run time. Measured on this game's own
/// art, by the mean luminance of the texels a cutout keeps, against the
/// full-size picture:
///
/// ```text
/// plants/leaves.png         1.00 -> 1.00 -> 0.67 -> 0.51 -> 0.49
/// plants/birch_leaves.png   1.00 -> 1.00 -> 0.77 -> 0.48 -> 0.45
/// ```
///
/// A quarter of a leaf texture is holes, so a distant crown loses
/// **half its brightness** to colour that was never meant to be seen --
/// which is the "dark cubic lumps with a few bright specks" a player
/// photographed, and why the same trees look right when you stand under
/// them (up close `crisp_uv` snaps the sample to a texel centre and no
/// blend happens at all).
///
/// This is one of the two things that were darkening it. The other is
/// in `downsample`, which averaged the sRGB *bytes* rather than the
/// light they stand for; the numbers above are with that still in
/// place. Fixing only this one takes the oak's chain to
/// `1.00 -> 1.00 -> 0.94 -> 0.85 -> 0.81`, and fixing both to
/// `1.00 -> 1.00 -> 1.01 -> 0.96 -> 0.95`.
///
/// The fix is to give the invisible texels the colour of their visible
/// neighbours, so that every average -- mip, bilinear, anisotropic --
/// is an average of leaf colours and nothing else. Alpha is not
/// touched, so the silhouette, `mask_coverage` and `held_to_coverage`
/// all see exactly the picture they saw before.
///
/// **Two alternatives were rejected.** Weighting `downsample` by alpha
/// fixes the mip chain and nothing else: the card's own filters are not
/// alpha-weighted, and they are what smears the black across a face at
/// the distances this is worst at. Storing the atlas premultiplied
/// would fix both, but every path that samples it -- the terrain, the
/// items, the crack sheet, the tinted garments -- would have to be
/// changed to unpremultiply, and `fs_cutout` compares alpha against a
/// half, which premultiplication makes a different question.
///
/// Ring by ring outwards rather than a true nearest-neighbour search:
/// the pictures are at most 128 texels square, an eight-neighbour
/// average is smoother than the nearest single texel, and the only
/// thing that reads the result is a filter kernel.
fn bleed_into_transparency(image: &RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    let mut out = image.clone();
    // A texel is empty only when it is *fully* transparent. Anything
    // with a scrap of alpha is drawn somewhere -- the crack sheets and
    // the rain sprites are made of such texels -- and its own colour is
    // already the right answer.
    let mut filled: Vec<bool> = image.pixels().map(|p| p.0[3] > 0).collect();
    // Every ordinary block texture is opaque and leaves here at once,
    // which is why this costs the load nothing. A picture with no
    // visible texel at all has nothing to spread and leaves too.
    if filled.iter().all(|f| *f) || !filled.iter().any(|f| *f) {
        return out;
    }
    // Bounded by the picture's own size: each pass fills at least the
    // border of what is filled already, so a picture cannot need more
    // passes than it is wide.
    for _ in 0..width.max(height) {
        let mut next = filled.clone();
        let mut changed = false;
        for y in 0..height {
            for x in 0..width {
                if filled[(y * width + x) as usize] {
                    continue;
                }
                let mut sum = [0u32; 3];
                let mut taken = 0u32;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                            continue;
                        }
                        // Read against the *previous* pass, so what a
                        // texel is filled from does not depend on which
                        // order this loop happens to visit its
                        // neighbours in.
                        if !filled[(ny as u32 * width + nx as u32) as usize] {
                            continue;
                        }
                        let p = out.get_pixel(nx as u32, ny as u32).0;
                        for (channel, value) in sum.iter_mut().zip(&p[..3]) {
                            *channel += *value as u32;
                        }
                        taken += 1;
                    }
                }
                if taken == 0 {
                    continue;
                }
                let pixel = out.get_pixel_mut(x, y);
                for (channel, total) in pixel.0.iter_mut().zip(sum) {
                    *channel = (total / taken) as u8;
                }
                next[(y * width + x) as usize] = true;
                changed = true;
            }
        }
        filled = next;
        if !changed {
            break;
        }
    }
    out
}

/// One channel of the array's own transfer curve, and its inverse.
///
/// The array is `Rgba8UnormSrgb`: the bytes in it are sRGB and the card
/// undoes the curve on every fetch, so a byte of 128 is not half the
/// light of a byte of 255 -- it is about a fifth of it.
fn srgb_to_linear(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let c = value.clamp(0.0, 1.0);
    let encoded = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Halves an image with a box filter.
///
/// A box filter rather than anything cleverer because the source is
/// 16x16 pixel art: at that size a wider kernel is mostly reaching
/// outside the texel it is meant to be averaging.
///
/// **The colours are averaged as light, not as bytes.** This used to
/// take the mean of the four bytes, which is the mean of an sRGB
/// encoding -- and averaging an encoding is not averaging what it
/// encodes. A black texel beside a white one came out at 128, which the
/// card reads back as a fifth of the light rather than a half, so every
/// picture in the game got darker as it receded, in proportion to how
/// much contrast it had. A leaf canopy is the most contrasty thing
/// there is -- bright leaf against dark outline -- and it lost about a
/// fifth of its light on the way down the chain, on top of everything
/// the holes were already costing it (see `bleed_into_transparency`).
///
/// Alpha stays a plain average: it is coverage, not light, and the
/// format does not put a curve on it. Which also means
/// `held_to_coverage` and `mask_coverage` see exactly what they saw
/// before.
fn downsample(source: &RgbaImage, size: u32) -> RgbaImage {
    let mut out = RgbaImage::new(size, size);
    let (sw, sh) = (source.width(), source.height());
    for y in 0..size {
        for x in 0..size {
            let mut light = [0.0f32; 3];
            let mut alpha = 0u32;
            let mut taken = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                    if sx >= sw || sy >= sh {
                        continue;
                    }
                    let p = source.get_pixel(sx, sy).0;
                    for (c, channel) in light.iter_mut().enumerate() {
                        *channel += srgb_to_linear(p[c]);
                    }
                    alpha += p[3] as u32;
                    taken += 1;
                }
            }
            let taken = taken.max(1);
            out.put_pixel(
                x,
                y,
                image::Rgba([
                    linear_to_srgb(light[0] / taken as f32),
                    linear_to_srgb(light[1] / taken as f32),
                    linear_to_srgb(light[2] / taken as f32),
                    (alpha / taken) as u8,
                ]),
            );
        }
    }
    out
}

/// The block sampler for a given anisotropy setting.
///
/// wgpu will only accept anisotropy above 1 when **every** filter mode
/// is linear -- magnification included. That is a problem for a game
/// made of 16x16 pixel art, where a linear magnification filter turns
/// every block you stand next to into a smear.
///
/// The way out is not to fight the sampler but to fix the coordinate:
/// the terrain shader snaps UVs to texel centres with a sub-texel ramp
/// (see `crisp_uv` in shader.wgsl), so a linear sampler reproduces
/// nearest-neighbour under magnification while still filtering, mipping
/// and anisotropically sampling everything in the distance -- which is
/// where the shimmer this setting exists to remove actually lives.
///
/// See `BLOCK_ADDRESS_MODE` for the other half of the settlement, which
/// is what a coordinate outside 0..1 means.
pub fn build_sampler(device: &wgpu::Device, anisotropy: u16) -> wgpu::Sampler {
    let anisotropy = anisotropy.clamp(1, 16);
    let filtered = anisotropy > 1;
    let mode = if filtered {
        wgpu::FilterMode::Linear
    } else {
        wgpu::FilterMode::Nearest
    };
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("block texture sampler"),
        address_mode_u: BLOCK_ADDRESS_MODE,
        address_mode_v: BLOCK_ADDRESS_MODE,
        address_mode_w: BLOCK_ADDRESS_MODE,
        mag_filter: mode,
        min_filter: mode,
        mipmap_filter: mode,
        anisotropy_clamp: anisotropy,
        ..Default::default()
    })
}

/// What the block sampler does with a texture coordinate outside 0..1.
///
/// **`Repeat`, and merging coplanar faces is why.** A merged rectangle
/// sixteen blocks wide is one quad whose coordinate runs 0..16, and the
/// only thing that turns that into sixteen tiles of the picture is the
/// address mode. With `ClampToEdge` it was one tile followed by fifteen
/// blocks of the picture's last texel column stretched out sideways --
/// which is what a player photographed as dark bands running across a
/// meadow, and what `gpu_plains_standing` in the offscreen repro shows.
///
/// **This used to be `ClampToEdge`, for a reason that is now void.** The
/// worry was that a coordinate nudged a hair past 1.0 would wrap to the
/// far edge of the layer and leave a bright seam along every block face.
/// It cannot: `crisp_uv` pulls a magnified sample to the *centre* of its
/// texel and only lets it ramp across a boundary over one screen pixel,
/// so the outermost fragment of a face lands half a texel inside the
/// picture rather than on its edge. Under minification the wrap happens
/// inside a mip level whose texels are smaller than a pixel, where for
/// terrain it is not merely harmless but correct -- the block next door
/// wears the continuation of the same tiling image.
///
/// The alternative, keeping the clamp and wrapping in the shader with
/// `fract`, was rejected on the measurement in `sample_block`: a
/// wrapped coordinate has a discontinuous derivative at every cell
/// boundary, so the mip selector reads a seam as "this fragment covers
/// the whole texture" unless every fragment in the world is switched to
/// the explicit-gradient fetch -- which that comment records as about
/// two thirds of the frame.
pub const BLOCK_ADDRESS_MODE: wgpu::AddressMode = wgpu::AddressMode::Repeat;

/// The sampler the flat UI uses, which is always nearest.
///
/// The hotbar and the menus draw the same texture array, but as 2D art
/// at a fixed size: there is no distance for filtering to help with, and
/// a linear filter there just makes the font fuzzy. Giving the UI its
/// own sampler is what lets the world have anisotropy without the text
/// paying for it.
pub fn build_ui_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("ui texture sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

pub fn load_window_icon(assets_dir: &Path) -> Option<(Vec<u8>, u32, u32)> {
    const SCALE: u32 = 4;

    let path = assets_dir.join("textures").join(ICON_FILE);
    let img = match image::open(&path) {
        Ok(img) => img.to_rgba8(),
        // Not an error: with no assets folder the built-in copy is the
        // expected source, and a missing icon is cosmetic either way.
        Err(_) => {
            let bytes = crate::embedded::texture(ICON_FILE)?;
            match image::load_from_memory(bytes) {
            Ok(img) => img.to_rgba8(),
            Err(e) => {
                eprintln!("the built-in window icon failed to decode ({e})");
                return None;
            }
        }
        },
    };

    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let (out_w, out_h) = (w * SCALE, h * SCALE);
    let mut pixels = Vec::with_capacity((out_w * out_h * 4) as usize);
    for y in 0..out_h {
        for x in 0..out_w {
            let px = img.get_pixel(x / SCALE, y / SCALE);
            pixels.extend_from_slice(&px.0);
        }
    }
    Some((pixels, out_w, out_h))
}

/// The texture that doubles as the application icon.
pub const ICON_FILE: &str = "terrain/workbench_side.png";

/// Writes the game's icon out as a square PNG of the given size.
///
/// For the Android packaging script; see `--export-icon`. Scaled
/// nearest-neighbour, so a sixteen-pixel picture becomes a
/// hundred-and-ninety-two-pixel picture *of sixteen pixels* rather than
/// a blur of them -- which is what the game looks like and therefore
/// what its icon should look like.
pub fn export_icon(path: &Path, size: u32) -> anyhow::Result<()> {
    let bytes = crate::embedded::texture(ICON_FILE)
        .ok_or_else(|| anyhow::anyhow!("the built-in icon {ICON_FILE} is missing"))?;
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let scaled = image::imageops::resize(
        &img,
        size.max(1),
        size.max(1),
        image::imageops::FilterType::Nearest,
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    scaled.save(path)?;
    Ok(())
}

/// One texture: the file on disk if there is one, else the copy built
/// into the binary, else the placeholder.
/// Loads an animal's sheet at whatever size it is drawn at.
///
/// **Not resized on the way in**, unlike everything else: a sheet is a
/// grid of tiles, and squashing the whole grid to one tile's worth of
/// pixels is how a texture pack at a higher resolution would come back
/// as a smear. It is cut up first and each tile resized after -- see
/// `sheet_tiles`.
fn load_sheet(path: &Path, filename: &str) -> RgbaImage {
    if path.is_file() {
        match image::open(path) {
            Ok(img) => return img.to_rgba8(),
            Err(e) => eprintln!(
                "warning: failed to load {} ({e}); falling back",
                path.display()
            ),
        }
    }
    if let Some(bytes) = crate::embedded::texture(filename) {
        match image::load_from_memory(bytes) {
            Ok(img) => return img.to_rgba8(),
            Err(e) => eprintln!("warning: the built-in {filename} failed to decode ({e})"),
        }
    }
    eprintln!("warning: no sheet called {filename}; using placeholder");
    placeholder_texture(16)
}

/// Cuts a sheet into its tiles, each one scaled to the array's
/// resolution.
///
/// The tile size comes from the *picture*, not from a constant: a pack
/// that draws its animals at 32 pixels a tile hands over a 128x96 sheet
/// and gets tiles of 32, which are then resized like every other image.
/// A sheet that is not a whole number of tiles across is used as far as
/// it goes and the rest comes back blank, which reads on screen as the
/// animal wearing its hide -- the same as any other missing picture.
fn sheet_tiles(sheet: &RgbaImage, resolution: u32) -> Vec<RgbaImage> {
    let tile_w = (sheet.width() / SHEET_COLUMNS).max(1);
    let tile_h = (sheet.height() / SHEET_ROWS).max(1);
    (0..SHEET_SLOTS)
        .map(|slot| {
            let column = slot as u32 % SHEET_COLUMNS;
            let row = slot as u32 / SHEET_COLUMNS;
            let (x, y) = (column * tile_w, row * tile_h);
            if x + tile_w > sheet.width() || y + tile_h > sheet.height() {
                return RgbaImage::new(resolution, resolution);
            }
            let tile = image::imageops::crop_imm(sheet, x, y, tile_w, tile_h).to_image();
            if tile.width() == resolution && tile.height() == resolution {
                tile
            } else {
                image::imageops::resize(
                    &tile,
                    resolution,
                    resolution,
                    image::imageops::FilterType::Nearest,
                )
            }
        })
        .collect()
}

/// Is this tile empty -- that is, has the artist left it alone?
///
/// Fully transparent everywhere. Not "mostly": a single opaque texel is
/// a decision somebody made, and guessing that they did not mean it is
/// how a pack loses a detail with no message anywhere.
fn is_blank(tile: &RgbaImage) -> bool {
    tile.pixels().all(|p| p.0[3] == 0)
}

fn load_or_placeholder(path: &Path, filename: &str, resolution: u32) -> RgbaImage {
    if path.is_file() {
        match load_image(path, resolution) {
            Ok(img) => return img,
            // A file that exists but will not decode is worth
            // complaining about; one that simply is not there is the
            // normal case for a single-file install.
            Err(e) => eprintln!(
                "warning: failed to load {} ({e}); falling back",
                path.display()
            ),
        }
    }

    if let Some(bytes) = crate::embedded::texture(filename) {
        match decode(bytes, resolution) {
            Ok(img) => return img,
            Err(e) => eprintln!("warning: the built-in {filename} failed to decode ({e})"),
        }
    }

    // A rock's pebble is made, not drawn: see `pebble_art`.
    if let Some(img) = crate::engine::pebble_art::generate(filename, resolution) {
        return img;
    }

    eprintln!("warning: no texture called {filename}; using placeholder");
    placeholder_texture(resolution)
}

/// Decodes an image already in memory and resizes it if it isn't the
/// configured resolution.
fn decode(bytes: &[u8], resolution: u32) -> anyhow::Result<RgbaImage> {
    let img = image::load_from_memory(bytes)?;
    Ok(resize_to(img, resolution))
}

fn load_image(path: &Path, resolution: u32) -> anyhow::Result<RgbaImage> {
    Ok(resize_to(image::open(path)?, resolution))
}

fn resize_to(img: image::DynamicImage, resolution: u32) -> RgbaImage {
    let (width, height) = img.dimensions();
    if (width, height) == (resolution, resolution) {
        return img.to_rgba8();
    }
    // **A strip is scaled once and tiled, never stretched.** A picture
    // wider than it is tall is drawn for a part-height block -- the
    // campfire's side is 16x4 because the block is four sixteenths tall
    // -- and stretching it to the square layer multiplies every drawn
    // row into eight, which destroys the row-for-sixteenth
    // correspondence the mesher's side crop depends on (see
    // `mesh::build_mesh`). Scaled uniformly by width and repeated down
    // the layer instead: the crop shows exactly the drawn strip, and
    // anything that samples below it -- a mip, a face nobody meant to
    // see -- finds the same strip again rather than garbage.
    if width > height {
        let strip_height = (height * resolution / width.max(1)).max(1);
        let strip = image::imageops::resize(
            &img.to_rgba8(),
            resolution,
            strip_height,
            FilterType::Nearest,
        );
        let mut out = RgbaImage::new(resolution, resolution);
        for y in 0..resolution {
            for x in 0..resolution {
                out.put_pixel(x, y, *strip.get_pixel(x, y % strip_height));
            }
        }
        return out;
    }
    img.resize_exact(resolution, resolution, FilterType::Nearest)
        .to_rgba8()
}

/// Classic magenta/black checkerboard "missing texture" placeholder.
fn placeholder_texture(resolution: u32) -> RgbaImage {
    let mut img = RgbaImage::new(resolution, resolution);
    let half = (resolution / 2).max(1);
    for y in 0..resolution {
        for x in 0..resolution {
            let checker = (x / half + y / half).is_multiple_of(2);
            let color = if checker {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
            img.put_pixel(x, y, image::Rgba(color));
        }
    }
    img
}

/// Where to look for `assets/` when nothing is configured: an `assets`
/// folder next to the executable (a packaged build), else the
/// workspace-relative path baked in at compile time (for `cargo run`).
pub fn resolve_assets_dir(configured: &str) -> PathBuf {
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("assets");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The body of each top-level `fn` in the terrain shader, by name.
    fn shader_functions() -> Vec<(String, String)> {
        include_str!("shader.wgsl")
            .replace("\r\n", "\n")
            .split("\nfn ")
            .skip(1)
            .map(|chunk| {
                let name = chunk.split('(').next().unwrap_or("").trim().to_string();
                // A function ends where the next top-level item's comment or
                // attribute begins; the closing brace at column 0 is enough.
                let body = chunk.split("\n}\n").next().unwrap_or(chunk).to_string();
                (name, body)
            })
            .collect()
    }

    /// **`texture_params.x` is a switch, not a size.** The frame hands the
    /// atlas resolution there only while filtering is on and zero with
    /// anisotropy off, which is how `sample_block` knows to take the plain
    /// nearest fetch. `sample_cutout` read it as the picture's size with a
    /// floor of one, so with anisotropy off a texel was the whole picture:
    /// the top of every plant, flame and campfire was held on its middle row
    /// and drawn as solid stripes. Anything that needs the size asks the
    /// texture (`textureDimensions`).
    #[test]
    fn only_the_snapping_reads_the_filtering_switch() {
        let readers: Vec<String> = shader_functions()
            .into_iter()
            .filter(|(_, body)| body.lines().any(|l| !l.trim_start().starts_with("//") && l.contains("texture_params.x")))
            .map(|(name, _)| name)
            .collect();
        assert_eq!(readers, vec!["sample_block".to_string()], "texture_params.x is zero with anisotropy off");
    }

    /// **A face still magnified along one texture axis is snapped.** The
    /// plain filtered fetch used to be taken as soon as *either* axis was
    /// minified, which is ground at a slant: along the view a texel is under
    /// a pixel, across it it is several, and anisotropic filtering takes its
    /// level from that short axis -- a bilinear magnification of the
    /// full-size picture, unsnapped. The middle distance went soft at
    /// anisotropy 4 and 16 and stayed hard at 1 (`aniso_repro`).
    #[test]
    fn a_texel_wider_than_a_pixel_along_either_axis_is_still_snapped() {
        let (_, body) = shader_functions()
            .into_iter()
            .find(|(name, _)| name == "sample_block")
            .expect("sample_block in shader.wgsl");
        let code: String = body.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        // The first early return is filtering off; the second is the
        // minified one, and its condition is the last `if (` before it.
        let guard = code
            .split("return plain;")
            .nth(1)
            .and_then(|between| between.rsplit("if (").next())
            .expect("sample_block leaves early for minified fragments");
        assert!(guard.contains("min("), "the early exit must need both axes minified: `if ({guard}`");
        assert!(code.contains("min(ramp, vec2<f32>(1.0))"), "a minified axis must reach crisp_uv unsqueezed");
    }

    fn parse(toml_text: &str) -> BlocksToml {
        toml::from_str(toml_text).expect("config should parse")
    }

    /// **The atlas has to fit on a phone, not only on a graphics card.**
    ///
    /// Two ceilings of 256 used to meet in this file, and sharing a
    /// number made them look like one. The block *vertex* addressed a
    /// layer in eight bits, and the *texture array itself* was capped at
    /// 256 by wgpu's default limits. 1.5 walked into the second one --
    /// `Dimension Z value 260 exceeds the limit of 256`, before a frame was
    /// drawn -- and both were lifted: the array ceiling asked of the
    /// adapter (`renderer::terrain_limits`), a ninth bit in the vertex.
    ///
    /// That was one number again, 512, and the atlas reached it at 522 --
    /// and it was never a number a GLES phone had, since GLES promises
    /// 256 layers an array and not a layer more. What it is now is the
    /// split (`AtlasSplit`): eleven bits a vertex, and an array a device
    /// cannot make deep enough made several. So the property is that the
    /// atlas can be *addressed* on the least a conforming device offers --
    /// eight arrays of 256 -- which is also what a vertex names.
    ///
    /// It counts the way `load` counts rather than trusting a number
    /// somebody wrote down.
    #[test]
    fn the_whole_atlas_fits_in_what_the_gles_minimum_can_address() {
        let config: BlocksToml =
            toml::from_str(crate::embedded::BLOCKS_TOML).expect("the built-in config parses");
        let resolution = config.resolution.clamp(1, 512);

        // Distinct *images*, which is what a layer is -- two blocks
        // naming the same file share one.
        let mut images: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for spec in config.textures.values() {
            for face in 0..FACES {
                images.extend(spec.for_face(face));
            }
            images.extend(spec.for_item());
        }

        let (columns, rows) = FontAtlas::grid(resolution);
        let glyph_layers = GLYPHS.chars().count().div_ceil((columns * rows).max(1) as usize);

        // **The animals, counted the way `load` counts them.** They were
        // once left out of this sum, and the check passed at two hundred
        // while the game was creating an array of two hundred and forty.
        // A blank tile is an alias for the hide rather than a layer.
        let animal_layers: usize = ANIMAL_SHEETS
            .iter()
            .map(|filename| {
                let sheet = load_sheet(std::path::Path::new(filename), filename);
                let tiles = sheet_tiles(&sheet, resolution);
                1 + (1..SHEET_SLOTS).filter(|&slot| !is_blank(&tiles[slot])).count()
            })
            .sum();

        // **A layer without a file still costs a layer**: the fire's
        // mirrored second sheet.
        let derived_layers = FLAME_FRAMES as usize;

        let total = (1 // the placeholder
            + images.len()
            + EXTRA_TEXTURES.len()
            + derived_layers
            + BREAK_STAGES
            + animal_layers
            + glyph_layers) as u32;

        let phone = AtlasSplit::new(total, MIN_PER_ARRAY);
        assert!(
            phone.addresses(total),
            "{total} layers: {} block images, {} extra plus {derived_layers} the fire derives \
             from them, {BREAK_STAGES} break stages, {animal_layers} of animal and \
             {glyph_layers} of font. At GLES's 256 a layer that is {} arrays, and a fragment \
             stage binds {MAX_ARRAYS}; a vertex names {}. The atlas is full.\n\n\
             The ways past it, cheapest first:\n\
             1. POINT AT AN EXISTING FILE. Identical filenames in blocks.toml are uploaded \
             once and share a layer. `stripped_log` wears `log_top.png` and cost nothing.\n\
             2. TINT A SHARED PICTURE. Draw it in greys and give the block a colour in \
             `types::garment_tint`. Twelve pieces of armour share four pictures that way.\n\
             3. PACK SEVERAL PER LAYER, the way the font already does (`FontAtlas::place`).\n\n\
             ...and past those, a twelfth bit in the vertex (bit 28 of the coordinate word \
             is spare, beside `mesh::LAYER_TOP_SHIFT`) together with more arrays than a GLES \
             fragment stage has units for -- which is the phone this split exists for.",
            images.len(),
            EXTRA_TEXTURES.len(),
            phone.arrays,
            crate::engine::mesh::MAX_TEXTURE_LAYERS,
        );
        // ...and on a desktop, where an array holds 2048, it is still one.
        assert_eq!(AtlasSplit::new(total, MAX_PER_ARRAY).arrays, 1, "{total} layers split on Vulkan");
    }

    /// **A layer comes back from its array and slot**, at every edge an
    /// array has. A slip of one here is not a crash: it is the last
    /// picture of one array drawn where the first of the next belongs --
    /// one block in the world in somebody else's texture, on phones only.
    #[test]
    fn a_layer_comes_back_from_its_array_and_slot_at_every_array_boundary() {
        let layers = crate::engine::mesh::MAX_TEXTURE_LAYERS;
        for ceiling in [MIN_PER_ARRAY, 300, 512, 1000, 1024] {
            let split = AtlasSplit::new(layers, ceiling);
            assert!(split.addresses(layers), "{ceiling}: {split:?} cannot address {layers}");
            let depths: u32 = (0..split.arrays).map(|a| split.depth(a, layers)).sum();
            assert_eq!(depths, layers, "{ceiling}: the arrays do not hold the atlas exactly");
            let mut edges = vec![0, layers - 1];
            for array in 1..split.arrays {
                let first = array * split.per_array;
                edges.extend([first - 1, first, first + 1]);
            }
            for layer in edges {
                let (array, slot) = split.locate(layer);
                assert!(array < split.arrays, "{ceiling}: layer {layer} in array {array}");
                assert!(
                    slot < split.depth(array, layers),
                    "{ceiling}: layer {layer} is slot {slot} of an array {} deep",
                    split.depth(array, layers)
                );
                assert_eq!(split.layer(array, slot), layer, "{ceiling}: layer {layer} did not come back");
            }
            assert_eq!(split.locate(split.per_array - 1), (0, split.per_array - 1));
            assert_eq!(split.locate(split.per_array), (1, 0));
        }
    }

    /// **Where one array holds the atlas, one array is what is made** --
    /// and the shaders are the text in their files, so the frame on a
    /// desktop is the frame from before the split existed.
    #[test]
    fn an_atlas_the_device_can_hold_whole_is_one_array_and_the_shaders_as_written() {
        for (layers, ceiling) in [(522, 2048), (256, 256), (1, 256), (2048, 4096)] {
            let split = AtlasSplit::new(layers, ceiling);
            assert_eq!(split.arrays, 1, "{layers} layers at {ceiling}");
            assert_eq!(split.depth(0, layers), layers);
            assert_eq!(split.locate(layers - 1), (0, layers - 1));
            assert_eq!(split.layout_entries().len(), 2, "one array still binds two entries");
            let source = include_str!("shader.wgsl");
            let specialised = split.specialise(std::borrow::Cow::Borrowed(source));
            assert!(
                matches!(specialised, std::borrow::Cow::Borrowed(s) if std::ptr::eq(s, source)),
                "one array rewrote the shader"
            );
        }
        assert_eq!(AtlasSplit::new(257, 256).arrays, 2, "one past a GLES array is two");
    }

    /// **A phone with the GLES minimum can bind every array**, next to
    /// everything else the terrain's fragment stage reads. Counted out of
    /// `shader.wgsl`, so a fourth shadow texture tomorrow fails here and
    /// not on a device nobody is holding.
    #[test]
    fn a_phone_with_the_gles_minimum_can_bind_every_array() {
        let source = include_str!("shader.wgsl");
        let others = source
            .lines()
            .filter(|line| line.trim_start().starts_with("var ") && line.contains(": texture_"))
            .filter(|line| !line.contains("block_textures"))
            .count() as u32;
        let units = wgpu::Limits::downlevel_webgl2_defaults().max_sampled_textures_per_shader_stage;
        assert!(
            MAX_ARRAYS + others <= units,
            "{MAX_ARRAYS} arrays and {others} other textures in one fragment stage, which GLES \
             only promises {units} of"
        );
    }

    /// **Every shader that reads the atlas still compiles split**, and
    /// nothing in it still samples the first array by name. A sampling site
    /// the rewrite missed compiles perfectly -- it reads `block_textures`,
    /// which exists -- and on a phone draws the first array's picture of
    /// that slot: a torch wearing a block of dirt.
    #[test]
    fn the_split_shaders_compile_and_nothing_reads_one_array_by_name() {
        use crate::engine::lighting::Quality;
        use naga::valid::{Capabilities, ValidationFlags, Validator};
        let shaders = [
            ("shader.wgsl", Quality::Simple.specialise(include_str!("shader.wgsl"))),
            ("shader.wgsl, Balanced", Quality::Balanced.specialise(include_str!("shader.wgsl"))),
            ("shader.wgsl, High", Quality::High.specialise(include_str!("shader.wgsl"))),
            ("hotbar.wgsl", std::borrow::Cow::Borrowed(include_str!("hotbar.wgsl"))),
            ("particles.wgsl", std::borrow::Cow::Borrowed(include_str!("particles.wgsl"))),
        ];
        for arrays in [2, 3, MAX_ARRAYS] {
            let split = AtlasSplit { per_array: MIN_PER_ARRAY, arrays };
            for (name, source) in &shaders {
                let split_source = split.specialise(std::borrow::Cow::Borrowed(source.as_ref()));
                let module = naga::front::wgsl::parse_str(&split_source).unwrap_or_else(|e| {
                    panic!("{name} in {arrays} arrays failed to parse:\n{}", e.emit_to_string(&split_source))
                });
                Validator::new(ValidationFlags::all(), Capabilities::all())
                    .validate(&module)
                    .unwrap_or_else(|e| panic!("{name} in {arrays} arrays failed validation: {e:?}"));
                // Everything above the appended functions has already been
                // rewritten, so rewriting it again changes nothing.
                let marker = split_source.find("// ---- the atlas in").expect("the dispatch was appended");
                let rewritten = &split_source[..marker];
                assert!(rewritten.contains("atlas_sample("), "{name}: nothing was rewritten");
                for call in ["textureSample(", "textureSampleGrad(", "textureSampleLevel(", "textureLoad("] {
                    assert_eq!(
                        rewrite_calls(rewritten, call, "missed("),
                        rewritten,
                        "{name}: a {call}block_textures, ..) was left reading the first array"
                    );
                }
            }
        }
    }

    /// **The belief that cost the atlas its last layer, tested.**
    ///
    /// For two years this repository said a GTX 1050 Ti refuses an array
    /// deeper than 256 -- from an error message that was wgpu's validation
    /// against its own default limits and not the card's answer at all.
    ///
    /// This asks: it makes every array the split asks this device for, as
    /// deep as the atlas at its fullest, one texel wide so it costs nothing.
    #[test]
    fn the_device_makes_every_array_the_split_asks_it_for() {
        let Some((device, _queue)) = crate::engine::test_gpu() else {
            println!("no GPU adapter on this machine; skipping");
            return;
        };
        let layers = crate::engine::mesh::MAX_TEXTURE_LAYERS;
        let split = AtlasSplit::new(layers, device.limits().max_texture_array_layers);
        assert!(split.addresses(layers), "{split:?} cannot address {layers} layers");
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let arrays: Vec<wgpu::Texture> = (0..split.arrays)
            .map(|array| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("an array of the fullest atlas"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: split.depth(array, layers) },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            })
            .collect();
        let refused = pollster::block_on(device.pop_error_scope());
        assert!(refused.is_none(), "this device will not make the arrays {split:?} asks for: {refused:?}");
        drop(arrays);
    }

    /// **A picture in a slot has to have something in it.**
    ///
    /// `seeds.png` was seven lit pixels of two hundred and fifty-six --
    /// one sprout, which is exactly right for a cell that has just been
    /// sown and invisible in a hand. A player carrying seeds saw an
    /// empty slot and an empty fist, reported it as "the seed cannot be
    /// seen", and nothing in the build disagreed with them: the file
    /// was there, it loaded, it drew, and what it drew was nothing.
    ///
    /// Five per cent is the bar and the faintest real icon is a flint
    /// knife head at seven, so this has room to be true without being a
    /// rule about how to draw. What it catches is the case above: a
    /// *world* picture used where a *hand* picture was needed. The fix
    /// is an `item = ` entry in `blocks.toml`, which is what `ash` and
    /// `backpack` already do and why the mechanism existed before the
    /// bug did.
    #[test]
    fn nothing_a_player_can_hold_is_drawn_as_a_handful_of_nothing() {
        let config: BlocksToml =
            toml::from_str(crate::embedded::BLOCKS_TOML).expect("the built-in config parses");
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
        let mut faint: Vec<(String, f32)> = Vec::new();
        for (name, spec) in &config.textures {
            // What the hotbar and the hand would sample: the item
            // picture if there is one, and the block's own otherwise.
            // See `FaceLayers::layer_for_item`.
            let Some(picture) = spec.for_item().or_else(|| spec.for_face(0)) else {
                continue;
            };
            let path = dir.join(picture);
            if !path.is_file() {
                continue; // a name with no file is the placeholder's business
            }
            let image = load_sheet(&path, picture);
            let lit = image.pixels().filter(|p| p.0[3] > 0).count();
            let share = lit as f32 / (image.width() * image.height()).max(1) as f32;
            if share < 0.05 {
                faint.push((format!("{name} ({picture})"), share));
            }
        }
        assert!(
            faint.is_empty(),
            "these are invisible in a slot; give them an `item = ` picture in blocks.toml: {faint:?}"
        );
    }

    /// **Every animal wears its own coat.**
    ///
    /// `sheet_index` is a hand-written map from species to sheet, and a
    /// species with no sheet of its own does not fail to draw -- it
    /// silently wears somebody else's. The fowl answered zero for five
    /// versions, which is the hare's, so every bird in the world was a
    /// small brown hare-coloured thing and a player reported that there
    /// were no birds at all. Nothing in the build disagreed with them.
    #[test]
    fn no_two_animals_share_a_coat() {
        use primitive_shared::animals::Species;
        let mut seen = std::collections::HashMap::new();
        for &species in Species::ALL {
            let index = sheet_index(species);
            assert!(
                index < ANIMAL_SHEETS.len(),
                "{} points at sheet {index} and there are {}",
                species.name(),
                ANIMAL_SHEETS.len()
            );
            if let Some(other) = seen.insert(index, species) {
                panic!(
                    "{} and {} both wear {}",
                    species.name(),
                    other.name(),
                    ANIMAL_SHEETS[index]
                );
            }
        }
    }


    #[test]
    fn the_fire_is_one_drawn_sheet_of_frames_and_one_derived_from_it() {
        // What the shader is told and what the array actually holds have
        // to be the same thing. `animated` divides a layer's distance
        // from the first frame by the frame count to find its sheet, so
        // the drawn run must be exactly `FLAME_FRAMES` long, must start
        // at `EXTRA_FLAME`, and the derived sheet must sit immediately
        // behind it -- which is why nothing else may be filed between
        // them and why `EXTRA_STRETCHED_HIDE` counts files rather than
        // layers.
        // **By prefix and not by "contains".** The torch's fire is a
        // second set of files whose names also have `flame` in them --
        // deliberately a different picture, for a reason written at
        // `EXTRA_TORCH_FLAME` -- and a substring match counted twelve
        // where it wanted six. The rule this test is about is the
        // hearth's alone.
        let flames: Vec<&&str> = EXTRA_TEXTURES
            .iter()
            .filter(|name| name.starts_with("effects/flame."))
            .collect();
        assert_eq!(
            flames.len(),
            FLAME_FRAMES as usize,
            "the fire is drawn once and mirrored in `load`; a second set of files would be
             a copy that goes stale the first time a frame is redrawn"
        );
        // The torch's own set: the same count, its own contiguous run,
        // and no derived sheet -- one billboard has no second
        // silhouette to disagree with.
        for frame in 0..FLAME_FRAMES as usize {
            assert_eq!(
                EXTRA_TEXTURES[EXTRA_TORCH_FLAME + frame],
                format!("effects/torch_flame.{frame}.png"),
                "the torch's fire is not a run of {FLAME_FRAMES} starting at
                 EXTRA_TORCH_FLAME, and the hand adds a frame number to one layer"
            );
        }
        for frame in 0..FLAME_FRAMES as usize {
            assert_eq!(
                EXTRA_TEXTURES[EXTRA_FLAME + frame],
                format!("effects/flame.{frame}.png"),
                "the frames are out of order, and out of order they play as a shuffle"
            );
        }
        assert_eq!(EXTRA_TEXTURES[EXTRA_STRETCHED_HIDE], "hide/stretched_hide.png");
        assert_eq!(EXTRA_TEXTURES[EXTRA_STRETCHED_LEATHER], "hide/stretched_leather.png");

        // ...and the arithmetic the mesher uses agrees with all of it.
        let layers = FaceLayers::numbered_for_test();
        assert_eq!(layers.flame(1), layers.flame(0) + FLAME_FRAMES);
        assert_eq!(layers.flame(2), layers.flame(0), "the sheet index wraps");
        assert_ne!(
            layers.flame(1),
            layers.stretched_hide(),
            "the derived sheet's layers are not the drying rack's: a fixture that says
             they are cannot see the collision it exists to catch"
        );
    }

    #[test]
    fn the_fire_s_second_sheet_is_the_first_one_mirrored_and_late() {
        // The rule, held to without a graphics card. A frame of the
        // second sheet is a *different frame* of the first, flipped --
        // and it has to be both: same frame flipped is two quads
        // breathing in step, and a different frame unflipped leaves the
        // two silhouettes closer than they need to be for free.
        let drawn: Vec<RgbaImage> = (0..FLAME_FRAMES)
            .map(|frame| {
                let mut image = RgbaImage::new(4, 1);
                // A row that is not its own mirror, and a value that
                // says which frame it came from.
                image.put_pixel(0, 0, image::Rgba([frame as u8 + 1, 0, 0, 255]));
                image
            })
            .collect();
        let derived = mirrored_flame_sheet(&drawn);
        assert_eq!(derived.len(), drawn.len());
        for (frame, picture) in derived.iter().enumerate() {
            let source = (frame + FLAME_SHEET_LAG as usize) % FLAME_FRAMES as usize;
            assert_eq!(
                picture.get_pixel(3, 0).0,
                [source as u8 + 1, 0, 0, 255],
                "frame {frame} of the derived sheet is neither mirrored nor {FLAME_SHEET_LAG}
                 frames late"
            );
        }
    }

    #[test]
    fn every_frame_of_the_fire_is_a_silhouette_with_air_around_it() {
        // **A fire with no transparency is a burning box.** The frames
        // are two quads crossing on a cell's diagonals, so everything
        // that is not flame has to show the world behind it -- and the
        // way this breaks is not in code: it is somebody saving a frame
        // from an editor that flattened the alpha, which looks perfectly
        // fine in a file browser.
        for frame in 0..FLAME_FRAMES as usize {
            let name = format!("effects/flame.{frame}.png");
            let bytes = crate::embedded::texture(&name).expect("the frame is in the binary");
            let picture = image::load_from_memory(bytes).expect("the frame is a picture").to_rgba8();
            let (width, height) = picture.dimensions();
            assert_eq!(width, height, "{name} is not square");
            let opaque = picture.pixels().filter(|texel| texel.0[3] > 0).count();
            let texels = (width * height) as usize;
            assert!(opaque > 0, "{name} is empty");
            assert!(
                opaque * 2 < texels,
                "{name} covers {opaque} of {texels} texels, which is a block of fire rather
                 than a fire"
            );
            // The top row especially: a flame that reaches the ceiling
            // of its own picture has been cropped, not drawn.
            assert!(
                (0..width).all(|x| picture.get_pixel(x, 0).0[3] == 0),
                "{name} touches the top of its tile"
            );
        }
    }

    #[test]
    fn a_sheet_is_cut_into_tiles_and_a_blank_one_means_no_such_part() {
        // The two rules the animal sheets rest on. A tile is cut at the
        // *picture's* own scale and then resized, so a pack drawn at 32
        // pixels a tile does not come back as a smear; and a tile left
        // alone is not a picture at all, which is what lets a hare have
        // no tusks without a line of code anywhere saying so.
        let mut sheet = RgbaImage::new(SHEET_COLUMNS * 32, SHEET_ROWS * 32);
        // Slot 0 painted, slot 1 painted, everything else left alone.
        for y in 0..32 {
            for x in 0..32 {
                sheet.put_pixel(x, y, image::Rgba([10, 20, 30, 255]));
                sheet.put_pixel(32 + x, y, image::Rgba([40, 50, 60, 255]));
            }
        }
        let tiles = sheet_tiles(&sheet, 16);
        assert_eq!(tiles.len(), SHEET_SLOTS);
        for tile in &tiles {
            assert_eq!(tile.dimensions(), (16, 16), "a tile came out the wrong size");
        }
        assert!(!is_blank(&tiles[0]));
        assert!(!is_blank(&tiles[1]));
        assert_eq!(tiles[0].get_pixel(0, 0).0, [10, 20, 30, 255], "the tiles are in the wrong order");
        assert_eq!(tiles[1].get_pixel(0, 0).0, [40, 50, 60, 255]);
        for (slot, tile) in tiles.iter().enumerate().skip(2) {
            assert!(is_blank(tile), "slot {slot} is not blank");
        }
        // One opaque texel is a decision somebody made.
        let mut nearly = RgbaImage::new(SHEET_COLUMNS * 16, SHEET_ROWS * 16);
        nearly.put_pixel(SHEET_COLUMNS * 16 - 1, 0, image::Rgba([1, 2, 3, 1]));
        assert!(!is_blank(&sheet_tiles(&nearly, 16)[SHEET_COLUMNS as usize - 1]));
    }

    #[test]
    fn every_picture_the_game_asks_for_is_actually_in_the_binary() {
        // **A missing texture is not an error anywhere.** `load_texture`
        // falls back to the magenta placeholder and prints a warning, and
        // a warning on stderr behind a running game is a warning nobody
        // reads -- so the way this fails in practice is that somebody
        // adds a picture, forgets the line in `embedded.rs`, and a player
        // reports that an animal's face is a checkerboard.
        //
        // Both lists, because they are reached differently: block faces
        // come out of `blocks.toml` by name, and everything that is not a
        // block face -- the animals, the rain, the break stages -- is
        // asked for by name from here.
        let mut missing: Vec<String> = Vec::new();
        // The animals' sheets, which are asked for by name like the rest
        // and would come back as a magenta checkerboard the same way.
        for name in ANIMAL_SHEETS {
            if crate::embedded::texture(name).is_none() {
                missing.push((*name).to_string());
            }
        }
        for name in EXTRA_TEXTURES {
            if crate::embedded::texture(name).is_none() {
                missing.push((*name).to_string());
            }
        }
        for stage in 0..BREAK_STAGES {
            let name = format!("effects/break.{stage}.png");
            if crate::embedded::texture(&name).is_none() {
                missing.push(name);
            }
        }
        let config: BlocksToml =
            toml::from_str(crate::embedded::BLOCKS_TOML).expect("the built-in config parses");
        for spec in config.textures.values() {
            let faces = (0..FACES).flat_map(|face| spec.for_face(face)).chain(spec.for_item());
            for name in faces {
                if crate::embedded::texture(name).is_none() && !crate::engine::pebble_art::is_generated(name) {
                    missing.push(name.to_string());
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "not compiled in, so the game ships a placeholder instead: {missing:?}"
        );
    }

    #[test]
    fn packing_the_font_puts_every_glyph_somewhere_of_its_own() {
        // Glyphs share a layer now, so "somewhere" is a layer *and* a
        // corner -- and two characters landing on the same corner of the
        // same layer is one of them drawn as the other.
        let atlas = FontAtlas {
            base: 7,
            u_max: 6.0 / 16.0,
            v_max: 9.0 / 16.0,
            per_layer: 2,
            columns: 2,
            stride_u: 0.5,
            stride_v: 1.0,
        };
        let mut seen = std::collections::HashSet::new();
        for c in GLYPHS.chars() {
            let (layer, u, v) = atlas.place(c);
            assert!(layer >= atlas.base, "{c:?} landed before the font");
            assert!(
                seen.insert((layer, u.to_bits(), v.to_bits())),
                "{c:?} shares a place with another character"
            );
            // ...and stays inside its layer.
            assert!(u + atlas.u_max <= 1.0 + 1e-6, "{c:?} runs off the side");
            assert!(v + atlas.v_max <= 1.0 + 1e-6, "{c:?} runs off the bottom");
        }
    }

    #[test]
    fn a_glyph_is_read_out_of_the_square_it_was_drawn_in() {
        // **The bug this is here for**, and it was invisible at the one
        // resolution the game ships with: the sheet drew glyphs on a
        // grid of `layer / columns` and the table read them off a grid
        // of `cell`, and the two agree only when the layer is sixteen
        // texels and the grid is two by one. A pack of any other size --
        // and a 32-texel pack is the ordinary next step -- came out
        // reading somebody else's letters, with no error anywhere.
        //
        // Checked by *reconstruction*: draw a sheet, then read the
        // pixels back out of the square `place` points at and compare
        // them with the font's own bitmap.
        for resolution in [16u32, 24, 32, 48] {
            let atlas = FontAtlas::for_size(resolution, 0);
            let per_layer = {
                let (columns, rows) = FontAtlas::grid(resolution);
                (columns * rows).max(1) as usize
            };
            let glyphs: Vec<char> = GLYPHS.chars().collect();
            for (index, chunk) in glyphs.chunks(per_layer).enumerate() {
                let sheet = glyph_sheet(chunk, resolution);
                for &c in chunk {
                    let (layer, u, v) = atlas.place(c);
                    assert_eq!(layer as usize, index, "{c:?} at {resolution} is on the wrong layer");
                    let (ox, oy) = (
                        (u * resolution as f32).round() as u32,
                        (v * resolution as f32).round() as u32,
                    );
                    assert!(
                        ox + crate::engine::font::GLYPH_WIDTH as u32 <= resolution
                            && oy + crate::engine::font::GLYPH_HEIGHT as u32 <= resolution,
                        "{c:?} at {resolution} is packed off the layer"
                    );
                    for (row, bits) in crate::engine::font::glyph(c).iter().enumerate() {
                        for column in 0..crate::engine::font::GLYPH_WIDTH {
                            let lit = bits & (1 << (crate::engine::font::GLYPH_WIDTH - 1 - column))
                                != 0;
                            let pixel = sheet.get_pixel(ox + column as u32, oy + row as u32);
                            assert_eq!(
                                pixel.0[3] > 0,
                                lit,
                                "{c:?} at {resolution}: the pixel at {column},{row} of the \
                                 square `place` points at is not the one the font drew"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_sheet_is_the_size_of_a_layer_whatever_is_on_it() {
        // Every image in the array has to be the same square, or the
        // upload writes the wrong number of bytes into a layer.
        for chunk in GLYPHS.chars().collect::<Vec<_>>().chunks(2) {
            let sheet = glyph_sheet(chunk, 16);
            assert_eq!(sheet.dimensions(), (16, 16));
        }
        // A pack whose tiles are smaller than a glyph still gets one
        // glyph per layer rather than a division by zero.
        assert_eq!(FontAtlas::grid(4), (1, 1));
        assert_eq!(glyph_sheet(&['A'], 4).dimensions(), (4, 4));
    }

    #[test]
    fn a_plain_string_applies_to_every_face() {
        let cfg = parse("[textures]\nstone = \"stone.png\"\n");
        let spec = &cfg.textures["stone"];
        for face in 0..FACES {
            assert_eq!(spec.for_face(face), Some("stone.png"));
        }
    }

    #[test]
    fn top_side_bottom_resolve_per_face() {
        let cfg = parse(
            "[textures]\ngrass = { top = \"grass_top.png\", side = \"grass_side.png\", bottom = \"dirt.png\" }\n",
        );
        let spec = &cfg.textures["grass"];
        assert_eq!(spec.for_face(FACE_TOP), Some("grass_top.png"));
        assert_eq!(spec.for_face(FACE_BOTTOM), Some("dirt.png"));
        for face in [FACE_NORTH, FACE_SOUTH, FACE_EAST, FACE_WEST] {
            assert_eq!(spec.for_face(face), Some("grass_side.png"));
        }
    }

    #[test]
    fn side_does_not_leak_onto_top_or_bottom() {
        // A block with only `side` set must fall through to the
        // placeholder on top/bottom, not silently reuse the side image.
        let cfg = parse("[textures]\nlog = { side = \"log_side.png\" }\n");
        let spec = &cfg.textures["log"];
        assert_eq!(spec.for_face(FACE_EAST), Some("log_side.png"));
        assert_eq!(spec.for_face(FACE_TOP), None);
        assert_eq!(spec.for_face(FACE_BOTTOM), None);
    }

    #[test]
    fn all_is_the_last_resort_and_a_named_face_beats_it() {
        let cfg = parse(
            "[textures]\nchest = { all = \"chest_side.png\", north = \"chest_front.png\" }\n",
        );
        let spec = &cfg.textures["chest"];
        assert_eq!(spec.for_face(FACE_NORTH), Some("chest_front.png"));
        assert_eq!(spec.for_face(FACE_SOUTH), Some("chest_side.png"));
        assert_eq!(spec.for_face(FACE_TOP), Some("chest_side.png"));
    }

    #[test]
    fn a_typo_in_a_face_name_is_rejected_loudly() {
        // `deny_unknown_fields` matters here: silently ignoring "tpo"
        // would ship a block textured with the placeholder and no
        // explanation of why.
        let result: Result<BlocksToml, _> =
            toml::from_str("[textures]\ngrass = { tpo = \"grass_top.png\" }\n");
        assert!(result.is_err(), "an unknown face key should be an error");
    }

    #[test]
    fn the_old_single_string_config_still_parses() {
        // Backwards compatibility: existing blocks.toml files must keep
        // working unchanged.
        let cfg = parse(
            "resolution = 16\n[textures]\ngrass = \"grass.png\"\ndirt = \"dirt.png\"\nstone = \"stone.png\"\n",
        );
        assert_eq!(cfg.resolution, 16);
        assert_eq!(cfg.textures.len(), 3);
    }

    #[test]
    fn the_placeholder_is_a_visible_checkerboard() {
        let img = placeholder_texture(16);
        assert_eq!(img.dimensions(), (16, 16));
        assert_ne!(img.get_pixel(0, 0), img.get_pixel(15, 0));
    }

    #[test]
    fn local_face_tables_are_rotations_not_reflections() {
        use primitive_shared::types::Axis;
        // Face index -> the signed unit vector it names, in the
        // mesher's order: +Y, -Y, +X, -X, +Z, -Z.
        fn vec_of(face: usize) -> [i32; 3] {
            [[0, 1, 0], [0, -1, 0], [1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]][face]
        }
        let cross = |a: [i32; 3], b: [i32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let dot = |a: [i32; 3], b: [i32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

        for axis in [Axis::Y, Axis::X, Axis::Z] {
            // The map as a matrix: where the world's +X, +Y and +Z axes
            // land in the block's own frame.
            let x = vec_of(local_face(2, axis));
            let y = vec_of(local_face(0, axis));
            let z = vec_of(local_face(4, axis));
            // A turned block is *turned*: determinant +1. A -1 here is a
            // reflection, which no rotation produces -- it means two
            // opposite faces have been swapped, and each end's texture
            // is drawn on the other end.
            assert_eq!(
                dot(x, cross(y, z)),
                1,
                "{axis:?}: the table is a reflection, not a rotation"
            );
            // ...and opposite world faces must show opposite local ones.
            for face in 0..FACES {
                assert_eq!(
                    local_face(face, axis) ^ 1,
                    local_face(face ^ 1, axis),
                    "{axis:?}: faces {face} and {} do not map to an opposite pair",
                    face ^ 1
                );
            }
        }
        // The anchors the docs promise: a block lying along an axis
        // shows its own top at the positive end of that axis.
        assert_eq!(local_face(FACE_EAST, Axis::X), FACE_TOP);
        assert_eq!(local_face(FACE_SOUTH, Axis::Z), FACE_TOP);
        // Y is the identity: an upright block is not turned at all.
        for face in 0..FACES {
            assert_eq!(local_face(face, Axis::Y), face);
        }
    }

    /// The chain of mips built the way `TextureManager::load` builds it,
    /// with the share of texels a cutout would keep at each level.
    fn coverage_chain(image: &RgbaImage, held: bool) -> Vec<f32> {
        let wanted = mask_coverage(image);
        let mut level = image.clone();
        let mut size = image.width();
        let mut out = Vec::new();
        loop {
            let shown = match wanted {
                Some(share) if held && !out.is_empty() => held_to_coverage(&level, share),
                _ => level.clone(),
            };
            let kept = shown.pixels().filter(|p| p.0[3] >= CUTOUT_BYTE).count();
            out.push(kept as f32 / shown.pixels().len() as f32);
            if size == 1 {
                return out;
            }
            size = (size / 2).max(1);
            level = downsample(&level, size);
        }
    }

    /// **A canopy must not fill in, and a meadow must not go bald.**
    ///
    /// Both are the same fault: a box-filtered mip plus a hard cutoff
    /// keeps a share of the picture that has nothing to do with the
    /// share the picture had. Measured on the game's own art before the
    /// fix, leaves went 0.742 -> 0.984 -> 1.000 and grass went
    /// 0.207 -> 0.219 -> 0.250 -> 0.000: the tree became a solid cube
    /// at a distance and the grass disappeared. The test is on the
    /// mechanism -- *the coverage stays near the coverage* -- and not on
    /// either picture, so a repainted leaf is still covered by it.
    #[test]
    fn a_mip_of_a_cutout_keeps_as_much_of_it_as_the_picture_had() {
        let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
        for name in ["plants/leaves.png", "plants/grass_mesh.png", "plants/wheat.png"] {
            let Ok(image) = image::open(assets.join(name)) else {
                // The pictures are in the repository; a checkout without
                // them is not a reason to fail the build.
                continue;
            };
            let image = image.to_rgba8();
            let base = mask_coverage(&image).expect("these three are cutout pictures");
            let before = coverage_chain(&image, false);
            let after = coverage_chain(&image, true);
            for (level, (was, now)) in before.iter().zip(&after).enumerate().skip(1) {
                // Never further from the picture than doing nothing --
                // true at every level, including the two-texel one where
                // the only coverages that exist are quarters. This is
                // the half of the property that holds everywhere, and it
                // is the half a future "simplification" of the search
                // would break.
                assert!(
                    (now - base).abs() <= (was - base).abs() + 1e-6,
                    "{name} level {level}: coverage {now:.3} is further from {base:.3}                      than the plain mip's {was:.3}"
                );
                // ...and nothing vanishes or fills in solid, which is
                // the fault itself. Not asked of the last two levels: at
                // four texels and at one, 0 and 1 are two of the five
                // coverages there are, and a sprite that small is one
                // pixel on the screen.
                if level + 2 >= before.len() {
                    continue;
                }
                assert!(
                    *now > 0.0 && *now < 1.0,
                    "{name} level {level}: coverage {now:.3} against {base:.3} -- it has gone                      {} (it was {was:.3} before the fix)",
                    if *now == 0.0 { "bald" } else { "solid" }
                );
            }
        }
    }

    /// A picture whose alpha is a blend weight is left alone.
    ///
    /// The crack sheets and the rain sprite carry soft alpha, and
    /// stretching that would be stretching a *shade* -- a distant crack
    /// drawn darker than a near one. Nothing names those files here;
    /// the shape of their alpha is what excludes them, so a new soft
    /// sprite is excluded on the day it is added.
    #[test]
    fn only_a_mask_is_held_to_its_coverage() {
        let mut soft = RgbaImage::new(2, 2);
        soft.put_pixel(0, 0, image::Rgba([255, 255, 255, 0]));
        soft.put_pixel(1, 0, image::Rgba([255, 255, 255, 90]));
        soft.put_pixel(0, 1, image::Rgba([255, 255, 255, 200]));
        soft.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        assert_eq!(mask_coverage(&soft), None, "a soft sprite is not a mask");

        let mut opaque = RgbaImage::new(2, 2);
        for pixel in opaque.pixels_mut() {
            *pixel = image::Rgba([12, 34, 56, 255]);
        }
        assert_eq!(mask_coverage(&opaque), None, "an opaque block has no coverage to keep");

        let mut mask = RgbaImage::new(2, 2);
        mask.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        mask.put_pixel(1, 0, image::Rgba([255, 255, 255, 0]));
        mask.put_pixel(0, 1, image::Rgba([255, 255, 255, 0]));
        mask.put_pixel(1, 1, image::Rgba([255, 255, 255, 0]));
        assert_eq!(mask_coverage(&mask), Some(0.25));
    }

    /// The mean brightness of the texels a cutout keeps, level by
    /// level, as a fraction of the full-size picture's own.
    ///
    /// Measured in linear light, because that is what the shader gets:
    /// the array is `Rgba8UnormSrgb` and the card undoes the transfer
    /// curve on every fetch. Averaging the bytes instead would flatter
    /// the answer by about a third.
    fn brightness_chain(image: &RgbaImage, bleed: bool) -> Vec<f32> {
        fn linear(byte: u8) -> f32 {
            let c = byte as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        // Rec. 709, the weights the eye reads a green canopy with.
        let luminance = |p: &image::Rgba<u8>| {
            0.2126 * linear(p.0[0]) + 0.7152 * linear(p.0[1]) + 0.0722 * linear(p.0[2])
        };
        let mean = |level: &RgbaImage| {
            let kept: Vec<f32> = level
                .pixels()
                .filter(|p| p.0[3] >= CUTOUT_BYTE)
                .map(luminance)
                .collect();
            if kept.is_empty() {
                0.0
            } else {
                kept.iter().sum::<f32>() / kept.len() as f32
            }
        };

        let wanted = mask_coverage(image);
        let base = mean(image);
        let mut level = if bleed {
            bleed_into_transparency(image)
        } else {
            image.clone()
        };
        let mut size = image.width();
        let mut out = Vec::new();
        loop {
            let shown = match wanted {
                Some(share) if !out.is_empty() => held_to_coverage(&level, share),
                _ => level.clone(),
            };
            out.push(mean(&shown) / base.max(f32::EPSILON));
            if size == 1 {
                return out;
            }
            size = (size / 2).max(1);
            level = downsample(&level, size);
        }
    }

    /// **A canopy at forty blocks is still made of leaves, and a
    /// meadow at forty blocks is still made of grass.**
    ///
    /// Anything drawn on transparency carries `#000000` under its
    /// transparent texels -- that is what a paint program leaves there.
    /// Nothing ever draws those texels, and everything *averages* them:
    /// the mip box filter here, and the card's own linear and
    /// anisotropic filters at run time. Measured on the game's own art
    /// before the fix, by the mean brightness of the texels the cutout
    /// keeps:
    ///
    /// ```text
    /// plants/leaves.png       1.00 -> 1.00 -> 0.67 -> 0.51 -> 0.49
    /// plants/grass_mesh.png   1.00 -> 1.00 -> 0.64 -> 0.43 -> 0.16
    /// plants/wheat.png        1.00 -> 1.00 -> 0.23 -> 0.20 -> 0.12
    /// ```
    ///
    /// A player photographed a wood of near-black cubes with a few
    /// bright specks in it, and then said the plants had it too. They
    /// are one fault: a leaf, a tuft of grass, a flower and an ear of
    /// wheat are the same alpha mask through the same `fs_cutout`.
    ///
    /// So the test is asked of **every masked picture in the pack**
    /// rather than of a list somebody has to remember to extend: a new
    /// bush, a new crop or a repainted leaf is covered by it on the day
    /// it is added. `mask_coverage` is what decides which pictures
    /// those are, and it is the same judge the loader uses.
    #[test]
    fn every_cutout_picture_keeps_its_colour_all_the_way_down_the_mip_chain() {
        let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
        // What `blocks.toml` asks for, and therefore the size every
        // layer in the array is. Read from the pack rather than
        // written down here, so a pack that moves to 64 moves the test
        // with it instead of quietly testing a size nothing uses.
        let stock_resolution = std::fs::read_to_string(assets.join("blocks.toml"))
            .ok()
            .and_then(|text| {
                text.lines()
                    .find_map(|line| line.strip_prefix("resolution =")?.trim().parse::<u32>().ok())
            })
            .unwrap_or(32);
        let mut checked = 0;
        for entry in walk_pictures(assets) {
            let Ok(image) = image::open(&entry) else {
                // The pictures are in the repository; a checkout without
                // them is not a reason to fail the build.
                continue;
            };
            // At the size the loader actually puts it in the array, so
            // the chain under test is the chain the card is handed. A
            // 16x16 picture upscaled to 32 gains a level at the top,
            // and it is the levels *below* that this is about.
            let image = resize_to(image, stock_resolution);
            if mask_coverage(&image).is_none() {
                // Opaque, or soft alpha: neither is a cutout, and
                // neither is what this is about.
                continue;
            }
            checked += 1;
            let name = entry.display().to_string();
            let chain = brightness_chain(&image, true);
            for (level, share) in chain.iter().enumerate() {
                // Only while a level is still eight texels square.
                //
                // Below that a mask is a handful of texels and the
                // cutout keeps three or four of them, so "the mean
                // colour of what survives" is a statement about which
                // three -- not about colour. It is also a level nothing
                // reads until the thing is a single pixel on the
                // screen. The coverage test stops for the same reason
                // and says so in the same words.
                if stock_resolution >> level < 8 {
                    continue;
                }
                // **Tight below, loose above, and the asymmetry is the
                // point.** The fault is a picture going *dark* -- the
                // black under the holes averaged into it -- and that is
                // what the floor catches. Drifting the other way is
                // arithmetic rather than a fault: the cutout keeps the
                // most opaque texels of a level, and on a thin shape
                // eight texels square those happen to be its brighter
                // ones. The chain is a box filter over a picture whose
                // leaves are not all one green, so it wanders either
                // way; what it must not do is walk off towards nothing.
                assert!(
                    (0.85..=1.30).contains(share),
                    "{name} level {level}: what survives the cutout is at {share:.2} of the                      colour it was painted -- the whole chain is {chain:.2?}"
                );
            }
        }
        assert!(
            checked > 5,
            "only {checked} masked pictures found -- the walk over the pack is broken, and a                  test that inspects nothing passes"
        );
    }

    /// Every `.png` under a directory, however deep.
    ///
    /// The pack is sorted into `plants/`, `metal/`, `fire/` and the
    /// rest, and a test that only looked in the top level would quietly
    /// check nothing at all.
    fn walk_pictures(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk_pictures(&path));
            } else if path.extension().is_some_and(|e| e == "png") {
                out.push(path);
            }
        }
        out.sort();
        out
    }

    /// The bleed moves colour and never alpha.
    ///
    /// Which is what makes it safe to put in front of everything else
    /// in the loader: `mask_coverage` and `held_to_coverage` both read
    /// alpha alone, and the cutout's silhouette is alpha alone, so a
    /// bleed that kept its hands off alpha cannot change the shape of
    /// anything on screen -- only what colour it is.
    #[test]
    fn bleeding_a_picture_into_its_holes_changes_no_texel_that_is_drawn() {
        let mut leaf = RgbaImage::new(4, 4);
        for (i, pixel) in leaf.pixels_mut().enumerate() {
            *pixel = if i % 3 == 0 {
                image::Rgba([0, 0, 0, 0])
            } else {
                image::Rgba([70, 140, 40, 255])
            };
        }
        let bled = bleed_into_transparency(&leaf);
        for (x, y, before) in leaf.enumerate_pixels() {
            let after = bled.get_pixel(x, y);
            assert_eq!(before.0[3], after.0[3], "the alpha at {x},{y} moved");
            if before.0[3] > 0 {
                assert_eq!(before.0[..3], after.0[..3], "a visible texel at {x},{y} moved");
            } else {
                assert_ne!(
                    after.0[..3],
                    [0, 0, 0],
                    "the hole at {x},{y} is still black, which is what the filters average in"
                );
            }
        }

        // An opaque picture -- which is nearly every block in the game
        // -- comes back untouched, so this costs the loader nothing on
        // the layers that have no holes to fill.
        let mut stone = RgbaImage::new(2, 2);
        for pixel in stone.pixels_mut() {
            *pixel = image::Rgba([90, 90, 96, 255]);
        }
        assert_eq!(bleed_into_transparency(&stone), stone);
    }

    /// The stretch touches alpha and nothing else.
    ///
    /// It would be an easy thing to write as a multiply over the whole
    /// pixel, and the result would be a canopy that gets brighter with
    /// distance -- which is the sort of fault that gets blamed on the
    /// lighting.
    #[test]
    fn holding_the_coverage_leaves_the_colour_alone() {
        let mut level = RgbaImage::new(4, 4);
        for (i, pixel) in level.pixels_mut().enumerate() {
            *pixel = image::Rgba([200, 100, 50, (i * 16) as u8]);
        }
        let held = held_to_coverage(&level, 0.5);
        for (before, after) in level.pixels().zip(held.pixels()) {
            assert_eq!(before.0[..3], after.0[..3], "the colour moved");
        }
        let kept = held.pixels().filter(|p| p.0[3] >= CUTOUT_BYTE).count();
        assert_eq!(kept, 8, "half of sixteen texels should survive the cutoff");
    }

    #[test]
    fn an_explicit_assets_dir_wins() {
        assert_eq!(
            resolve_assets_dir("/tmp/custom-assets"),
            PathBuf::from("/tmp/custom-assets")
        );
    }

    /// A tool: the whole mip chain of one plant picture, level by
    /// level, exactly as `TextureManager::load` builds it -- coverage,
    /// the mean colour of what survives the cutout, and the alphas that
    /// are left at each size.
    ///
    /// **What it is for.** When a player says a plant looks wrong at a
    /// distance, the first suspects are always in here: a mip that
    /// darkens, a coverage that fills in or goes bald, survivors
    /// sitting on the knife of `ALPHA_CUTOFF`. Printing the whole chain
    /// takes a second and rules all three in or out before anyone
    /// touches the sampler. It is how the tall-grass complaint was
    /// pinned on the picture rather than on the filtering: grass came
    /// back `1.00 -> 1.00 -> 0.99 -> 1.01`, which is a chain with
    /// nothing wrong with it.
    ///
    /// ```text
    /// cargo test -p primitive_client --lib what_a_plant_looks_like_down_its_mip_chain \
    ///     -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a tool: prints numbers"]
    fn what_a_plant_looks_like_down_its_mip_chain() {
        fn linear(byte: u8) -> f32 {
            let c = byte as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        }
        let lum = |p: &image::Rgba<u8>| {
            0.2126 * linear(p.0[0]) + 0.7152 * linear(p.0[1]) + 0.0722 * linear(p.0[2])
        };
        let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
        for name in [
            "plants/grass_mesh.png",
            "plants/leaves.png",
            "plants/flower.png",
            "plants/wheat.png",
        ] {
            let Ok(image) = image::open(assets.join(name)) else { continue };
            let image = resize_to(image, 32);
            let Some(wanted) = mask_coverage(&image) else {
                println!("{name}: not a mask");
                continue;
            };
            println!("== {name} coverage {wanted:.3}");
            let base: Vec<f32> =
                image.pixels().filter(|p| p.0[3] >= CUTOUT_BYTE).map(&lum).collect();
            let base_mean = base.iter().sum::<f32>() / base.len() as f32;
            let mut level = bleed_into_transparency(&image);
            let mut size = image.width();
            let mut first = true;
            loop {
                let shown = if first { level.clone() } else { held_to_coverage(&level, wanted) };
                first = false;
                let kept: Vec<&image::Rgba<u8>> =
                    shown.pixels().filter(|p| p.0[3] >= CUTOUT_BYTE).collect();
                let mean = kept.iter().map(|p| lum(p)).sum::<f32>() / kept.len().max(1) as f32;
                let mut greens: Vec<u8> = kept.iter().map(|p| p.0[1]).collect();
                greens.sort_unstable();
                println!(
                    "  {size:>3}: kept {:>4}/{:<4} cover {:.3} lum {:.2}x greens {:?}..{:?} \
                     alphas {:?}",
                    kept.len(),
                    shown.pixels().len(),
                    kept.len() as f32 / shown.pixels().len() as f32,
                    mean / base_mean,
                    greens.first(),
                    greens.last(),
                    {
                        let mut a: Vec<u8> = shown.pixels().map(|p| p.0[3]).collect();
                        a.sort_unstable();
                        a.dedup();
                        a
                    }
                );
                if size == 1 { break; }
                size = (size / 2).max(1);
                level = downsample(&level, size);
            }
        }
    }

    /// The mean brightness of a picture's drawn texels, in linear
    /// light, and how red it is against how green.
    ///
    /// Linear because that is what the shader gets: the array is
    /// `Rgba8UnormSrgb` and the card undoes the transfer curve on every
    /// fetch, so averaging the bytes would flatter a dark picture by
    /// about a third. Rec. 709, the weights the eye reads a green field
    /// with.
    fn drawn_colour(image: &RgbaImage) -> (f32, f32, f32) {
        fn linear(byte: u8) -> f32 {
            let c = byte as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        let drawn: Vec<&image::Rgba<u8>> =
            image.pixels().filter(|p| p.0[3] >= CUTOUT_BYTE).collect();
        if drawn.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        let luminance = |p: &image::Rgba<u8>| {
            0.2126 * linear(p.0[0]) + 0.7152 * linear(p.0[1]) + 0.0722 * linear(p.0[2])
        };
        let mean = drawn.iter().map(|p| luminance(p)).sum::<f32>() / drawn.len() as f32;
        let brightest = drawn.iter().map(|p| luminance(p)).fold(0.0f32, f32::max);
        let red = drawn.iter().map(|p| f32::from(p.0[0])).sum::<f32>();
        let green = drawn.iter().map(|p| f32::from(p.0[1])).sum::<f32>();
        (mean, brightest, red / green.max(1.0))
    }

    /// **A blade of grass is made of the turf it grows out of.**
    ///
    /// The bug, and it was one picture. `plants/grass_mesh.png` was
    /// painted in a green nothing else in the world wears -- `#00b100`
    /// on four texels in ten, against the grass block's own `#238b0b` --
    /// which put its mean linear luminance at **1.51x** the turf's and
    /// its red at 0.08 of its green where the turf sits at 0.28.
    ///
    /// What that looked like, measured on the frame a player
    /// photographed by re-rendering the same seat with the tufts
    /// suppressed and diffing the two: the pixels a tuft covered came
    /// out **2.06x brighter than the ground under them**, and along the
    /// far shoreline 5.4x. Two complaints, one cause. Up close a blade
    /// one texel wide, its bright core against its own dark edge -- a
    /// step of 3.3x between neighbouring texels, each of them six
    /// screen pixels under magnification -- reads as a dark outline
    /// drawn round every stem. Far off the tufts overlap at a grazing
    /// angle until they cover the ground completely, and a surface
    /// twice as bright as the field beside it is a band along the tree
    /// line.
    ///
    /// **And it got worse with distance rather than better**, which is
    /// what made it look like a filtering fault and is not one. A
    /// cutout does not fade: `held_to_coverage` exists precisely so a
    /// distant meadow does not go bald, so a tuft a hundred blocks away
    /// still paints a fifth of its quad at the full strength of its
    /// brightest texels while everything around it averages down. The
    /// mip chain was checked and is innocent -- grass runs
    /// `1.00 -> 1.00 -> 0.99 -> 1.01` down it. Nothing in the sampler
    /// can rescue a colour that is already wrong at the top of the
    /// chain.
    ///
    /// So the property is about the **two pictures together**: repaint
    /// either and the test moves with it. The bounds are wide on
    /// purpose -- this guards against a palette borrowed from another
    /// game, it does not dictate art.
    #[test]
    fn a_tuft_of_tall_grass_is_the_same_green_as_the_turf_it_grows_out_of() {
        let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
        let (Ok(tuft), Ok(turf)) = (
            image::open(assets.join("plants/grass_mesh.png")),
            image::open(assets.join("terrain/grass_top.png")),
        ) else {
            // The pictures are in the repository; a checkout without
            // them is not a reason to fail the build.
            return;
        };
        let (tuft_mean, tuft_max, tuft_warmth) = drawn_colour(&tuft.to_rgba8());
        let (turf_mean, turf_max, turf_warmth) = drawn_colour(&turf.to_rgba8());

        let ratio = tuft_mean / turf_mean;
        assert!(
            (0.75..=1.25).contains(&ratio),
            "a tuft averages {tuft_mean:.4} against the turf's {turf_mean:.4}, which is \
             {ratio:.2}x -- it will read as a band wherever the tufts hide the ground"
        );
        // No blade brighter than the brightest thing the field itself
        // has in it. This is the half that bit at distance: the cutout
        // keeps a level's *most opaque* texels, which on a thin shape
        // are its brightest ones, so the top of the picture is what a
        // far meadow is made of.
        assert!(
            tuft_max <= turf_max + 1e-4,
            "the brightest blade is at {tuft_max:.4} against the turf's brightest {turf_max:.4}"
        );
        // ...and the same hue. A ratio rather than an absolute, so a
        // pack that repaints both keeps passing.
        let warmth = tuft_warmth / turf_warmth;
        assert!(
            (0.6..=1.6).contains(&warmth),
            "a blade's red is {tuft_warmth:.3} of its green against the turf's \
             {turf_warmth:.3} -- the tuft is a hue the ground it stands in never wears"
        );
    }

    /// **The biome tint is a multiplier, and 1.55 times nothing is
    /// nothing.**
    ///
    /// `foliage_tint` in the shader spreads plant life across the
    /// climate square mostly in the *red* channel -- 0.45 in a swamp
    /// against 1.55 in dry steppe -- because that is what turns a green
    /// field straw-coloured without a second texture. A picture painted
    /// with red at zero is outside that arrangement entirely: it is the
    /// same green in every biome in the world while the ground under it
    /// changes, and no amount of tinting can pull it back into key.
    /// That is how `grass_mesh.png` came to be `#00b100` in a meadow
    /// whose turf is `#238b0b`, and it is the second half of why the
    /// tufts read as something drawn on top of the world rather than
    /// part of it.
    ///
    /// Asked of every picture any foliage block wears, found through
    /// `is_foliage` and `blocks.toml` rather than from a list somebody
    /// has to remember to extend -- so a new bush is covered on the day
    /// it is added.
    #[test]
    fn every_foliage_picture_leaves_the_biome_tint_something_to_multiply() {
        let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
        let Ok(text) = std::fs::read_to_string(assets.join("blocks.toml")) else {
            return;
        };
        let config: BlocksToml = toml::from_str(&text).expect("the shipped blocks.toml parses");
        let mut checked = 0;
        for &(id, name) in ALL_BLOCK_IDS {
            if !primitive_shared::types::is_foliage(id) {
                continue;
            }
            let Some(spec) = config.textures.get(name) else {
                continue;
            };
            for face in 0..FACES {
                let Some(file) = spec.for_face(face) else {
                    continue;
                };
                let Ok(image) = image::open(assets.join(file)) else {
                    continue;
                };
                let (_, _, warmth) = drawn_colour(&image.to_rgba8());
                checked += 1;
                // A sixth. Every picture in the pack sits between 0.27
                // and 1.19; the one that broke the meadow was at 0.079.
                assert!(
                    warmth >= 1.0 / 6.0,
                    "{file} (worn by {name}) has red at {warmth:.3} of its green -- the \
                     biome tint cannot move it, so it is one colour in every climate"
                );
            }
        }
        assert!(checked > 0, "no foliage picture was looked at; the walk found nothing");
    }

    /// A chest put down from each of the four sides shows its front -- the
    /// picture `blocks.toml` gives its north face -- on the side that looks
    /// back at whoever put it there, and on no other.
    ///
    /// **The picture half of the facing, which is the half that was
    /// wrong.** The id was right all along (`types` holds that the step
    /// points back at the placer); the ring in `faced_face` turned the
    /// pictures the other way, so east and west swapped sides and a kiln
    /// built by a player looking along x had its mouth on the far side.
    #[test]
    fn a_chest_placed_from_each_side_shows_its_front_to_whoever_placed_it() {
        use primitive_shared::types::{placed, BLOCK_CHEST};
        let layers = FaceLayers::numbered_for_test();
        let front = layers.layer_for_face(BLOCK_CHEST, FACE_NORTH);
        let sides = [(FACE_EAST, 1.0f32, 0.0f32), (FACE_WEST, -1.0, 0.0), (FACE_SOUTH, 0.0, 1.0), (FACE_NORTH, 0.0, -1.0)];
        for turn in 0..4 {
            let yaw = turn as f32 * std::f32::consts::FRAC_PI_2;
            let (fx, fz) = (yaw.cos(), yaw.sin());
            let chest = placed(BLOCK_CHEST, yaw, (0, 1, 0));
            // The side whose outward normal points back along the look.
            let toward_eye = sides
                .iter()
                .min_by(|a, b| (a.1 * fx + a.2 * fz).total_cmp(&(b.1 * fx + b.2 * fz)))
                .map(|side| side.0)
                .unwrap();
            for &(face, _, _) in &sides {
                let shows = layers.layer_for_face(chest, face);
                if face == toward_eye {
                    assert_eq!(shows, front, "looking along {yaw}, the front is not on the side facing the eye");
                } else {
                    assert_ne!(shows, front, "looking along {yaw}, face {face} shows the front as well");
                }
            }
        }
    }

    /// **Which blocks turn is read off what they look like, both ways.**
    ///
    /// The player's rule: a block may be put down facing different ways
    /// only if it is not the same from every side. Two things say whether
    /// it is, and neither is a list: the pictures in `blocks.toml`, and the
    /// mesher's own output for the models. So for every block a player can
    /// put down, or that carries a direction anyway:
    ///
    /// * a **front** is shown when a quarter turn about the vertical
    ///   changes the drawing -- the side pictures differ, or the model is
    ///   not the same turned -- and the row's `faces` must say exactly
    ///   that. A model drawn differently in different *cells* (a tuft, a
    ///   carcass) is turned by the world rather than by a placer, and must
    ///   not turn as well.
    /// * a **length** is shown by a cube whose two ends are one picture and
    ///   whose sides are another, standing on its own feet (a cactus grows
    ///   up out of the sand and is not laid down), and the row's
    ///   `orientable` must say so -- or be there because breaking the block
    ///   depends on its axis (`felled`), which is the stripped trunk.
    ///
    /// A new block with a mouth drawn on one side and no `faces`, or a
    /// stone given a facing it cannot show, fails here by name.
    #[test]
    fn a_block_turns_when_it_is_placed_exactly_when_turning_it_would_show() {
        use crate::engine::mesh::{drawn_as_model, what_a_quarter_turn_does, QuarterTurn};
        let config: BlocksToml = toml::from_str(crate::embedded::BLOCKS_TOML).expect("the shipped blocks.toml parses");
        let mut wrong = Vec::new();
        for &(id, name) in ALL_BLOCK_IDS {
            let def = primitive_shared::blocks::definition(id);
            if !def.placeable && !def.faces && !def.orientable {
                continue;
            }
            let picture = |face: usize| config.textures.get(name).and_then(|spec| spec.for_face(face));
            let model = drawn_as_model(id);
            let sides = [FACE_NORTH, FACE_EAST, FACE_SOUTH, FACE_WEST].map(picture);
            let (top, bottom) = (picture(FACE_TOP), picture(FACE_BOTTOM));
            let turn = what_a_quarter_turn_does(id);

            let shows_a_front = turn == QuarterTurn::Changed
                || (!model && sides.iter().any(|side| *side != sides[0]));
            let wants_front = shows_a_front && turn != QuarterTurn::ChosenByTheCell;
            if def.faces != wants_front {
                wrong.push(format!(
                    "{name}: `faces: {}`, but a quarter turn of it {} ({turn:?})",
                    def.faces,
                    if shows_a_front { "shows" } else { "shows nothing" }
                ));
            }

            let shows_a_length = !model
                && !def.propped
                && top == bottom
                && sides.iter().all(|side| *side == sides[0])
                && top != sides[0];
            if shows_a_length && !def.orientable {
                wrong.push(format!("{name}: its ends and its sides differ, and it cannot be laid down"));
            }
            if def.orientable && !shows_a_length && def.felled.is_none() {
                wrong.push(format!("{name}: `orientable`, and laying it down shows nothing and changes nothing"));
            }
        }
        assert!(wrong.is_empty(), "blocks whose direction disagrees with their look:\n{}", wrong.join("\n"));
    }

}
