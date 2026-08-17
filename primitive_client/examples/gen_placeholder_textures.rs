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
    // Wet riverbank earth: grey-blue and almost even, because clay is
    // the one soil with no grain in it.
    ("clay.png", [142, 146, 158], 8),
    // Stones of every size with nothing holding them together, so the
    // speckle is the strongest in the set -- gravel that is not noisy
    // reads as concrete.
    ("gravel.png", [122, 118, 114], 34),
];

/// Two-tone textures for the sides of blocks whose top differs from
/// their flanks: grass fading into the dirt below it, and the bark of a
/// log against its cut end. This is what the per-face texture system in
/// `texture.rs` exists for.
#[allow(clippy::type_complexity)] // a table, and the tuple is its columns
const SIDE_TEXTURES: &[(&str, [u8; 3], [u8; 3], u32, i32)] = &[
    // (file, top colour, bottom colour, how many rows of top colour, speckle)
    ("grass_side.png", [86, 148, 62], [123, 88, 58], 5, 16),
    ("log_side.png", [104, 74, 44], [88, 62, 38], 16, 22),
];

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
    std::fs::create_dir_all(&dir)?;

    let only: Option<String> = std::env::args().nth(1);
    let wanted = |name: &str| only.as_deref().is_none_or(|o| o == name);
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

    for (filename, generate) in PLANTS {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate())?;
        written += 1;
    }

    for (filename, ore, shadow) in ORES {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_ore(&dir, filename, *ore, *shadow))?;
        written += 1;
    }

    for (filename, metal, highlight) in INGOTS {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_ingot(*metal, *highlight))?;
        written += 1;
    }

    for (filename, head, highlight) in PICKAXES {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_pickaxe(*head, *highlight))?;
        written += 1;
    }

    for (filename, generate) in KNAPPED {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate())?;
        written += 1;
    }

    for stage in 0..BREAK_STAGES {
        let filename = format!("break.{stage}.png");
        if !wanted(&filename) {
            continue;
        }
        write(&dir, &filename, generate_break(stage))?;
        written += 1;
    }

    if wanted(CLOUDS) {
        write(&dir, CLOUDS, generate_clouds())?;
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

/// Textures that are mostly empty: the cross-shaped plants and the
/// cactus that stands among them. Drawn rather than speckled -- a tuft
/// of grass is a silhouette, and a field of noise in the shape of a
/// square is not one.
/// A named picture and the code that draws it. Named, because the pair
/// turns up in every table below and clippy is right that the bare tuple
/// is a mouthful.
type Drawn = (&'static str, fn() -> RgbaImage);

const PLANTS: &[Drawn] = &[
    ("grass_mesh.png", generate_grass_mesh),
    ("stick.png", generate_stick),
    ("cactus.png", generate_cactus),
    ("fiber.png", generate_fiber),
    ("pebble.png", generate_pebble),
    ("flint.png", generate_flint),
    ("chest_side.png", generate_chest_side),
    ("chest_top.png", generate_chest_top),
    ("backpack_side.png", generate_backpack_side),
    ("backpack_top.png", generate_backpack_top),
    ("ash.png", generate_ash),
    ("ash_item.png", generate_ash_item),
];

/// A tuft of grass: a few blades fanning up from the bottom of the tile.
///
/// Transparent everywhere else, because this is drawn on two crossed
/// planes (see `is_cross`) and everything that is not a blade has to
/// show the world behind it.
fn generate_grass_mesh() -> RgbaImage {
    const BLADE: [u8; 3] = [86, 142, 58];
    const BLADE_DARK: [u8; 3] = [62, 108, 44];
    const BLADE_PALE: [u8; 3] = [116, 168, 74];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // (x at the root, how far it leans, how tall, colour)
    let blades: [(i32, f32, u32, [u8; 3]); 5] = [
        (3, -1.6, 9, BLADE_DARK),
        (6, -0.4, 12, BLADE),
        (8, 0.3, 14, BLADE_PALE),
        (10, 1.2, 11, BLADE),
        (12, 2.0, 8, BLADE_DARK),
    ];
    for (root, lean, height, colour) in blades {
        for step in 0..height {
            // Blades bend more the further up they go, which is what
            // makes them read as grass rather than as a comb.
            let t = step as f32 / height.max(1) as f32;
            let x = root as f32 + lean * t * t;
            let y = RESOLUTION as i32 - 1 - step as i32;
            put_opaque(&mut img, x.round() as i32, y, colour);
            // The base of a blade is thicker than its tip.
            if t < 0.35 {
                put_opaque(&mut img, x.round() as i32 + 1, y, colour);
            }
        }
    }
    img
}

/// A stick: one length of wood lying at an angle, with a stub of branch.
fn generate_stick() -> RgbaImage {
    const WOOD: [u8; 3] = [122, 88, 50];
    const WOOD_DARK: [u8; 3] = [92, 64, 36];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for step in 0..12 {
        let x = 3 + step / 2;
        let y = 13 - step;
        put_opaque(&mut img, x, y, WOOD);
        put_opaque(&mut img, x + 1, y, WOOD_DARK);
    }
    // A short branch, so it is a stick and not a plank on its edge.
    for step in 0..4 {
        put_opaque(&mut img, 7 + step, 8 - step, WOOD_DARK);
    }
    img
}

/// Plant fibre: a bundle of dried strands, pulled out of a tuft of
/// grass.
///
/// Opaque, unlike the tuft it came from, and that is not a stylistic
/// choice. Fibre is an item, and an item lying in the world is drawn as
/// a small cube on the *solid* pipeline -- whose fragment shader cannot
/// discard. A texture with transparent texels there would come out as
/// whatever happened to be in those pixels, which is the "trees are
/// cubes" bug wearing a different hat.
fn generate_fiber() -> RgbaImage {
    const STRAW: [u8; 3] = [198, 176, 108];
    const STRAW_DARK: [u8; 3] = [156, 134, 78];
    const STRAW_PALE: [u8; 3] = [222, 206, 148];
    const SHADOW: [u8; 3] = [120, 102, 62];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // A bed of the darkest tone, so nothing shows through the strands.
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let noise = (hash(0xF1BE, x, y) % 11) as i32 - 5;
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(SHADOW[0] as i32 + noise),
                    clamp_u8(SHADOW[1] as i32 + noise),
                    clamp_u8(SHADOW[2] as i32 + noise),
                    255,
                ]),
            );
        }
    }
    // Strands laid across it at shallow angles, each one a run of pixels
    // stepping sideways -- a bundle rather than a weave, which is what
    // separates fibre from cloth at this size.
    let strands: [(i32, i32, f32, [u8; 3]); 6] = [
        (0, 2, 0.35, STRAW_DARK),
        (0, 5, -0.20, STRAW),
        (2, 8, 0.28, STRAW_PALE),
        (0, 10, -0.35, STRAW),
        (1, 13, 0.18, STRAW_DARK),
        (4, 6, 0.55, STRAW_PALE),
    ];
    for (start, row, slope, colour) in strands {
        for step in 0..(RESOLUTION as i32 - start) {
            let x = start + step;
            let y = row + (step as f32 * slope) as i32;
            put_opaque(&mut img, x, y, colour);
            // Twisted, so a strand has a little thickness where it lies
            // flat and none where it turns.
            if step % 3 != 0 {
                put_opaque(&mut img, x, y + 1, colour);
            }
        }
    }
    img
}

