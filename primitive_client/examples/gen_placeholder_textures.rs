//! Regenerates the starter block PNGs in `assets/textures/`.
//!
//! These are placeholders: a flat base colour plus a cheap deterministic
//! speckle so adjacent blocks of the same type don't look like one solid
//! sheet. Replace them with real art whenever -- nothing else changes,
//! since `blocks.toml` is the only thing that knows the filenames.
//!
//! Run: `cargo run -p primitive_client --example gen_placeholder_textures`
//!
//! Pass a filename to write only that one:
//!
//! ```text
//! cargo run -p primitive_client --example gen_placeholder_textures -- workbench_side.png
//! ```
//!
//! That exists so a new texture can be added without rewriting the
//! others. They are deterministic and would come out byte-identical, but
//! "would" is doing a lot of work in a folder where someone may have
//! replaced a placeholder with real art.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};

const RESOLUTION: u32 = 16;

/// (filename, base colour, speckle strength). Glowstone gets a bright
/// core and strong speckle so it reads as a light source even before the
/// lighting engine touches it.
const TEXTURES: &[(&str, [u8; 3], i32)] = &[
    ("grass_top.png", [86, 148, 62], 18),
    ("dirt.png", [123, 88, 58], 16),
    ("stone.png", [128, 128, 130], 14),
    ("sand.png", [214, 201, 148], 12),
    ("snow.png", [238, 243, 248], 8),
    ("water.png", [58, 108, 190], 10),
    ("log_top.png", [140, 104, 62], 20),
    ("leaves.png", [58, 122, 52], 26),
    ("glowstone.png", [232, 200, 108], 30),
    ("planks.png", [166, 128, 78], 14),
];

/// Two-tone textures for the sides of blocks whose top differs from
/// their flanks: grass fading into the dirt below it, and the bark of a
/// log against its cut end. This is what the per-face texture system in
/// `texture.rs` exists for.
const SIDE_TEXTURES: &[(&str, [u8; 3], [u8; 3], u32, i32)] = &[
    // (file, top colour, bottom colour, how many rows of top colour, speckle)
    ("grass_side.png", [86, 148, 62], [123, 88, 58], 5, 16),
    ("log_side.png", [104, 74, 44], [88, 62, 38], 16, 22),
];

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
    std::fs::create_dir_all(&dir)?;

    let only: Option<String> = std::env::args().nth(1);
    let wanted = |name: &str| only.as_deref().map_or(true, |o| o == name);
    let mut written = 0;

    for (filename, base, speckle) in TEXTURES {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate(*base, *speckle, filename))?;
        written += 1;
    }

    for (filename, top, bottom, top_rows, speckle) in SIDE_TEXTURES {
        if !wanted(filename) {
            continue;
        }
        let img = generate_two_tone(*top, *bottom, *top_rows, *speckle, filename);
        write(&dir, filename, img)?;
        written += 1;
    }

    if wanted(WORKBENCH) {
        write(&dir, WORKBENCH, generate_workbench())?;
        written += 1;
    }

    if written == 0 {
        anyhow::bail!("no texture called {:?}", only.unwrap_or_default());
    }
    println!("done -- {written} texture(s)");
    Ok(())
}

fn write(dir: &std::path::Path, filename: &str, img: RgbaImage) -> anyhow::Result<()> {
    let path = dir.join(filename);
    img.save(&path)?;
    println!("wrote {}", path.display());
    Ok(())
}

const WORKBENCH: &str = "workbench_side.png";

/// The workbench side: a planked panel with a dark frame and a lighter
/// worktop.
///
/// Drawn rather than speckled because it does double duty as the game's
/// window icon, and at 16x16 in a taskbar an even field of noise is a
/// smudge. It needs a silhouette: a dark border and a strong horizontal
/// division survive being scaled down to something the size of a
/// thumbnail, which is the only size anyone will ever see it at.
fn generate_workbench() -> RgbaImage {
    const FRAME: [u8; 3] = [58, 38, 22];
    const PLANK: [u8; 3] = [140, 100, 58];
    const PLANK_DARK: [u8; 3] = [116, 82, 46];
    const TOP: [u8; 3] = [176, 134, 84];
    const TOOL: [u8; 3] = [78, 52, 30];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let seed = WORKBENCH
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let mut base = if y < 3 {
                TOP // the worktop, seen edge-on
            } else if (y / 4) % 2 == 0 {
                PLANK
            } else {
                PLANK_DARK
            };

            // Horizontal seams between planks, and a vertical seam down
            // the middle offset every other course, so it reads as
            // brickwork rather than as stripes.
            let course = y / 4;
            if y >= 3 && (y % 4 == 3 || (x + course * 5) % 8 == 0) {
                base = PLANK_DARK;
            }

            // A saw hanging on the panel: a blade and a handle. Small,
            // dark, and asymmetric, which is what makes the icon
            // recognisable at a glance.
            if (6..=8).contains(&y) && (3..=11).contains(&x) {
                base = TOOL;
            }
            if y == 9 && (3..=5).contains(&x) {
                base = TOOL;
            }

            // The frame goes on last so nothing draws over it.
            if x == 0 || y == 0 || x == RESOLUTION - 1 || y == RESOLUTION - 1 {
                base = FRAME;
            }

            let noise = (hash(seed, x, y) % 13) as i32 - 6;
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(base[0] as i32 + noise),
                    clamp_u8(base[1] as i32 + noise),
                    clamp_u8(base[2] as i32 + noise),
                    255,
                ]),
            );
        }
    }
    img
}

/// A side texture: `top_rows` rows of one colour, the rest of another,
/// with a slightly ragged boundary so grass doesn't end in a ruler line.
fn generate_two_tone(
    top: [u8; 3],
    bottom: [u8; 3],
    top_rows: u32,
    speckle: i32,
    seed_name: &str,
) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let seed = seed_name
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    for x in 0..RESOLUTION {
        // Per-column jitter of the boundary, deterministic per column.
        let jitter = (hash(seed, x, 0) % 3) as i32 - 1;
        let boundary = (top_rows as i32 + jitter).clamp(0, RESOLUTION as i32) as u32;
        for y in 0..RESOLUTION {
            let base = if y < boundary { top } else { bottom };
            let noise = hash(seed, x, y + 1) % (speckle.unsigned_abs() * 2 + 1);
            let delta = noise as i32 - speckle;
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(base[0] as i32 + delta),
                    clamp_u8(base[1] as i32 + delta),
                    clamp_u8(base[2] as i32 + delta),
                    255,
                ]),
            );
        }
    }
    img
}

fn generate(base: [u8; 3], speckle: i32, seed_name: &str) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // Seed from the filename so each texture speckles differently but
    // regenerating produces byte-identical files.
    let seed = seed_name
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let noise = hash(seed, x, y) % (speckle.unsigned_abs() * 2 + 1);
            let delta = noise as i32 - speckle;
            let px = [
                clamp_u8(base[0] as i32 + delta),
                clamp_u8(base[1] as i32 + delta),
                clamp_u8(base[2] as i32 + delta),
                255,
            ];
            img.put_pixel(x, y, Rgba(px));
        }
    }
    img
}

/// Small deterministic integer hash -- no rng dependency needed for
/// speckle this simple.
fn hash(seed: u32, x: u32, y: u32) -> u32 {
    let mut h = seed
        .wrapping_add(x.wrapping_mul(374_761_393))
        .wrapping_add(y.wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}
