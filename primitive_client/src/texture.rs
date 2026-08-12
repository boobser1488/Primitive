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

use primitive_shared::types::{BlockId, ALL_BLOCK_IDS};

/// Face order must match `mesh::faces()`.
pub const FACE_TOP: usize = 0;
pub const FACE_BOTTOM: usize = 1;
pub const FACE_EAST: usize = 2;
pub const FACE_WEST: usize = 3;
pub const FACE_SOUTH: usize = 4;
#[allow(dead_code)] // completes the face-name set; used by tests and configs
pub const FACE_NORTH: usize = 5;
pub const FACES: usize = 6;

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
            layers: std::sync::Arc::from(vec![0u32; 64 * FACES]),
            max_block_id: 63,
        }
    }

    #[inline]
    pub fn layer_for_face(&self, block_id: BlockId, face: usize) -> u32 {
        if block_id > self.max_block_id || face >= FACES {
            return 0;
        }
        self.layers[block_id as usize * FACES + face]
    }
}

/// First and last character with a layer of its own.
const FIRST_GLYPH: u8 = 0x20;
const LAST_GLYPH: u8 = 0x7e;
/// Layer 0 is the missing-texture placeholder; the font starts after it.
const FONT_BASE: u32 = 1;

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
        let code = c as u32;
        if code < FIRST_GLYPH as u32 || code > LAST_GLYPH as u32 {
            return 0; // the placeholder layer
        }
        FONT_BASE + (code - FIRST_GLYPH as u32)
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
    use crate::font::{GLYPH_HEIGHT, GLYPH_WIDTH};

    let mut img = RgbaImage::new(resolution, resolution);
    let rows = crate::font::glyph(c);
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
    /// Number of distinct images uploaded (array layers).
    pub layer_count: u32,
    /// Where the font sits in that array.
    pub font: FontAtlas,
    /// Flat table: `block_id * FACES + face` -> array layer.
    face_layers: Vec<u32>,
    max_block_id: BlockId,
}

impl TextureManager {
    pub fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets_dir: &Path,
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
        let mut face_layers = vec![0u32; (max_block_id as usize + 1) * FACES];

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
        for byte in FIRST_GLYPH..=LAST_GLYPH {
            images.push(glyph_texture(byte as char, resolution));
        }

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
                face_layers[block_id as usize * FACES + face] = layer;
            }
        }

        let layer_count = images.len() as u32;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("block texture array"),
            size: wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: layer_count,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for (layer, img) in images.iter().enumerate() {
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                img.as_raw(),
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * resolution),
                    rows_per_image: Some(resolution),
                },
                wgpu::Extent3d {
                    width: resolution,
                    height: resolution,
                    depth_or_array_layers: 1,
                },
            );
        }

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("block texture sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            // Nearest, not linear -- blocky look, crisp texel edges.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

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
            layer_count,
            font: FontAtlas {
                u_max: crate::font::GLYPH_WIDTH as f32 / resolution as f32,
                v_max: crate::font::GLYPH_HEIGHT as f32 / resolution as f32,
            },
            face_layers,
            max_block_id,
        })
    }

    /// A sendable copy of the face lookup, for the mesher threads.
    pub fn face_layers(&self) -> FaceLayers {
        FaceLayers {
            layers: self.face_layers.clone().into(),
            max_block_id: self.max_block_id,
        }
    }

    /// Array layer for one face of one block. Hot path: called once per
    /// emitted face.
    #[inline]
    pub fn layer_for_face(&self, block_id: BlockId, face: usize) -> u32 {
        if block_id > self.max_block_id || face >= FACES {
            return 0;
        }
        self.face_layers[block_id as usize * FACES + face]
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
pub fn load_window_icon(assets_dir: &Path) -> Option<(Vec<u8>, u32, u32)> {
    const SCALE: u32 = 4;

    let path = assets_dir.join("textures").join(ICON_FILE);
    let img = match image::open(&path) {
        Ok(img) => img.to_rgba8(),
        // Not an error: with no assets folder the built-in copy is the
        // expected source, and a missing icon is cosmetic either way.
        Err(_) => match crate::embedded::texture(ICON_FILE) {
            Some(bytes) => match image::load_from_memory(bytes) {
                Ok(img) => img.to_rgba8(),
                Err(e) => {
                    eprintln!("the built-in window icon failed to decode ({e})");
                    return None;
                }
            },
            None => return None,
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
            let checker = (x / half + y / half) % 2 == 0;
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
    fn an_explicit_assets_dir_wins() {
        assert_eq!(
            resolve_assets_dir("/tmp/custom-assets"),
            PathBuf::from("/tmp/custom-assets")
        );
    }
}