/// Loose stones, seen from above: three or four rounded grey pebbles
/// scattered across an otherwise empty tile.
///
/// Mostly transparent, because this is drawn as a single quad laid on
/// the ground and everything that is not a stone has to show the grass
/// or sand under it. Rounded rather than square: at sixteen pixels the
/// difference between a stone and a tile of gravel is entirely in the
/// silhouette.
fn generate_pebble() -> RgbaImage {
    const STONE: [u8; 3] = [138, 136, 132];
    const STONE_DARK: [u8; 3] = [102, 100, 98];
    const STONE_PALE: [u8; 3] = [170, 168, 162];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // (centre x, centre y, radius, colour). Sizes and places chosen so
    // they do not touch: three stones lying apart read as three stones,
    // three stones in contact read as one lump.
    let stones: [(f32, f32, f32, [u8; 3]); 4] = [
        (4.5, 5.0, 2.6, STONE),
        (10.5, 4.0, 1.9, STONE_PALE),
        (11.0, 10.5, 2.4, STONE_DARK),
        (5.0, 11.5, 1.6, STONE_PALE),
    ];
    for (cx, cy, radius, colour) in stones {
        let reach = radius.ceil() as i32 + 1;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (x, y) = (cx + dx as f32, cy + dy as f32);
                let (ox, oy) = (x - cx, y - cy);
                if ox * ox + oy * oy > radius * radius {
                    continue;
                }
                // Lit from the top left, so a stone reads as round
                // rather than as a disc.
                let lift = ((-ox - oy) * 6.0) as i32;
                let noise = (hash(0x5701, x.max(0.0) as u32, y.max(0.0) as u32) % 11) as i32 - 5;
                put_opaque(
                    &mut img,
                    x as i32,
                    y as i32,
                    [
                        clamp_u8(colour[0] as i32 + lift + noise),
                        clamp_u8(colour[1] as i32 + lift + noise),
                        clamp_u8(colour[2] as i32 + lift + noise),
                    ],
                );
            }
        }
    }
    img
}

