//! Block textures, now **per face**.
//!
//! `assets/textures/blocks.toml` accepts either form:
//!
//! ```toml
//! stone = "stone.png"                                   # same on all 6 faces
//! grass = { top = "grass_top.png", side = "grass_side.png", bottom = "dirt.png" }
//! log   = { top = "log_top.png", side = "log_side.png" }
//! chest = { north = "chest_front.png", side = "chest_side.png", all = "chest_side.png" }
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
}

impl FaceLayers {
    /// An all-placeholder table, for tests that exercise the meshing
    /// pipeline without a GPU.
    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self {
            layers: std::sync::Arc::from(vec![0u32; 64 * SLOTS]),
            max_block_id: 63,
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
        let kind = primitive_shared::types::block_kind(block_id);
        if kind > self.max_block_id || face >= FACES {
            return 0;
        }
        let face = local_face(face, primitive_shared::types::block_axis(block_id));
        self.layers[kind as usize * SLOTS + face]
    }

    /// The picture for this block *in the pack*, if it has one of its
    /// own. See `ITEM_SLOT`.
    #[inline]
    pub fn layer_for_item(&self, block_id: BlockId) -> Option<u32> {
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

/// Every character with a layer of its own, in layer order.
///
/// **A list rather than a range**, because the game speaks four
/// languages and their alphabets do not sit next to each other in
/// Unicode. Printable ASCII, then Cyrillic, then the Polish letters that
/// are not already in ASCII -- see `font::glyph`, which draws them.
///
/// Order is the only thing that matters here and it must match nothing
/// else: `FontAtlas::layer` finds a character by searching this, so
/// adding a letter anywhere is safe.
pub const GLYPHS: &str = concat!(
    " !\"#$%&'()*+,-./0123456789:;<=>?@",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`",
    "abcdefghijklmnopqrstuvwxyz{|}~",
    "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯЁабвгдежзийклмнопрстуфхцчшщъыьэюяёĄąĆćĘęŁłŃńÓóŚśŹźŻż",
);
/// Layer 0 is the missing-texture placeholder; the font starts after it.
const FONT_BASE: u32 = 1;

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

/// Where the font lives in the texture array, and how much of each layer
/// one glyph occupies.
///
/// `Copy` and three words wide, so it is passed by value to everything
/// that draws text rather than reached for through the renderer.
#[derive(Debug, Clone, Copy)]
pub struct FontAtlas {
    /// Fraction of the layer the glyph covers. The glyph sits in the
    /// top-left at its native size, so this is `6/resolution` by
    /// `9/resolution` -- not 1.0, and not resolution-independent, which
    /// is why it is carried rather than assumed.
    pub u_max: f32,
    pub v_max: f32,
}

impl FontAtlas {
    /// The array layer holding one character, or the placeholder for
    /// anything outside printable ASCII -- which draws the same visible
    /// box `font::glyph` would have.
    #[inline]
    pub fn layer(&self, c: char) -> u32 {
        match GLYPHS.chars().position(|g| g == c) {
            Some(index) => FONT_BASE + index as u32,
            // The placeholder layer, which draws the same visible box a
            // missing glyph would.
            None => 0,
        }
    }

    /// A stand-in for tests, which lay text out without a GPU.
    pub fn for_test() -> Self {
        Self {
            u_max: 1.0,
            v_max: 1.0,
        }
    }
}

/// One glyph, drawn white-on-transparent in the top-left of a layer.
///
/// Native size, not stretched to fill: a 6x9 bitmap scaled by a
/// non-integer factor gives pixels of uneven width, which is the one
/// thing a pixel font must not do.
fn glyph_texture(c: char, resolution: u32) -> RgbaImage {
    use crate::engine::font::{GLYPH_HEIGHT, GLYPH_WIDTH};

    let mut img = RgbaImage::new(resolution, resolution);
    let rows = crate::engine::font::glyph(c);
    for (row_index, row) in rows.iter().enumerate() {
        if row_index as u32 >= resolution {
            break;
        }
        for column in 0..GLYPH_WIDTH {
            if column as u32 >= resolution {
                break;
            }
            // Most significant bit is the leftmost pixel, the same way
            // `Painter::text` used to read it.
            if row & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                continue;
            }
            img.put_pixel(
                column as u32,
                row_index as u32,
                image::Rgba([255, 255, 255, 255]),
            );
        }
    }
    let _ = GLYPH_HEIGHT;
    img
}

pub struct TextureManager {
    pub texture_view: wgpu::TextureView,
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
    /// Shapes for the blocks that are not cubes, by block kind.
    item_models: HashMap<BlockId, ItemModel>,
}

impl TextureManager {
    pub fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets_dir: &Path,
        anisotropy: u16,
    ) -> anyhow::Result<Self> {
        let textures_dir = assets_dir.join("textures");
        let config_path = textures_dir.join("blocks.toml");

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

        // Then the font, one glyph per layer, at fixed indices.
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
        // A glyph per layer makes a character one textured quad. The
        // layers live in the block array so this needs no second texture,
        // no second sampler, no second bind group and no shader change:
        // the UI shader already samples this array by layer index.
        for glyph in GLYPHS.chars() {
            images.push(glyph_texture(glyph, resolution));
        }

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

        // The breaking overlay, last: five stages of cracks laid over
        // whatever block is being mined. They live in the same array as
        // everything else, so drawing them needs no second texture, no
        // second bind group and no shader of their own -- the terrain
        // shader already samples this array by layer index, and the
        // cracks are a quad with a layer like any other.
        let mut break_layers = [0u32; BREAK_STAGES];
        for (stage, layer) in break_layers.iter_mut().enumerate() {
            let filename = format!("break.{stage}.png");
            images.push(load_or_placeholder(
                &textures_dir.join(&filename),
                &filename,
                resolution,
            ));
            *layer = (images.len() - 1) as u32;
        }

        let layer_count = images.len() as u32;

        // A terrain vertex carries its layer in eight bits (see
        // `mesh::MAX_TEXTURE_LAYERS`), so a pack with more images than
        // that would silently wrap and dress half the world in glyphs.
        // Refusing to start says so instead. The stock install uses
        // about 120 of the 256, so this is a guard rail rather than a
        // limit anybody is near.
        anyhow::ensure!(
            layer_count <= crate::engine::mesh::MAX_TEXTURE_LAYERS,
            "{layer_count} textures configured, but a block vertex can only \
             address {}; use fewer distinct images",
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

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("block texture array"),
            size: wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: layer_count,
            },
            mip_level_count: mip_levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for (layer, img) in images.iter().enumerate() {
            let mut level = img.clone();
            let mut size = resolution;
            for mip in 0..mip_levels {
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &texture,
                        mip_level: mip,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    level.as_raw(),
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

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let sampler = build_sampler(device, anisotropy);
        let ui_sampler = build_ui_sampler(device);
        let clouds = CloudTexture::load(device, queue, &textures_dir);

        println!(
            "textures: {} block(s), {} distinct image layer(s) at {}x{}",
            ALL_BLOCK_IDS.len(),
            layer_count,
            resolution,
            resolution
        );

        Ok(Self {
            texture_view,
            sampler,
            ui_sampler,
            clouds,
            resolution,
            layer_count,
            font: FontAtlas {
                u_max: crate::engine::font::GLYPH_WIDTH as f32 / resolution as f32,
                v_max: crate::engine::font::GLYPH_HEIGHT as f32 / resolution as f32,
            },
            face_layers,
            max_block_id,
            break_layers,
            item_models,
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

    /// A sendable copy of the face lookup, for the mesher threads.
    pub fn face_layers(&self) -> FaceLayers {
        FaceLayers {
            layers: self.face_layers.clone().into(),
            max_block_id: self.max_block_id,
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

const CLOUD_FILE: &str = "sky_clouds.png";

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
/// Halves an image with a box filter.
///
/// A box filter rather than anything cleverer because the source is
/// 16x16 pixel art: at that size a wider kernel is mostly reaching
/// outside the texel it is meant to be averaging.
fn downsample(source: &RgbaImage, size: u32) -> RgbaImage {
    let mut out = RgbaImage::new(size, size);
    let (sw, sh) = (source.width(), source.height());
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0u32; 4];
            let mut taken = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                    if sx >= sw || sy >= sh {
                        continue;
                    }
                    let p = source.get_pixel(sx, sy).0;
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                    taken += 1;
                }
            }
            let taken = taken.max(1);
            out.put_pixel(
                x,
                y,
                image::Rgba([
                    (sum[0] / taken) as u8,
                    (sum[1] / taken) as u8,
                    (sum[2] / taken) as u8,
                    (sum[3] / taken) as u8,
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
/// `ClampToEdge` rather than `Repeat` because of that snapping: a UV
/// nudged a hair past 1.0 would wrap to the opposite edge of the layer
/// and put a bright seam along every block face.
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
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: mode,
        min_filter: mode,
        mipmap_filter: mode,
        anisotropy_clamp: anisotropy,
        ..Default::default()
    })
}

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
pub const ICON_FILE: &str = "workbench_side.png";

/// One texture: the file on disk if there is one, else the copy built
/// into the binary, else the placeholder.
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
    let img = if img.dimensions() != (resolution, resolution) {
        img.resize_exact(resolution, resolution, FilterType::Nearest)
    } else {
        img
    };
    img.to_rgba8()
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

    fn parse(toml_text: &str) -> BlocksToml {
        toml::from_str(toml_text).expect("config should parse")
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

    #[test]
    fn an_explicit_assets_dir_wins() {
        assert_eq!(
            resolve_assets_dir("/tmp/custom-assets"),
            PathBuf::from("/tmp/custom-assets")
        );
    }
}