/// A chest: a wooden crate, side and top.
///
/// Drawn rather than speckled, for the same reason the workbench is:
/// this has to be recognisable as *the thing you put things in* from
/// across a room, and a field of brown noise is not.
///
/// What makes a crate read as a crate at sixteen pixels is the frame
/// rather than the wood. Corner posts down both sides, a rail along the
/// top and the bottom, boards between them running the other way, and a
/// pale iron band across the middle with a latch on it. Every one of
/// those is a straight line of contrast, and straight lines of contrast
/// are the only thing that survives being sixteen pixels tall.
///
/// `lid` draws the top face instead: the same frame seen from above,
/// with the boards running across it and the band round the rim.
fn generate_chest(lid: bool) -> RgbaImage {
    const POST: [u8; 3] = [96, 62, 32];
    const POST_DARK: [u8; 3] = [70, 44, 22];
    const BOARD: [u8; 3] = [140, 100, 56];
    const BOARD_DARK: [u8; 3] = [118, 82, 44];
    const GROOVE: [u8; 3] = [88, 58, 30];
    const IRON: [u8; 3] = [122, 122, 130];
    const IRON_DARK: [u8; 3] = [86, 86, 94];
    const LATCH: [u8; 3] = [206, 178, 92];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let seed = if lid { 0xC0FFEE } else { 0xC4E57 };
    let last = RESOLUTION - 1;

    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            // The boards. On the side they run across; on the lid they
            // run the other way, so the two faces of one block do not
            // look like the same picture twice.
            let along = if lid { x } else { y };
            let mut base = if (along / 3) % 2 == 0 { BOARD } else { BOARD_DARK };
            // A groove between every pair of them.
            if along % 3 == 0 {
                base = GROOVE;
            }

            // The frame: corner posts and rails, inset by one so the
            // block still has a dark outline of its own.
            let in_post = x <= 2 || x >= last - 2;
            let in_rail = y <= 1 || y >= last - 1;
            if in_post || in_rail {
                base = if (x + y) % 2 == 0 { POST } else { POST_DARK };
            }

            if lid {
                // Seen from above: an iron band round the rim, and the
                // hinge along the back edge.
                if x == 2 || x == last - 2 || y == 2 || y == last - 2 {
                    base = IRON_DARK;
                }
                if y <= 1 && (4..=last - 4).contains(&x) {
                    base = IRON;
                }
            } else {
                // The band the lid closes on, across the middle of the
                // face, with the latch hanging off it.
                if (7..=8).contains(&y) {
                    base = if x % 4 == 0 { IRON_DARK } else { IRON };
                }
                if (6..=10).contains(&y) && (7..=8).contains(&x) {
                    base = LATCH;
                }
            }

            // The outline goes on last so nothing draws over it.
            if x == 0 || y == 0 || x == last || y == last {
                base = POST_DARK;
            }

            let noise = (hash(seed, x, y) % 9) as i32 - 4;
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

/// Wood ash as a *floor*: pale grey powder with what did not burn still
/// in it.
///
/// Speckle alone would do for the powder and would read as concrete.
/// What makes it ash is the charcoal: a scattering of near-black flecks
/// and a few pale ones, sized so that at a glance the surface has
/// *grain* rather than noise.
///
/// This is the block, and a block of it is a drift you walk over. What
/// a handful looks like is `generate_ash_item`, and the two are
/// deliberately different pictures rather than one picture used twice --
/// see the note there.
fn generate_ash() -> RgbaImage {
    const ASH: [u8; 3] = [138, 134, 128];
    const ASH_PALE: [u8; 3] = [176, 172, 166];
    const CHAR: [u8; 3] = [46, 42, 40];
    const EMBER: [u8; 3] = [96, 74, 60];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let roll = hash(0xA54E5, x, y) % 100;
            let base = if roll < 6 {
                CHAR
            } else if roll < 12 {
                EMBER
            } else if roll < 30 {
                ASH_PALE
            } else {
                ASH
            };
            let noise = (hash(0x5EED, x, y) % 15) as i32 - 7;
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

/// Wood ash as a *thing you are carrying*: a handful of it.
///
/// **Why this is not the block texture.** A tile of ash tells you what a
/// floor of ash looks like, and at icon size that is a grey square --
/// indistinguishable from stone, from gravel and from every other grey
/// square in the pack. It is also a lie about what is in your hand: you
/// are not carrying a floor. A heap, seen against nothing, says both how
/// much of it there is and that it is loose.
///
/// Three things make it read as a heap rather than as a grey blob, and
/// taking away any one of them loses it:
///
/// * **A rounded footprint** that does not reach the edge of the tile,
///   with transparency round it. This is drawn over an inventory slot,
///   so what is not ash has to show the slot.
/// * **A dome**: pale at the crown, darker at the foot, lit from the top
///   left like every other rounded thing here (see `generate_pebble`).
/// * **A grainy edge.** A clean ellipse reads as a drawn circle; powder
///   has no edge anybody could draw, so the boundary is roughened by a
///   texel of noise.
///
/// The charcoal is what keeps it ash rather than flour, and there is
/// more of it at the foot -- the heavy bits do not stay on top of a heap
/// of powder.
fn generate_ash_item() -> RgbaImage {
    const ASH: [u8; 3] = [138, 134, 128];
    const ASH_PALE: [u8; 3] = [184, 180, 174];
    const CHAR: [u8; 3] = [46, 42, 40];
    const EMBER: [u8; 3] = [96, 74, 60];

    // Where the heap sits and how far it spreads. Low and wide, and a
    // little below the middle: a handful tipped out lands as a low cone
    // with its weight at the bottom, not as a ball in the air.
    const CX: f32 = 7.8;
    const CY: f32 = 9.6;
    const RX: f32 = 6.6;
    const RY: f32 = 5.0;

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (ox, oy) = ((fx - CX) / RX, (fy - CY) / RY);
            // Zero at the crown, one at the edge of the heap.
            let reach = (ox * ox + oy * oy).sqrt();
            let grain = (hash(0x6841, x, y) % 19) as f32 / 100.0 - 0.09;
            if reach + grain > 1.0 {
                continue; // left transparent: the slot shows through
            }

            // The dome, and the light on it. Between them these are the
            // whole of why it reads as a mound: brightness that falls
            // off toward the foot, and one more fall-off across it.
            let dome = 1.0 - reach * reach;
            let lift = (dome * 30.0) as i32 + ((-ox - oy) * 13.0) as i32;

            let roll = hash(0xA54E5, x, y) % 100;
            let heavy = 4 + (reach * 10.0) as u32;
            let base = if roll < heavy {
                CHAR
            } else if roll < heavy + 5 {
                EMBER
            } else if roll < 40 {
                ASH_PALE
            } else {
                ASH
            };

            let noise = (hash(0x5EED, x, y) % 15) as i32 - 7;
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(base[0] as i32 + lift + noise),
                    clamp_u8(base[1] as i32 + lift + noise),
                    clamp_u8(base[2] as i32 + lift + noise),
                    255,
                ]),
            );
        }
    }
    img
}

fn generate_chest_side() -> RgbaImage {
    generate_chest(false)
}

fn generate_chest_top() -> RgbaImage {
    generate_chest(true)
}

/// A backpack: what a player leaves behind where they died.
///
/// It stands next to the chest and has to *not* be the chest. Both are
/// brown boxes you open, so telling them apart cannot rest on the
/// colour -- the chest is a frame of posts and rails around straight
/// boards, and this is the opposite: no straight lines except the two
/// straps, a flap with a rounded corner, and a bulge of a body that is
/// darker at the edges than in the middle. A crate is built; a bag is
/// stuffed, and at sixteen pixels the difference between them is that
/// one has corners and the other does not.
///
/// It also has to be found on grass and on dirt, which is where players
/// die. Hence the tan of the straps and the brass of the buckles: two
/// tones nothing in the terrain palette has, in a pattern (two vertical
/// bands) nothing else in the game draws.
///
/// `lid` draws the top face, which is the flap seen from above with the
/// carry handle on it.
fn generate_backpack(lid: bool) -> RgbaImage {
    const LEATHER: [u8; 3] = [126, 84, 48];
    const LEATHER_LIT: [u8; 3] = [150, 104, 62];
    const LEATHER_DARK: [u8; 3] = [86, 56, 30];
    const FLAP: [u8; 3] = [104, 66, 38];
    const STRAP: [u8; 3] = [72, 46, 26];
    const BRASS: [u8; 3] = [198, 158, 74];
    const STITCH: [u8; 3] = [176, 146, 96];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let seed = if lid { 0xBA61D } else { 0xBA65E };
    let last = RESOLUTION - 1;
    let middle = (RESOLUTION / 2) as i32;

    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            // The bulge: lit down the middle and shaded towards both
            // sides, which is the whole of what makes a bag read as
            // stuffed rather than as a panel.
            let from_middle = (x as i32 - middle).abs();
            let mut base = if from_middle <= 2 {
                LEATHER_LIT
            } else if from_middle >= 6 {
                LEATHER_DARK
            } else {
                LEATHER
            };

            if lid {
                // Seen from above the flap is the whole face, so the
                // shading is fore-and-aft instead of side to side.
                base = if (5..=10).contains(&y) { FLAP } else { LEATHER };
                // The carry handle: a loop standing up off the flap,
                // drawn as its two uprights and the bar between them.
                if (6..=9).contains(&y) && (6..=9).contains(&x)
                    && (y == 6 || y == 9 || x == 6 || x == 9)
                {
                    base = STRAP;
                }
            } else {
                // The flap over the mouth of the bag, with its stitched
                // edge. It hangs a row lower in the middle than at the
                // sides, which is the rounded corner.
                let flap_bottom = if from_middle <= 4 { 6 } else { 5 };
                if (y as i32) <= flap_bottom {
                    base = FLAP;
                }
                if y as i32 == flap_bottom {
                    base = STITCH;
                }
                // A stitched pocket on the belly, so the lower two
                // thirds are not an empty field.
                let pocket = (9..=13).contains(&y) && (5..=10).contains(&x);
                if pocket && (y == 9 || y == 13 || x == 5 || x == 10) {
                    base = STITCH;
                }
            }

            // The two straps, running the whole height of both faces --
            // the one thing that is the same picture from every angle,
            // and the reason the block is recognisable from above.
            if (3..=4).contains(&x) || (11..=12).contains(&x) {
                base = STRAP;
                // A buckle on each, just below the flap on the side and
                // level with the handle on the lid.
                if (7..=8).contains(&y) {
                    base = BRASS;
                }
            }

            // The outline goes on last so nothing draws over it.
            if x == 0 || y == 0 || x == last || y == last {
                base = LEATHER_DARK;
            }

            let noise = (hash(seed, x, y) % 9) as i32 - 4;
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

fn generate_backpack_side() -> RgbaImage {
    generate_backpack(false)
}

fn generate_backpack_top() -> RgbaImage {
    generate_backpack(true)
}

/// Flint: two nodules, angular where a pebble is round.
///
/// Drawn as facets rather than as discs, and that is the whole of what
/// separates it from the loose stone beside it at this size. Flint
/// fractures conchoidally -- it comes apart in shells with edges you
/// could cut yourself on -- so the shape is a couple of straight-sided
/// chips with one lit face each, and the colour is near-black with a
/// blue-grey sheen rather than the warm grey of ordinary rock.
fn generate_flint() -> RgbaImage {
    const FLINT: [u8; 3] = [58, 58, 66];
    const FLINT_LIT: [u8; 3] = [96, 98, 110];
    const FLINT_DARK: [u8; 3] = [34, 34, 40];
    const EDGE: [u8; 3] = [132, 136, 148];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // (centre x, centre y, half-width, half-height). Two of them, apart,
    // and deliberately different sizes -- a pair of identical chips
    // reads as a pattern rather than as something lying on the ground.
    let chips: [(i32, i32, i32, i32); 2] = [(5, 6, 3, 3), (11, 11, 2, 2)];
    for (cx, cy, half_w, half_h) in chips {
        for dy in -half_h..=half_h {
            for dx in -half_w..=half_w {
                // A diamond rather than a rectangle: straight edges
                // meeting at points, which is how a struck flake looks.
                let reach = (dx.abs() * half_h + dy.abs() * half_w) as f32
                    / (half_w * half_h) as f32;
                if reach > 1.15 {
                    continue;
                }
                // One face of each chip catches the light, and the
                // ridge between the two faces is the sharp edge.
                let facet = dx + dy;
                let base = if facet < -1 {
                    FLINT_LIT
                } else if facet > 1 {
                    FLINT_DARK
                } else {
                    FLINT
                };
                // Only the lit side of the rim catches enough to show:
                // an outline all the way round is a sticker, not a
                // stone, and at sixteen pixels it is most of the tile.
                let base = if reach > 0.92 && facet < 0 { EDGE } else { base };
                let noise =
                    (hash(0xF117, (cx + dx).max(0) as u32, (cy + dy).max(0) as u32) % 9) as i32 - 4;
                put_opaque(
                    &mut img,
                    cx + dx,
                    cy + dy,
                    [
                        clamp_u8(base[0] as i32 + noise),
                        clamp_u8(base[1] as i32 + noise),
                        clamp_u8(base[2] as i32 + noise),
                    ],
                );
            }
        }
    }
    img
}

/// A cactus: a ribbed green column. Opaque, unlike the plants -- it is a
/// block you walk into.
fn generate_cactus() -> RgbaImage {
    const FLESH: [u8; 3] = [58, 116, 60];
    const RIB: [u8; 3] = [44, 94, 48];
    const SPINE: [u8; 3] = [214, 216, 178];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            // Vertical ribs, and a darker margin so a column of cactus
            // reads as segmented rather than as one green post.
            let ribbed = matches!(x % 5, 0) || x == 0 || x == RESOLUTION - 1;
            let mut colour = if ribbed { RIB } else { FLESH };
            // Spines along the ribs, every few rows.
            if ribbed && y % 4 == 2 && x > 0 && x < RESOLUTION - 1 {
                colour = SPINE;
            }
            let noise = (hash(0x5EED, x, y) % 9) as i32 - 4;
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(colour[0] as i32 + noise),
                    clamp_u8(colour[1] as i32 + noise),
                    clamp_u8(colour[2] as i32 + noise),
                    255,
                ]),
            );
        }
    }
    img
}

// ---- ore, metal and tools ----
//
// Three families, three generators, and one table each. They are
// parameterised by colour rather than written out one function per file
// because that is what they actually are: an ore is stone with a colour
// in it, an ingot is a bar of a colour, a pick is a haft with a head of
// a colour. Thirteen hand-drawn functions differing only in three bytes
// would be thirteen places to fix the next time the shape is wrong.

/// (file, ore colour, its shadow). The base rock is the same for all of
/// them -- see `generate_ore`.
const ORES: &[(&str, [u8; 3], [u8; 3])] = &[
    // Coal: no metallic sheen at all, so the "shadow" is barely darker
    // than the flecks. It is soot, not metal.
    ("coal_ore.png", [32, 30, 30], [18, 17, 17]),
    // Copper as it is *found*: green, not orange. Native copper weathers
    // to malachite, and a hillside speckled with green is a much better
    // thing to spot from a distance than one speckled with the colour of
    // the ingot -- which is what the smelting is for.
    ("copper_ore.png", [86, 154, 118], [46, 104, 82]),
    // Cassiterite: dark, almost black-brown, with a resinous glint. It
    // is deliberately the least eye-catching of the four, because
    // finding tin is supposed to be the hard part.
    ("tin_ore.png", [74, 62, 54], [44, 36, 32]),
    ("iron_ore.png", [178, 150, 132], [128, 100, 84]),
];

/// (file, metal, its highlight).
const INGOTS: &[(&str, [u8; 3], [u8; 3])] = &[
    ("copper_ingot.png", [186, 108, 62], [226, 156, 104]),
    ("tin_ingot.png", [186, 190, 198], [226, 230, 236]),
    // Bronze sits between its parents, which is the point: it should
    // read as copper that has been *changed* rather than as a third
    // unrelated metal.
    ("bronze_ingot.png", [176, 134, 66], [214, 178, 106]),
    ("iron_ingot.png", [154, 152, 148], [198, 196, 192]),
];

/// (file, head, its highlight).
///
/// One row, and the generator is still parameterised by colour. The
/// metal picks it used to have are gone from the game (see
/// `primitive_shared::types::BLOCK_FLINT_PICKAXE`) and the parameters
/// stay, because the day a copper pick comes back this is a line rather
/// than a function -- which is the whole argument for a table.
const PICKAXES: &[(&str, [u8; 3], [u8; 3])] = &[
    // The flint head is not metal: near-black with a blue-grey sheen,
    // the same colours the loose nodules are drawn in, so a player can
    // see what their first tool is made of.
    ("flint_pickaxe.png", FLINT, FLINT_EDGE),
];

// ---- the stone age: parts, and the two tools that are not the pick ----
//
// Seven pictures for one chain, and they have to be legible *as a
// chain*: a player holding a flake, a haft and a head has three things
// in the pack that are all obviously halfway to something. So they share
// a palette with the nodule they came from (`generate_flint`) and with
// each other, and the difference between them is silhouette alone --
// which is all there is to go on in a slot a centimetre across.

/// Knapped flint, the same near-black with a blue-grey sheen the loose
/// nodules are drawn in.
const FLINT: [u8; 3] = [58, 58, 66];
const FLINT_LIT: [u8; 3] = [96, 98, 110];
const FLINT_DARK: [u8; 3] = [34, 34, 40];
/// The struck edge: the one part of a flint object that is genuinely
/// bright, because a fresh fracture is glassy.
const FLINT_EDGE: [u8; 3] = [132, 136, 148];
/// A trimmed haft: paler than the branch it was, because what a haft is
/// is a branch with the bark taken off it.
const HAFT: [u8; 3] = [158, 122, 76];
const HAFT_DARK: [u8; 3] = [118, 88, 52];
/// The lashing. Two or three pixels of straw where head meets haft, and
/// they are there because the fibre is a third of what a tool costs --
/// a picture that did not show it would be lying about the recipe.
const LASHING: [u8; 3] = [198, 176, 108];

const KNAPPED: &[Drawn] = &[
    ("flint_flake.png", generate_flake),
    ("worked_stick.png", generate_worked_stick),
    ("flint_knife_head.png", generate_knife_head),
    ("flint_axe_head.png", generate_axe_head),
    ("flint_pick_head.png", generate_pick_head),
    ("flint_knife.png", generate_knife),
    ("flint_axe.png", generate_axe),
];

/// One pixel of flint, speckled the way the nodules are.
///
/// The speckle is what keeps a shape drawn in three flat tones from
/// looking like a logo. Same hash, same amplitude as `generate_flint`.
fn flint_pixel(img: &mut RgbaImage, x: i32, y: i32, base: [u8; 3]) {
    let noise = (hash(0xF1A6, x.max(0) as u32, y.max(0) as u32) % 9) as i32 - 4;
    put_opaque(
        img,
        x,
        y,
        [
            clamp_u8(base[0] as i32 + noise),
            clamp_u8(base[1] as i32 + noise),
            clamp_u8(base[2] as i32 + noise),
        ],
    );
}

/// A shape given as one horizontal run per row: `(y, first x, last x)`.
///
/// Spans rather than a formula, and deliberately. At sixteen pixels a
/// curve is four decisions, not a function -- every attempt to derive
/// these shapes from a radius produced something that was symmetrical
/// and read as a pill. A flake is not symmetrical.
type Span = (i32, i32, i32);

/// Fills a run of spans as knapped flint, lit from the left.
///
/// The leftmost pixel of each row is the struck edge and the rightmost
/// is in shadow, which between them are what make a flat silhouette read
/// as something with two faces meeting at a line.
fn knap(img: &mut RgbaImage, spans: &[Span], edge_on_the_left: bool) {
    for &(y, from, to) in spans {
        for x in from..=to {
            let along = if to > from {
                (x - from) as f32 / (to - from) as f32
            } else {
                0.0
            };
            let along = if edge_on_the_left { along } else { 1.0 - along };
            let base = if along < 0.18 {
                FLINT_EDGE
            } else if along < 0.55 {
                FLINT_LIT
            } else if along < 0.85 {
                FLINT
            } else {
                FLINT_DARK
            };
            flint_pixel(img, x, y, base);
        }
    }
}

/// A struck flake: one shard, leaf-shaped, with an edge down one side.
///
/// Small on purpose -- it fills about a third of the tile, against the
/// nodule's two chips and the heads' solid mass. Three of these come off
/// one nodule, and the picture should say so.
fn generate_flake() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // A teardrop: widest a third of the way down, where the blow landed.
    const SHARD: &[Span] = &[
        (3, 8, 9),
        (4, 7, 10),
        (5, 6, 11),
        (6, 5, 11),
        (7, 5, 12),
        (8, 5, 11),
        (9, 6, 11),
        (10, 6, 10),
        (11, 7, 9),
        (12, 8, 8),
    ];
    knap(&mut img, SHARD, true);
    img
}

/// A haft: a branch pared straight, with the pale wood showing where the
/// bark came off.
///
/// The same diagonal every tool in this set is drawn on, and the same
/// two-pixel width as the raw stick -- so the two sit next to each other
/// in the pack and differ by *taper and colour* rather than by shape.
/// That is the difference the recipe made, and it is the difference the
/// picture should show.
fn generate_worked_stick() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for step in 0..13 {
        let x = 2 + step;
        let y = 14 - step;
        put_opaque(&mut img, x, y, HAFT);
        put_opaque(&mut img, x + 1, y, HAFT_DARK);
        // Whittled facets: every third pixel down the lit side is a
        // shaving mark. Without them a trimmed haft is a smooth bar,
        // which is what a machine makes, not a knife.
        if step % 3 == 1 {
            put_opaque(&mut img, x, y - 1, HAFT_DARK);
        }
    }
    img
}

/// A knife head: a narrow blade with a straight back and a curved edge.
///
/// The smallest of the three heads, because it costs the least, and the
/// only one that is longer than it is thick.
fn generate_knife_head() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // Three pixels wide and eleven long, running corner to corner. The
    // first attempt was five wide and read as a small axe head -- at
    // this size the *ratio* is the only thing that says "blade", and
    // anything thicker than a third of its length is a lump of stone.
    const BLADE: &[Span] = &[
        (2, 11, 13),
        (3, 10, 12),
        (4, 9, 12),
        (5, 8, 11),
        (6, 7, 10),
        (7, 6, 9),
        (8, 5, 8),
        (9, 4, 7),
        (10, 3, 6),
        (11, 3, 5),
        (12, 3, 4),
    ];
    knap(&mut img, BLADE, true);
    img
}

/// An axe head: a wedge, butt at the top and the cutting edge fanning
/// out at the bottom.
///
/// Fat where the knife is thin. An axe works by *mass* -- it splits with
/// weight behind a short edge -- and the silhouette is the only place
/// that can be said.
fn generate_axe_head() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const WEDGE: &[Span] = &[
        (3, 6, 9),
        (4, 5, 10),
        (5, 5, 10),
        (6, 4, 11),
        (7, 4, 11),
        (8, 3, 12),
        (9, 3, 12),
        (10, 2, 13),
        (11, 3, 12),
        (12, 5, 10),
    ];
    knap(&mut img, WEDGE, true);
    img
}

/// A pick head: a long bar tapering to a point at each end.
///
/// Two points rather than one, for the reason `generate_pickaxe` gives:
/// one point is a hoe and two is unmistakably a pick. Drawn across the
/// tile rather than along the diagonal, so it does not read as the
/// finished pick with the haft rubbed out.
fn generate_pick_head() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const BAR: &[Span] = &[
        (5, 7, 9),
        (6, 6, 10),
        (7, 3, 13),
        (8, 1, 14),
        (9, 3, 13),
        (10, 6, 10),
        (11, 7, 9),
    ];
    knap(&mut img, BAR, true);
    img
}

/// Draws the haft every finished tool shares, bottom left to middle.
///
/// One function because it is one object: the tools differ by what is
/// lashed to the top of it, and a haft drawn three times would drift
/// three ways.
fn tool_haft(img: &mut RgbaImage, steps: i32) {
    for step in 0..steps {
        let x = 2 + step;
        let y = 14 - step;
        put_opaque(img, x, y, HAFT);
        put_opaque(img, x + 1, y, HAFT_DARK);
    }
}

/// A flint knife: a short haft with a blade running on from it.
///
/// One straight line from the butt to the tip, which is what a knife is,
/// and what tells it apart from the axe at a glance -- the axe's head
/// sticks out sideways and the knife's does not.
fn generate_knife() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    tool_haft(&mut img, 5);
    // The lashing, at the join: a knife is bound at one point because
    // that is all the leverage there is on one.
    for x in 6..=8 {
        put_opaque(&mut img, x, 10 - (x - 6), LASHING);
    }
    const BLADE: &[Span] = &[
        (2, 11, 12),
        (3, 10, 12),
        (4, 9, 11),
        (5, 8, 11),
        (6, 8, 10),
        (7, 7, 9),
        (8, 7, 8),
    ];
    knap(&mut img, BLADE, true);
    img
}

/// A flint axe: the wedge lashed across the top of a full-length haft.
///
/// The head sits *beside* the top of the haft rather than on it, because
/// that is how a hafted axe is actually made -- the stone goes into a
/// split or a socket in the wood -- and because a head balanced on the
/// end reads as a hammer.
fn generate_axe() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    tool_haft(&mut img, 10);
    const WEDGE: &[Span] = &[
        (1, 9, 11),
        (2, 8, 12),
        (3, 8, 13),
        (4, 7, 13),
        (5, 8, 12),
        (6, 9, 11),
    ];
    knap(&mut img, WEDGE, true);
    // Bound twice, above and below the head: an axe is swung, and
    // everything the swing does to the stone goes into the binding.
    for step in 0..3 {
        put_opaque(&mut img, 8 + step, 6 - step, LASHING);
        put_opaque(&mut img, 7 + step, 8 - step, LASHING);
    }
    img
}

/// An ore: the stone texture with nodules of something in it.
///
/// **Drawn on the stone that is actually in the folder**, read off disk,
/// rather than on the placeholder stone this file would generate. The
/// two are not the same image -- somebody has replaced the stone with
/// real art -- and an ore whose rock is a different grey from the rock
/// around it is the one mistake in this whole set that would be visible
/// from across a cave. The generated stone is the fallback for a folder
/// that has none yet.
fn generate_ore(dir: &std::path::Path, filename: &str, ore: [u8; 3], shadow: [u8; 3]) -> RgbaImage {
    let mut img = match image::open(dir.join("stone.png")) {
        Ok(stone) => image::imageops::resize(
            &stone.to_rgba8(),
            RESOLUTION,
            RESOLUTION,
            image::imageops::FilterType::Nearest,
        ),
        Err(_) => generate([128, 128, 130], 14, "stone.png"),
    };

    let seed = filename
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    // Five blobs, placed by the hash and never touching the tile's edge.
    // Not on the edge because these tile against each other: a nodule
    // running off one side turns into a seam across a wall of ore, and
    // the eye finds a seam immediately.
    for blob in 0..5u32 {
        let cx = 3 + (hash(seed, blob, 0) % 10) as i32;
        let cy = 3 + (hash(seed, blob, 1) % 10) as i32;
        let radius = 1 + (hash(seed, blob, 2) % 2) as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs() + dy.abs() > radius + 1 {
                    continue; // a rounded lump rather than a square
                }
                // Lit from the top left, like everything else here.
                let base = if dx + dy > 0 { shadow } else { ore };
                let noise = (hash(seed, (cx + dx) as u32, (cy + dy) as u32) % 11) as i32 - 5;
                put_opaque(
                    &mut img,
                    cx + dx,
                    cy + dy,
                    [
                        clamp_u8(base[0] as i32 + noise),
                        clamp_u8(base[1] as i32 + noise),
                        clamp_u8(base[2] as i32 + noise),
                    ],
                );
            }
        }
    }
    img
}

/// An ingot: a bar seen at a low angle, with a lit top face.
///
/// A trapezoid rather than a rectangle, and that single choice is what
/// makes it read as a solid object rather than as a swatch of colour.
/// The top face is the lighter one because everything in this set is lit
/// from above.
fn generate_ingot(metal: [u8; 3], highlight: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let shadow = [
        clamp_u8(metal[0] as i32 - 46),
        clamp_u8(metal[1] as i32 - 46),
        clamp_u8(metal[2] as i32 - 46),
    ];

    // The top face: a narrow band, inset from the front face on both
    // sides, which is what a bar looks like from slightly above.
    for x in 5..=10 {
        for y in 5..=6 {
            put_opaque(&mut img, x, y, if y == 5 { highlight } else { metal });
        }
    }
    // The front face, splaying outwards as it comes down.
    for y in 7..=10 {
        let spread = (y - 6) / 2;
        for x in (4 - spread)..=(11 + spread) {
            let base = if y >= 10 { shadow } else { metal };
            // A single lit pixel where the two faces meet, so the edge
            // of the bar catches the light rather than being a line.
            let base = if y == 7 && x > 4 && x < 11 { highlight } else { base };
            put_opaque(&mut img, x, y, base);
        }
    }
    img
}

/// A pickaxe: a haft running corner to corner with a head across the top.
///
/// Diagonal because a vertical tool at sixteen pixels is a stick with a
/// blob on it -- the diagonal is what makes the silhouette read as a
/// pick at the size these are actually seen, which is a slot in the
/// hotbar about a centimetre across.
fn generate_pickaxe(head: [u8; 3], highlight: [u8; 3]) -> RgbaImage {
    // The haft is the shared one -- see `HAFT`. It used to be a darker
    // brown of its own, which was fine while the pick was the only tool
    // and became a lie the moment there were three: they are made from
    // the same worked stick, and three shades of it in one row of the
    // pack reads as three different sticks.
    let shadow = [
        clamp_u8(head[0] as i32 - 40),
        clamp_u8(head[1] as i32 - 40),
        clamp_u8(head[2] as i32 - 40),
    ];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);

    // The haft: two pixels wide, from the bottom left up to where the
    // head is lashed onto it.
    for step in 0..10 {
        let x = 2 + step;
        let y = 14 - step;
        put_opaque(&mut img, x, y, HAFT);
        put_opaque(&mut img, x + 1, y, HAFT_DARK);
    }

    // The head: a bar lying across the top of the haft with both ends
    // turned down. Two tips rather than one, because a single point is a
    // hoe and two is unmistakably a pick -- and the silhouette is all
    // there is to go on at this size.
    for x in 8..=12 {
        put_opaque(&mut img, x, 2, head);
    }
    for x in 6..=14 {
        put_opaque(&mut img, x, 3, head);
    }
    for x in [6, 7, 13, 14] {
        put_opaque(&mut img, x, 4, head);
    }
    // Lit along the top and shadowed under the tips: four pixels each,
    // and between them they are what makes it read as metal (or, for the
    // first tier, as a knapped edge).
    for x in 9..=11 {
        put_opaque(&mut img, x, 2, highlight);
    }
    put_opaque(&mut img, 6, 3, highlight);
    for x in [6, 7, 13, 14] {
        put_opaque(&mut img, x, 5, shadow);
    }
    img
}

/// Writes one fully opaque pixel, ignoring anything off the tile.
fn put_opaque(img: &mut RgbaImage, x: i32, y: i32, colour: [u8; 3]) {
    if x < 0 || y < 0 || x >= RESOLUTION as i32 || y >= RESOLUTION as i32 {
        return;
    }
    img.put_pixel(
        x as u32,
        y as u32,
        Rgba([colour[0], colour[1], colour[2], 255]),
    );
}

// ---- breaking overlay ----
//
// `break.0.png` .. `break.4.png`: the cracks drawn over a block while it
// is being mined. Transparent everywhere except the cracks themselves,
// because they are laid over the block's own texture rather than
// replacing it -- see `mining::build_break_mesh_into`.

/// How many stages of damage there are. The mining progress bar is cut
/// into this many steps.
pub const BREAK_STAGES: u32 = 5;

/// Draws one stage.
///
/// The cracks grow rather than being redrawn: stage *n* contains every
/// line stage *n-1* had, extended, plus a new one. Independent patterns
/// per stage would flicker into each other as the block breaks, and the
/// eye reads that as the texture changing rather than as damage
/// spreading.
fn generate_break(stage: u32) -> RgbaImage {

    /// Cracks radiate from here, all of them, at every stage.
    const ARMS: u32 = 6;

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let centre = RESOLUTION as f32 / 2.0;

    // What each stage adds: more arms, and each one reaching further.
    //
    // Bounded well short of the tile's edge on purpose. The first
    // version reached almost to the corners with eight arms and a
    // widened edge on each, and the result was a block that went *black*
    // as it broke rather than one that cracked -- the overlay stopped
    // being a marking on the block and became a coat of paint. Damage
    // has to read against the block, which means most of the block has
    // to still be there.
    let arms = (2 + stage).min(ARMS);
    let reach = 0.22 + stage as f32 * 0.10;

    for arm in 0..arms {
        // Evenly spread and then nudged, so the star is not a snowflake
        // but the arms still cover the tile instead of bunching.
        let seed = 0x9E37_79B9u32.wrapping_add(arm.wrapping_mul(0x85EB_CA6B));
        let spread = std::f32::consts::TAU / ARMS as f32;
        let heading = arm as f32 * spread + ((hash(seed, 0, 0) % 100) as f32 / 100.0 - 0.5) * spread;

        draw_crack(&mut img, centre, centre, heading, reach * RESOLUTION as f32, seed, stage);

        // Branches, only at the end: a crack that forks reads as
        // something splitting, where a straight line reads as a
        // scratch. Earlier than this and the tile fills up.
        if stage >= 3 && arm % 2 == 0 {
            let from = reach * RESOLUTION as f32 * 0.45;
            let (bx, by) = (
                centre + heading.cos() * from,
                centre + heading.sin() * from,
            );
            draw_crack(
                &mut img,
                bx,
                by,
                heading + 0.8,
                reach * RESOLUTION as f32 * 0.45,
                seed ^ 0x5BF0_3635,
                stage,
            );
        }
    }
    img
}

/// One crack: a walk in roughly one direction, wobbling as it goes.
///
/// The wobble is a function of how far along the walk is rather than an
/// accumulating turn. An accumulating one curls -- every crack ends up
/// spiralling the same way round, which at 16x16 looks like a bad
/// texture rather than like breakage.
#[allow(clippy::too_many_arguments)]
fn draw_crack(
    img: &mut RgbaImage,
    mut x: f32,
    mut y: f32,
    heading: f32,
    length: f32,
    seed: u32,
    stage: u32,
) {
    /// Dark, but not black, and not quite opaque: the crack is a mark
    /// *on* a block, and the block's own texture showing faintly through
    /// it is what keeps it looking like stone that has been hit rather
    /// than a hole cut in the world.
    const CRACK: [u8; 3] = [38, 34, 32];
    const CRACK_ALPHA: u8 = 215;
    /// A softer edge beside the crack, so it does not look like a line
    /// drawn with a single-pixel pen. Faint -- this is the part that
    /// doubled the ink in the first version.
    const EDGE: [u8; 3] = [70, 64, 60];
    const EDGE_ALPHA: u8 = 110;

    let phase = (hash(seed, 3, 3) % 100) as f32 / 100.0 * std::f32::consts::TAU;
    let steps = (length / 0.6).max(1.0) as u32;
    for step in 0..steps {
        let t = step as f32;
        let angle = heading + ((t * 0.55 + phase).sin()) * 0.45;
        x += angle.cos() * 0.6;
        y += angle.sin() * 0.6;
        if !(0.0..RESOLUTION as f32).contains(&x) || !(0.0..RESOLUTION as f32).contains(&y) {
            return;
        }
        put(img, x as u32, y as u32, CRACK, CRACK_ALPHA);
        // A crack widens where it started and thins out towards the
        // end, and only once there is real damage to widen. One side
        // only: widening both turns every line into a two-pixel bar,
        // which at 16x16 is a quarter of the tile per crack.
        if stage >= 3 && t < steps as f32 * 0.45 {
            put(img, x as u32 + 1, y as u32, EDGE, EDGE_ALPHA);
        }
    }
}

/// Writes a pixel unless something darker is already there, so crossing
/// cracks do not thin each other out.
fn put(img: &mut RgbaImage, x: u32, y: u32, colour: [u8; 3], alpha: u8) {
    if x >= RESOLUTION || y >= RESOLUTION {
        return;
    }
    let existing = img.get_pixel(x, y);
    if existing[3] >= alpha {
        return;
    }
    img.put_pixel(x, y, Rgba([colour[0], colour[1], colour[2], alpha]));
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

const CLOUDS: &str = "sky_clouds.png";

/// How wide the cloud picture is. Bigger than a block texture because it
/// is not a block texture: it is stretched over the whole sky, and one
/// cloud pixel has to land on a couple of texels or the grid the clouds
/// are drawn on comes out finer than the picture it is drawn from.
const CLOUD_RESOLUTION: u32 = 512;

/// How much of the cloud layer's own coordinate space one tile covers.
///
/// The layer scales world metres by 0.0022, so six of these units is
/// about 2,700 blocks: far enough that the repeat is not something you
/// notice from the ground, near enough that the picture still has
/// texels to spare for the finest octave.
///
/// It also fixes the *feature size*. The field below is built on a
/// lattice of one unit, the same lattice the shader's noise used, so a
/// cloud comes out the size a cloud used to be.
const CLOUD_TILE: u32 = 6;

/// The cloud field, baked: **red** is the density the shader's threshold
/// cuts, and **green** and **blue** are the two fields that bend a
/// cloud's outline.
///
/// **This is the same field the shader used to compute per pixel**, and
/// deliberately so. It was four octaves of value noise for the density,
/// two more for the warp and four more for the self-shadowing -- about
/// forty hashes on every pixel of sky, in the one pass that runs for
/// every pixel the terrain did not cover. Here it is three texture
/// fetches, and the picture can be replaced by a painted one without a
/// shader being touched.
///
/// **The distribution is copied, not improved.** The octave amplitudes
/// are the shader's -- a half, a quarter, an eighth, a sixteenth, summed
/// and *not* normalised -- so the field runs from zero to about 0.94
/// with its middle near 0.47, exactly where the thresholds in `fs_sky`
/// expect it. Normalising it to fill nought-to-one is the obvious tidy
/// thing to do and it is wrong: the cut at a given cloudiness then lands
/// far up the distribution, only the highest peaks survive it, and a
/// cloudy sky comes out as white specks. That was tried.
///
/// Three channels because the shader wants three fields and a fetch
/// brings back four numbers whether they are wanted or not. The warp is
/// already inside the pixel that had to be read anyway.
///
/// **Tiling is a property of the lattice, not of the image.** A field
/// that does not wrap shows a seam every time it repeats across the sky,
/// and a seam in a cloud layer is a straight line running from one
/// horizon to the other. That is why the lattice takes its coordinates
/// modulo the period at every octave, and why the octaves double exactly
/// rather than by the shader's 2.03 -- an irrational step cannot wrap.
fn generate_clouds() -> RgbaImage {
    let mut img = RgbaImage::new(CLOUD_RESOLUTION, CLOUD_RESOLUTION);
    for y in 0..CLOUD_RESOLUTION {
        for x in 0..CLOUD_RESOLUTION {
            let p = (x as f32, y as f32);
            let mut density = 0.0;
            let mut amplitude = 0.5;
            for octave in 0..4 {
                density += cloud_noise(p, CLOUD_TILE << octave, 0x5C10 + octave * 977) * amplitude;
                amplitude *= 0.5;
            }
            // The two warp fields, on the same lattice as the base
            // octave: the shader samples them at a coordinate of their
            // own, so what matters is that they vary at the same rate
            // the old single-octave `noise2` did.
            let warp_a = cloud_noise(p, CLOUD_TILE, 0x11A7);
            let warp_b = cloud_noise(p, CLOUD_TILE, 0x41B3);
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8((density * 255.0) as i32),
                    clamp_u8((warp_a * 255.0) as i32),
                    clamp_u8((warp_b * 255.0) as i32),
                    255,
                ]),
            );
        }
    }
    img
}

/// Value noise on a lattice of `period` cells that wraps at the edge of
/// the tile.
fn cloud_noise(p: (f32, f32), period: u32, seed: u32) -> f32 {
    let scale = period as f32 / CLOUD_RESOLUTION as f32;
    let (px, py) = (p.0 * scale, p.1 * scale);
    let (ix, iy) = (px.floor(), py.floor());
    let (fx, fy) = (px - ix, py - iy);
    // Smoothstep, so the lattice does not show as a grid of creases.
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let corner = |dx: i32, dy: i32| {
        let cx = (ix as i32 + dx).rem_euclid(period as i32) as u32;
        let cy = (iy as i32 + dy).rem_euclid(period as i32) as u32;
        (hash(seed, cx, cy) % 4096) as f32 / 4095.0
    };
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * ux;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * ux;
    top + (bottom - top) * uy
}
