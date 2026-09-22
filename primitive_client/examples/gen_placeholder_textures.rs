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
    // The rock under the soil, in three kinds, and the bog's peat: plain
    // speckled stone, each its own colour, because the colour is the
    // whole of what tells a player which country they are cutting into.
    // See `types::BLOCK_SANDSTONE`.
    ("terrain/sandstone.png", [204, 180, 130], 12),
    ("terrain/limestone.png", [198, 192, 174], 10),
    ("terrain/granite.png", [154, 142, 140], 24),
    ("terrain/peat.png", [70, 52, 36], 14),
    ("terrain/grass_top.png", [86, 148, 62], 18),
    ("terrain/dirt.png", [123, 88, 58], 16),
    ("terrain/stone.png", [128, 128, 130], 14),
    ("terrain/sand.png", [214, 201, 148], 12),
    ("terrain/snow.png", [238, 243, 248], 8),
    ("terrain/water.png", [58, 108, 190], 10),
    ("terrain/log_top.png", [140, 104, 62], 20),
    ("plants/leaves.png", [58, 122, 52], 26),
    ("terrain/glowstone.png", [232, 200, 108], 30),
    ("terrain/planks.png", [166, 128, 78], 14),
    // Wet riverbank earth: grey-blue and almost even, because clay is
    // the one soil with no grain in it.
    ("pottery/clay.png", [142, 146, 158], 8),
    // Stones of every size with nothing holding them together, so the
    // speckle is the strongest in the set -- gravel that is not noisy
    // reads as concrete.
    ("terrain/gravel.png", [122, 118, 114], 34),
];

/// Two-tone textures for the sides of blocks whose top differs from
/// their flanks: grass fading into the dirt below it, and the bark of a
/// log against its cut end. This is what the per-face texture system in
/// `texture.rs` exists for.
#[allow(clippy::type_complexity)] // a table, and the tuple is its columns
const SIDE_TEXTURES: &[(&str, [u8; 3], [u8; 3], u32, i32)] = &[
    // (file, top colour, bottom colour, how many rows of top colour, speckle)
    ("terrain/grass_side.png", [86, 148, 62], [123, 88, 58], 5, 16),
    ("terrain/log_side.png", [104, 74, 44], [88, 62, 38], 16, 22),
];

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures"));
    std::fs::create_dir_all(&dir)?;

    // `--force` anywhere in the arguments; anything else is a filename.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let forced = args.iter().any(|a| a == "--force");
    let only: Option<String> = args.into_iter().find(|a| a != "--force");
    // Naming one file is itself the second ask -- see `write`.
    let force = forced || only.is_some();
    let wanted = |name: &str| only.as_deref().is_none_or(|o| o == name);
    let mut written = 0;

    for (filename, base, speckle) in TEXTURES {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate(*base, *speckle, filename), force)?;
        written += 1;
    }

    for (filename, top, bottom, top_rows, speckle) in SIDE_TEXTURES {
        if !wanted(filename) {
            continue;
        }
        let img = generate_two_tone(*top, *bottom, *top_rows, *speckle, filename);
        write(&dir, filename, img, force)?;
        written += 1;
    }

    if wanted(WORKBENCH) {
        write(&dir, WORKBENCH, generate_workbench(), force)?;
        written += 1;
    }

    for (filename, generate) in PLANTS {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate(), force)?;
        written += 1;
    }

    for (filename, ore, shadow) in ORES {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_ore(&dir, filename, *ore, *shadow), force)?;
        written += 1;
    }

    for (filename, metal, highlight) in INGOTS {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_ingot(*metal, *highlight), force)?;
        written += 1;
    }

    for (filename, head, highlight) in PICKAXES {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_pickaxe(*head, *highlight), force)?;
        written += 1;
    }

    for (filename, generate) in KNAPPED {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate(), force)?;
        written += 1;
    }

    // ---- 1.5, and the kiln 1.6 added ----
    for (filename, generate) in GROWN
        .iter()
        .chain(FORAGED)
        .chain(CARRIED)
        .chain(FIRED)
        .chain(WORN)
        .chain(REDRAWN)
        .chain(TORCH_FLAMES)
        .chain(BUTCHERED)
        .chain(HAFTED)
        .chain(REPAINTED)
        .chain(PALM_ENDS)
        .chain(CORPSE_PICTURES)
    {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate(), force)?;
        written += 1;
    }

    if wanted(BACKPACK_SIDE_FLAT) {
        write(&dir, BACKPACK_SIDE_FLAT, generate_backpack_side_flat(&dir), force)?;
        written += 1;
    }

    if wanted(GLUED_AXE) {
        write(&dir, GLUED_AXE, generate_glued_axe(&dir), force)?;
        written += 1;
    }

    // The fir and the saxaul: the oak's own pictures in another wood's
    // colour. See `repaint_wood`.
    for (filename, source, colour, sparse) in WOOD_REPAINTS {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, repaint_wood(&dir, source, *colour, *sparse), force)?;
        written += 1;
    }

    for (filename, shape, metal) in METAL_TOOLS {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_metal_tool(*shape, *metal), force)?;
        written += 1;
    }

    // One sheet per animal: everything a wolf wears in one picture,
    // cut back into tiles when the game loads it. See `animal_sheet`.
    for (animal, (name, _, _)) in PELTS.iter().enumerate() {
        let filename = format!("animals/{name}.png");
        if !wanted(&filename) {
            continue;
        }
        write(&dir, &filename, animal_sheet(animal), force)?;
        written += 1;
    }

    if wanted(PLAYER_SKIN) {
        write(&dir, PLAYER_SKIN, generate_player_skin(), force)?;
        written += 1;
    }

    for stage in 0..BREAK_STAGES {
        let filename = format!("effects/break.{stage}.png");
        if !wanted(&filename) {
            continue;
        }
        write(&dir, &filename, generate_break(stage), force)?;
        written += 1;
    }

    if wanted(CLOUDS) {
        write(&dir, CLOUDS, generate_clouds(), force)?;
        written += 1;
    }

    // ---- fire that got loose, resin and the standing torch ----
    for (filename, source, look) in FIRE_RECOLOURS {
        if !wanted(filename) {
            continue;
        }
        write(&dir, filename, generate_fire_recolour(&dir, filename, source, *look), force)?;
        written += 1;
    }

    if written == 0 {
        anyhow::bail!("no texture called {:?}", only.unwrap_or_default());
    }
    println!("done -- {written} texture(s)");
    Ok(())
}

/// Writes one texture -- **and refuses to overwrite one that is already
/// there** unless asked twice.
///
/// This example makes *placeholders*. The textures in the folder are not
/// placeholders any more: they are drawn by hand, and a run of this
/// program with no argument used to walk the whole table and replace
/// every one of them with the generated stand-in. That is a working tree
/// full of destroyed artwork and no error message, which is exactly the
/// kind of accident a tool should not be able to have.
///
/// So the default is to keep what exists and say so. `--force` puts the
/// old behaviour back for whoever genuinely wants it, and naming a
/// single file (`gen_placeholder_textures stone.png`) still writes that
/// one, because asking for a file by name *is* the second ask.
fn write(dir: &std::path::Path, filename: &str, img: RgbaImage, force: bool) -> anyhow::Result<()> {
    // The names carry their folder now -- `animals/wolf.png` -- so the
    // folder has to exist before anything is written into it. Cheap and
    // idempotent; the alternative is a first run that fails on every
    // file.
    if let Some(parent) = std::path::Path::new(filename).parent() {
        std::fs::create_dir_all(dir.join(parent))?;
    }
    let path = dir.join(filename);
    if path.exists() && !force {
        println!("kept {} (already drawn; --force to replace)", path.display());
        return Ok(());
    }
    img.save(&path)?;
    println!("wrote {}", path.display());
    Ok(())
}

const WORKBENCH: &str = "terrain/workbench_side.png";

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
    ("plants/grass_mesh.png", generate_grass_mesh),
    ("plants/stick.png", generate_stick),
    ("plants/cactus.png", generate_cactus),
    ("plants/fiber.png", generate_fiber),
    ("terrain/pebble.png", generate_pebble),
    ("tools/flint.png", generate_flint),
    ("terrain/chest_side.png", generate_chest_side),
    ("terrain/chest_front.png", generate_chest_front),
    ("terrain/chest_top.png", generate_chest_top),
    ("terrain/backpack_side.png", generate_backpack_side),
    ("terrain/backpack_top.png", generate_backpack_top),
    ("fire/ash.png", generate_ash),
    ("fire/ash_item.png", generate_ash_item),
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
                // **...and darker again at the very rim**, all the way
                // round. The gradient alone gave a disc that was pale on
                // one side and dark on the other -- which is a coin
                // standing on edge, not a stone lying on the ground. A
                // pebble turns away from the light at every edge at
                // once, and one ring of shadow is the difference between
                // four grey discs and four stones.
                let rim = if ox * ox + oy * oy > (radius - 0.9).max(0.0).powi(2) {
                    -22
                } else {
                    0
                };
                let lift = lift + rim;
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
/// Which face of a chest is being drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chest {
    Lid,
    Front,
    Side,
}

fn generate_chest(face: Chest) -> RgbaImage {
    let lid = face == Chest::Lid;
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
                // The band the lid closes on runs round every side --
                // it is a band -- but the *latch* hangs on the front
                // alone, which is the whole of how a player can tell
                // which way a chest is facing.
                if (7..=8).contains(&y) {
                    base = if x % 4 == 0 { IRON_DARK } else { IRON };
                }
                if face == Chest::Front && (6..=10).contains(&y) && (7..=8).contains(&x) {
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
    generate_chest(Chest::Side)
}

/// The face with the latch on it. Every side of a chest used to have
/// one, which is four chests in a trench coat.
fn generate_chest_front() -> RgbaImage {
    generate_chest(Chest::Front)
}

fn generate_chest_top() -> RgbaImage {
    generate_chest(Chest::Lid)
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
///
/// **Two ribs, at x = 3 and x = 12, with a spine on each every fifth row.**
/// That is where the picture on disk has always had them and where
/// `generate_cactus_top` reads them from, so the top's grooves run in from
/// its edges exactly where the flank's end. This function used to rule a
/// rib every fifth column instead, and nobody saw it because the file it
/// would have written was never overwritten: the flank in the code and the
/// flank in the game disagreed, and regenerating it would have broken the
/// one detail the top was drawn to match.
///
/// **What it adds is the flesh between the ribs.** The flank was one green
/// with noise in it -- eleven colours within eight levels of each other --
/// and a stand of cacti read as painted posts. Each lobe is lit beside the
/// rib on its left and shaded against the rib on its right, light from the
/// upper left like everything else in this file, so a rib is a groove
/// between two swellings rather than a line ruled on a board. The mean is
/// the old flank's, so a cactus does not step lighter against its own top.
fn generate_cactus() -> RgbaImage {
    const FLESH: [u8; 3] = [58, 116, 60];
    const LIT: [u8; 3] = [72, 134, 72];
    const SHADE: [u8; 3] = [49, 101, 51];
    const RIB: [u8; 3] = [43, 93, 47];
    const SPINE: [u8; 3] = [210, 212, 174];
    const RIBS: [u32; 2] = [3, 12];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            // `by` texels to the right of rib `r`, round the tile's edge:
            // the lobe between x = 12 and x = 3 is the one that crosses it.
            let past = |r: u32, by: u32| (r + by) % RESOLUTION == x;
            let (colour, grained) = if RIBS.contains(&x) {
                if y % 5 == 0 {
                    (SPINE, false)
                } else {
                    (RIB, true)
                }
            } else if RIBS.iter().any(|&r| past(r, 1) || past(r, 2)) {
                (LIT, true)
            } else if RIBS.iter().any(|&r| past(r, RESOLUTION - 1)) {
                (SHADE, true)
            } else {
                (FLESH, true)
            };
            let noise = if grained { (hash(0x5EED, x, y) % 7) as i32 - 3 } else { 0 };
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
    // **Coal is not in this table any more.** `generate_ore` draws five
    // rounded nodules on the pack's stone, and on coal the player asked for
    // the older drawing back -- "верни текстуру угля" -- which is streaks
    // running to the tile's edges rather than nodules kept off them. That
    // picture is not this function with other numbers, so leaving the row
    // here would mean the next full run of this program quietly replaced
    // it. `metal/coal_ore.png` is drawn art now; every other ore below is
    // still stone with a colour in it.
    // Copper as it is *found*: green, not orange. Native copper weathers
    // to malachite, and a hillside speckled with green is a much better
    // thing to spot from a distance than one speckled with the colour of
    // the ingot -- which is what the smelting is for.
    ("metal/copper_ore.png", [86, 154, 118], [46, 104, 82]),
    // Cassiterite: dark, almost black-brown, with a resinous glint. It
    // is deliberately the least eye-catching of the four, because
    // finding tin is supposed to be the hard part.
    ("metal/tin_ore.png", [74, 62, 54], [44, 36, 32]),
    ("metal/iron_ore.png", [178, 150, 132], [128, 100, 84]),
];

/// (file, metal, its highlight).
const INGOTS: &[(&str, [u8; 3], [u8; 3])] = &[
    ("metal/copper_ingot.png", [186, 108, 62], [226, 156, 104]),
    ("metal/tin_ingot.png", [186, 190, 198], [226, 230, 236]),
    // Bronze sits between its parents, which is the point: it should
    // read as copper that has been *changed* rather than as a third
    // unrelated metal.
    ("metal/bronze_ingot.png", [176, 134, 66], [214, 178, 106]),
    ("metal/iron_ingot.png", [154, 152, 148], [198, 196, 192]),
];

/// (file, head, its highlight).
///
/// One row, and the generator is still parameterised by colour. The
/// metal picks it used to have are gone from *this* table: they came
/// back to the game in 1.7 and went to `METAL_TOOLS`, because a forged
/// pick turned out not to be a knapped one in a different colour --
/// see `generate_metal_tool` for what that cost when it was tried. The
/// parameters stay all the same. A second stone the game could knap
/// would be a line here rather than a function, which is the whole
/// argument for a table.
const PICKAXES: &[(&str, [u8; 3], [u8; 3])] = &[
    // The flint head is not metal: near-black with a blue-grey sheen,
    // the same colours the loose nodules are drawn in, so a player can
    // see what their first tool is made of.
    ("tools/stone_pickaxe.png", FLINT, FLINT_EDGE),
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
    // The torch, in its three states. Here rather than in a table of its
    // own because it is the same kind of thing as everything else in
    // this one: a head bound to a stick.
    ("tools/torch.png", generate_torch_ready),
    ("tools/torch_lit.png", generate_torch_lit),
    ("tools/torch_spent.png", generate_torch_spent),
    ("tools/flint_flake.png", generate_flake),
    ("tools/worked_stick.png", generate_worked_stick),
    ("tools/flint_knife_head.png", generate_knife_head),
    ("tools/stone_axe_head.png", generate_axe_head),
    ("tools/stone_pick_head.png", generate_pick_head),
    ("tools/flint_knife.png", generate_knife),
    ("tools/stone_axe.png", generate_axe),
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

/// The torch, at the three points in its life.
///
/// **Upright, and it is the only hafted thing in this pack that is.**
/// The first draft drew it on `tool_haft`, the shared diagonal every
/// axe, pick and knife is drawn on, and argued for it: a pack is a grid
/// of centimetre squares and what tells one row of it from another is
/// silhouette, so a torch that broke the family silhouette would read as
/// a different kind of object.
///
/// It read as a different kind of object all right -- a lollipop. A
/// diagonal stick with a lump balanced on the end is the silhouette of a
/// mallet, and no amount of colour on the lump argues a player out of
/// the shape they already saw. The premise was wrong twice over, too: a
/// torch *is* a different kind of object from an axe. It is not a tool,
/// it is not swung, and it is the one thing in the pack that is on fire.
/// Standing it up says all three before a single pixel of colour is
/// read, and there is nothing left for it to be confused with.
///
/// What makes it a torch rather than a stick is the *join*: a bundle of
/// fibre lashed on, wider than the stick, ragged where it was cut. A
/// smooth head is a ball; a head with the binding showing and loose
/// ends at the top is a bundle. The binding is also honest about the
/// recipe, exactly as the lashing is on the flint tools.
///
/// **Twice the stick and no more.** The bundle was six texels across a
/// two-texel stick to begin with, and three-to-one is the proportion of
/// a lollipop however it is shaded -- in the hand it came out a fist of
/// straw with a twig under it. Four across reads as fibre wound on a
/// stick, which is what it is, and it leaves the flame room to be the
/// widest thing at the top.
///
/// **Rounded at the top, and that is the flame's doing.** The wad was
/// square-shouldered, and its two top corners were the last two texels
/// the fire could not cover on the shortest of its six frames -- see
/// `FLAME_ABOVE` in `hand`, where the covering is computed. Cutting
/// them made the flame a texel smaller in every direction *and* made
/// the head truer: a bundle cut off square is a broom, and grass tied
/// to a stick comes to a round top.
///
/// The three states differ in the head and only in the head, because
/// that is what actually changes. Drawing them at three lengths would
/// say the stick burned too.
///
/// `+` lit fibre, `#` fibre, `-` its shade, `=` the lashing, `|` and `!`
/// the stick's lit and shaded columns, `o` char.
fn generate_torch(state: Torch) -> RgbaImage {
    /// Dry grass wound on the end. The same pale straw the lashings on
    /// the flint tools are, because it is the same fibre out of the same
    /// meadow -- see `LASHING`.
    const WAD: [u8; 3] = [198, 176, 108];
    const WAD_LIT: [u8; 3] = [224, 206, 148];
    const WAD_DARK: [u8; 3] = [152, 132, 74];
    /// Fibre that has burned. **Not black**, and the reason is now
    /// measured rather than asserted: charcoal keeps a little of the
    /// brown it was, a pure black head reads as a hole in the icon, and
    /// at 52 it also failed
    /// `every_cutout_picture_keeps_its_colour_all_the_way_down_the_mip_chain`
    /// -- two levels down, the thin stick stops surviving the cutout and
    /// the head is all that is left, so the whole picture fell to 0.54
    /// of the colour it was painted. A burnt torch that goes black at
    /// distance is the same fault leaves had, arrived at from the
    /// palette instead of from the alpha.
    const CHAR: [u8; 3] = [92, 78, 66];
    const CHAR_LIT: [u8; 3] = [118, 102, 88];
    const CHAR_DARK: [u8; 3] = [66, 56, 48];
    /// The wad while it is burning -- glowing through, not on fire *at*
    /// the tip. The flame itself is not in this picture at all: in the
    /// hand it is the campfire's own six frames drawn over the head (see
    /// `hand`), and in the pack it is a head that is plainly alight.
    const EMBER: [u8; 3] = [232, 138, 44];
    const EMBER_HOT: [u8; 3] = [252, 206, 96];
    const EMBER_DARK: [u8; 3] = [176, 82, 26];
    /// The cord that holds the bundle on, and **it is not `LASHING`.**
    /// On a flint axe the binding is straw against stone and reads at
    /// once; here it would be straw against straw, which is a head with
    /// no join in it -- drawn that way first, and the wad came out as
    /// one solid brick sitting on a stick. So it is the darker twisted
    /// cord it would actually be, close to the haft's own shade, which
    /// is what makes the eye see two things tied together.
    const CORD: [u8; 3] = [140, 106, 62];
    const CORD_LIT: [u8; 3] = [166, 128, 78];
    const CORD_DARK: [u8; 3] = [104, 76, 44];

    let (body, lit, dark) = match state {
        Torch::Ready => (WAD, WAD_LIT, WAD_DARK),
        Torch::Lit => (EMBER, EMBER_HOT, EMBER_DARK),
        Torch::Spent => (CHAR, CHAR_LIT, CHAR_DARK),
    };

    // The head. A spent torch's is a stub and a scorched binding, and
    // that is the whole of what "burnt down" looks like in a square this
    // size -- there is nothing left to be big.
    let head: &[&str] = match state {
        Torch::Spent => &[
            "................",
            "................",
            "................",
            "................",
            "................",
            ".......##.......",
            "......+##-......",
            "......+##-......",
            "......wWWm......",
        ],
        _ => &[
            "................",
            "................",
            ".......##.......",
            ".......##.......",
            "......+##-......",
            "......-##-......",
            "......+##-......",
            "......+##-......",
            "......wWWm......",
        ],
    };

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The stick, straight down the middle and out of the bottom of the
    // tile. Lit column then shaded column, the same two-pixel section
    // and the same light `tool_haft` uses, so a torch and an axe are
    // plainly made of the same stick.
    for y in (head.len() as i32)..(RESOLUTION as i32) {
        put_opaque(&mut img, 7, y, HAFT);
        put_opaque(&mut img, 8, y, HAFT_DARK);
    }
    for (y, row) in head.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            let colour = match cell {
                b'#' => body,
                b'+' => lit,
                b'-' => dark,
                // The binding, and a burnt torch's is scorched: the
                // cord is right where the fire was. Scorched, not
                // burned away -- it is under the fibre, and a torch
                // whose binding had gone would have dropped its head.
                b'w' | b'W' | b'm' if state == Torch::Spent => CORD_DARK,
                b'w' => CORD_LIT,
                b'W' => CORD,
                b'm' => CORD_DARK,
                _ => continue,
            };
            // Grain, and more of it than the tools carry. **Not a
            // pattern.** The strands were drawn in first -- a darker
            // texel every other row -- and four texels across is not
            // enough room for that to read as fibre: it came out a
            // chequerboard, which is the one thing a bundle of grass is
            // not. So the shape is a clean slab, lit on one side and
            // shaded on the other, and what makes it look like dry
            // grass is noise.
            let noise = (hash(0x70C4, x as u32, y as u32) % 15) as i32 - 7;
            put_opaque(
                &mut img,
                x as i32,
                y as i32,
                [
                    clamp_u8(colour[0] as i32 + noise),
                    clamp_u8(colour[1] as i32 + noise),
                    clamp_u8(colour[2] as i32 + noise),
                ],
            );
        }
    }
    img
}

/// Which of the three a torch picture is. See `generate_torch`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Torch {
    Ready,
    Lit,
    Spent,
}

fn generate_torch_ready() -> RgbaImage {
    generate_torch(Torch::Ready)
}

fn generate_torch_lit() -> RgbaImage {
    generate_torch(Torch::Lit)
}

fn generate_torch_spent() -> RgbaImage {
    generate_torch(Torch::Spent)
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
    let mut img = match image::open(dir.join("terrain/stone.png")) {
        Ok(stone) => image::imageops::resize(
            &stone.to_rgba8(),
            RESOLUTION,
            RESOLUTION,
            image::imageops::FilterType::Nearest,
        ),
        Err(_) => generate([128, 128, 130], 14, "terrain/stone.png"),
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
    // **The specular streak, and the hammer marks under it.**
    //
    // The bar had the right silhouette and the wrong material: four flat
    // bands of one colour apiece, which is how you draw a painted wooden
    // block. What separates metal from everything else in a picture is
    // that it does not shade evenly -- it carries a bright *line* where
    // the surface is flattest, and it carries the dents of whatever
    // flattened it. Two rows of that and the same shape stops being a
    // trapezoid of orange and starts being copper.
    for x in 6..=9 {
        put_opaque(&mut img, x, 5, mix(highlight, [255, 255, 255], 0.45));
    }
    // Facets across the front, in the metal's own two tones: an ingot
    // out of a mould is not smooth, and the marks run along the bar.
    for x in [5, 8, 11] {
        put_opaque(&mut img, x, 8, mix(metal, shadow, 0.45));
        put_opaque(&mut img, x + 1, 9, mix(metal, highlight, 0.5));
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

    // The haft: two pixels wide, from the bottom left up to the middle
    // of the head.
    //
    // **It has to actually reach the head.** It used to stop a row
    // short and a couple of pixels off to one side, so the head floated
    // over the end of a stick that was pointing past it -- which is
    // what "the tool textures are broken" was. A pick is a bar with a
    // handle in the middle of it; anywhere else and the eye reads a
    // walking stick with something balanced on top.
    for step in 0..10 {
        let x = 2 + step;
        let y = 14 - step;
        put_opaque(&mut img, x, y, HAFT);
        put_opaque(&mut img, x + 1, y, HAFT_DARK);
    }

    // The lashing at the join, drawn *before* the head so the head
    // covers whatever of it would stand proud of the metal.
    for step in 0..2 {
        put_opaque(&mut img, 10 + step, 7 - step, LASHING);
        put_opaque(&mut img, 11 + step, 7 - step, LASHING);
    }

    // The head: a bar lying across the top of the haft with both ends
    // turned down, centred on the haft. Two tips rather than one,
    // because a single point is a hoe and two is unmistakably a pick --
    // and the silhouette is all there is to go on at this size.
    for x in 9..=13 {
        put_opaque(&mut img, x, 3, head);
    }
    for x in 7..=15 {
        put_opaque(&mut img, x, 4, head);
    }
    for x in [7, 8, 14, 15] {
        put_opaque(&mut img, x, 5, head);
    }
    for x in [7, 15] {
        put_opaque(&mut img, x, 6, head);
    }
    // Lit along the top and shadowed under the tips: what makes it read
    // as metal (or, for the first tier, as a knapped edge).
    for x in 10..=12 {
        put_opaque(&mut img, x, 3, highlight);
    }
    put_opaque(&mut img, 7, 4, highlight);
    for x in [8, 14] {
        put_opaque(&mut img, x, 6, shadow);
    }
    put_opaque(&mut img, 15, 5, shadow);
    img
}

// ---- fire that got loose ----
//
// **Every one a recolour of a picture the game already has**, for the rule
// the tools follow (`recolour_tools.py`): a charred log is the log it was,
// with the same grain and the same rings, gone black. Drawing char fresh
// would give a burnt house walls that do not line up with the logs still
// standing beside them. The resin is the lump of coal's silhouette in
// amber, and the standing torch the hand torch with its wad gone to resin.

/// What a recolour does to its source.
#[derive(Clone, Copy)]
enum FireLook {
    /// Burnt through: black, the grain's dark lines deepest, the light
    /// ones gone to a grey film of ash.
    Charred,
    /// Char with the grain's dark lines glowing: wood that is alight.
    Burning,
    /// The whole picture in amber, darkest where the source is darkest:
    /// a lump of resin.
    Amber,
    /// Only the wad of a torch -- the straw-coloured pixels -- in amber;
    /// the stick is left as it was.
    AmberWad,
}

/// (file written, file it is recoloured from, how).
const FIRE_RECOLOURS: &[(&str, &str, FireLook)] = &[
    ("fire/charred_log_side.png", "terrain/log_side.png", FireLook::Charred),
    ("fire/charred_log_top.png", "terrain/log_top.png", FireLook::Charred),
    ("fire/charred_planks.png", "terrain/planks.png", FireLook::Charred),
    ("fire/burning_log_side.png", "terrain/log_side.png", FireLook::Burning),
    ("fire/burning_planks.png", "terrain/planks.png", FireLook::Burning),
    ("plants/resin.png", "metal/coal.png", FireLook::Amber),
    ("tools/standing_torch.png", "tools/torch.png", FireLook::AmberWad),
];

fn generate_fire_recolour(dir: &std::path::Path, filename: &str, source: &str, look: FireLook) -> RgbaImage {
    let mut img = image::open(dir.join(source))
        .map(|picture| picture.to_rgba8())
        .unwrap_or_else(|_| generate([128, 96, 60], 14, source));
    let seed = filename
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    // The picture's own range of brightness, so "the dark lines" means the
    // darkest third of *this* picture and not of some fixed scale.
    let luma = |p: &Rgba<u8>| (u32::from(p[0]) * 3 + u32::from(p[1]) * 6 + u32::from(p[2])) / 10;
    let (low, high) = img
        .pixels()
        .filter(|p| p[3] > 0)
        .fold((255u32, 0u32), |(lo, hi), p| (lo.min(luma(p)), hi.max(luma(p))));
    let span = (high.saturating_sub(low)).max(1) as f32;
    let (width, height) = img.dimensions();
    for y in 0..height {
        for x in 0..width {
            let p = *img.get_pixel(x, y);
            if p[3] == 0 {
                continue;
            }
            let t = (luma(&p).saturating_sub(low)) as f32 / span; // 0 dark .. 1 light
            let noise = (hash(seed, x, y) % 9) as i32 - 4;
            let colour: [i32; 3] = match look {
                FireLook::Charred | FireLook::Burning => {
                    // The dark lines of the grain are the cracks char opens
                    // along; the light wood between them is black with a
                    // grey bloom of ash on its high points.
                    let base = if t < 0.35 {
                        [14, 12, 11]
                    } else if t > 0.8 {
                        [74, 70, 66]
                    } else {
                        let v = 26 + (t * 34.0) as i32;
                        [v + 3, v, v - 2]
                    };
                    let glowing = matches!(look, FireLook::Burning) && t < 0.35 && !hash(seed, x, y).is_multiple_of(3);
                    if glowing {
                        // Hotter in the middle of a crack: the brighter the
                        // hash, the more yellow.
                        let heat = (hash(seed ^ 0x5eed, x, y) % 60) as i32;
                        [205 + heat / 2, 80 + heat, 18 + heat / 4]
                    } else {
                        base
                    }
                }
                FireLook::Amber => {
                    let v = 0.35 + 0.65 * t;
                    [(196.0 * v) as i32, (118.0 * v) as i32, (28.0 * v) as i32]
                }
                FireLook::AmberWad => {
                    // The wad is the straw of the picture: yellower than the
                    // stick's brown, which is where green runs close to red.
                    let straw = i32::from(p[1]) * 10 > i32::from(p[0]) * 8;
                    if !straw {
                        continue;
                    }
                    let v = 0.45 + 0.55 * t;
                    [(200.0 * v) as i32, (120.0 * v) as i32, (30.0 * v) as i32]
                }
            };
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(colour[0] + noise),
                    clamp_u8(colour[1] + noise),
                    clamp_u8(colour[2] + noise / 2),
                    p[3],
                ]),
            );
        }
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

const CLOUDS: &str = "effects/sky_clouds.png";

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

// ============================================================
// 1.5: what grows, what you eat, the fire, and what is alive
// ============================================================
//
// Twenty-six pictures, and the thing they have to do collectively is
// stay legible in a slot a centimetre across. Every one of them is
// therefore built out of a silhouette first and a palette second -- the
// eye picks a shape out of a hotbar long before it picks a colour, and
// the four foods are the sharp end of that: berries, raw meat, cooked
// meat and a hide are all "a small brownish-red thing" if you draw them
// by colour.

/// The plants. All cross-shaped, so everything that is not the plant is
/// transparent -- see `is_cross`.
/// The half-height side of the bag, derived from the drawn one.
const BACKPACK_SIDE_FLAT: &str = "terrain/backpack_side_flat.png";

const GROWN: &[Drawn] = &[
    ("plants/berry_bush.png", generate_berry_bush),
    ("plants/bare_bush.png", generate_bare_bush),
    ("food/mushroom.png", generate_mushroom),
    ("plants/reeds.png", generate_reeds),
    ("plants/flower.png", generate_flower),
];

/// What a carcass is made of, and the bog's dried peat.
///
/// No picture for the carcasses themselves: a carcass is the animal's
/// own model lying on its side (`animal_model::build_fallen`), drawn in
/// the animal's skins. The first version drew it as a low cube with a
/// fur-and-seam texture from this table, and it read as a crate.
const BUTCHERED: &[Drawn] = &[
    ("hide/sinew.png", generate_sinew),
    ("hide/bone.png", generate_bone),
    ("fire/dried_peat.png", generate_dried_peat),
];

/// What there is to forage, and the one thing there is to get wrong.
///
/// A table of its own because these arrived together and answer each
/// other: a root is food you have to *dig up* and is worth little until
/// it has been in a fire, which is the same argument the meat makes; and
/// a toadstool is the first thing in this world that is worth knowing
/// the difference about. The mushroom's cap is plain and this one is
/// spotted, which is the oldest warning colour there is and the reason
/// the two are drawn from the same silhouette.
const FORAGED: &[Drawn] = &[
    ("plants/roots.png", generate_roots_plant),
    ("plants/toadstool.png", generate_toadstool),
    ("food/root.png", generate_root),
    ("food/roasted_root.png", generate_roasted_root),
];

// **The fire is not here any more, and must not come back.**
//
// It used to be `FLAMES`: eight frames, two sheets, one function with a
// seed per frame. The frames in `assets/textures/effects/` are drawn by
// hand now, and a table entry naming them would mean that one
// `--force` run of this example -- a run somebody makes to add a single
// new placeholder -- silently paints over a drawing. The refusal in
// `write` is not enough on its own: `--force` exists precisely to get
// past it.
//
// So the rule is the one in this file's header, applied: a picture
// somebody drew is not a placeholder, and the way to be sure this
// example cannot destroy it is for this example not to know its name.


/// The flame on a torch, as six frames of one tongue.
///
/// **Its own drawing, and the campfire's would not do.** The torch's
/// fire was the hearth's for a while, on the argument that a fire
/// already drawn, already animated and already in the atlas costs no
/// layer and cannot come to disagree with itself. That argument is
/// still good and it lost to a measurement: the hearth's fire is drawn
/// **to the full width of its bottom row** -- sixteen texels of
/// sixteen, all six frames -- because in the world it is fire filling
/// the cell inside a ring of stones.
///
/// On a stick that shape cannot win. The head of a torch is four texels
/// across; a quad big enough for the shortest frame to swallow the end
/// of it is a quad whose flat, full-width base hangs a hand's breadth
/// out into the air on either side of the stick, and one narrow enough
/// to hide that base leaves the bundle's tip poking out of the top.
/// Both were photographed, and there is no third setting -- the
/// conflict is in the drawing, so the drawing is what had to change.
///
/// So: a tongue that is **four texels wide where it meets the fibre**,
/// exactly the width of the head it sits on, so its base has nothing to
/// stick out past. It bulges above that, because that is what a flame
/// on a wick does, and tapers to a point whose height is the frame.
///
/// The palette is the hearth's own, sampled from it, so the two fires
/// are plainly the same fire even though they are now two drawings.
fn generate_torch_flame(frame: u32) -> RgbaImage {
    /// Sampled from `effects/flame.*`: the outer edge, the body, the
    /// bright part and the core.
    const EDGE: [u8; 3] = [207, 88, 10];
    const BODY: [u8; 3] = [255, 170, 42];
    const BRIGHT: [u8; 3] = [255, 230, 0];
    const CORE: [u8; 3] = [255, 255, 255];

    /// How high each frame reaches, as the row its tip touches.
    ///
    /// Up over four pictures and back over two, which is the shape the
    /// hearth's hand-drawn set has and the reason it reads as fire
    /// rather than as a pulse. The loop is not closed by repeating the
    /// first frame -- see `FLAME_FRAMES`, where that mistake is
    /// written up.
    const TIP: [f32; 6] = [3.0, 2.0, 1.5, 2.0, 3.0, 4.0];
    /// ...and how far the tongue leans, so it is not the same cone six
    /// times. Half a texel is enough at this size; a whole one reads as
    /// the flame being blown sideways.
    const LEAN: [f32; 6] = [-0.2, 0.0, 0.18, 0.25, 0.08, -0.12];

    let tip = TIP[frame as usize % 6];
    let lean = LEAN[frame as usize % 6];
    let base = RESOLUTION as f32 - 1.0;
    let span = base - tip;

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        let up = (base - y as f32) / span;
        if !(0.0..=1.0).contains(&up) {
            continue;
        }
        // **Full width for the lower half, then a point.** Not a cone:
        // this flame has a job the hearth's has not, which is to hide a
        // wad of fibre four texels across and six deep behind it. A
        // tongue that starts narrowing at its base is only wide enough
        // over the bottom row or two, and no size of quad then covers
        // the head without the flame being taller than the frame. So it
        // holds three texels either side of the middle until it is
        // nearly half way up, which is where the wad ends, and only
        // then closes.
        const SHOULDER: f32 = 0.55;
        let taper = ((up - SHOULDER) / (1.0 - SHOULDER)).max(0.0);
        let mut half = 3.0 * (1.0 - taper).powf(0.7);
        // **...and it draws in a little at the foot.** The hearth's
        // fire is widest on its very bottom row, because in the world
        // it is fire filling a cell; a torch's is not, and a flat
        // full-width bottom row is a bar of fire lying across whatever
        // it lands on. A little, and not a lot: drawn down to under
        // half its width the foot stopped covering the two bottom
        // corners of the wad, and the wad showing round the fire is the
        // fault this whole drawing exists to fix. Three quarters hides
        // the join and still covers the fibre.
        const FOOT: f32 = 1.5;
        let foot = ((base - y as f32) / FOOT).min(1.0);
        half *= 0.75 + 0.25 * foot;
        // **Ragged, row by row.** Drawn smooth this came out a solid
        // wedge -- at sixteen texels magnified to a quarter of the
        // screen, a clean edge is the one thing that says "shape" and
        // not "fire". A third of a texel is the whole of the wobble and
        // it is enough: what the eye reads as flame is the *edge*
        // moving, and every frame has its own.
        // Stepped every other row, not every row: taken per row the
        // wobble put single texels out on their own at the edge, and a
        // lone texel beside a flame is a speck rather than a tongue.
        half += (hash(0x11A3 + frame, 0, y as u32 / 2) % 7) as f32 / 11.0 - 0.28;
        // The centre wanders with height, and the wander is the lean.
        // Half a texel at the middle and none at either end, because
        // the ends are where it matters: the base has to stay over the
        // fibre and the tip has to stay over the middle of the flame.
        // At a whole texel the tongue leaned far enough to uncover one
        // shoulder of the wad, which is the fault this whole drawing
        // exists to fix.
        let centre = 7.5 + lean * (up * std::f32::consts::PI).sin();
        for x in 0..RESOLUTION as i32 {
            let across = ((x as f32 + 0.5) - centre).abs();
            if across > half {
                continue;
            }
            // **Mostly orange, and the pale part is a thread.** Drawn
            // as even bands the flame came out a yellow slab with a
            // white brick in it -- magnified to a quarter of the screen
            // that is a shape, not a fire. What the eye reads as flame
            // is a dark outline and a thin bright thread up the middle,
            // so the outermost texel of every row is the dark edge
            // whatever that row's width, the body takes most of what is
            // left, and the pale core is a texel wide and only in the
            // bottom third. Every widening of these two thresholds has
            // been tried and photographed; they go the other way.
            let inward = 1.0 - across / half.max(0.001);
            let colour = if across > half - 1.0 {
                EDGE
            } else if inward > 0.88 && up < 0.30 {
                CORE
            } else if inward > 0.75 {
                BRIGHT
            } else {
                BODY
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

fn generate_torch_flame_0() -> RgbaImage {
    generate_torch_flame(0)
}
fn generate_torch_flame_1() -> RgbaImage {
    generate_torch_flame(1)
}
fn generate_torch_flame_2() -> RgbaImage {
    generate_torch_flame(2)
}
fn generate_torch_flame_3() -> RgbaImage {
    generate_torch_flame(3)
}
fn generate_torch_flame_4() -> RgbaImage {
    generate_torch_flame(4)
}
fn generate_torch_flame_5() -> RgbaImage {
    generate_torch_flame(5)
}

/// 1.9: the torch's own fire.
///
/// A table of its own, and named nothing like `effects/flame.*`, so
/// that the rule above it still holds: this example must not know the
/// name of a picture somebody drew by hand.
const TORCH_FLAMES: &[Drawn] = &[
    ("effects/torch_flame.0.png", generate_torch_flame_0),
    ("effects/torch_flame.1.png", generate_torch_flame_1),
    ("effects/torch_flame.2.png", generate_torch_flame_2),
    ("effects/torch_flame.3.png", generate_torch_flame_3),
    ("effects/torch_flame.4.png", generate_torch_flame_4),
    ("effects/torch_flame.5.png", generate_torch_flame_5),
];

/// 1.7: the tannery, the jug, and what a person wears.
///
/// A table of its own for the same reason `FIRED` is one: these arrived
/// together and belong to one another. The four garment pictures are
/// **greyscale on purpose** -- twelve garments share them and are told
/// apart by the tint the icon is drawn with (see
/// `types::garment_tint`), so the pictures carry the *shape* and the
/// colour comes from the material. Twelve separate images would be
/// twelve texture layers, and there are not twelve to spare -- the whole
/// atlas is capped at 256 by the hardware.
const WORN: &[Drawn] = &[
    ("hide/leather.png", generate_leather),
    ("hide/wool.png", generate_wool),
    ("hide/stretched_hide.png", generate_stretched_hide),
    ("terrain/drying_rack.png", generate_drying_rack),
    ("pottery/jug_raw.png", generate_jug_raw),
    ("pottery/jug.png", generate_jug),
    ("pottery/jug_water.png", generate_jug_water),
    ("worn/cap.png", generate_cap),
    ("worn/tunic.png", generate_tunic),
    ("worn/leggings.png", generate_leggings),
    ("worn/boots.png", generate_boots),
];

/// What you carry: four foods, the fire's faces, and the two things the
/// weather is drawn out of.
const CARRIED: &[Drawn] = &[
    ("food/berries.png", generate_berries),
    ("food/raw_meat.png", generate_raw_meat),
    ("food/cooked_meat.png", generate_cooked_meat),
    ("food/human_flesh.png", generate_human_flesh),
    // The roast is **drawn by hand**, not generated: the player redrew it, and
    // a `--force` run of this table must not paint over their picture.
    ("food/dried_meat.png", generate_dried_meat),
    ("food/hide.png", generate_hide),
    ("fire/campfire_side.png", generate_campfire_side),
    ("fire/campfire_top.png", generate_campfire_top),
    ("fire/campfire_lit_top.png", generate_campfire_lit_top),
    ("effects/rain.png", generate_rain),
    ("effects/snow_fall.png", generate_snowfall),
];

/// 1.6: the second fire and what comes out of it.
///
/// A table of its own rather than more rows on `CARRIED`, because none
/// of these is carried: three of them are faces of a cube and the fourth
/// is the brick that cube is made of.
const FIRED: &[Drawn] = &[
    ("fire/kiln_side.png", generate_kiln_side),
    ("fire/kiln_top.png", generate_kiln_top),
    ("fire/kiln_lit_side.png", generate_kiln_lit_side),
    ("fire/kiln_front.png", generate_kiln_front),
    ("fire/kiln_lit_front.png", generate_kiln_lit_front),
    ("fire/kiln_lit_top.png", generate_kiln_lit_top),
    ("pottery/brick.png", generate_brick),
    ("pottery/bricks.png", generate_bricks),
    ("terrain/sandstone_bricks.png", generate_sandstone_bricks),
    // ---- the field ----
    ("tools/hoe.png", generate_hoe),
    ("terrain/farmland.png", generate_farmland),
    ("plants/seeds.png", generate_seeds),
    ("plants/wheat.png", generate_wheat_young),
    ("plants/wheat_ripe.png", generate_wheat_ripe),
    ("food/grain.png", generate_grain),
    ("food/dough.png", generate_dough),
    ("food/bread.png", generate_bread),
    // ---- the copper age ----
    ("metal/native_copper.png", generate_native_copper),
    ("pottery/vessel_raw.png", generate_vessel_raw),
    ("pottery/vessel.png", generate_vessel),
    ("pottery/mould_raw.png", generate_mould_raw),
    ("pottery/mould.png", generate_mould),
    ("metal/iron_bloom.png", generate_iron_bloom),
    ("fire/bloomery_side.png", generate_bloomery_side),
    ("fire/bloomery_lit_side.png", generate_bloomery_lit_side),
    ("fire/bloomery_front.png", generate_bloomery_front),
    ("fire/bloomery_lit_front.png", generate_bloomery_lit_front),
    ("fire/bloomery_top.png", generate_bloomery_top),
    ("fire/bloomery_lit_top.png", generate_bloomery_lit_top),
    // ---- 1.8 ----
    ("terrain/ice.png", generate_ice),
    // The carried pictures. See `generate_campfire_item`.
    ("fire/campfire_item.png", || generate_campfire_item(false)),
    ("fire/campfire_lit_item.png", || generate_campfire_item(true)),
    ("terrain/backpack_item.png", generate_backpack_item),
];

/// A campfire as a thing you are carrying, rather than as a face of the
/// block it becomes.
///
/// **The picture a half-height block shows in the pack was the wrong
/// picture twice over.** A campfire's side is sixteen pixels by *four* --
/// the block fills half its cell, so drawing the other twelve rows would
/// be drawing something that is not there -- and an icon is square, so
/// the pack stretched four rows of stones into a sixteen-row smear.
/// What a player saw was a grey block: the same grey block the chest,
/// the kiln and everything else showed, which is what "everything looks
/// like a cube in the inventory" was.
///
/// So the fire gets an icon of its own: a laid hearth seen from the
/// side, on nothing. Transparent everywhere else, which is the other
/// half of the difference -- an icon is an *object*, and the eye reads
/// an object by its outline before it reads any of the pixels inside it.
fn generate_campfire_item(lit: bool) -> RgbaImage {
    const STONE: [u8; 3] = [122, 120, 118];
    const STONE_DARK: [u8; 3] = [88, 86, 84];
    const LOG: [u8; 3] = [132, 96, 58];
    const LOG_DARK: [u8; 3] = [96, 68, 40];
    const EMBER: [u8; 3] = [226, 120, 44];
    const FLAME: [u8; 3] = [242, 176, 60];
    const FLAME_PALE: [u8; 3] = [250, 226, 126];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);

    // The ring of stones, five of them across the bottom, each two
    // pixels tall so they read as stones rather than as a kerb.
    for (i, x) in (2..14).step_by(3).enumerate() {
        let shade = if i % 2 == 0 { STONE } else { STONE_DARK };
        for dx in 0..3 {
            put_opaque(&mut img, x + dx, 12, shade);
            put_opaque(&mut img, x + dx, 13, STONE_DARK);
        }
    }

    // Two logs crossed over them. Drawn as two diagonals rather than as
    // a stack, because crossed sticks are the one arrangement that says
    // "laid fire" at any size.
    for step in 0..9 {
        put_opaque(&mut img, 3 + step, 11 - step / 2, LOG);
        put_opaque(&mut img, 3 + step, 12 - step / 2, LOG_DARK);
        put_opaque(&mut img, 12 - step, 11 - step / 2, LOG_DARK);
    }

    if lit {
        // The flame: a tongue over the middle of the pile, brightest at
        // its heart. Three columns, because a single one reads as a
        // candle and five fills the tile.
        for (x, top) in [(6, 6), (7, 3), (8, 4), (9, 6)] {
            for y in top..=9 {
                let colour = if y <= top + 1 {
                    FLAME_PALE
                } else if y <= top + 3 {
                    FLAME
                } else {
                    EMBER
                };
                put_opaque(&mut img, x, y, colour);
            }
        }
    }
    img
}

/// A dead player's pack, as a thing rather than as the flap of a
/// half-height block. Same argument as the campfire above: its side
/// texture is eight rows and an icon is sixteen.
fn generate_backpack_item() -> RgbaImage {
    const CANVAS: [u8; 3] = [126, 96, 62];
    const CANVAS_DARK: [u8; 3] = [92, 68, 42];
    const STRAP: [u8; 3] = [66, 48, 30];
    const BUCKLE: [u8; 3] = [188, 164, 96];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The body: a rounded bag, wider at the bottom than at the top.
    for y in 4..=13 {
        let inset = match y {
            4 => 5,
            5 => 4,
            13 => 4,
            _ => 3,
        };
        for x in inset..(16 - inset) {
            let colour = if x == inset || y == 13 { CANVAS_DARK } else { CANVAS };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The flap, and the strap that holds it down.
    for x in 4..=11 {
        put_opaque(&mut img, x, 6, CANVAS_DARK);
    }
    for y in 4..=9 {
        put_opaque(&mut img, 7, y, STRAP);
        put_opaque(&mut img, 8, y, STRAP);
    }
    put_opaque(&mut img, 7, 8, BUCKLE);
    put_opaque(&mut img, 8, 8, BUCKLE);
    img
}

/// Ice: pale blue, and cracked rather than speckled.
///
/// **What makes ice read as ice is the cracks**, not the colour. A tile
/// of flat pale blue is a tile of flat pale blue at any brightness, and
/// the speckle every other mineral here uses would make it read as
/// frosted stone -- the eye is looking for straight lines through a
/// clear body, because that is what it has ever seen in ice.
///
/// So: a very faint blue wash, a few straight fractures running across
/// the tile at angles that are not the tile's own edges, and a paler
/// bloom where two of them meet. Drawn deterministically, like
/// everything else here, so regenerating it gives back the same file.
fn generate_ice() -> RgbaImage {
    const BODY: [u8; 3] = [168, 206, 232];
    const DEEP: [u8; 3] = [138, 182, 216];
    const CRACK: [u8; 3] = [214, 236, 248];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let seed = 0x1CE_u32;
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            // The body, darker in the middle of the slab than at its top
            // and bottom rows.
            //
            // **It used to run from the top row to the bottom one** -- lit
            // from above, darker where the slab is thicker -- and that is a
            // gradient with a different colour at each end. The bottom row
            // of one block sat against the top row of the next, so a wall of
            // cut ice or a frozen lake seen along its length was a stack of
            // stripes, the step across the joint 3.6 times any step inside
            // the picture. Out and back, the curve the fractures below use,
            // so both ends are one colour and the slab still has a depth.
            let along = (y as f32 + 0.5) / RESOLUTION as f32;
            let depth = 1.0 - (2.0 * along - 1.0).abs();
            let wobble = (hash(seed, x, y) % 9) as i32 - 4;
            let px = [
                clamp_u8(lerp(BODY[0], DEEP[0], depth) + wobble),
                clamp_u8(lerp(BODY[1], DEEP[1], depth) + wobble),
                clamp_u8(lerp(BODY[2], DEEP[2], depth) + wobble),
                255,
            ];
            img.put_pixel(x, y, Rgba(px));
        }
    }

    // Trapped air, first, and under everything else. A frozen pool is
    // full of bubbles and they are the reason ice is not glass: a few
    // pale specks at no spacing at all, which the fractures then run
    // past.
    for &(x, y) in &[(4u32, 3u32), (11, 6), (6, 9), (13, 12), (2, 13), (9, 14)] {
        let existing = img.get_pixel(x, y).0;
        img.put_pixel(
            x,
            y,
            Rgba([
                clamp_u8((existing[0] as i32 * 2 + CRACK[0] as i32) / 3),
                clamp_u8((existing[1] as i32 * 2 + CRACK[1] as i32) / 3),
                clamp_u8((existing[2] as i32 * 2 + CRACK[2] as i32) / 3),
                255,
            ]),
        );
    }

    // The fractures: (x where the crack starts, where it ends at the
    // bottom of the tile). Neither vertical nor at forty-five degrees,
    // so none of them lies along the grid the tile is made of.
    //
    // **Each one leaves and arrives at the same column**, which is what
    // lets a frozen lake tile: a crack that walked off the right edge
    // used to stop dead there, and a field of ice showed a grid of
    // sixteen-texel panels with the cracks all ending on the joints. So
    // the ends are the *same* x, the line wanders in between, and a
    // crack now runs from one block into the next.
    for (column, lean) in [(2.0f32, 3.5f32), (9.0, -2.5), (13.0, 2.0)] {
        for y in 0..RESOLUTION {
            // A sine would need a dependency and a straight line is a
            // ruled edge; this is the cheapest curve there is -- out and
            // back over the height of the tile.
            let along = y as f32 / RESOLUTION as f32;
            let bow = 1.0 - (2.0 * along - 1.0).abs();
            let x = column + lean * bow;
            let px = x.rem_euclid(RESOLUTION as f32) as u32;
            let existing = img.get_pixel(px, y).0;
            // Lightened rather than painted: a crack in clear ice is
            // light caught in it, and a hard white line would read as a
            // scratch on the surface.
            img.put_pixel(
                px,
                y,
                Rgba([
                    clamp_u8((existing[0] as i32 + CRACK[0] as i32) / 2),
                    clamp_u8((existing[1] as i32 + CRACK[1] as i32) / 2),
                    clamp_u8((existing[2] as i32 + CRACK[2] as i32) / 2),
                    255,
                ]),
            );
            // A soft shoulder on one side, so the fracture has a
            // thickness. A one-texel line at this size reads as a hair
            // laid on the block rather than as a split in it.
            if y.is_multiple_of(3) {
                let beside = (px + 1) % RESOLUTION;
                let near = img.get_pixel(beside, y).0;
                img.put_pixel(
                    beside,
                    y,
                    Rgba([
                        clamp_u8((near[0] as i32 * 3 + CRACK[0] as i32) / 4),
                        clamp_u8((near[1] as i32 * 3 + CRACK[1] as i32) / 4),
                        clamp_u8((near[2] as i32 * 3 + CRACK[2] as i32) / 4),
                        255,
                    ]),
                );
            }
        }
    }
    img
}

/// One channel of a colour, mixed towards another. The generator has
/// wanted this three times and written it inline three times.
fn lerp(from: u8, to: u8, t: f32) -> i32 {
    (from as f32 + (to as f32 - from as f32) * t.clamp(0.0, 1.0)) as i32
}

/// Wet clay and fired clay: the same material at the two stages the
/// whole copper age turns on.
const WET_CLAY: [u8; 3] = [138, 134, 128];
const WET_CLAY_DARK: [u8; 3] = [110, 106, 100];
/// Fired: the terracotta the kiln turns it into. The *same* red the
/// brick and the kiln's daub are, because it is the same clay.
const FIRED_CLAY: [u8; 3] = [162, 104, 74];
const FIRED_CLAY_DARK: [u8; 3] = [124, 78, 56];

/// A nodule of native copper: stone with green-stained metal in it.
///
/// The green is what says copper at a glance -- a weathered nodule is
/// malachite-stained, and a plain orange lump on a scree slope reads as
/// a stone somebody has coloured in.
fn generate_native_copper() -> RgbaImage {
    const ROCK: [u8; 3] = [104, 100, 96];
    const METAL: [u8; 3] = [186, 116, 68];
    const PATINA: [u8; 3] = [86, 148, 116];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 5..12 {
        for x in 3..13 {
            // A lump: corners cut.
            if (y == 5 || y == 11) && !(5..11).contains(&x) {
                continue;
            }
            let roll = hash(0xC0FF, x as u32, y as u32) % 7;
            let colour = match roll {
                0 | 1 => METAL,
                2 => mix(METAL, [255, 255, 255], 0.25),
                3 => PATINA,
                4 => mix(PATINA, [0, 0, 0], 0.2),
                _ => ROCK,
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

fn generate_vessel_raw() -> RgbaImage {
    pot(WET_CLAY, WET_CLAY_DARK)
}

fn generate_vessel() -> RgbaImage {
    pot(FIRED_CLAY, FIRED_CLAY_DARK)
}

/// A crucible seen from the side: a bowl with a rim and a shadow inside.
///
/// The mouth is the whole picture. A pot drawn as a solid lump is a
/// stone; what makes it read as something you put ore *into* is that you
/// can see down into it.
fn pot(body: [u8; 3], dark: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 4..14 {
        // Tapered: narrow at the foot, wide at the rim.
        let inset = if y >= 12 { 3 } else { (13 - y) / 4 };
        for x in (3 + inset)..(13 - inset) {
            let roll = hash(0x90A7, x as u32, y as u32) % 6;
            let colour = if roll == 0 { dark } else { body };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The mouth: a dark band at the top with a lip either side of it.
    for x in 4..12 {
        put_opaque(&mut img, x, 4, mix(body, [255, 255, 255], 0.2));
        put_opaque(&mut img, x, 5, mix(dark, [0, 0, 0], 0.45));
    }
    img
}

fn generate_mould_raw() -> RgbaImage {
    mould(WET_CLAY, WET_CLAY_DARK)
}

fn generate_mould() -> RgbaImage {
    mould(FIRED_CLAY, FIRED_CLAY_DARK)
}

/// An ingot mould: a slab with an ingot-shaped trough pressed into it.
fn mould(body: [u8; 3], dark: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 5..12 {
        for x in 2..14 {
            let roll = hash(0x0D1E, x as u32, y as u32) % 6;
            put_opaque(&mut img, x, y, if roll == 0 { dark } else { body });
        }
    }
    // The trough, which is the only thing that makes this a mould.
    for y in 7..10 {
        for x in 4..12 {
            let deep = y == 8;
            put_opaque(&mut img, x, y, mix(dark, [0, 0, 0], if deep { 0.55 } else { 0.3 }));
        }
    }
    img
}

/// A bloom: iron and slag in one lump, still glowing in the cracks.
fn generate_iron_bloom() -> RgbaImage {
    const SLAG: [u8; 3] = [72, 64, 60];
    const IRON: [u8; 3] = [148, 142, 138];
    const GLOW: [u8; 3] = [214, 116, 48];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 4..13 {
        for x in 3..13 {
            if (y == 4 || y == 12) && !(5..11).contains(&x) {
                continue;
            }
            let roll = hash(0xB100, x as u32, y as u32) % 8;
            let colour = match roll {
                0 | 1 => IRON,
                2 => mix(IRON, [255, 255, 255], 0.2),
                7 => GLOW,
                _ => SLAG,
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// The bloomery: a shaft of stone with a brick lining and a tuyere.
/// The bloomery's plain wall: coursed stone, no opening.
///
/// Same change the kiln had, for the same reason: a shaft with an arch
/// on all four sides is not a shaft, it is a ring of doorways.
fn generate_bloomery_side() -> RgbaImage {
    bloomery_side(false, false)
}

fn generate_bloomery_lit_side() -> RgbaImage {
    bloomery_side(true, false)
}

/// ...and the wall with the arch and the tuyere in it.
fn generate_bloomery_front() -> RgbaImage {
    bloomery_side(false, true)
}

fn generate_bloomery_lit_front() -> RgbaImage {
    bloomery_side(true, true)
}

fn bloomery_side(lit: bool, mouth: bool) -> RgbaImage {
    const STONE: [u8; 3] = [118, 114, 110];
    const STONE_DARK: [u8; 3] = [88, 84, 82];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            // Coursed stone: a joint every four rows.
            let joint = y % 4 == 0;
            let roll = hash(0xB10E, x as u32, y as u32) % 6;
            let colour = if joint {
                STONE_DARK
            } else if roll == 0 {
                mix(STONE, [255, 255, 255], 0.12)
            } else if roll == 5 {
                STONE_DARK
            } else {
                STONE
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    if !mouth {
        // The kiln's reason (see `kiln_side`): the lit wall was the cold
        // wall byte for byte, a layer spent on a copy. A shaft this hot
        // shows it at the joints, so the mortar of the two middle courses
        // glows in places -- not the top or bottom one, which sit against
        // the next block's and would glow in pairs across the join.
        if lit {
            for y in [4, 8] {
                for x in 0..RESOLUTION as i32 {
                    if hash(0x61E7, x as u32, y as u32).is_multiple_of(3) {
                        put_opaque(&mut img, x, y, mix(STONE_DARK, [236, 140, 44], 0.55));
                    }
                }
            }
        }
        return img;
    }
    // The brick lining showing through the arch, and the tuyere below
    // it: the hole the air is forced in through, which is the one thing
    // that separates a bloomery from a chimney.
    for y in 6..12 {
        for x in 6..10 {
            let arch = y == 6 && (x == 6 || x == 9);
            if arch {
                continue;
            }
            let colour = if lit {
                if hash(0xF1A3, x as u32, y as u32).is_multiple_of(3) {
                    [252, 214, 118]
                } else {
                    [236, 140, 44]
                }
            } else if hash(0x0B1C, x as u32, y as u32).is_multiple_of(4) {
                FIRED_CLAY_DARK
            } else {
                [42, 36, 34]
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

fn generate_bloomery_top() -> RgbaImage {
    bloomery_top(false)
}

fn generate_bloomery_lit_top() -> RgbaImage {
    bloomery_top(true)
}

/// The mouth of the shaft, seen from above.
fn bloomery_top(lit: bool) -> RgbaImage {
    const STONE: [u8; 3] = [118, 114, 110];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let roll = hash(0xB107, x as u32, y as u32) % 6;
            let colour = if roll == 0 {
                mix(STONE, [255, 255, 255], 0.1)
            } else if roll == 5 {
                mix(STONE, [0, 0, 0], 0.2)
            } else {
                STONE
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    for y in 4..12 {
        for x in 4..12 {
            let lip = x == 4 || x == 11 || y == 4 || y == 11;
            let colour = if lip {
                FIRED_CLAY_DARK
            } else if lit {
                if hash(0xF107, x as u32, y as u32).is_multiple_of(3) {
                    [252, 214, 118]
                } else {
                    [236, 140, 44]
                }
            } else {
                [38, 32, 30]
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// The colours a field is drawn in, from the seed to the loaf.
const SOIL: [u8; 3] = [92, 68, 48];
const SOIL_DARK: [u8; 3] = [70, 50, 36];
const STALK_GREEN: [u8; 3] = [110, 148, 68];
const STALK_GOLD: [u8; 3] = [206, 172, 78];
const GRAIN_GOLD: [u8; 3] = [214, 182, 96];
const CRUST: [u8; 3] = [166, 116, 62];

/// A hoe: a flint blade lashed *across* the end of a haft.
///
/// The one thing this picture has to say is that the blade is crosswise.
/// A blade in line with the haft is a knife, and at sixteen pixels that
/// is the only difference between the two.
fn generate_hoe() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The haft, corner to corner like every other hafted tool here.
    tool_haft(&mut img, 11);
    // The head, across the top end.
    const FLINT_PALE: [u8; 3] = [176, 172, 166];
    const FLINT_DARK: [u8; 3] = [104, 100, 96];
    for x in 2..10 {
        for y in 2..4 {
            let colour = if y == 2 && x % 3 != 0 { FLINT_PALE } else { FLINT_DARK };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The lashing where the two meet.
    for (x, y) in [(8, 4), (9, 4), (8, 5), (9, 5)] {
        put_opaque(&mut img, x, y, [138, 116, 74]);
    }
    img
}

/// Turned earth: soil in furrows.
///
/// The furrows are the whole picture. Tilled ground the same colour as
/// dirt with no lines in it is dirt, and a player has to be able to see
/// from across the field which part of it they have worked.
///
/// **Drawn at sixteen, not at eight.** It used to go through
/// `at_density(8)`, which is the right tool for a one-texel-wide tusk on
/// an animal and the wrong one for a whole block face: every texel came
/// out as a two-by-two square, so a tilled field laid next to plain
/// `dirt.png` -- which is drawn at sixteen -- looked like the same soil
/// photographed at half the resolution. Coarseness is a property of the
/// *part*, never of the ground.
///
/// What the furrows are made of changed with it. A dark row every third
/// line is a rake mark; a furrow is a trench, so each one is a dark
/// bottom with a lit crest on the side that faces the light, and the
/// clods that survived the plough sit along the ridges between them.
fn generate_farmland() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            // Four furrows across the tile, which at sixteen texels is a
            // trough four wide -- wide enough to have a floor and two
            // walls, and the smallest thing that has.
            let phase = y % 4;
            let clod = hash(0xF1E1, x, y) % 7;
            let base = match phase {
                // The floor of the furrow, in its own shadow.
                0 => mix(SOIL_DARK, [0, 0, 0], 0.34),
                // The far wall, coming up towards the light.
                1 => mix(SOIL_DARK, [0, 0, 0], 0.12),
                // The ridge: the top of the turned earth, and the only
                // part of a ploughed field the sun actually reaches.
                2 => mix(SOIL, [255, 255, 255], 0.16),
                _ => SOIL,
            };
            // Clods on the ridges and nothing in the troughs, because
            // that is where a plough leaves them.
            let colour = if phase >= 2 && clod == 0 {
                mix(SOIL, [255, 255, 255], 0.28)
            } else if phase >= 2 && clod == 6 {
                SOIL_DARK
            } else {
                base
            };
            let grain = (hash(0xF1E2, x, y) % 7) as i32 - 3;
            put_opaque(
                &mut img,
                x as i32,
                y as i32,
                [
                    clamp_u8(colour[0] as i32 + grain),
                    clamp_u8(colour[1] as i32 + grain),
                    clamp_u8(colour[2] as i32 + grain),
                ],
            );
        }
    }
    img
}

/// Seed in the ground: a few specks and two shoots, on nothing.
fn generate_seeds() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for (x, y) in [(6, 13), (7, 14), (9, 13), (10, 14), (8, 15)] {
        put_opaque(&mut img, x, y, mix(STALK_GREEN, [0, 0, 0], 0.25));
    }
    for (x, y) in [(6, 12), (10, 12)] {
        put_opaque(&mut img, x, y, STALK_GREEN);
    }
    img
}

/// The crop, half grown: green stalks, no ears.
fn generate_wheat_young() -> RgbaImage {
    wheat(STALK_GREEN, None, 7)
}

/// ...and ripe: gold, with the ears that are the whole point.
fn generate_wheat_ripe() -> RgbaImage {
    wheat(STALK_GOLD, Some(GRAIN_GOLD), 12)
}

/// A stand of wheat, as stalks rising from the bottom of the tile.
///
/// Drawn on two crossed planes, so everything that is not a stalk has to
/// be transparent -- see `is_cross`.
fn wheat(stalk: [u8; 3], ear: Option<[u8; 3]>, height: i32) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const COLUMNS: [i32; 5] = [2, 5, 8, 11, 14];
    for (index, x) in COLUMNS.into_iter().enumerate() {
        // Not all the same height: a row of identical stalks is a comb.
        let top = RESOLUTION as i32 - height - (index as i32 % 3);
        for y in top..RESOLUTION as i32 {
            let shade = if (x + y) % 4 == 0 {
                mix(stalk, [0, 0, 0], 0.2)
            } else {
                stalk
            };
            put_opaque(&mut img, x, y, shade);
        }
        if let Some(ear) = ear {
            // Three cells of head, and a fatter one in the middle --
            // which is what makes a ripe field read as ripe at a
            // distance rather than merely yellow.
            for y in top..(top + 3).min(RESOLUTION as i32) {
                put_opaque(&mut img, x, y, ear);
                if y == top + 1 {
                    put_opaque(&mut img, x - 1, y, mix(ear, [0, 0, 0], 0.15));
                    put_opaque(&mut img, x + 1, y, mix(ear, [0, 0, 0], 0.15));
                }
            }
        }
    }
    img
}

/// Threshed grain: a heap of seed.
fn generate_grain() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 8..13 {
        for x in 3..13 {
            // A heap, so the top row is narrower than the bottom.
            let inset = (12 - y) / 2;
            if !(3 + inset..13 - inset).contains(&x) {
                continue;
            }
            // **Grains, not sand.** Per-texel noise over a heap gives a
            // pile of *powder*, and this is threshed wheat -- a heap of
            // separate hard things, each of which catches the light on
            // one side. Sampling the hash in pairs across and singly
            // down makes every grain two texels wide with its own
            // shadow beside it, which at hotbar size is exactly as much
            // grain as the eye can count.
            let seed = hash(0x6A1E, (x / 2) as u32, y as u32);
            let colour = match seed % 5 {
                0 => mix(GRAIN_GOLD, [0, 0, 0], 0.3),
                1 => mix(GRAIN_GOLD, [0, 0, 0], 0.14),
                4 => mix(GRAIN_GOLD, [255, 255, 255], 0.22),
                _ => GRAIN_GOLD,
            };
            // The right-hand texel of each pair is the shaded side of
            // its grain. One comparison, and it is what stops the heap
            // reading as a woven mat.
            let colour = if x % 2 == 1 {
                mix(colour, [0, 0, 0], 0.12)
            } else {
                colour
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// Dough: a pale round lump.
fn generate_dough() -> RgbaImage {
    round_loaf([224, 208, 176], [196, 178, 146], 0)
}

/// Bread: the same lump, baked, with a slash across the top.
fn generate_bread() -> RgbaImage {
    round_loaf(CRUST, mix(CRUST, [0, 0, 0], 0.3), 2)
}

/// A round thing sitting in the middle of the tile.
///
/// **It domes now.** Two flat bands -- a top colour and a side colour
/// with a straight join between them -- is a slab seen edge on, and both
/// things drawn through here are supposed to be lumps somebody rolled by
/// hand. What turns the slab into a lump is that the light falls off
/// towards every edge instead of at one line: the same two colours, the
/// same silhouette, one term more.
fn round_loaf(top: [u8; 3], side: [u8; 3], slashes: i32) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 5..12 {
        for x in 3..13 {
            // Corners cut, which is as round as sixteen pixels get.
            let edge = (y == 5 || y == 11) && !(5..11).contains(&x);
            if edge {
                continue;
            }
            let low = y >= 10;
            let base = if low { side } else { top };
            // How far from the lit shoulder, which is up and to the
            // left of the middle like everything else in this folder.
            let away = (((x - 6) * (x - 6)) as f32 / 16.0
                + ((y - 7) * (y - 7)) as f32 / 6.0)
                .sqrt();
            let base = if away < 0.6 {
                mix(base, [255, 255, 255], 0.18)
            } else if away > 1.7 {
                mix(base, side, 0.6)
            } else {
                base
            };
            put_opaque(&mut img, x, y, base);
        }
    }
    for slash in 0..slashes {
        let x = 6 + slash * 3;
        for y in 6..9 {
            put_opaque(&mut img, x + (y - 6), y, mix(side, [0, 0, 0], 0.35));
        }
    }
    img
}

/// The colours the kiln and the brick are both mixed from.
///
/// One palette for the two, because they are the same material at two
/// stages: a kiln is daub that has been fired in place by what happens
/// inside it, and a brick is the same clay fired on purpose. Getting
/// them from one list is what stops a player's brickwork from looking
/// like it came out of somebody else's kiln.
const DAUB: [u8; 3] = [150, 96, 68];
const DAUB_DARK: [u8; 3] = [116, 70, 48];
const DAUB_PALE: [u8; 3] = [176, 122, 92];
const MORTAR: [u8; 3] = [138, 130, 116];
const SOOT: [u8; 3] = [46, 38, 34];
const EMBER: [u8; 3] = [232, 138, 44];
const EMBER_HOT: [u8; 3] = [252, 214, 118];

/// The side of a kiln: a footing of stone under a chimney of daub, with
/// the stoke hole in the middle of it.
///
/// The hole is the whole picture. A kiln is a cube of one colour without
/// it, and the one thing a player has to be able to see from across a
/// clearing is which face they feed.
/// The kiln's plain wall: daub on a stone footing, and nothing else.
///
/// **The mouth is not on it any more.** Every side of a kiln wore the
/// stoke hole, so a kiln in the world was a box with four mouths and no
/// back -- which is what "one texture on most of the sides" means. The
/// opening is now the *front*, and the block knows which way it is
/// looking (see `types::Facing`).
fn generate_kiln_side() -> RgbaImage {
    kiln_side(None, false)
}

fn generate_kiln_lit_side() -> RgbaImage {
    kiln_side(Some(()), false)
}

/// ...and the wall you feed it through.
fn generate_kiln_front() -> RgbaImage {
    kiln_side(None, true)
}

fn generate_kiln_lit_front() -> RgbaImage {
    kiln_side(Some(()), true)
}

fn kiln_side(lit: Option<()>, mouth: bool) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let roll = hash(0x51A7, x as u32, y as u32) % 8;
            // The bottom three rows are the stone footing the daub is
            // built on -- the same ring of cobble a campfire sits in,
            // which is what the recipe asks for.
            let base = if y >= 13 {
                if roll < 3 { STONE_DARK_KILN } else { STONE_KILN }
            } else if roll < 2 {
                DAUB_DARK
            } else if roll == 7 {
                DAUB_PALE
            } else {
                DAUB
            };
            put_opaque(&mut img, x, y, base);
        }
    }
    if !mouth {
        // **A burning kiln's plain wall is not the cold one.** It was, byte
        // for byte -- two files, two layers of the atlas, and nothing on
        // screen for the second: from anywhere but the front a lit kiln and
        // a cold one were the same box, and the only way to find out was to
        // walk round to the mouth. Heat gets out where the daub has cracked,
        // so two cracks under the rim glow, and they are high on the wall
        // because that is where a player looking across a yard sees it.
        if lit.is_some() {
            for &(x, y, hot) in &[
                (4, 1, false),
                (5, 2, true),
                (5, 3, false),
                (6, 4, false),
                (11, 2, false),
                (10, 3, true),
                (10, 4, false),
            ] {
                put_opaque(&mut img, x, y, if hot { EMBER_HOT } else { mix(DAUB_DARK, EMBER, 0.65) });
            }
        }
        return img;
    }
    // The stoke hole: an arch four wide and five tall, sooted round the
    // lip because that is where the smoke comes out.
    for y in 7..13 {
        for x in 6..10 {
            let arch = y == 7 && (x == 6 || x == 9);
            if arch {
                continue;
            }
            let colour = match lit {
                None => SOOT,
                Some(()) => {
                    // Hottest low in the middle where the fuel sits,
                    // and flickering everywhere else.
                    let core = (7..9).contains(&x) && y >= 9;
                    if core || hash(0xE41B, x as u32, y as u32).is_multiple_of(3) {
                        EMBER_HOT
                    } else {
                        EMBER
                    }
                }
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // Soot above the mouth, lit or not: a kiln that has ever been fired
    // is stained, and one that has not is a kiln nobody has used.
    //
    // Kept sparse and kept *narrower than the mouth*. A denser stain the
    // same width joins onto the opening, and the two together read as
    // one tall dark shape standing against the wall -- which is not a
    // kiln, it is a doorway with somebody in it.
    for y in 5..7 {
        for x in 7..9 {
            if hash(0x50FA, x as u32, y as u32).is_multiple_of(3) {
                put_opaque(&mut img, x, y, mix(DAUB_DARK, SOOT, 0.6));
            }
        }
    }
    img
}

const STONE_KILN: [u8; 3] = [126, 124, 120];
const STONE_DARK_KILN: [u8; 3] = [92, 90, 88];

/// The top of a kiln: daub with the chimney opening in the middle.
fn generate_kiln_top() -> RgbaImage {
    kiln_top(false)
}

/// ...and with the fire showing up it.
fn generate_kiln_lit_top() -> RgbaImage {
    kiln_top(true)
}

fn kiln_top(lit: bool) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let roll = hash(0x7C3D, x as u32, y as u32) % 8;
            let base = if roll < 2 {
                DAUB_DARK
            } else if roll == 7 {
                DAUB_PALE
            } else {
                DAUB
            };
            put_opaque(&mut img, x, y, base);
        }
    }
    // The flue, square because everything here is.
    for y in 5..11 {
        for x in 5..11 {
            let lip = x == 5 || x == 10 || y == 5 || y == 10;
            let colour = if lip {
                SOOT
            } else if lit {
                if hash(0x9B22, x as u32, y as u32).is_multiple_of(3) {
                    EMBER_HOT
                } else {
                    EMBER
                }
            } else {
                // Ash and cold charcoal down the shaft.
                if hash(0x9B22, x as u32, y as u32).is_multiple_of(3) {
                    [70, 66, 62]
                } else {
                    SOOT
                }
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// A single fired brick, lying on its face. An item, so everything
/// around it is transparent.
fn generate_brick() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // **Lit from the top left, like every other item in the pack.** It
    // used to be a flat slab with a dark outline and an occasional pale
    // speckle -- four colours in the whole icon -- which reads as a
    // sticker rather than as a thing with a near side and a far side.
    // What makes a brick look solid at sixteen pixels is not detail: it
    // is that the top course catches the light and the underside does
    // not, so the eye is told which way is up before it is told what
    // the object is.
    const TOP: i32 = 6;
    const BOTTOM: i32 = 10;
    for y in TOP..=BOTTOM {
        for x in 2..14 {
            let lit_face = y == TOP;
            let underside = y == BOTTOM;
            let left_edge = x == 2;
            let right_edge = x == 13;
            // A dry, uneven face: two thirds ordinary clay, a scatter of
            // paler grains and a few darker ones, so no two bricks in a
            // stack look stamped from the same die.
            let grain = hash(0xB21C, x as u32, y as u32) % 9;
            let face = match grain {
                0 | 1 => DAUB_PALE,
                2 => mix(DAUB, DAUB_DARK, 0.45),
                _ => DAUB,
            };
            let colour = if underside {
                // Darkest: the face turned away from the light.
                mix(DAUB_DARK, [0, 0, 0], 0.25)
            } else if lit_face {
                mix(face, DAUB_PALE, 0.65)
            } else if right_edge {
                DAUB_DARK
            } else if left_edge {
                mix(face, DAUB_PALE, 0.25)
            } else {
                face
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// Brickwork: courses of brick with the joints offset, which is the
/// whole of what makes a wall read as laid rather than as stacked.
fn generate_bricks() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const COURSE: i32 = 4;
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let course = y / COURSE;
            // Every other course starts half a brick along.
            let offset = if course % 2 == 0 { 0 } else { 4 };
            let bed = y % COURSE == 0;
            let joint = (x + offset).rem_euclid(8) == 0;
            let colour = if bed || joint {
                MORTAR
            } else {
                let roll = hash(0x3E90 + course as u32, x as u32, y as u32) % 7;
                if roll == 0 {
                    DAUB_DARK
                } else if roll == 6 {
                    DAUB_PALE
                } else {
                    DAUB
                }
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// Sandstone bricks: longer, paler blocks than fired brick in thin sandy
/// joints, each block a little lighter or darker than its neighbour the way
/// cut stone from different beds is.
fn generate_sandstone_bricks() -> RgbaImage {
    const STONE: [u8; 3] = [214, 190, 140];
    const STONE_DARK: [u8; 3] = [190, 164, 116];
    const STONE_PALE: [u8; 3] = [228, 210, 166];
    const JOINT: [u8; 3] = [168, 146, 106];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const COURSE: i32 = 4;
    const BLOCK: i32 = 8;
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let course = y / COURSE;
            let offset = if course % 2 == 0 { 0 } else { BLOCK / 2 };
            let bed = y % COURSE == 0;
            let joint = (x + offset).rem_euclid(BLOCK) == 0;
            let colour = if bed || joint {
                JOINT
            } else {
                let block = (x + offset).div_euclid(BLOCK);
                let tone = hash(0x5A4D + course as u32, block as u32, 7) % 5;
                let grain = hash(0x5A4E, x as u32, y as u32) % 9;
                match (tone, grain) {
                    (_, 0) => STONE_DARK,
                    (_, 8) => STONE_PALE,
                    (0, _) => STONE_DARK,
                    (4, _) => STONE_PALE,
                    _ => STONE,
                }
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// The nine metal tools: (file, shape, metal).
///
/// One table for three shapes times three metals, because that is
/// exactly what the tools *are* -- see `types::BLOCK_COPPER_KNIFE`. The
/// colours are the ingots' own, so a player can tell at a glance which
/// bar their axe came out of.
///
/// One colour a metal and not two. The highlight used to be a second
/// column here, and it was a second number to keep in step with the
/// ingot's for no gain: `forge` builds the whole five-step ramp by
/// scaling the one colour, so a polished head is the same metal by
/// construction rather than by somebody remembering.
const METAL_TOOLS: &[(&str, ToolShape, [u8; 3])] = &[
    ("tools/copper_knife.png", ToolShape::Knife, COPPER),
    // The copper axe and pick are **not here**, and neither are the
    // shovel, the hoe or the four heads: somebody drew them, and a
    // drawing beats a generator. `write` would refuse to overwrite them
    // anyway, but a row that names a file this program is no longer the
    // author of is a row that lies -- and a `--force` run would cash
    // that lie in for a folder of destroyed artwork.
    ("tools/bronze_knife.png", ToolShape::Knife, BRONZE),
    ("tools/bronze_axe.png", ToolShape::Axe, BRONZE),
    ("tools/bronze_pickaxe.png", ToolShape::Pick, BRONZE),
    ("tools/iron_knife.png", ToolShape::Knife, IRON),
    ("tools/iron_axe.png", ToolShape::Axe, IRON),
    ("tools/iron_pickaxe.png", ToolShape::Pick, IRON),
];

/// The same three metals the ingots are drawn in. Named separately so a
/// tool and the bar it came out of can never drift apart.
const COPPER: [u8; 3] = [186, 108, 62];
const BRONZE: [u8; 3] = [176, 134, 66];
const IRON: [u8; 3] = [154, 152, 148];

/// Which of the three a metal tool is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolShape {
    Knife,
    Axe,
    Pick,
}

/// The three animal skins.
///
/// Speckled two-tone, generated by the same function the plain blocks
/// use, because an animal here is a box wearing one texture and what it
/// needs from that texture is a *colour with a grain in it*. Anything
/// more -- eyes, a face, a marking -- would be drawn on all six faces of
/// a box, which is worse than a hide.
/// What colour each animal is, in `Species::ALL` order.
///
/// The body picture is still generated the same way it always was; what
/// changed is where it ends up -- slot 0 of that animal's sheet rather
/// than a file of its own. See `ANIMAL_SHEETS`.
const PELTS: &[(&str, [u8; 3], i32)] = &[
    // A hare: dust-brown, so it disappears against a summer meadow at
    // twenty paces, which is exactly the right amount of frustrating.
    ("hare", HARE_PELT, 20),
    // A deer: warmer and redder, and the largest of the three, so it is
    // the one a player spots first across open country.
    ("deer", DEER_PELT, 18),
    // A boar, which is drawn rather than speckled -- see
    // `generate_boar_hide`.
    ("boar", BOAR_HIDE, 0),
    // A wolf: grey, and *cooler* than anything else alive here. Every
    // other animal in this world is some shade of the ground it stands
    // on, which is the point of them -- they are hard to see. A wolf is
    // the one that has to read instantly at a distance, because seeing
    // it late is the difference between a decision and a fight, so it
    // is the one colour a meadow does not contain.
    ("wolf", WOLF_PELT, 16),
    // A sheep: the palest thing alive here, and deliberately so. Every
    // other animal is some shade of the ground it stands on, because
    // being hard to see is what they are for. This one is meant to be
    // spotted from the other side of a field -- it is a supply, not a
    // hunt, and a player who has to search for a flock is a player
    // doing the boring half of the job twice.
    ("sheep", SHEEP_FLEECE, 8),
];

/// Every picture one animal wears, in `Skin::slot` order.
///
/// **One sheet per animal instead of nine files.** A wolf used to be a
/// pelt, an eye, the flip of the eye, a muzzle, a nose, an ear, a paw
/// and a coarse weave; every one of them was a line in this table, a
/// line in `EXTRA_TEXTURES`, a line in `embedded.rs` and an arm of a
/// per-species match. It is one picture now, and where each part sits on
/// it is `animal_model::Skin::slot` -- the single place that decides.
///
/// A `None` is a part this animal has not got. It comes out transparent,
/// and the loader reads a blank tile as "wears the hide instead" -- so
/// giving a hare tusks is drawing them here and nothing else anywhere.
/// The slot order, spelled out so this table and `Skin::slot` can be
/// read against each other.
const SLOT_HIDE: usize = 0;
const SLOT_HEAD: usize = 1;
const SLOT_HEAD_M: usize = 2;
const SLOT_FACE: usize = 3;
const SLOT_SNOUT: usize = 4;
const SLOT_NOSE: usize = 5;
const SLOT_EAR: usize = 6;
const SLOT_HOOF: usize = 7;
const SLOT_FUR: usize = 8;
const SLOT_TUSK: usize = 9;
const SLOT_ANTLER: usize = 10;
const SHEET_COLUMNS: u32 = 4;
const SHEET_ROWS: u32 = 3;
const SHEET_SLOTS: usize = (SHEET_COLUMNS * SHEET_ROWS) as usize;

/// Draws one animal's whole sheet.
fn animal_sheet(animal: usize) -> RgbaImage {
    let mut sheet = RgbaImage::new(RESOLUTION * SHEET_COLUMNS, RESOLUTION * SHEET_ROWS);
    for slot in 0..SHEET_SLOTS {
        let Some(tile) = animal_tile(animal, slot) else {
            continue; // left blank: this animal has no such part
        };
        let (column, row) = (slot as u32 % SHEET_COLUMNS, slot as u32 / SHEET_COLUMNS);
        image::imageops::overlay(
            &mut sheet,
            &tile,
            (column * RESOLUTION) as i64,
            (row * RESOLUTION) as i64,
        );
    }
    sheet
}

/// One tile of one animal, or `None` where it has nothing.
fn animal_tile(animal: usize, slot: usize) -> Option<RgbaImage> {
    let (_, pelt, speckle) = PELTS[animal];
    let hide = || {
        if animal == 2 {
            generate_boar_hide()
        } else {
            generate(pelt, speckle, "hide")
        }
    };
    Some(match (animal, slot) {
        (_, SLOT_HIDE) => hide(),
        (_, SLOT_FUR) => coarse_fur(pelt),
        (_, SLOT_FACE) => muzzle_face(pelt),
        (_, SLOT_EAR) => ear(pelt, ear_inside(animal)),
        // The boar is the one animal drawn rather than speckled, so its
        // head, snout and nose are their own pictures.
        (2, SLOT_HEAD) => generate_boar_head(),
        (2, SLOT_HEAD_M) => mirrored(&generate_boar_head()),
        (2, SLOT_SNOUT) => generate_boar_snout(),
        (2, SLOT_NOSE) => generate_boar_nose(),
        (2, SLOT_TUSK) => generate_boar_tusk(),
        (2, SLOT_HOOF) => generate_boar_hoof(),
        // Everything else: an eye on its own hide, a muzzle in its own
        // key, and a foot.
        (_, SLOT_HEAD) => generate_eyed_head(pelt, speckle, "head"),
        (_, SLOT_HEAD_M) => mirrored(&generate_eyed_head(pelt, speckle, "head")),
        (_, SLOT_SNOUT) => bare_muzzle(muzzle_skin(animal)),
        (_, SLOT_NOSE) => nose_front(muzzle_skin(animal)),
        (3, SLOT_HOOF) => generate_wolf_paw(),
        (1, SLOT_HOOF) => generate_deer_hoof(),
        // A sheep is cloven-hoofed like a deer, and the picture is four
        // dark pixels either way -- a second one would be a layer spent
        // on a difference nobody can see.
        (4, SLOT_HOOF) => generate_deer_hoof(),
        (1, SLOT_ANTLER) => generate_deer_antler(),
        _ => return None,
    })
}

/// The bare skin of an animal's muzzle.
fn muzzle_skin(animal: usize) -> [u8; 3] {
    match animal {
        0 => [176, 148, 124], // hare
        1 => [74, 58, 48],    // deer
        2 => BOAR_SNOUT,
        3 => [78, 74, 76],    // wolf
        // A sheep's face is the dark part of it, and much darker than
        // the fleece: it is the contrast that makes the head legible
        // against the body at any distance worth seeing one from.
        _ => [64, 58, 54], // sheep
    }
}

/// ...and the inside of its ear.
fn ear_inside(animal: usize) -> [u8; 3] {
    match animal {
        0 => [212, 168, 154],
        1 => [198, 156, 132],
        2 => [172, 128, 118],
        3 => [148, 128, 124],
        _ => [186, 152, 142], // sheep
    }
}

/// Berries on a bush: a tangle of dark leaves with red in it.
///
/// The berries are what makes this legible from a distance, so there are
/// more of them than a real bush would carry and they are the brightest
/// thing in the tile.
fn generate_berry_bush() -> RgbaImage {
    let mut img = generate_bare_bush();
    const BERRY: [u8; 3] = [186, 44, 52];
    const BERRY_LIT: [u8; 3] = [226, 92, 88];
    for &(x, y) in &[(4, 7), (7, 5), (10, 8), (5, 11), (11, 12), (8, 10)] {
        put_opaque(&mut img, x, y, BERRY);
        put_opaque(&mut img, x + 1, y, BERRY);
        put_opaque(&mut img, x, y + 1, BERRY);
        put_opaque(&mut img, x + 1, y + 1, BERRY_LIT);
    }
    img
}

/// The same bush with nothing on it. Drawn first and shared, so a picked
/// bush is visibly the *same plant* rather than a different one -- which
/// is the whole point of it being a bush you come back to.
fn generate_bare_bush() -> RgbaImage {
    const LEAF: [u8; 3] = [58, 96, 48];
    const LEAF_DARK: [u8; 3] = [40, 70, 36];
    const LEAF_PALE: [u8; 3] = [84, 124, 60];
    const STEM: [u8; 3] = [86, 66, 42];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // A stem up the middle, forking near the top.
    for y in 6..RESOLUTION as i32 {
        put_opaque(&mut img, 7, y, STEM);
    }
    for step in 0..4 {
        put_opaque(&mut img, 7 - step, 8 - step / 2, STEM);
        put_opaque(&mut img, 8 + step, 8 - step / 2, STEM);
    }
    // Foliage, thickest in the middle and ragged at the edges: a bush
    // with a smooth outline reads as a hedge somebody has clipped.
    for y in 3..14i32 {
        for x in 2..14i32 {
            let dx = (x as f32 - 7.5) / 6.0;
            let dy = (y as f32 - 8.0) / 5.5;
            if dx * dx + dy * dy > 1.0 {
                continue;
            }
            let roll = hash(0x8005, x as u32, y as u32) % 10;
            if roll < 2 {
                continue; // the ragged edge, and the gaps you see through
            }
            let colour = match roll {
                2..=4 => LEAF_DARK,
                5..=7 => LEAF,
                _ => LEAF_PALE,
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// A mushroom: a pale stalk under a domed cap.
fn generate_mushroom() -> RgbaImage {
    const CAP: [u8; 3] = [166, 74, 58];
    const CAP_DARK: [u8; 3] = [122, 50, 40];
    const CAP_LIT: [u8; 3] = [200, 112, 88];
    const STALK: [u8; 3] = [226, 214, 190];
    const STALK_DARK: [u8; 3] = [186, 172, 148];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 9..15i32 {
        put_opaque(&mut img, 7, y, STALK);
        put_opaque(&mut img, 8, y, STALK_DARK);
    }
    // The cap: a half-ellipse, lit on the left.
    for y in 4..10i32 {
        for x in 2..14i32 {
            let dx = (x as f32 - 7.5) / 5.5;
            let dy = (y as f32 - 9.0) / 5.0;
            if dx * dx + dy * dy > 1.0 {
                continue;
            }
            let colour = if x < 6 {
                CAP_LIT
            } else if x > 10 {
                CAP_DARK
            } else {
                CAP
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // Gills: one row of shadow under the rim, which is what tells a
    // mushroom from a red ball on a stick.
    for x in 3..13i32 {
        put_opaque(&mut img, x, 9, CAP_DARK);
    }
    img
}

/// A toadstool: the mushroom's silhouette, spotted.
///
/// **Drawn from the same shape on purpose.** What makes this worth
/// having in the world is that a player has to look, and a warning that
/// is a different *shape* is a warning nobody has to learn -- they would
/// simply never pick up the second thing. Same cap, same stalk, a
/// paler and colder colour, and the white flecks every poisonous
/// mushroom on earth advertises itself with.
fn generate_toadstool() -> RgbaImage {
    const CAP: [u8; 3] = [186, 84, 96];
    const CAP_DARK: [u8; 3] = [140, 58, 70];
    const CAP_LIT: [u8; 3] = [214, 118, 128];
    const SPOT: [u8; 3] = [238, 234, 226];
    const STALK: [u8; 3] = [232, 226, 210];
    const STALK_DARK: [u8; 3] = [192, 184, 166];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 9..15i32 {
        put_opaque(&mut img, 7, y, STALK);
        put_opaque(&mut img, 8, y, STALK_DARK);
    }
    // The ring: a skirt of veil left on the stalk, which is the other
    // thing a field guide tells you to look for.
    put_opaque(&mut img, 6, 11, SPOT);
    put_opaque(&mut img, 7, 11, SPOT);
    put_opaque(&mut img, 8, 11, SPOT);
    put_opaque(&mut img, 9, 11, STALK_DARK);

    for y in 4..10i32 {
        for x in 2..14i32 {
            let dx = (x as f32 - 7.5) / 5.5;
            let dy = (y as f32 - 9.0) / 5.0;
            if dx * dx + dy * dy > 1.0 {
                continue;
            }
            let colour = if x < 6 {
                CAP_LIT
            } else if x > 10 {
                CAP_DARK
            } else {
                CAP
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The flecks. Placed rather than scattered by hash: four of them at
    // sixteen texels is a pattern a person recognises, and a random one
    // is a smudge.
    for &(x, y) in &[(4i32, 7i32), (7, 5), (10, 8), (11, 6), (6, 8)] {
        put_opaque(&mut img, x, y, SPOT);
    }
    for x in 3..13i32 {
        put_opaque(&mut img, x, 9, CAP_DARK);
    }
    img
}

/// The leaves of a root vegetable: a low rosette, nothing above the
/// knee.
///
/// It has to read as *not* tall grass from a few paces, or a player
/// walking a meadow would never look down. So the fronds arc outward
/// from one point instead of standing up in a row, and there is a
/// shoulder of the root itself showing at the base -- the one cue that
/// says there is something under it.
fn generate_roots_plant() -> RgbaImage {
    const LEAF: [u8; 3] = [96, 146, 66];
    const LEAF_DARK: [u8; 3] = [70, 112, 50];
    const LEAF_PALE: [u8; 3] = [132, 172, 84];
    const CROWN: [u8; 3] = [214, 186, 132];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // Five fronds, each a short walk out and up from the crown.
    for (index, (dx, dy, length)) in [
        (-2.0f32, -1.0f32, 6),
        (-1.0, -1.4, 8),
        (0.0, -1.5, 9),
        (1.0, -1.4, 8),
        (2.0, -1.0, 6),
    ]
    .into_iter()
    .enumerate()
    {
        let colour = match index {
            0 | 1 => LEAF_PALE,
            4 => LEAF_DARK,
            _ => LEAF,
        };
        let (mut x, mut y) = (7.5f32, 13.0f32);
        for step in 0..length {
            x += dx * 0.35;
            y += dy * 0.55;
            put_opaque(&mut img, x.round() as i32, y.round() as i32, colour);
            // The fronds thicken at the base and thin at the tip, which
            // is what keeps them from reading as wire.
            if step < length / 2 {
                put_opaque(&mut img, x.round() as i32, (y + 1.0).round() as i32, LEAF_DARK);
            }
        }
    }
    // The crown of the root, just breaking the soil.
    for x in 6..10i32 {
        put_opaque(&mut img, x, 14, CROWN);
    }
    put_opaque(&mut img, 7, 15, CROWN);
    put_opaque(&mut img, 8, 15, CROWN);
    img
}

/// A root, as it comes out of the ground: pale, tapering, with the
/// leaves cut short.
///
/// **Drawn as a taper down the middle rather than as a diagonal.** The
/// first version stepped a pixel across for every pixel down, which at
/// sixteen texels is a chequerboard: single pixels touching at the
/// corners, with daylight between them. A shape this small has to be
/// built out of *rows*.
fn generate_root() -> RgbaImage {
    const BODY: [u8; 3] = [226, 200, 146];
    const BODY_DARK: [u8; 3] = [192, 164, 112];
    const BODY_LIT: [u8; 3] = [242, 224, 180];
    const RING: [u8; 3] = [204, 174, 118];
    const TOP: [u8; 3] = [96, 146, 66];
    const TOP_DARK: [u8; 3] = [70, 112, 50];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // Shoulder to tip: five texels across at the top, one at the
    // bottom, which is the shape of every root anybody has pulled up.
    const ROWS: [(i32, i32); 10] = [
        (5, 10),
        (5, 10),
        (5, 10),
        (6, 10),
        (6, 10),
        (6, 9),
        (6, 9),
        (7, 9),
        (7, 8),
        (7, 8),
    ];
    for (index, (x0, x1)) in ROWS.into_iter().enumerate() {
        let y = 5 + index as i32;
        for x in x0..x1 {
            let colour = if x == x0 {
                BODY_LIT
            } else if x >= x1 - 1 {
                BODY_DARK
            } else {
                BODY
            };
            put_opaque(&mut img, x, y, colour);
        }
        // The rings a root grows in, across the whole width so they read
        // as growth rather than as damage.
        if index % 3 == 2 {
            for x in x0..x1 {
                put_opaque(&mut img, x, y, RING);
            }
        }
    }
    // What is left of the leaves, cut short.
    for &(x, y, colour) in &[
        (6i32, 4i32, TOP),
        (7, 3, TOP),
        (8, 4, TOP_DARK),
        (5, 3, TOP_DARK),
        (9, 3, TOP),
    ] {
        put_opaque(&mut img, x, y, colour);
    }
    img
}

/// The same root after a fire has had it: darker, shrunken, marked.
///
/// Shorter and narrower than the raw one on purpose -- a root loses its
/// water over coals -- so the two are told apart in a hotbar by
/// silhouette as well as by colour, which is the rule the whole food
/// half of this pack is drawn to.
fn generate_roasted_root() -> RgbaImage {
    const BODY: [u8; 3] = [166, 116, 62];
    const BODY_DARK: [u8; 3] = [124, 82, 44];
    const BODY_LIT: [u8; 3] = [196, 146, 84];
    const CHAR: [u8; 3] = [72, 50, 34];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // **Shrunken, not starved.** It was four texels across and nine
    // tall, drawn a texel narrower than the raw root at every row, and
    // what came out was a burnt twig: at hotbar size the eye reads a
    // three-texel column as a stick, whatever colour it is. A root over
    // coals loses its water, which makes it *shorter and wrinkled*, not
    // thinner -- so the loss goes into the length and the widest part
    // stays wide enough to be food.
    const ROWS: [(i32, i32); 8] = [
        (5, 11),
        (5, 11),
        (5, 11),
        (5, 10),
        (6, 10),
        (6, 10),
        (7, 10),
        (7, 9),
    ];
    for (index, (x0, x1)) in ROWS.into_iter().enumerate() {
        let y = 6 + index as i32;
        for x in x0..x1 {
            let colour = if x == x0 {
                BODY_LIT
            } else if x >= x1 - 1 {
                BODY_DARK
            } else {
                BODY
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The wrinkles: short dark strokes *across* the root, which is the
    // direction skin shrinks in. Along it they would read as a grain and
    // this would be a piece of wood again.
    for &(x, y) in &[(6i32, 8i32), (7, 8), (7, 11), (8, 11)] {
        put_opaque(&mut img, x, y, BODY_DARK);
    }
    // Where it sat on the coals: three marks down one side, not a
    // pattern across the whole thing.
    for &(x, y) in &[(9i32, 7i32), (9, 10), (8, 12)] {
        put_opaque(&mut img, x, y, CHAR);
    }
    // The cut shoulder, pale where the greens were twisted off. It is
    // the one bright texel in a dark picture and it is what tells a
    // player which end was up.
    for x in 6..9i32 {
        put_opaque(&mut img, x, 6, mix(BODY_LIT, [255, 255, 255], 0.35));
    }
    img
}

/// Reeds: tall straight stems with a seed head on two of them.
fn generate_reeds() -> RgbaImage {
    const STEM: [u8; 3] = [116, 148, 78];
    const STEM_DARK: [u8; 3] = [82, 112, 58];
    const STEM_PALE: [u8; 3] = [150, 176, 100];
    const HEAD: [u8; 3] = [122, 94, 58];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // (root x, lean, top y, colour)
    let stems: [(i32, f32, i32, [u8; 3]); 5] = [
        (3, -0.8, 4, STEM_DARK),
        (6, -0.2, 1, STEM),
        (8, 0.2, 0, STEM_PALE),
        (11, 0.7, 3, STEM),
        (13, 1.2, 6, STEM_DARK),
    ];
    for &(root, lean, top, colour) in &stems {
        for y in top..RESOLUTION as i32 {
            let t = (RESOLUTION as i32 - y) as f32 / (RESOLUTION as i32 - top) as f32;
            let x = root + (lean * t * t * 4.0) as i32;
            put_opaque(&mut img, x, y, colour);
        }
    }
    // Two seed heads, because a stand of reeds with none reads as very
    // tall grass.
    for &(x, y) in &[(6, 1), (11, 3)] {
        for dy in 0..3 {
            put_opaque(&mut img, x, y + dy, HEAD);
            put_opaque(&mut img, x + 1, y + dy, HEAD);
        }
    }
    img
}

/// A flower: a short stem, two leaves, and a head of petals.
fn generate_flower() -> RgbaImage {
    const STEM: [u8; 3] = [76, 128, 60];
    const LEAF: [u8; 3] = [96, 152, 70];
    const PETAL: [u8; 3] = [216, 96, 132];
    const PETAL_PALE: [u8; 3] = [242, 152, 178];
    const HEART: [u8; 3] = [246, 216, 96];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 7..RESOLUTION as i32 {
        put_opaque(&mut img, 7, y, STEM);
    }
    for &(x, y) in &[(5, 10), (4, 11), (9, 12), (10, 13)] {
        put_opaque(&mut img, x, y, LEAF);
    }
    // Five petals around a heart, which is the smallest arrangement that
    // still reads as a flower rather than as a berry.
    for &(x, y) in &[(7, 3), (5, 5), (9, 5), (6, 7), (8, 7)] {
        put_opaque(&mut img, x, y, PETAL);
        put_opaque(&mut img, x + 1, y, PETAL_PALE);
        put_opaque(&mut img, x, y + 1, PETAL);
    }
    put_opaque(&mut img, 7, 5, HEART);
    put_opaque(&mut img, 8, 5, HEART);
    put_opaque(&mut img, 7, 6, HEART);
    img
}

/// A handful of berries, seen against nothing.
///
/// A cluster rather than one, so it cannot be mistaken for the single
/// round thing every other small item in the pack is.
fn generate_berries() -> RgbaImage {
    const BERRY: [u8; 3] = [176, 38, 46];
    const BERRY_LIT: [u8; 3] = [222, 88, 84];
    const BERRY_DARK: [u8; 3] = [122, 24, 34];
    const LEAF: [u8; 3] = [64, 106, 52];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for &(cx, cy, r) in &[(6.0f32, 9.0f32, 3.0f32), (10.0, 7.0, 2.6), (9.0, 11.0, 2.4)] {
        for y in 0..RESOLUTION as i32 {
            for x in 0..RESOLUTION as i32 {
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                if dx * dx + dy * dy > r * r {
                    continue;
                }
                let colour = if dx + dy < -r * 0.6 {
                    BERRY_LIT
                } else if dx + dy > r * 0.7 {
                    BERRY_DARK
                } else {
                    BERRY
                };
                put_opaque(&mut img, x, y, colour);
            }
        }
    }
    // A leaf on top, which is what stops three red circles reading as
    // something inedible.
    for &(x, y) in &[(11, 4), (12, 4), (12, 3), (13, 3)] {
        put_opaque(&mut img, x, y, LEAF);
    }
    img
}

/// Raw meat: a cut with a bone in it, drawn cool and pink.
fn generate_raw_meat() -> RgbaImage {
    meat(
        [206, 106, 108],
        [232, 148, 148],
        [162, 70, 78],
        [238, 232, 214],
    )
}

/// Cooked meat: the same cut, browner and darker at the edges.
///
/// **The same silhouette on purpose.** A player has to be able to tell
/// the two apart in the pack, and the way to do that is colour rather
/// than shape -- because they are the same object, and drawing the
/// cooked one as a different thing entirely would hide the fact that the
/// fire changed it.
fn generate_cooked_meat() -> RgbaImage {
    meat(
        [152, 96, 52],
        [190, 132, 74],
        [104, 62, 34],
        [230, 220, 198],
    )
}

/// Human flesh: the meat's own cut, paler and pinker than any animal's, with
/// a waxy yellow fat -- the one piece in the pack a player recognises before
/// they read its name.
fn generate_human_flesh() -> RgbaImage {
    meat([214, 142, 128], [236, 182, 164], [168, 98, 92], [238, 214, 150])
}

/// Dried meat: the same cut again, dark and leathery.
///
/// The deepest red of the three and the dullest: what the sun and the
/// wind take out of meat is water and shine. Same silhouette as its
/// siblings, same bone, for the reason `generate_cooked_meat` gives --
/// the three are one object at three points in its life, and colour is
/// how the pack tells them apart.
fn generate_dried_meat() -> RgbaImage {
    meat(
        [122, 52, 44],
        [150, 74, 56],
        [84, 34, 30],
        [214, 202, 178],
    )
}

/// The shape all three cuts share: a haunch with the bone at one end.
///
/// **Three flat bands became four tones and a grain**, and the reason is
/// that these three pictures differ from one another *only* by colour --
/// see `generate_cooked_meat`. When the only variable is the palette,
/// every bit of form the drawing carries has to be earned by the values
/// inside it: an ellipse split along a diagonal is a coloured pill, and
/// three coloured pills in a row is a pack a player has to read the
/// tooltip of. What is added here is a rim that darkens all the way
/// round, marbling that runs across the cut, and a bone with a knuckle
/// on it -- and the bone is the part that survives being small.
fn meat(flesh: [u8; 3], lit: [u8; 3], dark: [u8; 3], bone: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 3..14i32 {
        for x in 2..14i32 {
            let dx = (x as f32 - 8.0) / 5.4;
            let dy = (y as f32 - 8.5) / 5.0;
            let radius = dx * dx + dy * dy;
            if radius > 1.0 {
                continue;
            }
            // The rim goes dark all the way round rather than only at
            // the bottom right: a cut of meat is wet, and a wet thing
            // turns away from the light at every edge at once. Without
            // this the ellipse has a hard outline on three sides and
            // reads as a sticker.
            let colour = if radius > 0.78 {
                mix(dark, [0, 0, 0], 0.12)
            } else if dx + dy < -0.7 {
                lit
            } else if dx + dy > 0.6 {
                dark
            } else {
                flesh
            };
            // Marbling: fat and sinew, in short runs across the grain of
            // the cut. Sampled coarsely along x so it comes out as
            // streaks and not as pepper.
            let colour = if radius < 0.7 && hash(0x8EA7, (x / 2) as u32, y as u32).is_multiple_of(9) {
                mix(colour, bone, 0.4)
            } else {
                colour
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The bone, sticking out of the narrow end, with a knuckle on it.
    // A straight stub is a stick; the two texels that widen the end are
    // what make it a joint, and a joint is what says "this came off an
    // animal" at the size this is actually looked at.
    for &(x, y) in &[(4, 5), (3, 5), (3, 4), (4, 4), (2, 3), (3, 3), (2, 4)] {
        put_opaque(&mut img, x, y, bone);
    }
    for &(x, y) in &[(2, 3), (3, 4)] {
        put_opaque(&mut img, x, y, mix(bone, [255, 255, 255], 0.5));
    }
    img
}

/// A hide: a skin pegged out to dry, pale with a darker edge.
/// A pegged-out hide: a skin with legs, not a brown rectangle.
///
/// **What this replaced was a square of speckle.** A hide is the one
/// item in the pack that is a whole animal's worth of something, and the
/// shape is the only thing that says so: four legs splayed out from a
/// body, the pale flesh side up, and the darker fur showing round the
/// edge where it curls.
fn generate_hide() -> RgbaImage {
    const FLESH: [u8; 3] = [206, 176, 146];
    const FLESH_DARK: [u8; 3] = [176, 144, 116];
    const FUR: [u8; 3] = [126, 92, 62];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The outline, as a mask: a body with four stubs. Written out row by
    // row because that is what a shape this size is -- an equation for
    // it would be longer and less legible than the picture.
    const SHAPE: [&str; 16] = [
        "................",
        "..##........##..",
        ".####......####.",
        ".###############",
        "..##############",
        "...#############",
        "...#############",
        "...#############",
        "...#############",
        "...#############",
        "..##############",
        ".###############",
        ".####......####.",
        "..##........##..",
        "................",
        "................",
    ];
    for (y, row) in SHAPE.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            if cell != b'#' {
                continue;
            }
            let (x, y) = (x as i32, y as i32);
            // The rim curls over and shows the fur; everything inside is
            // the flesh side, which is what you see of a hide laid out.
            let edge = SHAPE[(y as usize).saturating_sub(1)].as_bytes()[x as usize] != b'#'
                || SHAPE[(y as usize + 1).min(15)].as_bytes()[x as usize] != b'#'
                || row.as_bytes()[(x as usize).saturating_sub(1)] != b'#'
                || row.as_bytes()[(x as usize + 1).min(15)] != b'#';
            let roll = hash(0x81DE, x as u32, y as u32) % 6;
            // **The spine.** A hide laid out flesh side up is not flat:
            // it is thick down the middle where the back was and thin at
            // the flanks, and a band of shadow along the centre line is
            // the whole of that. Without it the shape was right and the
            // fill was a swatch -- four legs round a rectangle of one
            // colour, which reads as a paper cut-out of a hide.
            let spine = (y - 7).abs() <= 1 && (3..14).contains(&x);
            let colour = if edge {
                FUR
            } else if spine {
                mix(FLESH_DARK, FUR, 0.25)
            } else if roll == 0 {
                FLESH_DARK
            } else if roll == 5 {
                mix(FLESH, [255, 255, 255], 0.12)
            } else {
                FLESH
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

#[allow(dead_code)]
fn generate_hide_old() -> RgbaImage {
    const SKIN: [u8; 3] = [178, 142, 104];
    const SKIN_LIT: [u8; 3] = [206, 176, 138];
    const EDGE: [u8; 3] = [124, 94, 66];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 4..13i32 {
        for x in 3..13i32 {
            let dx = (x as f32 - 8.0) / 4.6;
            let dy = (y as f32 - 8.5) / 4.4;
            if dx * dx + dy * dy > 1.0 {
                continue;
            }
            put_opaque(&mut img, x, y, if x + y < 13 { SKIN_LIT } else { SKIN });
        }
    }
    // The four pegged corners: what turns an oval into a stretched skin,
    // and the only thing in the pack shaped like this.
    for &(x, y) in &[
        (2, 3),
        (3, 3),
        (3, 4),
        (12, 3),
        (13, 3),
        (12, 4),
        (2, 13),
        (3, 13),
        (3, 12),
        (12, 13),
        (13, 13),
        (12, 12),
    ] {
        put_opaque(&mut img, x, y, EDGE);
    }
    img
}

/// Half the height of a full tile, in texels.
///
/// A campfire and a bag fill half their cell (see
/// `blocks::BlockDef::thickness`), and their side faces are drawn half a
/// block tall. The texture array is square, so the loader stretches an
/// 8-row image back to 16 -- and the face then squashes it by exactly
/// the same two, which lands one authored row on one screen row. Give
/// the same face a 16-row picture and every second row of it is thrown
/// away at draw time.
const HALF_TILE: u32 = RESOLUTION / 2;

/// The side of a campfire: a ring of stones with wood laid over it.
///
/// Eight rows rather than sixteen -- see `HALF_TILE`.
fn generate_campfire_side() -> RgbaImage {
    const STONE: [u8; 3] = [132, 130, 126];
    const STONE_LIT: [u8; 3] = [166, 164, 158];
    const STONE_DARK: [u8; 3] = [92, 90, 88];
    const WOOD: [u8; 3] = [120, 86, 50];
    const WOOD_DARK: [u8; 3] = [84, 60, 36];
    const CHAR: [u8; 3] = [46, 40, 38];

    // **Drawn, not sprinkled.** What was here was three bands of
    // per-pixel noise, which at four rows is not a campfire, it is a
    // gradient with dandruff. A fire seen from the side is a *ring of
    // stones* -- rounded lumps with gaps between them -- and a couple of
    // charred stick ends showing over the top, and both of those are
    // shapes rather than densities.
    //
    // Four rows, and each one has a job: the stick ends, the top of the
    // stones where the light catches, the body of them, and the shadow
    // where they meet the ground.
    //
    // `s` is a stone, `S` its lit crown, `d` its shaded foot, `w` a
    // stick, `c` a charred one, and `.` the gap between two stones.
    const ROWS: [&str; 4] = [
        "..c..wc...cw..c.",
        "SS.SSS.SSS.SS.SS",
        "ss.sss.sss.ss.ss",
        "dd.ddd.ddd.dd.dd",
    ];
    let mut img = RgbaImage::new(RESOLUTION, QUARTER_TILE);
    for (y, row) in ROWS.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            let (x, y) = (x as i32, y as i32);
            // A little grain inside each shape, so the stones are not
            // four flat colours -- but *inside* them, which is the
            // difference between texture and noise.
            let grain = hash(0xF12E, x as u32, y as u32).is_multiple_of(5);
            let colour = match cell {
                b'S' => if grain { STONE } else { STONE_LIT },
                b's' => if grain { STONE_LIT } else { STONE },
                b'd' => if grain { STONE } else { STONE_DARK },
                b'w' => if grain { WOOD_DARK } else { WOOD },
                b'c' => CHAR,
                // The gaps between the stones: the dark of the fire pit
                // behind them.
                _ => CHAR,
            };
            put_pixel_in(&mut img, x, y, colour);
        }
    }
    img
}

/// A quarter of a tile: the height a four-pixel block's side is drawn at.
const QUARTER_TILE: u32 = RESOLUTION / 4;

/// The side of a bag, at half height, **derived from the drawn one**.
///
/// The bag on disk is hand-drawn and sixteen rows tall; this reads it
/// and folds each pair of rows into one, so nothing in the drawing is
/// lost the way dropping every second row would lose a strap. It is
/// deliberately a *second file*: the artwork is not this program's to
/// overwrite, and a hand-drawn eight-row bag should replace this the
/// moment somebody draws one.
fn generate_backpack_side_flat(dir: &std::path::Path) -> RgbaImage {
    let drawn = match image::open(dir.join("terrain/backpack_side.png")) {
        Ok(img) => image::imageops::resize(
            &img.to_rgba8(),
            RESOLUTION,
            RESOLUTION,
            image::imageops::FilterType::Nearest,
        ),
        // No drawn bag in the folder: fall back to the generated one, so
        // a fresh checkout still produces something.
        Err(_) => generate_backpack_side(),
    };

    let mut img = RgbaImage::new(RESOLUTION, HALF_TILE);
    for y in 0..HALF_TILE {
        for x in 0..RESOLUTION {
            let (top, bottom) = (drawn.get_pixel(x, y * 2), drawn.get_pixel(x, y * 2 + 1));
            let mix = |a: u8, b: u8| ((a as u16 + b as u16) / 2) as u8;
            img.put_pixel(
                x,
                y,
                Rgba([
                    mix(top[0], bottom[0]),
                    mix(top[1], bottom[1]),
                    mix(top[2], bottom[2]),
                    mix(top[3], bottom[3]),
                ]),
            );
        }
    }
    img
}

/// `put_opaque`, for an image that is not a full tile tall.
fn put_pixel_in(img: &mut RgbaImage, x: i32, y: i32, colour: [u8; 3]) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    img.put_pixel(
        x as u32,
        y as u32,
        Rgba([colour[0], colour[1], colour[2], 255]),
    );
}

/// Looking down into an unlit fire: stones round the edge, ash and
/// crossed sticks in the middle.
fn generate_campfire_top() -> RgbaImage {
    campfire_top(false)
}

/// The same hearth with the fire burning in it.
///
/// The flame itself is not here any more -- it stands over the block as
/// two crossing quads, the way a tuft of grass does (see
/// `mesh::flame_block`). What this has to show is what is *under* the
/// flame: embers where the wood was, and ash where it has burnt through.
fn generate_campfire_lit_top() -> RgbaImage {
    campfire_top(true)
}

/// A hearth seen from above: a ring of stones with sticks laid across
/// the middle of it.
///
/// **A picture rather than a scatter.** The old one was noise in three
/// tones, which reads as gravel from any distance at all. A hearth from
/// above has a very definite shape -- a closed ring with a fire laid
/// inside it -- and that shape is what tells a player at twenty metres
/// that somebody has camped here.
///
/// `s` a stone, `S` its lit side, `w` a stick, `c` a charred one,
/// `.` the ash floor inside the ring, `-` the ground outside it.
fn campfire_top(lit: bool) -> RgbaImage {
    const STONE: [u8; 3] = [132, 130, 126];
    const STONE_LIT: [u8; 3] = [168, 166, 160];
    const WOOD: [u8; 3] = [120, 86, 50];
    const CHAR: [u8; 3] = [46, 40, 38];
    const ASH: [u8; 3] = [92, 86, 82];
    const EMBER: [u8; 3] = [206, 84, 26];
    const EMBER_HOT: [u8; 3] = [244, 158, 52];

    const ROWS: [&str; 16] = [
        "--sSsS--..--SsSs",
        "-sSs......----sS",
        "sSs...cwwc...-sS",
        "Ss...cwwwwc...sS",
        "s...cwwwwwwc...s",
        "S..cwww..wwwc..S",
        "s.cww......wwc.s",
        ".cww...cc...wwc.",
        ".cww...cc...wwc.",
        "s.cww......wwc.s",
        "S..cwww..wwwc..S",
        "s...cwwwwwwc...s",
        "Ss...cwwwwc...sS",
        "sSs...cwwc...sSs",
        "-sSs......--sSs-",
        "--sSsS--..--sSs-",
    ];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for (y, row) in ROWS.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            let (xi, yi) = (x as i32, y as i32);
            let grain = hash(0xC0A1, x as u32, y as u32) % 5;
            let colour = match cell {
                b'S' => if grain == 0 { STONE } else { STONE_LIT },
                b's' => if grain == 0 { STONE_LIT } else { STONE },
                b'w' if lit => {
                    // Burning wood: mostly ember, brightest where the
                    // sticks cross in the middle.
                    if grain < 2 { EMBER_HOT } else { EMBER }
                }
                b'w' => if grain == 0 { CHAR } else { WOOD },
                b'c' if lit => if grain < 3 { EMBER } else { CHAR },
                b'c' => CHAR,
                b'.' if lit => if grain == 0 { EMBER } else { ASH },
                b'.' => if grain == 0 { CHAR } else { ASH },
                // Outside the ring: nothing. The block is a cube, so
                // this is still opaque -- it is the earth the ring was
                // laid on.
                _ => if grain == 0 { ASH } else { [74, 62, 50] },
            };
            put_opaque(&mut img, xi, yi, colour);
        }
    }
    img
}

fn generate_rain() -> RgbaImage {
    const DROP: [u8; 3] = [176, 202, 230];
    const DROP_PALE: [u8; 3] = [214, 232, 246];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        // Brightest at the head and fading to nothing at the tail, which
        // is what a falling drop leaves on an eye.
        let along = y as f32 / RESOLUTION as f32;
        let colour = mix(DROP_PALE, DROP, along);
        let alpha = (255.0 * (1.0 - along * 0.55)) as u8;
        for x in 6..10i32 {
            if x < 0 || y < 0 || x >= RESOLUTION as i32 || y >= RESOLUTION as i32 {
                continue;
            }
            // The outer columns are thinner than the inner ones, so the
            // streak has an edge rather than being a bar.
            let a = if x == 6 || x == 9 { alpha / 2 } else { alpha };
            img.put_pixel(x as u32, y as u32, Rgba([colour[0], colour[1], colour[2], a]));
        }
    }
    img
}

/// Snow: one flake, filling its tile.
///
/// The same argument as the rain -- see `generate_rain`. A flake is
/// drawn on a quad a seventh of a block across, and a tile with six
/// flakes on it would put six of them in that space.
fn generate_snowfall() -> RgbaImage {
    const FLAKE: [u8; 3] = [246, 250, 254];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let dx = (x as f32 - 7.5) / 6.0;
            let dy = (y as f32 - 7.5) / 6.0;
            let r = dx * dx + dy * dy;
            if r > 1.0 {
                continue;
            }
            // Soft at the rim: a hard-edged disc reads as a pebble.
            let alpha = (255.0 * (1.0 - r * 0.6)) as u8;
            img.put_pixel(x as u32, y as u32, Rgba([FLAKE[0], FLAKE[1], FLAKE[2], alpha]));
        }
    }
    img
}

/// A metal tool: a forged head on the haft every tool in this pack
/// shares.
///
/// **What was here before was the flint drawing in a metal colour**, and
/// then a first redraw that fixed the shapes and got the light wrong.
/// Both mistakes are worth keeping written down, because they are the
/// two ways this picture can fail.
///
/// The flint version reused `generate_knife`'s and `generate_axe`'s
/// spans verbatim and only swapped `knap` for `cast`; the pick called
/// `generate_pickaxe`. `cast` painted the first texel of a row as the
/// highlight and the last as the shadow, which on a span three wide
/// leaves one texel of metal and on a span two wide leaves none: the
/// flint spans are narrow because they were drawn for `knap`, which
/// takes its tone from the fraction along the row and still reads at two
/// texels. A bronze knife came out as an outline of itself.
///
/// The redraw that followed made the shapes right and answered the
/// second problem -- **bronze and copper are the colour of the haft** --
/// with a dark contour round every head. It worked and it was wrong:
/// an outline is a line drawn *round* an object, and nothing else in
/// this pack has one, so nine tools stood out from every other picture
/// in the game as the ones somebody had traced. It also ate two texels
/// of a sixteen-texel head, which is why they came out fat.
///
/// So the separation is **value**, which is what it should have been:
/// a finished tool is ground and polished where a cast bar is not. See
/// `forge` for how the ramp is built, and why it is built by scaling the
/// metal's own channels rather than by mixing it toward white -- mixing
/// desaturates, and a desaturated bronze is cream.
///
/// The binding is **hide, not straw**, and that is the recipe: every
/// metal tool costs an ingot, a worked stick and a hide (see the forge
/// recipes in `crafting`), where the flint ones cost fibre. The old
/// drawing wrapped a bronze axe in `LASHING`, which is a picture of a
/// recipe that does not exist.
fn generate_metal_tool(shape: ToolShape, metal: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The haft first and the head over it: a head that does not cover
    // the top of the stick reads as balanced on the end of it.
    tool_haft(&mut img, shape.haft_steps());
    forge(&mut img, shape.head(), metal);
    img
}

impl ToolShape {
    /// How far up the haft runs before the head goes on.
    ///
    /// A knife is held in the fist and an axe is swung on a full arm,
    /// and how much stick there is under the head is the only place a
    /// picture this size can say which.
    fn haft_steps(self) -> i32 {
        match self {
            ToolShape::Knife => 5,
            ToolShape::Axe | ToolShape::Pick => 10,
        }
    }

    /// The head, one row of the tile per line.
    ///
    /// `@` the polished face, `#` the body, `=` a half tone, `-` the
    /// shaded side, `o` the darkest step, `x` and `X` the hide binding,
    /// `.` whatever the haft already put there.
    ///
    /// **Written out rather than derived**, which is the argument
    /// `Span`'s own doc makes one size up: at sixteen texels a cutting
    /// edge is six decisions, not a function, and every attempt to get
    /// one out of a radius came back symmetrical -- which is the
    /// difference between an axe and a mallet.
    fn head(self) -> &'static [&'static str; RESOLUTION as usize] {
        match self {
            // A knife: one straight line from the butt to the point,
            // three texels of blade with the polished back along the
            // top and the edge shaded under it. Longer than the flint
            // knife on purpose -- a cast blade can be longer than any
            // flake ever struck off a nodule, and length is the only
            // place that difference shows.
            ToolShape::Knife => &[
                "................",
                ".............@..",
                "............@=..",
                "..........@##=..",
                ".........@##=...",
                "........@##=....",
                ".......@##=.....",
                "......@##=......",
                "......@#=.......",
                "......@=........",
                "......xX........",
                ".....xX.........",
                "................",
                "................",
                "................",
                "................",
            ],
            // An axe. **The eye is on the right, where the haft is**,
            // and everything else follows from that: the poll is the
            // stub past it, and the bit is the mass that flares away to
            // the left and hangs below the line of the haft.
            //
            // A deep bit and a short poll is the whole of what makes it
            // an axe. The first redraw had the head the same depth all
            // the way across, and a head that deep and that even is a
            // mallet -- which is what it was reported as.
            ToolShape::Axe => &[
                "................",
                "......@@@@@=....",
                "....@@######=...",
                "..@@#########=..",
                "..@##########=..",
                "..@#########=...",
                "..@@#####=xX....",
                "...@@#=..xX.....",
                "....@=..........",
                "................",
                "................",
                "................",
                "................",
                "................",
                "................",
                "................",
            ],
            // A pick: a bar across the top of the haft with both ends
            // drawn down to a point, and the wood showing through the
            // arch between them. Two points rather than one, for the
            // reason `generate_pick_head` gives -- one point is a hoe.
            //
            // **The bar is solid and the arch is empty**, which is the
            // way round the old one had it backwards: that head was a
            // one-texel arch with the haft standing up inside it, and a
            // hoop with a stick through it is a croquet hoop.
            ToolShape::Pick => &[
                "................",
                "................",
                "........@@@.....",
                "......@@#####=..",
                "...@@########=..",
                "...@#=.....@#=..",
                "...@=.....xX@=..",
                ".........xX.....",
                "................",
                "................",
                "................",
                "................",
                "................",
                "................",
                "................",
                "................",
            ],
        }
    }
}

/// Paints one forged head over whatever the haft left.
///
/// The counterpart of `knap`, and the difference between the two is the
/// whole visual argument for the metal age: `knap` scatters its
/// highlights, because a struck flake is a field of tiny facets, and
/// this lays flat faces with one lit side, because a cast face is
/// smooth.
///
/// **The ramp is the metal's own channels scaled, not the metal mixed
/// toward white.** Mixing toward white raises the value and drops the
/// saturation together, and the first attempt at a bright tool did
/// exactly that: bronze came out cream and copper came out pink, which
/// is not a polished bronze axe, it is a different metal. Multiplying
/// keeps the ratio between the channels -- the hue -- and moves only the
/// value, so a polished bronze is bronze and a polished iron is iron.
///
/// The top of the ramp is deliberately above the ingot's own highlight.
/// A tool is ground and a bar is not, and it has to hold against a
/// wooden haft that the bar never lies on: `HAFT` is a mid brown, and
/// an unpolished bronze head laid on it is the same value as the wood.
/// That is what the contour this replaced was there to fix.
///
/// One colour in and five out, so there is nothing left for a tool and
/// the bar it came out of to drift apart over.
fn forge(img: &mut RgbaImage, rows: &[&str; RESOLUTION as usize], metal: [u8; 3]) {
    let scaled = |by: f32| {
        [
            clamp_u8((metal[0] as f32 * by) as i32),
            clamp_u8((metal[1] as f32 * by) as i32),
            clamp_u8((metal[2] as f32 * by) as i32),
        ]
    };
    for (y, row) in rows.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            let colour = match cell {
                b'@' => scaled(1.45),
                b'#' => scaled(1.22),
                b'=' => scaled(1.02),
                b'-' => scaled(0.84),
                b'o' => scaled(0.55),
                b'x' => HIDE,
                b'X' => HIDE_DARK,
                _ => continue,
            };
            put_opaque(img, x as i32, y as i32, colour);
        }
    }
}

/// The binding on a metal tool: a strip of hide round the haft where
/// the head sits on it. Not `LASHING`, which is straw and is what a
/// flint tool is bound with -- see `generate_metal_tool`.
const HIDE: [u8; 3] = [126, 92, 62];
const HIDE_DARK: [u8; 3] = [94, 66, 42];

// ---- the boar, in five pictures ----
//
// A face of a model wears a *whole* texture (see
// `logic::animal_model::Skin` for why), so a detailed animal is a
// handful of small pictures rather than one atlas. Five is what a boar
// needs: a hide, the sides of its head, its face, a tusk and a hoof.
//
// They share a palette on purpose. The eye reads an animal as one object
// or as a pile of boxes, and what decides which is whether the parts
// look like they came off the same beast.

/// The boar's own colours, dark and warm rather than grey -- a black
/// boar reads as a hole in the grass at any distance.
const BOAR_HIDE: [u8; 3] = [72, 60, 54];
const BOAR_DARK: [u8; 3] = [48, 40, 36];
const BOAR_LIT: [u8; 3] = [102, 86, 74];
/// The bristle along the spine, which is the one part of a boar that is
/// a different colour from the rest of it.
const BOAR_BRISTLE: [u8; 3] = [126, 108, 88];
const BOAR_SNOUT: [u8; 3] = [112, 84, 78];
const BOAR_TUSK: [u8; 3] = [226, 220, 198];


/// The hide: dark, with a coarse grain and a lighter belly.
///
/// Replaces the flat speckle the first boar wore. A single noisy colour
/// on every face of every box is exactly what makes a model read as
/// boxes: there is nothing for the eye to follow from one to the next.
fn generate_boar_hide() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            // Bristles: short vertical strokes rather than dots, because
            // a boar's coat lies one way and noise lies no way at all.
            let stroke = hash(0xB0A2, x as u32, (y / 3) as u32) % 10;
            let base = match stroke {
                0 | 1 => BOAR_DARK,
                2..=6 => BOAR_HIDE,
                7 | 8 => BOAR_LIT,
                _ => BOAR_BRISTLE,
            };
            // ...and a paler underside, which is what gives a flank a
            // top and a bottom at ten metres.
            let colour = if y >= 12 {
                mix(base, BOAR_LIT, 0.35)
            } else if y <= 2 {
                mix(base, BOAR_BRISTLE, 0.45)
            } else {
                base
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// The side of the head: the hide, plus an eye.
fn generate_boar_head() -> RgbaImage {
    const EYE: [u8; 3] = [24, 20, 18];
    const GLINT: [u8; 3] = [208, 196, 172];

    let mut img = generate_boar_hide();
    // Small, high and forward. A boar's eye is a bead set well back from
    // the snout, and drawing it big is what turns a boar into a piglet.
    for (x, y) in [(4, 5), (5, 5), (4, 6), (5, 6)] {
        put_opaque(&mut img, x, y, EYE);
    }
    put_opaque(&mut img, 4, 5, GLINT);
    // A crease from the eye back along the jaw, which is most of what
    // makes a flat square read as a face seen side-on.
    for x in 6..12 {
        put_opaque(&mut img, x, 8 + (x - 6) / 3, BOAR_DARK);
    }
    img
}

/// Draws a picture at a coarser grid than the tile it lives on.
///
/// **The thing this fixes is a mismatch nobody can unsee.** Every face
/// of a model wears a whole 16x16 picture, whatever size the box is --
/// so a tusk one pixel wide and a flank thirteen wear the same amount of
/// detail, and the tusk ends up carrying a sixteen-step gradient across
/// something the eye reads as a single pixel of the animal. Next to a
/// body whose texel is a sixteenth of its width, that is not detail, it
/// is noise.
///
/// So a small part is drawn on a small grid and blown up: `cells` of 2
/// means the picture is really 2x2, and one of its texels covers eight
/// on the tile. What reaches the screen then has the same texel size as
/// the body beside it.
fn at_density(cells: u32, mut paint: impl FnMut(u32, u32) -> [u8; 3]) -> RgbaImage {
    let cells = cells.clamp(1, RESOLUTION);
    let step = RESOLUTION / cells;
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for cy in 0..cells {
        for cx in 0..cells {
            let colour = paint(cx, cy);
            for y in 0..step {
                for x in 0..step {
                    put_opaque(
                        &mut img,
                        (cx * step + x) as i32,
                        (cy * step + y) as i32,
                        colour,
                    );
                }
            }
        }
    }
    img
}

/// A tusk: bone, lit along one edge.
fn generate_boar_tusk() -> RgbaImage {
    // Two cells across, because the box is one pixel wide and three
    // tall: anything finer is detail nobody can see and everybody can
    // tell is there. See `at_density`.
    at_density(2, |x, y| {
        // Lit on one side, yellowed at the root.
        let colour = if y == 0 { BOAR_TUSK } else { mix(BOAR_TUSK, [178, 160, 120], 0.6) };
        if x == 0 {
            mix(colour, [255, 255, 240], 0.35)
        } else {
            mix(colour, [150, 136, 104], 0.3)
        }
    })
}

/// A foot: the hide going dark toward the hoof.
fn generate_boar_hoof() -> RgbaImage {
    const HOOF: [u8; 3] = [34, 28, 26];
    // Four cells: the leg is four pixels wide and six tall, so this is
    // one texture pixel per model pixel -- the same density as the body
    // it hangs off.
    at_density(4, |x, y| {
        if y >= 3 {
            // The foot. The split down the middle is the only thing that
            // says hoof rather than boot.
            if x == 1 || x == 2 { mix(HOOF, BOAR_LIT, 0.3) } else { HOOF }
        } else if y == 2 {
            mix(BOAR_HIDE, HOOF, 0.5)
        } else if x == 0 {
            mix(BOAR_HIDE, BOAR_LIT, 0.4)
        } else {
            BOAR_HIDE
        }
    })
}

/// Blends two colours. `t` of nought is all of the first.
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        clamp_u8((a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as i32),
        clamp_u8((a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as i32),
        clamp_u8((a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as i32),
    ]
}

/// The snout, all round: bare skin rather than bristle.
///
/// This used to be `boar_face.png` -- a pink disc pasted over the hide
/// and worn by *both* the front of the head and every side of the snout
/// box, so a boar had a dinner plate for a face and four more of them
/// down its nose. A snout is its own box and gets its own picture.
fn generate_boar_snout() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let grain = hash(0xB0A6, x as u32, (y / 2) as u32) % 7;
            let colour = match grain {
                0 => mix(BOAR_SNOUT, BOAR_DARK, 0.35),
                1 | 2 => mix(BOAR_SNOUT, BOAR_DARK, 0.15),
                6 => mix(BOAR_SNOUT, [190, 150, 142], 0.4),
                _ => BOAR_SNOUT,
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// The end of it: the same skin with two nostrils in it.
///
/// The nostrils are the whole picture. They are what a boar's face *is*
/// from in front, and they are the one detail that survives being seen
/// at ten metres through fog.
fn generate_boar_nose() -> RgbaImage {
    const NOSTRIL: [u8; 3] = [38, 24, 24];
    let mut img = generate_boar_snout();
    // A pair of commas rather than two dots: a nostril is a slit with a
    // fold at the top, and two round holes read as a power socket.
    for (x, y) in [(4, 7), (5, 7), (4, 8), (4, 9)] {
        put_opaque(&mut img, x, y, NOSTRIL);
    }
    for (x, y) in [(10, 7), (11, 7), (11, 8), (11, 9)] {
        put_opaque(&mut img, x, y, NOSTRIL);
    }
    // The crease down the middle, which is what makes the two read as
    // one nose.
    for y in 5..13 {
        put_opaque(&mut img, 7, y, mix(BOAR_SNOUT, BOAR_DARK, 0.45));
        put_opaque(&mut img, 8, y, mix(BOAR_SNOUT, BOAR_DARK, 0.25));
    }
    img
}

/// An eye on a hide: what every head that is not a boar's needs, and
/// nothing else.
///
/// One function for the deer and the hare, because the difference
/// between their faces at this size is the colour of the fur around the
/// eye -- which is the hide it is drawn on.
fn generate_eyed_head(base: [u8; 3], _speckle: i32, _seed_name: &str) -> RgbaImage {
    const EYE: [u8; 3] = [22, 18, 16];
    const GLINT: [u8; 3] = [214, 206, 190];

    // **Eight cells, and the eye is one of them.**
    //
    // What was here before was a 16x16 picture with a five-texel eye and
    // a line ruled across the middle -- on a head box five pixels tall,
    // which put the eye somewhere in the animal's cheek and the line
    // straight across it. A head this size has room for an eye and a
    // muzzle and nothing else, so that is what it has.
    //
    // The eye sits forward and high, because the front of a head box is
    // the +Z end and a deer's eye is nearer the nose than the ear.
    at_density(8, |x, y| {
        let fur = if (x + y) % 3 == 0 {
            mix(base, [255, 255, 255], 0.08)
        } else if (x * 2 + y) % 5 == 0 {
            mix(base, [0, 0, 0], 0.1)
        } else {
            base
        };
        match (x, y) {
            (5, 2) => EYE,
            (5, 1) => GLINT,
            // The muzzle: the last column, going dark toward the nose.
            (7, _) => mix(base, [40, 30, 24], 0.55),
            (6, _) => mix(base, [40, 30, 24], 0.2),
            _ => fur,
        }
    })
}

/// The same hide, at a quarter of the detail.
///
/// For the parts that are two or three pixels across -- ears, a tail.
/// The full hide is about one texel per model pixel on a body thirteen
/// wide, and *eight* on an ear two wide, which is the mismatch that
/// makes a small part look like a screenful of noise stuck to a clean
/// animal. See `at_density`.
fn coarse_fur(base: [u8; 3]) -> RgbaImage {
    at_density(4, |x, y| match (x + y * 3) % 4 {
        0 => mix(base, [255, 255, 255], 0.12),
        1 => mix(base, [0, 0, 0], 0.12),
        _ => base,
    })
}

/// The same picture, flipped left to right.
///
/// The two sides of a box are mirror images of each other -- the +X face
/// maps `u = 1 - z` and the -X face maps `u = z` -- so a head drawn once
/// and worn on both has its eye at the nose on one side of the animal
/// and on the back of its skull on the other. A face wears a whole
/// picture and cannot wear a flipped one, so the flip is a second file.
fn mirrored(img: &RgbaImage) -> RgbaImage {
    let mut out = RgbaImage::new(img.width(), img.height());
    for y in 0..img.height() {
        for x in 0..img.width() {
            out.put_pixel(img.width() - 1 - x, y, *img.get_pixel(x, y));
        }
    }
    out
}

/// Every hide in one place, so a face and the head it is on cannot come
/// out of two different animals.
///
/// Each pelt used to be written down twice -- once in `PELTS` for the
/// body and once again at the head generator -- and with four more
/// pictures per animal wanting the same colour, twice would have become
/// six. The boar's is `BOAR_HIDE`, which was already named for exactly
/// this reason.
const DEER_PELT: [u8; 3] = [166, 116, 74];
const HARE_PELT: [u8; 3] = [156, 132, 102];
const WOLF_PELT: [u8; 3] = [110, 108, 112];
/// A sheep: off-white, warmed a little so it does not read as snow. The
/// same colour the wool item is drawn in and the same one
/// `types::garment_tint` multiplies the garments by, so the animal, the
/// fleece in the pack and the coat on the player are visibly one
/// material.
const SHEEP_FLEECE: [u8; 3] = [222, 216, 202];

/// The front of a head: the animal's own hide, going dark toward the
/// muzzle.
///
/// Deliberately *not* bare skin -- the bare part is the snout, which is
/// the box in front of this one. What this picture is for is the
/// transition: a head box whose front face is flat hide reads as a
/// cardboard box with a nose stuck on it.
///
/// The same shape the boar's face has, generalised, because the boar was
/// the only animal that had one at all.
fn muzzle_face(base: [u8; 3]) -> RgbaImage {
    const DARK: [u8; 3] = [42, 32, 26];
    at_density(8, |x, y| {
        // Darker toward the bottom, and darkest in the middle where the
        // muzzle actually is.
        let down = y as f32 / 7.0;
        let middle = 1.0 - ((x as f32 - 3.5).abs() / 3.5);
        let shade = (down * 0.55 + middle * 0.25).min(0.75);
        let fur = if (x * 3 + y) % 5 == 0 {
            mix(base, [255, 255, 255], 0.07)
        } else {
            base
        };
        mix(fur, DARK, shade)
    })
}

/// A muzzle: bare skin, all round, in the animal's own key.
///
/// One function for three animals because a muzzle at this size is a
/// colour and a grain, and the grain is the same grain on all of them.
fn bare_muzzle(skin: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let grain = hash(0x5D0E, x as u32, (y / 2) as u32) % 7;
            let colour = match grain {
                0 => mix(skin, [0, 0, 0], 0.28),
                1 | 2 => mix(skin, [0, 0, 0], 0.12),
                6 => mix(skin, [255, 255, 255], 0.18),
                _ => skin,
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}

/// The end of the muzzle: the same skin with two nostrils and a crease.
///
/// The nostrils are the whole picture -- they are what a face *is* from
/// in front, and the one detail that survives ten metres of fog. Narrower
/// and higher than the boar's, because everything else here has a nose
/// rather than a snout it digs with.
fn nose_front(skin: [u8; 3]) -> RgbaImage {
    let mut img = bare_muzzle(skin);
    let dark = mix(skin, [0, 0, 0], 0.62);
    for (x, y) in [(5, 6), (5, 7), (4, 7)] {
        put_opaque(&mut img, x, y, dark);
    }
    for (x, y) in [(10, 6), (10, 7), (11, 7)] {
        put_opaque(&mut img, x, y, dark);
    }
    // The crease that makes two holes read as one nose, and the lip
    // under it.
    for y in 5..11 {
        put_opaque(&mut img, 7, y, mix(skin, [0, 0, 0], 0.35));
        put_opaque(&mut img, 8, y, mix(skin, [0, 0, 0], 0.2));
    }
    for x in 5..11 {
        put_opaque(&mut img, x, 11, mix(skin, [0, 0, 0], 0.45));
    }
    img
}

/// An ear: fur outside, bare inside.
///
/// **The inside is the whole point.** An ear that is hide on all six
/// faces is a lump of fur, and the thing that makes an ear read as an
/// ear at this size is that you can see *into* it -- so the picture is a
/// pale hollow with a fur border, and the border is what keeps it from
/// looking like a hole cut in the animal.
fn ear(base: [u8; 3], inner: [u8; 3]) -> RgbaImage {
    at_density(8, |x, y| {
        let edge = x == 0 || x == 7 || y == 0 || y >= 6;
        if edge {
            if (x + y) % 3 == 0 {
                mix(base, [0, 0, 0], 0.18)
            } else {
                base
            }
        } else {
            let deep = 1.0 - (y as f32 / 6.0);
            mix(inner, [0, 0, 0], deep * 0.35)
        }
    })
}

/// An antler: bone, forked, on nothing.
///
/// The box it goes on is one pixel wide, so what this has to carry is a
/// colour and a highlight down one side -- the *fork* is model geometry
/// (see `DEER`, which spends two boxes a side on it) rather than
/// something a texture this size could say.
fn generate_deer_antler() -> RgbaImage {
    const BONE: [u8; 3] = [186, 172, 142];
    const BONE_DARK: [u8; 3] = [126, 112, 88];
    at_density(4, |x, y| {
        if x == 0 {
            mix(BONE, [255, 255, 255], 0.22)
        } else if x == 3 {
            BONE_DARK
        } else if (x + y) % 3 == 0 {
            mix(BONE, BONE_DARK, 0.35)
        } else {
            BONE
        }
    })
}

/// A wolf's foot: a dark pad under a grey leg.
///
/// The same shape as a deer's hoof and a different reason for it. A deer
/// ends in horn, which is nearly black; a wolf ends in a pad, which is
/// only a shade darker than the leg -- so the step between the two is
/// smaller, and what carries the foot is that the toes are lighter than
/// the pad rather than that the whole end is black.
fn generate_wolf_paw() -> RgbaImage {
    const PAD: [u8; 3] = [58, 56, 58];
    at_density(5, |x, y| {
        if y == 4 {
            PAD
        } else if y == 3 {
            mix(WOLF_PELT, PAD, 0.55)
        } else if x == 0 {
            mix(WOLF_PELT, [168, 166, 172], 0.3)
        } else if x == 4 {
            mix(WOLF_PELT, [64, 62, 66], 0.4)
        } else {
            WOLF_PELT
        }
    })
}

/// A deer's foot: a pale leg ending in a black slipper.
fn generate_deer_hoof() -> RgbaImage {
    const HOOF: [u8; 3] = [30, 26, 24];
    const DEER: [u8; 3] = [166, 116, 74];
    // A deer's leg is three pixels wide and ten tall: five cells down is
    // as much as the shape can carry.
    at_density(5, |x, y| {
        if y == 4 {
            HOOF
        } else if y == 3 {
            mix(DEER, HOOF, 0.65)
        } else if x == 0 {
            mix(DEER, [214, 176, 128], 0.35)
        } else if x == 4 {
            mix(DEER, [96, 66, 44], 0.4)
        } else {
            DEER
        }
    })
}


// ---- 1.7: the tannery, the jug, and what a person wears ----

/// Tanned leather: a cut panel, darker and more even than the raw hide
/// it came from.
///
/// The difference from `generate_hide` is the whole point of the drying
/// rack being a mechanic. A raw hide is pale, blotchy and has a ragged
/// edge; leather is a squared-off piece with a grain in it. A player who
/// has both in their pack has to be able to tell at a glance which is
/// which, and at sixteen pixels the only things that survive are the
/// silhouette and the value.
fn generate_leather() -> RgbaImage {
    const LEATHER: [u8; 3] = [122, 78, 46];
    const DARK: [u8; 3] = [92, 56, 32];
    const LIGHT: [u8; 3] = [152, 104, 66];
    /// Sinew, in the same pale the fibre item is drawn in -- because
    /// that is what a player used to sew it.
    const STITCH: [u8; 3] = [198, 176, 108];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // A rectangle with the corners taken off: a cut piece rather than a
    // whole skin.
    for y in 2..14u32 {
        for x in 2..14u32 {
            let corner = (x < 4 && y < 4)
                || (x >= 12 && y < 4)
                || (x < 4 && y >= 12)
                || (x >= 12 && y >= 12);
            if corner {
                continue;
            }
            // **The grain runs in short broken strokes, not in rules.**
            // It was a solid dark line every fourth row, which at hotbar
            // size is a piece of corrugated iron: four straight edges
            // across a rectangle read as pressed metal, and the one
            // thing leather has to say is that it is soft. Hide grain is
            // a crease here and a crease there, none of them the length
            // of the piece.
            let crease = y % 4 == 1 && !hash(0x1EA7, x / 2, y).is_multiple_of(5);
            let speck = hash(0x1EA8, x, y).is_multiple_of(7);
            let colour = if crease {
                DARK
            } else if speck {
                LIGHT
            } else {
                LEATHER
            };
            put(&mut img, x, y, colour, 255);
        }
    }
    // A rim, so the piece reads as having an edge rather than fading out.
    for n in 3..13u32 {
        put(&mut img, n, 2, DARK, 255);
        put(&mut img, n, 13, DARK, 255);
        put(&mut img, 2, n, DARK, 255);
        put(&mut img, 13, n, DARK, 255);
    }
    // **Stitching along two sides**, and it is the detail that does the
    // most work in the whole picture. A cut rectangle of brown is a
    // swatch of a material; the same rectangle with a run of stitches
    // along it is an object somebody made, and that is exactly the step
    // the drying rack represents. Two sides rather than four, because
    // four is a cushion.
    //
    // A running stitch, not a dotted line: a pale texel with a dark one
    // pressed in behind it, which is what a thread pulled through a hide
    // actually looks like -- it dimples the leather next to it. The
    // first cut put pale dots on the field alone and they vanished into
    // the speckle; the dimple is what makes them stay visible at hotbar
    // size, and it costs one more `put` per stitch.
    for n in (4..12u32).step_by(2) {
        put_opaque(&mut img, n as i32, 3, STITCH);
        put_opaque(&mut img, n as i32 + 1, 3, mix(DARK, [0, 0, 0], 0.3));
        put_opaque(&mut img, 3, n as i32, STITCH);
        put_opaque(&mut img, 3, n as i32 + 1, mix(DARK, [0, 0, 0], 0.3));
    }
    img
}

/// A fleece: a soft mass with no edge to it.
///
/// **Drawn as the opposite of the leather beside it**, which is the
/// whole job of the picture. Leather is a cut rectangle with a rim and a
/// grain running one way -- something worked, with a shape somebody gave
/// it. Wool is a blob: no straight line anywhere in it, a rounded
/// outline, and clumps rather than a grain. At hotbar size the player
/// never reads either of them in detail; what they read is "hard-edged
/// brown thing" against "soft pale thing", and that has to be right at
/// sixteen pixels because that is all there is.
fn generate_wool() -> RgbaImage {
    const WOOL: [u8; 3] = [226, 222, 210];
    const SHADE: [u8; 3] = [196, 190, 176];
    const DEEP: [u8; 3] = [166, 160, 148];
    const CREST: [u8; 3] = [246, 244, 236];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);

    // A rough circle rather than a rectangle. Measured from the centre
    // of the tile in half-pixels so the outline is not a diamond, which
    // is what an integer distance test gives at this size.
    let centre = RESOLUTION as f32 / 2.0;
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let dx = (x as f32 + 0.5) - centre;
            let dy = (y as f32 + 0.5) - centre;
            // Slightly wider than tall: a fleece sits rather than
            // stands.
            let distance = ((dx / 6.6).powi(2) + (dy / 5.8).powi(2)).sqrt();
            // A wobble on the edge, so the outline is fluffy instead of
            // drawn with a compass.
            let wobble = (hash(0x0FEE, x, y) % 5) as f32 / 40.0;
            if distance > 1.0 - wobble {
                continue;
            }
            // Clumps *and* a light, and it needs both.
            //
            // Clumps alone -- which is what this was first -- give a
            // flat disc with squares scattered on it: noise rather than
            // texture, because nothing tells the eye which way is up.
            // The gradient is what turns it into a mass: the underside
            // is in its own shadow, so the same clumps read as bundled
            // wool instead of as dirt on a circle.
            //
            // Two pixels to a clump rather than one. Per-pixel noise at
            // this size is dithering nobody can see; four-pixel clumps
            // are a chequerboard.
            // **Curls, not clumps.** Square clumps of two by two gave a
            // pale disc with a chequer on it: the mass was right and the
            // *material* was missing, because nothing in it ran in a
            // direction and wool is nothing but direction. Shifting the
            // clump's row by the column bends every one of them into a
            // short arc, and a field of short arcs at this size is a
            // fleece. It is one added term and it is the difference
            // between wool and porridge.
            let curl = x / 2 + (y / 3) % 2;
            let clump = hash(0x0FEE, curl, y / 2) % 10;
            // How far into shadow, from a light above and slightly to
            // the left -- which is where the light is in every other
            // picture in this folder.
            //
            // Curved rather than a horizontal band. Shading by the
            // pixel's *row* is one comparison and looks like a stripe
            // painted across a ball; adding the distance already
            // computed above bends the terminator round the form, so
            // the same three colours read as a sphere instead of as a
            // disc with a line on it.
            let shadow = dy / 7.0 + dx / 22.0 + distance * 0.55;
            let colour = if shadow > 0.72 {
                DEEP
            } else if shadow > 0.34 || clump < 3 {
                SHADE
            } else if clump > 8 && shadow < 0.1 {
                // The one place the light actually lands. A fleece with
                // no highlight at all is a cloud; three or four texels
                // of it and the same drawing is a solid thing with wool
                // standing up off it.
                CREST
            } else {
                WOOL
            };
            put(&mut img, x, y, colour, 255);
        }
    }

    // **The curls, stamped on top, and this is what the picture is.**
    //
    // Everything above builds a *mass*: an outline, a light and a
    // tremor. That was the whole drawing for two versions and it read as
    // a bread roll, because a mass is not a material -- it says how big
    // the fleece is and nothing about what it is made of. Wool is made
    // of locks, each of which is a little arc with its own shadow under
    // it, and eleven of them at three texels apiece is the entire
    // difference between wool and dough at this size.
    //
    // Placed by hand rather than by the hash. Scattering them cost the
    // same and put two of them on top of each other about a third of the
    // time, and two overlapping curls are one bigger blob -- which is
    // the exact thing being drawn away from.
    const CURLS: [(u32, u32); 11] = [
        (5, 5),
        (9, 4),
        (12, 6),
        (3, 8),
        (7, 7),
        (11, 9),
        (5, 11),
        (9, 11),
        (13, 10),
        (7, 9),
        (10, 6),
    ];
    for (cx, cy) in CURLS {
        // Only where there is already fleece: a curl hanging off the
        // outline is a burr.
        if img.get_pixel(cx, cy).0[3] == 0 {
            continue;
        }
        for (dx, dy, tone) in [
            (0i32, 1i32, DEEP),
            (-1, 0, SHADE),
            (1, 0, SHADE),
            (0, -1, CREST),
        ] {
            let (x, y) = (cx as i32 + dx, cy as i32 + dy);
            if x < 0 || y < 0 || x >= RESOLUTION as i32 || y >= RESOLUTION as i32 {
                continue;
            }
            if img.get_pixel(x as u32, y as u32).0[3] == 0 {
                continue;
            }
            img.put_pixel(x as u32, y as u32, Rgba([tone[0], tone[1], tone[2], 255]));
        }
    }
    img
}

/// A drying rack: a frame of sticks with a skin stretched across it.
///
/// Drawn as a *frame* rather than as a slab, because it is half a block
/// tall and is walked over -- what a player sees from standing height is
/// the top of it, so that is what has to read: four poles, a lashing at
/// each corner, and the pale skin between them.
/// The skin on a loaded rack, drawn for the slab it covers.
///
/// This is a *feature picture*, not a material: it maps corner to
/// corner onto the rack's slab (see `mesh::rack_block`), so the edges
/// of the picture are the edges of the skin -- an uneven margin, the
/// lace holes along it, and a hide that is lighter in the middle where
/// it has stretched thin. Before this the slab wore a crop of the
/// deer's own coat tile, which is featureless by design and read as a
/// sheet of cardboard.
fn generate_stretched_hide() -> RgbaImage {
    const SKIN: [u8; 3] = [178, 144, 102];
    const SPECK: [u8; 3] = [164, 130, 92];
    const THIN: [u8; 3] = [190, 158, 114];
    const EDGE: [u8; 3] = [126, 96, 64];
    const HOLE: [u8; 3] = [88, 66, 44];
    const CORD: [u8; 3] = [198, 176, 108];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            // How far into the skin this texel is, with the margin
            // wobbling a texel so the outline is a skin and not a tile.
            let wobble = (hash(0x51D3, (x + 31) as u32, (y + 17) as u32) % 2) as i32;
            let depth = x.min(y).min(15 - x).min(15 - y) - wobble;
            // The field is one quiet tan with a sparse darker grain --
            // the first cut painted a pale cloud in the middle for
            // "stretched thin", and at arm's length the cloud was all
            // anybody saw. The thinning survives as a few scattered
            // texels, not as a region.
            // **The grain runs one way**, which is the change that made
            // this stop reading as a tan tarpaulin. Scattering the
            // darker texels evenly gives a surface with no direction in
            // it, and no animal's coat has no direction: sampling the
            // hash by a stretched coordinate turns the same speckle into
            // hair lying from the spine outwards.
            let hair = hash(0x51D5, (x / 2) as u32, (y + x / 4) as u32);
            let colour = if depth <= 0 {
                EDGE
            } else if hash(0x51D4, x as u32, y as u32).is_multiple_of(11) && depth >= 4 {
                THIN
            } else if hair.is_multiple_of(4) {
                SPECK
            } else {
                SKIN
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The lace holes, spaced along the rim -- the detail that says
    // "stretched on a frame" rather than "painted on a board".
    for n in [2i32, 7, 12] {
        for &(x, y) in &[(n, 0), (n + 1, 15), (0, n + 1), (15, n)] {
            put_opaque(&mut img, x, y, HOLE);
        }
    }
    // ...and the cord through them. A hole on its own is a hole; a hole
    // with a thread pulling away from it is a skin under *tension*,
    // which is the whole of what a drying rack does to it. One texel
    // inwards from each hole, in the pale of the fibre it is tied with.
    for n in [2i32, 7, 12] {
        for &(x, y) in &[(n, 1), (n + 1, 14), (1, n + 1), (14, n)] {
            put_opaque(&mut img, x, y, CORD);
        }
    }
    img
}

fn generate_drying_rack() -> RgbaImage {
    const WOOD: [u8; 3] = [122, 92, 58];
    const WOOD_DARK: [u8; 3] = [86, 62, 38];
    const CORD: [u8; 3] = [178, 168, 128];
    const SKIN: [u8; 3] = [186, 158, 122];
    const SKIN_DARK: [u8; 3] = [156, 128, 96];
    /// What shows through the frame's gaps: shade, not transparency.
    /// The picture stays **opaque everywhere** -- a dropped rack is a
    /// spinning cube wearing this, and a part-transparent cube goes
    /// through the blended pass and reads as a pane of tan glass
    /// showing the sky through itself. That bug has been fixed here
    /// once already.
    const GAP: [u8; 3] = [46, 40, 32];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);

    // The block in the world is five poles and a skin (see
    // `mesh::rack_block`), so this is a *portrait of that model* rather
    // than a tile of material: the same two uprights, the same two
    // proud-ended crossbars, the same skin laced into the window
    // between them. It used to be a beige square with a thin border,
    // which in a pack slot read as "some tile" -- a player who had just
    // built the thing could not find it in their own inventory.
    // Painted back to front with plain overwrites: `put` refuses to
    // touch an opaque pixel, which is right for the speckle helpers and
    // wrong for painting a picture in layers.
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            put_opaque(&mut img, x as i32, y as i32, GAP);
        }
    }
    let wood = |img: &mut RgbaImage, x: u32, y: u32| {
        let grain = hash(0x0DA7, x, y).is_multiple_of(3);
        put_opaque(img, x as i32, y as i32, if grain { WOOD_DARK } else { WOOD });
    };
    // The two uprights, full height -- the silhouette the model has.
    for y in 0..RESOLUTION {
        for x in [1u32, 2, 13, 14] {
            wood(&mut img, x, y);
        }
    }
    // The crossbars, laid over the uprights and running the full width:
    // poles that overlap rather than mitre, exactly like the model.
    for x in 0..RESOLUTION {
        for y in [2u32, 3, 12, 13] {
            wood(&mut img, x, y);
        }
    }
    // The skin, stretched in the window and reaching the bars it is
    // laced to.
    for y in 4..12i32 {
        for x in 3..13i32 {
            let edge = y == 4 || y == 11 || x == 3 || x == 12;
            let speck = hash(0x5817, x as u32, y as u32).is_multiple_of(7);
            put_opaque(&mut img, x, y, if edge || speck { SKIN_DARK } else { SKIN });
        }
    }
    // The lacing: cord ties between the skin and the bars, which is the
    // one detail that says "stretched on" rather than "nailed to".
    for x in [4u32, 7, 10] {
        img.put_pixel(x, 4, Rgba([CORD[0], CORD[1], CORD[2], 255]));
        img.put_pixel(x + 1, 11, Rgba([CORD[0], CORD[1], CORD[2], 255]));
    }
    // ...and at the four joints of the frame.
    for &(x, y) in &[(1u32, 3u32), (14, 2), (1, 12), (14, 13)] {
        img.put_pixel(x, y, Rgba([CORD[0], CORD[1], CORD[2], 255]));
    }
    img
}

/// A jug, in wet clay and in fired clay and full.
///
/// One shape and three palettes, which is what the three actually are:
/// the same pot at three points in its life. Drawn with a neck and two
/// shoulders so the silhouette is a *jug* rather than the crucible's
/// open bowl -- a player with both in their pack has to tell them apart
/// at a glance.
fn jug(body: [u8; 3], dark: [u8; 3], contents: Option<[u8; 3]>) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // Rows 3..14, widening from the neck to the belly and in again.
    let half_width = |y: u32| -> u32 {
        match y {
            3..=4 => 2,  // the neck
            5 => 3,      // the shoulder
            6..=7 => 4,
            8..=11 => 5, // the belly
            12 => 4,
            13 => 3,
            _ => 0,
        }
    };
    for y in 3..14u32 {
        let w = half_width(y);
        if w == 0 {
            continue;
        }
        for x in (8 - w)..(8 + w) {
            let edge = x == 8 - w || x == 8 + w - 1 || y == 3 || y == 13;
            put(&mut img, x, y, if edge { dark } else { body }, 255);
        }
    }
    // A handle on the right, from the shoulder to the belly.
    for y in 6..11u32 {
        put(&mut img, 13, y, dark, 255);
    }
    put(&mut img, 12, 6, dark, 255);
    put(&mut img, 12, 10, dark, 255);
    // What is in it, seen through the mouth. The one pixel that tells a
    // full jug from an empty one at hotbar size, so it is bright.
    if let Some(liquid) = contents {
        for x in 6..10u32 {
            put(&mut img, x, 3, liquid, 255);
            put(&mut img, x, 4, liquid, 255);
        }
    }
    img
}

fn generate_jug_raw() -> RgbaImage {
    // The same wet grey-blue the raw crucible is, so the two read as the
    // same stage of the same process.
    jug([142, 146, 158], [108, 112, 124], None)
}

fn generate_jug() -> RgbaImage {
    jug(FIRED_CLAY, FIRED_CLAY_DARK, None)
}

fn generate_jug_water() -> RgbaImage {
    jug(FIRED_CLAY, FIRED_CLAY_DARK, Some([72, 132, 208]))
}

// ---- the four garments ----
//
// **Greyscale, and that is the mechanism rather than a shortcut.**
// Twelve garments -- leather, bronze and iron in four slots apiece --
// share these four pictures and are told apart by the tint their icon is
// drawn with (`types::garment_tint`). Twelve images would be twelve
// texture layers, and the atlas is capped at 256 by the hardware with
// only a handful to spare.
//
// So each of these is drawn in values rather than in colour: a light
// field, a darker outline, a highlight and a shadow. Multiplied by a
// leather brown it is leather; by a bronze it is bronze.

/// The weave, laid over a finished garment.
///
/// **The four had a shape and no material.** Flat fields with one
/// lighter half: multiplied by a leather brown that is a leather
/// balloon, and the pack showed four balloons in four colours. Cloth,
/// hide and beaten plate all have a grain you can see at arm's length,
/// and at sixteen texels the whole of that grain is a two-value tremor
/// with a direction in it.
///
/// Run as a pass over the finished picture rather than mixed into each
/// of the four drawings, and that is the whole argument for it being a
/// function: it has to be *the same weave* on all of them. A cap woven
/// one way and a tunic another is two garments off two different bolts,
/// and these twelve are supposed to be a set. Transparent texels are
/// left alone, so the silhouette each drawing decided is the silhouette
/// that ships.
fn weave(mut img: RgbaImage) -> RgbaImage {
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let px = img.get_pixel(x, y).0;
            if px[3] == 0 {
                continue;
            }
            // A thread every third column, stepped down the picture so
            // the threads lean; and a tremor everywhere else. The lean
            // is what gives the weave a direction -- the tremor on its
            // own is dust on a flat field.
            let thread = if (x + y / 2).is_multiple_of(3) { -8 } else { 3 };
            let tremor = (hash(0x57EA, x, y) % 7) as i32 - 3;
            img.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(px[0] as i32 + thread + tremor),
                    clamp_u8(px[1] as i32 + thread + tremor),
                    clamp_u8(px[2] as i32 + thread + tremor),
                    px[3],
                ]),
            );
        }
    }
    img
}

/// The pale field a garment is drawn on, and its two shading tones.
const CLOTH: [u8; 3] = [214, 210, 204];
const CLOTH_DARK: [u8; 3] = [138, 134, 130];
const CLOTH_EDGE: [u8; 3] = [84, 82, 80];
const CLOTH_LIGHT: [u8; 3] = [246, 244, 240];

/// A cap: a dome with a brow band.
fn generate_cap() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let half = |y: u32| -> u32 {
        match y {
            4 => 3,
            5 => 4,
            6..=9 => 5,
            10 => 6,
            _ => 0,
        }
    };
    for y in 4..11u32 {
        let w = half(y);
        if w == 0 {
            continue;
        }
        for x in (8 - w)..(8 + w) {
            let edge = x == 8 - w || x == 8 + w - 1 || y == 4;
            let band = y == 10;
            let colour = if band {
                CLOTH_EDGE
            } else if edge {
                CLOTH_DARK
            } else if x < 8 && y < 8 {
                CLOTH_LIGHT
            } else {
                CLOTH
            };
            put(&mut img, x, y, colour, 255);
        }
    }
    weave(img)
}

/// A tunic: a body with two sleeves and a neck cut out of the top.
fn generate_tunic() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 3..14u32 {
        for x in 4..12u32 {
            let neck = y < 5 && (6..10).contains(&x);
            if neck {
                continue;
            }
            let edge = x == 4 || x == 11 || y == 13;
            let colour = if edge {
                CLOTH_DARK
            } else if x < 7 {
                CLOTH_LIGHT
            } else {
                CLOTH
            };
            put(&mut img, x, y, colour, 255);
        }
    }
    // Sleeves.
    for y in 5..9u32 {
        for x in 1..4u32 {
            put(&mut img, x, y, if x == 1 { CLOTH_DARK } else { CLOTH_LIGHT }, 255);
        }
        for x in 12..15u32 {
            put(&mut img, x, y, if x == 14 { CLOTH_DARK } else { CLOTH }, 255);
        }
    }
    // A hem and a collar, so it reads as a made thing.
    for x in 4..12u32 {
        put(&mut img, x, 13, CLOTH_EDGE, 255);
    }
    for x in 5..11u32 {
        put(&mut img, x, 3, CLOTH_EDGE, 255);
    }
    weave(img)
}

/// Leggings: two legs with a waist over them.
fn generate_leggings() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The waist.
    for y in 2..5u32 {
        for x in 3..13u32 {
            let colour = if y == 2 { CLOTH_EDGE } else { CLOTH };
            put(&mut img, x, y, colour, 255);
        }
    }
    // Two legs, with a gap between them.
    for y in 5..15u32 {
        for x in 3..7u32 {
            put(&mut img, x, y, if x == 3 { CLOTH_DARK } else { CLOTH_LIGHT }, 255);
        }
        for x in 9..13u32 {
            put(&mut img, x, y, if x == 12 { CLOTH_DARK } else { CLOTH }, 255);
        }
    }
    for x in 3..7u32 {
        put(&mut img, x, 14, CLOTH_EDGE, 255);
    }
    for x in 9..13u32 {
        put(&mut img, x, 14, CLOTH_EDGE, 255);
    }
    weave(img)
}

/// Boots: a pair, seen from the side.
fn generate_boots() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let boot = |img: &mut RgbaImage, left: u32| {
        // The shaft.
        for y in 4..12u32 {
            for x in left..(left + 4) {
                let edge = x == left || y == 4;
                put(img, x, y, if edge { CLOTH_DARK } else { CLOTH }, 255);
            }
        }
        // The foot, sticking forward.
        for x in left..(left + 6) {
            put(img, x, 12, CLOTH_LIGHT, 255);
            put(img, x, 13, CLOTH_EDGE, 255);
        }
    };
    boot(&mut img, 1);
    boot(&mut img, 9);
    weave(img)
}

// ============================================================
// The seven pictures that had no drawing behind them
// ============================================================
//
// **These were on disk and in nobody's table.** Cobblestone, coal, the
// top of a cactus and the three birch faces were written by a version of
// this program that no longer exists, and the birch's leaves by nobody
// anybody can name: the files stayed, the code that made them went, and
// so there was no place to fix them. That is the
// worst state a texture can be in -- it looks generated, it *is*
// generated, and the only way to change it is to open a paint program
// and give up ever regenerating it again.
//
// So they come back here, drawn rather than speckled, and the folder has
// one story again: every picture in it is either somebody's artwork or a
// function in this file.
//
// The brightness of each is deliberately close to what it replaced. Two
// of these sit in the same wall as hand-drawn stone and hand-drawn
// planks, and a redraw that is half a stop lighter than its neighbour
// reads as a *different material* however good the drawing is. The
// contact sheet that checked this compared means, not opinions.
const REDRAWN: &[Drawn] = &[
    ("terrain/cobblestone.png", generate_cobblestone),
    ("metal/coal.png", generate_coal),
    ("plants/cactus_top.png", generate_cactus_top),
    ("terrain/birch_log_side.png", generate_birch_log_side),
    ("terrain/birch_log_top.png", generate_birch_log_top),
    ("terrain/birch_planks.png", generate_birch_planks),
    ("plants/birch_leaves.png", generate_birch_leaves),
];

/// Cobblestone: stones packed in mortar.
///
/// **What was here before was static.** Four greys thrown at every texel
/// with nothing joining them -- no stone had a size, an outline or a
/// side that faced the light. At arm's length it read as television
/// snow, and beside the hand-drawn `stone.png`, which is quiet, the
/// busier picture was the one that looked broken. Cobble is not noise:
/// it is *objects*, of different sizes, with shadow in the gaps between
/// them, and the eye finds the objects before it finds the colour.
///
/// **Drawn as cells rather than as a pattern of rectangles**, and that
/// choice is what makes it tile. Nine seed points on a jittered grid;
/// every texel belongs to the nearest one and takes that stone's own
/// grey, and a texel with two seeds nearly equidistant is mortar. The
/// distances are measured *across the wrap*, so a stone leaving the
/// right edge arrives at the left as the same stone -- a wall of these
/// has no seam in it. The rejected alternative was courses of rounded
/// rectangles, which is easier to read in the source and needs a second
/// offset row to hide its vertical joint, and still shows a horizon
/// every sixteen texels once a wall is four blocks high.
///
/// The gaps are dark and `bricks.png` next door has pale mortar. That is
/// not an accident either: the two are the only grid-shaped materials in
/// the game and they are laid side by side in the showcase, so the thing
/// that has to differ is the one visible from furthest away.
fn generate_cobblestone() -> RgbaImage {
    /// Stones across the tile. Three is five texels a stone, which is
    /// the smallest thing that still has a lit side and a shaded one.
    const GRID: i32 = 3;
    const STEP: f32 = RESOLUTION as f32 / GRID as f32;
    /// How close the two nearest seeds must be for a texel to be gap.
    /// Under one and the mortar breaks into dashes -- which is what the
    /// first cut did, and a broken gap reads as two stones fused rather
    /// than as two stones. Over one and a half they stop touching and it
    /// is gravel in cement.
    const MORTAR: f32 = 1.4;

    // A jittered grid rather than nine loose random points: loose points
    // clump, and two seeds a texel apart make a stone nobody can see
    // between two mortar lines that read as a crack.
    let seeds: Vec<(f32, f32, i32)> = (0..GRID * GRID)
        .map(|cell| {
            let index = cell as u32;
            let jitter =
                |axis: u32| (hash(0xC0BB1E, index, axis) % 26) as f32 / 10.0 - 1.3;
            (
                ((cell % GRID) as f32 + 0.5) * STEP + jitter(0),
                ((cell / GRID) as f32 + 0.5) * STEP + jitter(1),
                // Each stone its own value. Without this spread the wall
                // is one grey with lines scratched into it.
                (hash(0xC0BB1E, index, 2) % 40) as i32 - 16,
            )
        })
        .collect();

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut nearest = (f32::MAX, 0usize, 0.0f32, 0.0f32);
            let mut runner_up = f32::MAX;
            for (index, &(sx, sy, _)) in seeds.iter().enumerate() {
                let wrap = |d: f32| {
                    if d > RESOLUTION as f32 / 2.0 {
                        d - RESOLUTION as f32
                    } else if d < -(RESOLUTION as f32) / 2.0 {
                        d + RESOLUTION as f32
                    } else {
                        d
                    }
                };
                let (dx, dy) = (wrap(px - sx), wrap(py - sy));
                let distance = (dx * dx + dy * dy).sqrt();
                if distance < nearest.0 {
                    runner_up = nearest.0;
                    nearest = (distance, index, dx, dy);
                } else if distance < runner_up {
                    runner_up = distance;
                }
            }

            let grain = (hash(0xC0BB1F, x, y) % 9) as i32 - 4;
            let colour = if runner_up - nearest.0 < MORTAR {
                [
                    clamp_u8(43 + grain),
                    clamp_u8(43 + grain),
                    clamp_u8(45 + grain),
                ]
            } else {
                // Lit from the top left **within its own stone**, so
                // every cobble domes. One gradient over the whole tile
                // would be a lit wall, and a lit wall tiled four high is
                // a staircase of light nobody can unsee.
                let lift = ((-nearest.2 - nearest.3) * 3.6) as i32;
                let value = 86 + seeds[nearest.1].2 + lift + grain;
                [clamp_u8(value), clamp_u8(value), clamp_u8(value + 2)]
            };
            put_opaque(&mut img, x as i32, y as i32, colour);
        }
    }
    img
}

/// A lump of coal.
///
/// It was a black silhouette with nothing inside it: fifty-five texels
/// of one near-black, which at hotbar size is a hole in the bar. Coal is
/// the only *shiny* black thing in the game -- anthracite breaks along
/// flat faces and each face catches the sky differently -- so what makes
/// it read is not the colour, which barely varies, but the **facets**,
/// and above all the one bright edge where two of them meet.
///
/// Drawn angular on purpose. Round is a pebble, and there is a pebble
/// four slots away; the whole difference between them at this size is
/// whether the outline has corners in it.
fn generate_coal() -> RgbaImage {
    const BODY: [u8; 3] = [34, 32, 34];
    const LIT: [u8; 3] = [58, 56, 60];
    const DEEP: [u8; 3] = [17, 16, 18];
    const GLINT: [u8; 3] = [116, 116, 126];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The silhouette, row by row: an irregular block with corners, not a
    // circle. Written as spans because that is how the shape was
    // designed -- on paper, one row at a time.
    const SPANS: [(i32, i32); 11] = [
        (6, 10),
        (4, 11),
        (3, 12),
        (2, 13),
        (2, 13),
        (2, 12),
        (3, 12),
        (3, 11),
        (4, 11),
        (5, 10),
        (6, 9),
    ];
    for (row, &(from, to)) in SPANS.iter().enumerate() {
        let y = row as i32 + 3;
        for x in from..=to {
            // The fracture: a ridge running from the top left corner
            // down to the bottom right. Everything above it is the face
            // turned towards the light and everything below is the one
            // turned away -- two flat tones rather than a gradient,
            // because a gradient on a sixteen-pixel rock is mud.
            let ridge = (x - 3) - (y - 4);
            let colour = if ridge > 3 {
                DEEP
            } else if ridge > -2 {
                BODY
            } else {
                LIT
            };
            // A second, smaller fracture low on the lit face, so the
            // lump has three planes and not two. Two planes is a wedge.
            let colour = if x + y > 19 && x - y > 1 { BODY } else { colour };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The glint. Three texels along the top of the ridge, and they are
    // the reason this reads as coal rather than as a hole: a black shape
    // with one bright line in it is glossy, and a black shape without
    // one is a silhouette.
    for &(x, y) in &[(4, 5), (5, 5), (6, 6), (5, 6)] {
        put_opaque(&mut img, x, y, GLINT);
    }
    img
}

/// The top of a cactus.
///
/// It was a rectangle of one green -- the single flattest texel in the
/// game, and the one the player is looking straight down at while they
/// stand next to the plant.
///
/// **Its ribs line up with the sides.** `cactus.png` carries its two
/// dark ribs at x = 3 and x = 12 with a pale spine every fifth row, so
/// the top carries the same two ribs running in from all four edges and
/// the same spines where they reach the rim. That is the detail that
/// makes a cactus one object instead of five squares agreeing about a
/// colour: walk round it and the groove you were following does not
/// jump.
fn generate_cactus_top() -> RgbaImage {
    const FLESH: [u8; 3] = [66, 126, 68];
    const CROWN: [u8; 3] = [78, 140, 78];
    const RIB: [u8; 3] = [43, 93, 47];
    const RIM: [u8; 3] = [50, 104, 52];
    const SPINE: [u8; 3] = [210, 212, 174];
    /// Where the ribs are, read off `cactus.png` rather than chosen.
    const RIBS: [i32; 2] = [3, 12];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            // How far in from the nearest edge. The top of a cactus is
            // domed, so the middle is the part turned to the sky and it
            // is the lighter one.
            let depth = x.min(y).min(15 - x).min(15 - y);
            let grain = (hash(0xCAC7, x as u32, y as u32) % 7) as i32 - 3;
            let base = if depth == 0 {
                RIM
            } else if depth >= 5 {
                CROWN
            } else {
                FLESH
            };
            put_opaque(
                &mut img,
                x,
                y,
                [
                    clamp_u8(base[0] as i32 + grain),
                    clamp_u8(base[1] as i32 + grain * 2),
                    clamp_u8(base[2] as i32 + grain),
                ],
            );
        }
    }
    // The ribs, running in from each edge and stopping short of the
    // middle: a rib is a groove down the flank, and the flanks are what
    // the edges of this face are.
    //
    // Three texels deep, not five. Five met in the middle and turned the
    // face into a window frame -- eight lines crossing a green square,
    // which is a *grid*, and a grid is the one thing a plant must not
    // look like. The groove has to say where it comes from and then stop.
    for rib in RIBS {
        for depth in 0..3i32 {
            for &(x, y) in &[
                (rib, depth),
                (rib, 15 - depth),
                (depth, rib),
                (15 - depth, rib),
            ] {
                put_opaque(&mut img, x, y, RIB);
            }
        }
    }
    // The spines, where the ribs meet the rim -- the same pale as the
    // ones on the flank, in the places the flank's rows would put them.
    for rib in RIBS {
        for &(x, y) in &[(rib, 0), (rib, 15), (0, rib), (15, rib)] {
            put_opaque(&mut img, x, y, SPINE);
        }
    }
    img
}

/// Birch bark.
///
/// The old one was white with black almonds stamped through it at even
/// spacing, which at two blocks' distance is a chain-link fence. Real
/// bark is *papery*: a pale ground with lenticels -- short horizontal
/// scars -- of different lengths and no rhythm, and long shallow streaks
/// of grey where the sheet has lifted.
///
/// Kept close to as pale as it was, because birch stands in a grove next
/// to oak and a birch that had been quietly darkened would look like a
/// diseased one. The first cut of this drawing went the other way and
/// came out thirty-five values *brighter* than what it replaced -- which
/// is not a lighter wood, it is a lamp: a trunk that bright is the
/// brightest thing in a forest and the eye goes to it instead of to
/// whatever the player was looking for. The mean of the picture is a
/// constraint on a redraw, not an afterthought.
fn generate_birch_log_side() -> RgbaImage {
    const BARK: [u8; 3] = [177, 175, 167];
    const BARK_PALE: [u8; 3] = [199, 198, 190];
    // Barely darker than the field. It was twenty values down, and two
    // of the columns that drew it happened to land side by side: the
    // trunk grew a pair of grey seams down it and read as a cast pole.
    // A shade in bark is a shade, not a join.
    const BARK_GREY: [u8; 3] = [162, 160, 153];
    const SCAR: [u8; 3] = [64, 62, 58];
    const SCAR_SOFT: [u8; 3] = [104, 100, 94];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            // **The ground is nearly plain, and that took a second go.**
            // The first cut varied the field per column in blocks four
            // rows tall, which at a distance is a wall of pale concrete
            // panels: the variation was louder than the lenticels, and
            // the lenticels are the only thing that says "birch". So the
            // field is one pale tone with a single soft column of shade
            // every few texels, and everything else is left to the
            // scars below.
            let streak = hash(0x81C4, x, 0) % 9;
            let grain = (hash(0x81C5, x, y) % 5) as i32 - 2;
            let base = match streak {
                0 => BARK_GREY,
                1 | 2 => BARK_PALE,
                _ => BARK,
            };
            put_opaque(
                &mut img,
                x as i32,
                y as i32,
                [
                    clamp_u8(base[0] as i32 + grain),
                    clamp_u8(base[1] as i32 + grain),
                    clamp_u8(base[2] as i32 + grain),
                ],
            );
        }
    }
    // The lenticels. (row, first column, length) -- written out rather
    // than hashed, because the one thing that must not happen is two of
    // them meeting end to end, which reads as a crack running round the
    // trunk. None of them reaches an edge, so the tile joins its
    // neighbour on plain bark.
    const SCARS: [(i32, i32, i32); 9] = [
        (1, 2, 4),
        (2, 10, 2),
        (4, 6, 3),
        (6, 1, 3),
        (7, 11, 4),
        (9, 8, 2),
        (11, 3, 5),
        (13, 12, 2),
        (14, 6, 3),
    ];
    for (y, from, length) in SCARS {
        for step in 0..length {
            let x = from + step;
            // Tapered: dark in the middle of the scar and soft at both
            // ends, which is the difference between a mark in the bark
            // and a sticker on it.
            let end = step == 0 || step == length - 1;
            put_opaque(&mut img, x, y, if end { SCAR_SOFT } else { SCAR });
        }
    }
    img
}

/// The cut end of a birch: heartwood in rings, inside its own bark.
///
/// It was a brown square with three lines ruled across it. A cut log is
/// the one face in the game with a *centre*, and rings are the cheapest
/// possible way to say so -- the eye reads concentric anything as a
/// section through something.
///
/// The bark ring round the outside is what keeps it tiling: the border
/// is one colour all the way round, so four logs stacked show four
/// sections rather than one smeared one.
fn generate_birch_log_top() -> RgbaImage {
    const BARK: [u8; 3] = [170, 168, 161];
    const WOOD: [u8; 3] = [158, 131, 95];
    const WOOD_DARK: [u8; 3] = [131, 107, 75];
    const PITH: [u8; 3] = [100, 79, 53];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let centre = RESOLUTION as f32 / 2.0;
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let dx = (x as f32 + 0.5) - centre;
            let dy = (y as f32 + 0.5) - centre;
            // Slightly out of round, and off-centre by a texel: a
            // perfectly circular section drawn from the middle of the
            // tile reads as a target painted on the end of a log.
            let radius = ((dx + 0.5) * (dx + 0.5) * 1.06 + dy * dy).sqrt();
            let grain = (hash(0xB17C, x, y) % 9) as i32 - 4;
            let base = if x == 0 || y == 0 || x == RESOLUTION - 1 || y == RESOLUTION - 1 {
                BARK
            } else if radius < 1.4 {
                PITH
            } else if ((radius * 0.85) as u32).is_multiple_of(2) {
                WOOD_DARK
            } else {
                WOOD
            };
            put_opaque(
                &mut img,
                x as i32,
                y as i32,
                [
                    clamp_u8(base[0] as i32 + grain),
                    clamp_u8(base[1] as i32 + grain),
                    clamp_u8(base[2] as i32 + grain),
                ],
            );
        }
    }
    img
}

/// Birch planks: sawn boards, paler and cooler than the oak beside them.
///
/// The old picture put a knot at the same place in every board, which
/// tiled into a lattice of dots. Boards are told apart by their **ends**
/// and their **grain**, so this draws four courses with the butt joints
/// staggered and a grain line that wanders instead of repeating.
///
/// Two shades paler than `planks.png` and no more. Birch has to be
/// recognisably a second wood -- a player who mines the wrong tree
/// should see it -- and it must not be the brightest thing in a house,
/// which is what a properly cream-coloured birch would be next to this
/// pack's dark oak.
fn generate_birch_planks() -> RgbaImage {
    const BOARD: [u8; 3] = [132, 112, 82];
    const BOARD_PALE: [u8; 3] = [152, 131, 98];
    const BOARD_DARK: [u8; 3] = [110, 92, 66];
    const SEAM: [u8; 3] = [58, 47, 34];

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    /// Where each course's butt joint falls. Staggered, and none of them
    /// at 0 or 15: a joint on the tile's own edge doubles into a
    /// two-texel black line the moment two blocks are placed side by
    /// side.
    const JOINTS: [u32; 4] = [5, 11, 3, 13];
    for y in 0..RESOLUTION {
        let course = (y / 4) as usize;
        for x in 0..RESOLUTION {
            let grain = (hash(0x81C7, x, y) % 9) as i32 - 4;
            // The gap between courses, and the butt joint across one.
            let colour = if y % 4 == 3 || x == JOINTS[course] {
                SEAM
            } else {
                // Grain: streaks *along* the board. Sampled by the row
                // and by a coarse step across it, so a run of texels in
                // one row shares a tone and the line reads as figure in
                // the wood rather than as stipple on it.
                match hash(0x81C6, x / 4 + course as u32 * 5, y) % 9 {
                    0 | 1 => BOARD_DARK,
                    2 | 3 => BOARD_PALE,
                    4 => mix(BOARD_DARK, SEAM, 0.35),
                    _ => BOARD,
                }
            };
            put_opaque(
                &mut img,
                x as i32,
                y as i32,
                [
                    clamp_u8(colour[0] as i32 + grain),
                    clamp_u8(colour[1] as i32 + grain),
                    clamp_u8(colour[2] as i32 + grain),
                ],
            );
        }
    }
    img
}

/// Birch leaves: a light, open crown, against the oak's dark one.
///
/// **What was on disk was a lamp.** Its mean was (108, 197, 77) against
/// the oak's (74, 87, 44) -- not a lighter green, a *fluorescent* one,
/// and the only saturated colour in a pack whose whole palette is muted
/// naturals. A birch wood painted in it was the brightest thing on the
/// horizon from any distance, which is the same mistake
/// `generate_birch_log_side` records having made with the bark and
/// backed out of. It had no drawing behind it either, which is how it
/// stayed that way: nothing here could be fixed, only repainted.
///
/// So: lighter and yellower than the oak and no more. Birch is a pale
/// tree and it has to read as one at fifty metres -- that is the whole
/// point of `Biome::BirchForest` being a *place* rather than a texture
/// swap -- but the difference the eye needs is about thirty values, not
/// a hundred and ten.
///
/// **Sprays rather than speckle.** Foliage drawn as per-texel noise is
/// static, and the block next to it is drawn by hand: the oak's leaves
/// have clumps in them with shade underneath, and a neighbour made of
/// television snow reads as a hole in the wood. So this places leaf
/// sprays -- six texels apiece, lit at the tip and dark at the stalk --
/// over an understorey of leaves too deep in the crown to catch light.
/// Everything wraps, so a wall of leaves has no seam in it.
///
/// The gaps are the other half of it. About a quarter of the tile is
/// left empty, which is what `Biome::grass_spacing` already says about
/// a birch wood in prose: light gets in.
fn generate_birch_leaves() -> RgbaImage {
    const LEAF: [u8; 3] = [118, 160, 72];
    const LEAF_LIT: [u8; 3] = [158, 196, 104];
    const LEAF_DARK: [u8; 3] = [80, 120, 52];
    /// Leaves behind the ones in the light. Dark enough to be depth
    /// rather than another leaf -- without it the crown is a flat
    /// cut-out and every spray reads as a sticker on nothing.
    const LEAF_SHADE: [u8; 3] = [56, 90, 40];
    const SEED: u32 = 0xA33F;

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let mut put = |x: i32, y: i32, base: [u8; 3]| {
        // Everything wraps: this tile is laid against copies of itself
        // on all four sides, and a spray that ran off the edge and
        // stopped would draw a line down the middle of a canopy.
        let x = x.rem_euclid(RESOLUTION as i32) as u32;
        let y = y.rem_euclid(RESOLUTION as i32) as u32;
        // A little grain, so a spray is a leaf and not a decal.
        let grain = (hash(SEED + 3, x, y) % 9) as i32 - 4;
        img.put_pixel(
            x,
            y,
            Rgba([
                clamp_u8(base[0] as i32 + grain),
                clamp_u8(base[1] as i32 + grain),
                clamp_u8(base[2] as i32 + grain),
                255,
            ]),
        );
    };

    // The understorey first, so the sprays sit in front of it.
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            if hash(SEED + 7, x as u32, y as u32) % 10 < 6 {
                put(x, y, LEAF_SHADE);
            }
        }
    }

    /// One spray, as offsets from where it is planted: the tip in the
    /// light, three leaves across the middle, and the stalk end in
    /// shade. Six texels, because a spray of two is a pixel and a spray
    /// of ten is a hedge.
    const SPRAY: [(i32, i32, u8); 6] = [
        (0, 0, b'l'),
        (1, 0, b'm'),
        (-1, 1, b'm'),
        (0, 1, b'm'),
        (1, 1, b'd'),
        (0, 2, b'd'),
    ];
    for spray in 0..17u32 {
        let cx = (hash(SEED, spray, 0) % RESOLUTION) as i32;
        let cy = (hash(SEED, spray, 1) % RESOLUTION) as i32;
        // Half of them mirrored, or seventeen copies of one spray read
        // as a pattern rather than as leaves.
        let flip = hash(SEED, spray, 2) % 2 == 1;
        for (dx, dy, tone) in SPRAY {
            let dx = if flip { -dx } else { dx };
            let colour = match tone {
                b'l' => LEAF_LIT,
                b'd' => LEAF_DARK,
                _ => LEAF,
            };
            put(cx + dx, cy + dy, colour);
        }
    }
    img
}

// ---- the player ----
//
// One 64x32 sheet, cut into the six faces of six boxes. Everything
// about where a rectangle sits is `skin_net` below, which is a copy of
// `logic::player_model::net` -- an example can only see the crate's
// public surface, so the arithmetic exists twice and is pinned together
// by `every_face_of_the_model_lands_on_a_painted_part_of_the_skin`.
//
// **Why draw a person at all, when every animal here is a speckle.** An
// animal is a shape you recognise from its outline: a boar is a boar
// before you can see its eye. A person is not. Two arms, two legs and a
// head is also a scarecrow, a statue and every other humanoid, and what
// separates one from another at ten metres is entirely the surface --
// a face where a face should be, sleeves that end at the forearm, boots
// that are darker than the legs above them. That is why this one
// picture is drawn pixel by pixel instead of being a tinted field of
// noise like the hides.

const PLAYER_SKIN: &str = "players/player.png";

/// The sheet, in texels. Must match `player_model::SHEET_WIDTH/HEIGHT`.
const SKIN_W: u32 = 64;
const SKIN_H: u32 = 32;

/// Where one face of one box sits on the sheet: `[x, y, w, h]`.
///
/// The classic unfolded box -- four sides in a strip, the top and the
/// bottom above them offset by the depth. The twin of
/// `player_model::net`; see the note at the top of this section.
fn skin_net(sheet: [u32; 2], size: [u32; 3], face: usize) -> [u32; 4] {
    let [x, y] = sheet;
    let [w, h, d] = size;
    match face {
        0 => [x + d, y, w, d],             // +Y  crown
        1 => [x + d + w, y, w, d],         // -Y  underside
        2 => [x, y + d, d, h],             // +X  their right
        3 => [x + d + w, y + d, d, h],     // -X  their left
        4 => [x + d + w + d, y + d, w, h], // +Z  back
        _ => [x + d, y + d, w, h],         // -Z  front
    }
}

// A stone-age wardrobe, and every colour in it is doing a job at ten
// metres. Skin against hide against dark boots is three values far
// enough apart to survive being four pixels tall, which is what the
// figure comes to across a clearing.
const P_SKIN: [u8; 3] = [214, 166, 128];
const P_SKIN_DARK: [u8; 3] = [178, 130, 96];
const P_HAIR: [u8; 3] = [74, 50, 36];
const P_HAIR_LIT: [u8; 3] = [98, 68, 48];
const P_EYE_WHITE: [u8; 3] = [236, 234, 228];
const P_EYE: [u8; 3] = [58, 74, 96];
const P_MOUTH: [u8; 3] = [142, 88, 76];
/// Tanned hide: what everything on this player is made of, because it
/// is what the first hour of the game gives you.
const P_TUNIC: [u8; 3] = [150, 112, 72];
const P_TUNIC_DARK: [u8; 3] = [110, 80, 50];
const P_LACE: [u8; 3] = [206, 186, 146];
const P_BELT: [u8; 3] = [90, 64, 40];
// Lighter than the boots by a clear step. They were four values apart
// and the whole leg read as one dark mass with a lace on it: at ten
// metres a difference has to be a *step*, not a shade.
const P_TROUSER: [u8; 3] = [118, 100, 76];
const P_TROUSER_DARK: [u8; 3] = [88, 72, 54];
const P_BOOT: [u8; 3] = [62, 48, 38];
const P_SOLE: [u8; 3] = [44, 34, 28];

/// Fills a rectangle of the sheet from a function of its own corner.
///
/// Every face is written this way -- `(u, v) -> colour`, with `u` and
/// `v` counted from the rectangle's top left -- so a face's drawing
/// reads as a description of that face and never has to know where on
/// the sheet it landed.
fn paint(img: &mut RgbaImage, rect: [u32; 4], mut f: impl FnMut(u32, u32) -> [u8; 3]) {
    let [x, y, w, h] = rect;
    for v in 0..h {
        for u in 0..w {
            let c = f(u, v);
            img.put_pixel(x + u, y + v, Rgba([c[0], c[1], c[2], 255]));
        }
    }
}

/// Copies a rectangle of the sheet onto another, flipped left to right.
///
/// The two sides of a box are mirror images of each other -- `face_uv`
/// maps `u = 1 - z` on the +X face and `u = z` on -X -- so the head's
/// left side is literally the right side backwards. Drawing it that way
/// rather than by hand is what stops the ear from being in front of the
/// eye on one side of the head, which is the fault
/// `animal_model::Skin::HeadMirror` exists for.
fn mirror_rect(img: &mut RgbaImage, from: [u32; 4], to: [u32; 4]) {
    for v in 0..from[3] {
        for u in 0..from[2] {
            let px = *img.get_pixel(from[0] + u, from[1] + v);
            img.put_pixel(to[0] + from[2] - 1 - u, to[1] + v, px);
        }
    }
}

/// A speckle of grain, for the parts made of hide.
///
/// Two shades either way, keyed to the position, which is the same
/// trick `generate` plays on a block: without it a tunic is a flat
/// rectangle of brown and reads as plastic beside terrain that has
/// grain in every texel.
fn grain(colour: [u8; 3], seed: u32, u: u32, v: u32) -> [u8; 3] {
    let n = (hash(seed, u, v) % 5) as i32 - 2;
    [
        clamp_u8(colour[0] as i32 + n * 4),
        clamp_u8(colour[1] as i32 + n * 4),
        clamp_u8(colour[2] as i32 + n * 3),
    ]
}

/// The whole player, drawn face by face.
fn generate_player_skin() -> RgbaImage {
    let mut img = RgbaImage::new(SKIN_W, SKIN_H);

    const HEAD: ([u32; 2], [u32; 3]) = ([0, 0], [8, 8, 8]);
    const TORSO: ([u32; 2], [u32; 3]) = ([0, 16], [7, 12, 4]);
    const ARM: ([u32; 2], [u32; 3]) = ([22, 16], [3, 12, 3]);
    const LEG: ([u32; 2], [u32; 3]) = ([34, 16], [3, 12, 3]);
    let net = |part: ([u32; 2], [u32; 3]), face: usize| skin_net(part.0, part.1, face);

    // ---- the head ----
    //
    // The face is the whole reason this sheet exists, and it is eight
    // pixels across. Everything on it is therefore either two pixels or
    // one: eyes with a white and an iris each, a brow above them, a
    // shadow for the nose and a mouth. Any more detail at this size is
    // mud.
    paint(&mut img, net(HEAD, 5), |u, v| match (u, v) {
        // The fringe, ragged at the ends rather than ruled straight
        // across -- a level hairline reads as a helmet.
        (_, 0) | (_, 1) => P_HAIR,
        // Three locks hanging a texel lower than the rest, because a
        // hairline ruled straight across is a cap -- which is exactly
        // what the first draft of this looked like.
        (0, 2) | (3, 2) | (7, 2) => P_HAIR,
        (1, 3) | (2, 3) | (5, 3) | (6, 3) => P_HAIR,
        (1, 4) | (6, 4) => P_EYE_WHITE,
        (2, 4) | (5, 4) => P_EYE,
        (3, 5) | (4, 5) => P_SKIN_DARK,
        (3, 6) | (4, 6) => P_MOUTH,
        (0, v) | (7, v) if v >= 3 => P_SKIN_DARK,
        (_, 7) => P_SKIN_DARK,
        _ => P_SKIN,
    });
    // The right side. `u` runs from the back of the skull to the face,
    // because `face_uv` maps the +X face as `u = 1 - z` and the model
    // looks along -Z.
    paint(&mut img, net(HEAD, 2), |u, v| match (u, v) {
        (_, 0) | (_, 1) => P_HAIR,
        (5, 2) => P_HAIR,
        // The ear: two texels of shadowed skin against the hair, which
        // is the whole of what makes a head read as a head from the
        // side.
        (5, 3) | (5, 4) => P_SKIN_DARK,
        (6, 3) => P_SKIN,
        (u, _) if u <= 4 => P_HAIR,
        (_, 7) => P_SKIN_DARK,
        _ => P_SKIN,
    });
    let (right, left) = (net(HEAD, 2), net(HEAD, 3));
    mirror_rect(&mut img, right, left);
    // The back of the head and the crown: hair, with the light on top.
    paint(&mut img, net(HEAD, 4), |u, v| {
        if v >= 6 && (2..=5).contains(&u) {
            P_SKIN_DARK // the nape, under the hair
        } else if v <= 1 {
            P_HAIR_LIT
        } else {
            grain(P_HAIR, 0x9101, u, v)
        }
    });
    paint(&mut img, net(HEAD, 0), |u, v| {
        // A crown: lighter in the middle, where the light falls.
        if (2..=5).contains(&u) && (2..=5).contains(&v) {
            P_HAIR_LIT
        } else {
            grain(P_HAIR, 0x9102, u, v)
        }
    });
    // Under the jaw: shadow, with the neck in the middle of it.
    paint(&mut img, net(HEAD, 1), |u, v| {
        if (2..=5).contains(&u) && (2..=5).contains(&v) {
            P_HAIR
        } else {
            P_SKIN_DARK
        }
    });

    // ---- the torso ----
    //
    // A hide tunic laced up the front, belted at the waist, with a
    // ragged hem. The belt is the one horizontal line on the figure and
    // it is what makes the body read as having a waist rather than
    // being a plank.
    let tunic = |u: u32, v: u32| grain(P_TUNIC, 0x9201, u, v);
    paint(&mut img, net(TORSO, 5), |u, v| match (u, v) {
        // The neckline: bare chest under the collar.
        (2..=4, 0) => P_SKIN,
        (3, 1) => P_SKIN_DARK,
        (_, 8) => {
            if u == 3 {
                P_LACE // the buckle
            } else {
                P_BELT
            }
        }
        (3, 2..=7) => P_TUNIC_DARK, // the lacing seam
        (2, 3) | (4, 3) | (2, 6) | (4, 6) => P_LACE, // ...and its cross-stitches
        (_, 11) => P_TUNIC_DARK,    // the hem
        _ => tunic(u, v),
    });
    paint(&mut img, net(TORSO, 4), |u, v| match (u, v) {
        (_, 0) => P_TUNIC_DARK, // the collar, seen from behind
        (_, 8) => P_BELT,
        (_, 11) => P_TUNIC_DARK,
        _ => tunic(u, v),
    });
    let torso_side = |u: u32, v: u32| match (u, v) {
        (_, 8) => P_BELT,
        (_, 11) => P_TUNIC_DARK,
        // The side seam, which is what stops the flanks from being a
        // flat field of brown.
        (0, _) => P_TUNIC_DARK,
        _ => tunic(u, v),
    };
    paint(&mut img, net(TORSO, 2), torso_side);
    paint(&mut img, net(TORSO, 3), torso_side);
    // The shoulders, with the neck hole in them.
    paint(&mut img, net(TORSO, 0), |u, v| {
        if (2..=4).contains(&u) && (1..=2).contains(&v) {
            P_SKIN_DARK
        } else {
            tunic(u, v)
        }
    });
    paint(&mut img, net(TORSO, 1), |u, v| grain(P_TUNIC_DARK, 0x9202, u, v));

    // ---- the arms ----
    //
    // A sleeve to the elbow, bare forearm, and a hand. **The sleeve is
    // the point**: an arm in one colour from shoulder to fingertip is a
    // stick, and the change of value a third of the way down is what
    // makes it read as a limb with a joint in it.
    let arm_side = |u: u32, v: u32| match v {
        0..=4 => grain(P_TUNIC, 0x9301, u, v),
        5 => P_TUNIC_DARK, // the cuff
        9 => P_SKIN_DARK,  // the knuckles
        11 => {
            // The fingers, hinted at by alternating the shadow: at three
            // pixels across that is as much of a hand as there is room
            // for.
            if u == 1 {
                P_SKIN_DARK
            } else {
                P_SKIN
            }
        }
        _ => P_SKIN,
    };
    for face in [2, 3, 4, 5] {
        paint(&mut img, net(ARM, face), arm_side);
    }
    paint(&mut img, net(ARM, 0), |u, v| grain(P_TUNIC, 0x9302, u, v));
    paint(&mut img, net(ARM, 1), |u, v| {
        if u == 1 && v == 1 {
            P_SKIN_DARK // the palm
        } else {
            P_SKIN
        }
    });

    // ---- the legs ----
    //
    // Hide trousers into boots, and the boot is two thirds of what makes
    // a leg read as a leg: it is the darkest thing on the figure and it
    // is where the eye expects the ground.
    let leg_side = |u: u32, v: u32| match v {
        0..=5 => grain(P_TROUSER, 0x9401, u, v),
        6 => P_TROUSER_DARK, // the trouser hem, over the boot
        7 => P_BOOT,
        8 => {
            if u == 1 {
                P_LACE // one thong across the boot
            } else {
                P_BOOT
            }
        }
        11 => P_SOLE,
        _ => grain(P_BOOT, 0x9402, u, v),
    };
    for face in [2, 3, 4, 5] {
        paint(&mut img, net(LEG, face), leg_side);
    }
    paint(&mut img, net(LEG, 0), |u, v| grain(P_TROUSER, 0x9403, u, v));
    paint(&mut img, net(LEG, 1), |_, _| P_SOLE);

    img
}

/// Sinew: a twisted cord, pale and dry, lying corner to corner the way
/// the stick does -- it is the other half of a hafted tool and the two
/// should read as a pair in the pack.
fn generate_sinew() -> RgbaImage {
    const CORD: [u8; 3] = [214, 196, 150];
    const CORD_DARK: [u8; 3] = [172, 152, 108];
    const CORD_PALE: [u8; 3] = [236, 224, 190];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for step in 0..12 {
        let x = 2 + step;
        let y = 13 - step;
        // The twist: light and dark alternate along the strand.
        let (a, b) = if step % 2 == 0 { (CORD_PALE, CORD) } else { (CORD, CORD_DARK) };
        put_opaque(&mut img, x, y, a);
        put_opaque(&mut img, x + 1, y, b);
        put_opaque(&mut img, x, y + 1, CORD_DARK);
    }
    // A frayed end, so it is a tendon and not a straw.
    put_opaque(&mut img, 1, 14, CORD_DARK);
    put_opaque(&mut img, 2, 15, CORD);
    put_opaque(&mut img, 14, 1, CORD_DARK);
    put_opaque(&mut img, 15, 2, CORD);
    img
}

/// Bone: a shaft with a knob at each end, off-white with a grey shadow
/// side. Lying diagonally like every other long item.
fn generate_bone() -> RgbaImage {
    const BONE: [u8; 3] = [232, 226, 208];
    const BONE_DARK: [u8; 3] = [188, 180, 160];
    const BONE_PALE: [u8; 3] = [246, 242, 230];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for step in 0..9 {
        let x = 4 + step;
        let y = 11 - step;
        put_opaque(&mut img, x, y, BONE);
        put_opaque(&mut img, x + 1, y, BONE_DARK);
        put_opaque(&mut img, x, y - 1, BONE_PALE);
    }
    // The knobs: a small lump at each end, two texels proud of the shaft.
    for (cx, cy) in [(3, 12), (13, 2)] {
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1), (-1, 0), (0, -1)] {
            let colour = if dx + dy < 0 { BONE_PALE } else if dx + dy > 1 { BONE_DARK } else { BONE };
            put_opaque(&mut img, cx + dx, cy + dy, colour);
        }
    }
    img
}

/// A brick of dried peat: a dark fibrous block, cut square, with the
/// grain of the sods it was cut from running across it.
fn generate_dried_peat() -> RgbaImage {
    const PEAT: [u8; 3] = [78, 58, 40];
    const PEAT_DARK: [u8; 3] = [56, 40, 26];
    const FIBRE: [u8; 3] = [112, 90, 60];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 5..12 {
        for x in 2..14 {
            let colour = if (x * 3 + y * 5) % 7 == 0 { FIBRE } else if y == 11 || x == 13 { PEAT_DARK } else { PEAT };
            put_opaque(&mut img, x, y, colour);
        }
    }
    img
}


// ---- the honest tool chain, bog iron, and what has gone off ----
//
// Nine pictures that arrived together and are read together: the cord
// and the resin are the two halves of hafting a stone tool (see
// `types::BLOCK_CORD`), the glue is what the resin becomes, the glued
// axe and pick are the lashed ones with the joint set, the spear is the
// knife's point on a long haft, and the rusty stone and the dust are the
// iron a riverbank has. They share the flint and haft palette above so
// that a pack full of them reads as one kit and not nine strangers.
const HAFTED: &[Drawn] = &[
    ("tools/cord.png", generate_cord),
    ("tools/resin.png", generate_resin),
    ("tools/glue.png", generate_glue),
    ("tools/flint_spear.png", generate_flint_spear),
    // The glued axe is not here: it is collared onto the *drawn* stone
    // axe in the folder and so needs the folder -- see `GLUED_AXE` in
    // `main`, beside the flat backpack, which reads a file for the same
    // reason.
    ("tools/glued_pickaxe.png", generate_glued_pickaxe),
    ("terrain/rusty_stone.png", generate_rusty_stone),
    ("metal/iron_dust.png", generate_iron_dust),
    ("food/rotten.png", generate_rotten),
];

/// The glued axe, written from `main` because it reads the folder.
const GLUED_AXE: &str = "tools/glued_axe.png";

/// Cooked resin: near-black brown with a wet sheen. One colour for the
/// pot's contents and for the collar on a glued tool, so the eye ties
/// the two together without being told.
const GLUE: [u8; 3] = [46, 34, 26];
const GLUE_LIT: [u8; 3] = [104, 80, 58];

/// Cord: fibre twisted into rope and coiled, with a loose end.
///
/// A coil rather than the sinew's single diagonal strand, and that is
/// the whole of what tells them apart in a pack: they are the same
/// colour on purpose -- both are the thing a head is bound with -- and
/// a second straight strand would be a second sinew. The twist is
/// light and dark alternating *around* the ring, the way the sinew
/// alternates along its length, so the two still read as the same
/// material.
fn generate_cord() -> RgbaImage {
    const CORD: [u8; 3] = [190, 170, 120];
    const CORD_DARK: [u8; 3] = [146, 126, 82];
    const CORD_PALE: [u8; 3] = [222, 206, 162];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let (cx, cy) = (7.5f32, 7.0f32);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let r = (dx * dx + dy * dy).sqrt();
            if !(3.4..=6.4).contains(&r) {
                continue;
            }
            // Eight twists round the ring, offset by half a twist on
            // the outer half so the strands lie at a slant rather than
            // as spokes.
            let angle = dy.atan2(dx);
            let twist = ((angle / std::f32::consts::PI * 8.0).floor() as i32
                + if r > 4.9 { 1 } else { 0 })
                .rem_euclid(2);
            // Lit from the top left, shadowed where the ring turns away.
            let lit = dx + dy < -1.5;
            let dark = dx + dy > 3.0 || !(4.0..=5.7).contains(&r);
            let colour = match (twist, lit, dark) {
                (_, true, _) => CORD_PALE,
                (_, _, true) => CORD_DARK,
                (0, _, _) => CORD,
                _ => CORD_PALE,
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The loose end, trailing out of the coil to the bottom right: a
    // ring with no end is a bangle.
    for step in 0..4 {
        put_opaque(&mut img, 11 + step, 12 + step, CORD);
        put_opaque(&mut img, 12 + step, 12 + step, CORD_DARK);
    }
    put_opaque(&mut img, 15, 15, CORD_DARK);
    img
}

/// Resin: two drops of amber, one large and one small, each with a
/// bright bead of light on its shoulder.
///
/// Drops rather than a lump, because a lump of orange is a carrot. What
/// says "resin" at this size is the teardrop silhouette and the one
/// bright pixel that says the surface is glossy and the inside is not
/// -- amber is the one thing in the pack that looks lit from within.
fn generate_resin() -> RgbaImage {
    const AMBER: [u8; 3] = [214, 148, 50];
    const AMBER_DEEP: [u8; 3] = [160, 96, 28];
    const AMBER_GLOW: [u8; 3] = [240, 196, 96];
    const AMBER_BEAD: [u8; 3] = [252, 236, 190];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const BIG: &[Span] = &[
        (2, 6, 6),
        (3, 5, 7),
        (4, 5, 7),
        (5, 4, 8),
        (6, 3, 9),
        (7, 3, 9),
        (8, 3, 9),
        (9, 3, 9),
        (10, 4, 8),
        (11, 5, 7),
    ];
    const SMALL: &[Span] = &[
        (8, 12, 12),
        (9, 11, 13),
        (10, 11, 13),
        (11, 10, 14),
        (12, 10, 14),
        (13, 11, 13),
    ];
    for (drop, bead) in [(BIG, (4, 6)), (SMALL, (11, 10))] {
        for &(y, from, to) in drop {
            for x in from..=to {
                // Deep amber at the rim and along the shadowed side,
                // glowing in the middle where the light comes through.
                let rim = x == from || x == to;
                let shadow = x > from + (to - from) * 2 / 3 && y > drop[0].0 + 2;
                let colour = if rim && shadow {
                    AMBER_DEEP
                } else if rim || shadow {
                    AMBER
                } else {
                    AMBER_GLOW
                };
                put_opaque(&mut img, x, y, colour);
            }
        }
        put_opaque(&mut img, bead.0, bead.1, AMBER_BEAD);
    }
    img
}

/// Glue: a dark pot of cooked pitch with the stirring stick left in it.
///
/// The stick is what makes it glue rather than a lump of coal: a black
/// mass on its own is fuel, and a black mass with a haft standing out
/// of it is something being worked. The same haft colour as every tool,
/// because it is the same worked stick.
fn generate_glue() -> RgbaImage {
    const POT: [u8; 3] = [120, 92, 66];
    const POT_DARK: [u8; 3] = [82, 60, 42];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    // The pot: a squat clay bowl, rows 8..14, lit on the left.
    const BOWL: &[Span] = &[
        (8, 3, 12),
        (9, 3, 12),
        (10, 3, 12),
        (11, 4, 11),
        (12, 4, 11),
        (13, 5, 10),
    ];
    for &(y, from, to) in BOWL {
        for x in from..=to {
            let colour = if x >= to - 2 || y >= 12 { POT_DARK } else { POT };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The pitch, heaped over the rim and run down one side: dark, with
    // one wet highlight, and a drip that has set on the bowl.
    const PITCH: &[Span] = &[(5, 6, 8), (6, 4, 10), (7, 3, 11), (8, 3, 12), (9, 4, 11)];
    for &(y, from, to) in PITCH {
        for x in from..=to {
            put_opaque(&mut img, x, y, GLUE);
        }
    }
    put_opaque(&mut img, 5, 6, GLUE_LIT);
    put_opaque(&mut img, 6, 6, GLUE_LIT);
    for y in 10..=12 {
        put_opaque(&mut img, 11, y, GLUE);
    }
    // The stick, standing out of the pitch to the top right.
    for step in 0..6 {
        put_opaque(&mut img, 9 + step / 2, 6 - step, HAFT);
        put_opaque(&mut img, 10 + step / 2, 6 - step, HAFT_DARK);
    }
    img
}

/// A flint spear: the knife's point on a haft that runs the whole
/// diagonal, bound where the two meet.
///
/// Longer than any tool -- corner to corner -- because length is the
/// only thing that says "spear" against an axe and a knife drawn on the
/// same slant, and the point is small on purpose: it is the knife head,
/// and a big head on a long pole is a halberd.
fn generate_flint_spear() -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for step in 0..12 {
        let x = 1 + step;
        let y = 15 - step;
        put_opaque(&mut img, x, y, HAFT);
        put_opaque(&mut img, x + 1, y, HAFT_DARK);
    }
    // The lashing, two turns across the join, drawn before the point
    // so the stone covers whatever would stand proud of it.
    for step in 0..2 {
        put_opaque(&mut img, 10 + step, 5 - step, LASHING);
        put_opaque(&mut img, 11 + step, 5 - step, LASHING);
    }
    const POINT: &[Span] = &[(0, 15, 15), (1, 14, 15), (2, 13, 15), (3, 12, 14), (4, 12, 13)];
    knap(&mut img, POINT, true);
    img
}

/// A collar of set glue across a haft where the head is bound on.
///
/// Three texels wide and three long, lying across the haft rather than
/// along it, over whatever lashing was there: a glued joint is a
/// lashing that has been *buried*, and a band that left the lashing
/// showing would read as a tool with two bindings rather than one that
/// has been sealed. One lit texel at the top-left corner, because set
/// pitch is glossy and a dead black band is a gap in the tool.
fn glue_collar(img: &mut RgbaImage, x: i32, y: i32) {
    for step in 0..3 {
        for off in -1..=1 {
            put_opaque(img, x + step + off, y - step, GLUE);
        }
    }
    put_opaque(img, x - 1, y, GLUE_LIT);
}

/// The stone axe with its joint set in glue: the same axe, collared.
///
/// Drawn *from* the lashed axe rather than beside it, so the two can
/// never drift apart -- a player looks from one to the other in the
/// pack and the only difference they should find is the dark band,
/// because the only difference in the world is the glue.
///
/// **From the file in the folder, not from `generate_axe`.** The stone
/// axe in `assets/textures` is hand-drawn now -- a grey ground head,
/// which is what the recipe makes -- and the generated one is the old
/// knapped-flint stand-in. A glued axe collared onto the stand-in was a
/// black-headed axe beside a grey-headed one, which read as two
/// different tools, and the difference in the world is the glue and
/// nothing else. Falls back to the stand-in only when the drawing is
/// not there, so a fresh checkout still produces something; the same
/// bargain `generate_backpack_side_flat` makes.
fn generate_glued_axe(dir: &std::path::Path) -> RgbaImage {
    let mut img = match image::open(dir.join("tools/stone_axe.png")) {
        Ok(img) => image::imageops::resize(
            &img.to_rgba8(),
            RESOLUTION,
            RESOLUTION,
            image::imageops::FilterType::Nearest,
        ),
        Err(_) => generate_axe(),
    };
    glue_collar(&mut img, 7, 8);
    img
}

/// The stone pick with its joint set in glue, on the same terms.
fn generate_glued_pickaxe() -> RgbaImage {
    let mut img = generate_pickaxe(FLINT, FLINT_EDGE);
    glue_collar(&mut img, 9, 8);
    img
}

/// A rusty stone: one grey pebble with a bloom of rust across its
/// shoulder, and a plain small one beside it for scale.
///
/// The pebble's own shading (`generate_pebble`) so it lies on the
/// ground as a pebble does -- it *is* a pebble, with a stain -- and the
/// stain is what the player is looking for from three metres away, so
/// it is a patch and not a speckle: a dozen orange texels together read
/// as rust, a dozen scattered read as a dirty stone.
fn generate_rusty_stone() -> RgbaImage {
    const STONE: [u8; 3] = [138, 136, 132];
    const STONE_PALE: [u8; 3] = [170, 168, 162];
    const RUST: [u8; 3] = [168, 88, 40];
    const RUST_DARK: [u8; 3] = [124, 60, 28];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let stones: [(f32, f32, f32, [u8; 3]); 2] =
        [(7.0, 7.5, 4.2, STONE), (13.0, 12.5, 1.6, STONE_PALE)];
    for (cx, cy, radius, colour) in stones {
        let reach = radius.ceil() as i32 + 1;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (x, y) = (cx + dx as f32, cy + dy as f32);
                let (ox, oy) = (x - cx, y - cy);
                if ox * ox + oy * oy > radius * radius {
                    continue;
                }
                let lift = ((-ox - oy) * 6.0) as i32;
                let rim = if ox * ox + oy * oy > (radius - 0.9).max(0.0).powi(2) {
                    -22
                } else {
                    0
                };
                let noise = (hash(0x2057, x.max(0.0) as u32, y.max(0.0) as u32) % 11) as i32 - 5;
                let lift = lift + rim + noise;
                put_opaque(
                    &mut img,
                    x as i32,
                    y as i32,
                    [
                        clamp_u8(colour[0] as i32 + lift),
                        clamp_u8(colour[1] as i32 + lift),
                        clamp_u8(colour[2] as i32 + lift),
                    ],
                );
            }
        }
    }
    // The bloom: an off-centre patch on the big stone's upper right,
    // darker where it meets the rim, with a couple of specks beyond it
    // where the stain is spreading.
    const BLOOM: &[Span] = &[(4, 8, 9), (5, 7, 10), (6, 7, 11), (7, 8, 11), (8, 9, 10)];
    for &(y, from, to) in BLOOM {
        for x in from..=to {
            let noise = (hash(0x8057, x as u32, y as u32) % 7) as i32 - 3;
            let colour = if x == to || y == 8 { RUST_DARK } else { RUST };
            put_opaque(
                &mut img,
                x,
                y,
                [
                    clamp_u8(colour[0] as i32 + noise),
                    clamp_u8(colour[1] as i32 + noise),
                    clamp_u8(colour[2] as i32 + noise),
                ],
            );
        }
    }
    put_opaque(&mut img, 5, 9, RUST_DARK);
    put_opaque(&mut img, 9, 10, RUST);
    img
}

/// Iron dust: a reddish heap, coarse-grained, darker at its foot.
///
/// A mound rather than a scatter, for the reason the rust is a patch:
/// a heap says "gathered" and a scatter says "spilled", and this is the
/// thing the player has spent four stones making. The grain is the
/// strongest noise in the set short of gravel, because dust that is
/// smooth is paint.
fn generate_iron_dust() -> RgbaImage {
    const DUST: [u8; 3] = [146, 82, 52];
    const DUST_DARK: [u8; 3] = [96, 52, 34];
    const DUST_PALE: [u8; 3] = [188, 124, 88];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const HEAP: &[Span] = &[
        (6, 7, 8),
        (7, 6, 9),
        (8, 5, 10),
        (9, 4, 11),
        (10, 4, 12),
        (11, 3, 12),
        (12, 2, 13),
        (13, 2, 14),
        (14, 1, 14),
    ];
    for &(y, from, to) in HEAP {
        for x in from..=to {
            let grain = hash(0xD057, x as u32, y as u32) % 10;
            // The foot of the heap and its right flank are in shadow;
            // the crown and the left catch the light.
            let colour = if y >= 13 || x >= to - 1 {
                if grain < 2 {
                    DUST
                } else {
                    DUST_DARK
                }
            } else if grain == 0 {
                DUST_PALE
            } else if grain < 3 {
                DUST_DARK
            } else {
                DUST
            };
            let noise = (hash(0xD058, x as u32, y as u32) % 9) as i32 - 4;
            put_opaque(
                &mut img,
                x,
                y,
                [
                    clamp_u8(colour[0] as i32 + noise),
                    clamp_u8(colour[1] as i32 + noise),
                    clamp_u8(colour[2] as i32 + noise),
                ],
            );
        }
    }
    img
}

/// Food that has gone off: a greenish-grey lump with a bloom of mould
/// on it.
///
/// Deliberately *nothing in particular* -- not a green loaf, not a grey
/// steak. Everything perishable ends as this one block (see
/// `types::BLOCK_ROTTEN`), so the picture cannot say which food it was;
/// what it can say is "do not eat this", and the colour that says that
/// is the one no food is. The pale fuzz on the crown is what makes it
/// rot rather than a mossy stone.
fn generate_rotten() -> RgbaImage {
    const ROT: [u8; 3] = [104, 112, 70];
    const ROT_DARK: [u8; 3] = [62, 68, 40];
    const ROT_PALE: [u8; 3] = [150, 158, 116];
    const MOULD: [u8; 3] = [196, 204, 180];
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    const LUMP: &[Span] = &[
        (3, 6, 9),
        (4, 4, 11),
        (5, 3, 12),
        (6, 3, 13),
        (7, 2, 13),
        (8, 2, 13),
        (9, 3, 13),
        (10, 3, 12),
        (11, 4, 12),
        (12, 5, 10),
    ];
    for &(y, from, to) in LUMP {
        for x in from..=to {
            let speck = hash(0x2077, x as u32, y as u32) % 12;
            let rim = x == from || x == to || y == 12;
            let shadow = x > from + (to - from) * 3 / 4 || y >= 11;
            let colour = if (rim && shadow) || speck == 0 {
                ROT_DARK
            } else if speck == 1 && !shadow {
                ROT_PALE
            } else {
                ROT
            };
            put_opaque(&mut img, x, y, colour);
        }
    }
    // The mould, a soft pale patch on the crown and one spot lower down.
    for (x, y) in [(6, 4), (7, 4), (7, 3), (5, 5), (6, 5), (10, 8), (11, 8)] {
        put_opaque(&mut img, x, y, MOULD);
    }
    put_opaque(&mut img, 8, 4, ROT_PALE);
    put_opaque(&mut img, 10, 9, ROT_PALE);
    img
}

// ---- the palm's cut end ----
//
// The trunk's bark was drawn outside this file; its end had no picture at
// all, and a trunk broken through wore bark across the break.
const PALM_ENDS: &[Drawn] = &[("plants/palm_top.png", generate_palm_top)];

/// The cut end of a palm's trunk: fibre, and no rings.
///
/// **A palm has no growth rings, and a section drawn with them is an oak's.**
/// A palm never thickens by laying wood round its outside -- it is a bundle of
/// fibres from the day it comes up -- so its end is one soft ground shot
/// through with the dark ends of those fibres: sparse in the middle, packed
/// toward the rind. `log_top.png` and `generate_birch_log_top` are sections
/// through something that grew outwards; this is one through a sheaf.
///
/// **Drawn for the middle ten texels.** The trunk is ten sixteenths wide at
/// the root and eight under the crown (`worldgen::palm_cells`), and its ends
/// are cut from the picture at a texel to a sixteenth
/// (`mesh::palm_trunk_block`), so no end ever shows more than the middle ten.
/// The rind starts at the outermost row of the eight-wide cut and is the outer
/// two rows of the ten-wide one: a thin trunk has a thin rind and a thick one a
/// thicker, and neither shows a ring sliced through at its edge -- which is
/// what a rind drawn round the whole tile, as the log's is, would have been.
/// Past the ten is dark bark, for the icon.
///
/// A little brighter than the bark beside it, by about what the log's end is
/// brighter than the log's side: cut wood is paler than weathered bark, and a
/// pair that disagreed about that would read as two materials.
fn generate_palm_top() -> RgbaImage {
    const FIBRE: [u8; 3] = [140, 123, 94];
    const FIBRE_PALE: [u8; 3] = [152, 135, 104];
    const BUNDLE: [u8; 3] = [92, 77, 57];
    const RIND: [u8; 3] = [104, 91, 70];
    const RIND_DARK: [u8; 3] = [77, 66, 52];
    /// Where the rind starts, from the middle: past the centres of the second
    /// row in and short of the outermost row of an eight-wide cut.
    const RIND_FROM: f32 = 3.4;

    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    let centre = RESOLUTION as f32 / 2.0;
    // **Every fibre's end a dot on its own.** Thrown at the texels by chance
    // alone, neighbours joined into dark worms, and at the size a cut is seen
    // the end read as a maze rather than a sheaf. A dot is refused beside one
    // already drawn -- left, above, and both diagonals above, which is all of
    // its neighbours this row-by-row walk has visited.
    let mut dotted = [[false; RESOLUTION as usize]; RESOLUTION as usize];
    for y in 0..RESOLUTION {
        for x in 0..RESOLUTION {
            let dx = (x as f32 + 0.5 - centre).abs();
            let dy = (y as f32 + 0.5 - centre).abs();
            // A rounded square. The box the end is cut on is square and a
            // palm is round; a circle left the eight-wide cut's corners outside
            // its rind, and a square put a hard corner on a trunk.
            let r = (dx.powi(4) + dy.powi(4)).powf(0.25);
            let noise = hash(0x9A1A, x, y);
            let grain = (noise % 9) as i32 - 4;
            let (ux, uy) = (x as usize, y as usize);
            let beside_a_dot = (ux > 0 && dotted[uy][ux - 1])
                || (uy > 0 && (dotted[uy - 1][ux] || (ux > 0 && dotted[uy - 1][ux - 1]) || dotted[uy - 1].get(ux + 1).copied().unwrap_or(false)));
            let base = if r > 6.0 {
                RIND_DARK
            } else if r > RIND_FROM {
                if (noise >> 8).is_multiple_of(3) {
                    RIND_DARK
                } else {
                    RIND
                }
            } else {
                // One texel in eight a fibre's end at the middle, one in three
                // at the rind -- before the dots beside dots are refused.
                let chance = 12 + (r / RIND_FROM * 22.0) as u32;
                if !beside_a_dot && (noise >> 8) % 100 < chance {
                    dotted[uy][ux] = true;
                    BUNDLE
                } else if r < 1.5 {
                    FIBRE_PALE
                } else {
                    FIBRE
                }
            };
            put_opaque(
                &mut img,
                x as i32,
                y as i32,
                [
                    clamp_u8(base[0] as i32 + grain),
                    clamp_u8(base[1] as i32 + grain),
                    clamp_u8(base[2] as i32 + grain),
                ],
            );
        }
    }
    img
}

// ============================================================
// Two faces that were drawn once, outside this file, and tiled badly
// ============================================================
//
// **The bale of wool and the nest had no function here**, and both had
// the fault a picture nobody can regenerate keeps: it stays. The bale's
// curls ran off its right edge and did not come back on the left, so a
// wall of wool was a column of hooks with a seam at every block -- the
// step across the join 2.7 times any step inside the picture. The nest
// was courses of staggered rectangles, and on the bowl's walls, which
// wear it at a texel to a sixteenth, that is a brick wall two texels tall:
// a nest built by a mason.
// **The bale is not here any more, and that is the player's call.** It was
// redrawn to fix the seam described above, and the answer was "верни
// текстуру шерсти": the old picture is the one they want in their world,
// tiling fault and all. So the file is somebody's artwork again, the
// function that replaced it is gone, and this note is what stops the next
// person from "fixing" it a second time. Whoever revisits it: the fault is
// real (the curls run off the right edge and do not come back on the
// left), and the way to mend it without losing the drawing is to fix
// *that* picture rather than to draw a new one.
const REPAINTED: &[Drawn] = &[("plants/nest.png", generate_nest)];


/// The weave of a nest: twigs laid across each other at two angles.
///
/// **Diagonal, because what separates woven from built is that no line in
/// it is level.** A brick course is horizontal; a twig lies at whatever
/// angle the bird pushed it to. Two families of twig, one leaning each way,
/// every eight texels -- a period that divides the tile, so the bowl's
/// walls (`mesh::nest_block`) carry the weave round their corners without
/// a join. Where two cross, which one lies on top alternates from crossing
/// to crossing, and that is the detail that makes crossed sticks a weave
/// rather than a trellis.
///
/// The four browns the old picture had, dark gaps included and in about
/// the same share, so a nest is no brighter a thing in a canopy than it was.
fn generate_nest() -> RgbaImage {
    const GAP: [u8; 3] = [46, 34, 22];
    const UNDER: [u8; 3] = [84, 62, 38];
    const TWIG: [u8; 3] = [116, 88, 54];
    const LIT: [u8; 3] = [146, 114, 72];
    // Across a twig: its lit edge, its body, and the shadow it throws.
    let across = |band: i32| match band {
        0 => Some(LIT),
        1 => Some(TWIG),
        2 => Some(UNDER),
        _ => None,
    };
    let mut img = RgbaImage::new(RESOLUTION, RESOLUTION);
    for y in 0..RESOLUTION as i32 {
        for x in 0..RESOLUTION as i32 {
            let leaning = across((x + y).rem_euclid(8));
            let rising = across((x - y + 4).rem_euclid(8));
            let leaning_on_top = ((x + y).div_euclid(8) + (x - y).div_euclid(8)).rem_euclid(2) == 0;
            let colour = match (leaning, rising) {
                (Some(a), Some(b)) => {
                    if leaning_on_top {
                        a
                    } else {
                        b
                    }
                }
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => GAP,
            };
            let grain = (hash(0x0E57, x as u32, y as u32) % 7) as i32 - 3;
            put_opaque(
                &mut img,
                x,
                y,
                [
                    clamp_u8(colour[0] as i32 + grain),
                    clamp_u8(colour[1] as i32 + grain),
                    clamp_u8(colour[2] as i32 + grain),
                ],
            );
        }
    }
    img
}

// ---- what a death leaves ----
//
// A dead player's body, and the bones it rots into two days later. They
// replaced the backpack, which was a picture of luggage standing in for
// a person -- see `types::BLOCK_CORPSE`.
//
// **Both are drawn as what you see looking down**, because that is how a
// player meets one: you walk up to it and you are standing over it. The
// side faces are eight rows, like the bag's (`HALF_TILE`), and they are
// the same figure in profile so the block does not change species when
// you crouch.
//
// The palette is the player's own (`P_SKIN`, `P_TUNIC`, `P_TROUSER`,
// `P_BOOT` and the rest, shared with `generate_player_skin`), and that is
// the whole reason the block reads as *a person* rather than as another
// brown lump: the colours are the ones walking around in the world, in
// the arrangement a person makes lying down.
const CORPSE_PICTURES: &[Drawn] = &[
    ("terrain/corpse_top.png", generate_corpse_top),
    ("terrain/corpse_side.png", generate_corpse_side),
    ("terrain/remains_top.png", generate_remains_top),
    ("terrain/remains_side.png", generate_remains_side),
];

/// The ground a body lies on and the shadow it casts, which is every
/// texel of these four that is not the body itself.
///
/// A dark earth rather than transparency: the block is an ordinary cube
/// (a short one), so it is drawn opaque and an alpha texel would be
/// whatever happened to be in the buffer. Dark, because the hollow under
/// a fallen thing is dark and because it puts the body's own colours --
/// skin, tan, bone -- at the top of the contrast range, which is what
/// makes it findable on grass at forty blocks.
const GRAVE_EARTH: [u8; 3] = [48, 40, 32];
const GRAVE_EARTH_DARK: [u8; 3] = [34, 28, 22];

/// Paints one 16-wide picture from rows of letters.
///
/// The same trick `generate_campfire_side` uses, and for the same
/// reason: a figure is a *shape*, and a shape is far easier to read,
/// argue about and correct as a picture in the source than as thirty
/// lines of arithmetic about where an arm starts.
fn paint_rows(rows: &[&str], colour_of: impl Fn(u8) -> [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(RESOLUTION, rows.len() as u32);
    for (y, row) in rows.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            let base = colour_of(cell);
            // A little grain, inside the shapes rather than over them,
            // so a tunic is cloth and not a flat swatch.
            let grain = (hash(0xC0B5E, x as u32, y as u32) % 11) as i32 - 5;
            put_pixel_in(
                &mut img,
                x as i32,
                y as i32,
                [
                    clamp_u8(base[0] as i32 + grain),
                    clamp_u8(base[1] as i32 + grain),
                    clamp_u8(base[2] as i32 + grain),
                ],
            );
        }
    }
    img
}

/// What each letter in the two body pictures means. Shared by the top
/// and the side so that the figure cannot drift between the two faces.
fn corpse_colour(cell: u8) -> [u8; 3] {
    match cell {
        b'h' => P_HAIR,
        b's' => P_SKIN,
        b'S' => P_SKIN_DARK,
        b't' => P_TUNIC,
        b'T' => P_TUNIC_DARK,
        b'b' => P_BELT,
        b'p' => P_TROUSER,
        b'P' => P_TROUSER_DARK,
        b'B' => P_BOOT,
        b'o' => P_SOLE,
        b'l' => P_LACE,
        b'd' => GRAVE_EARTH_DARK,
        _ => GRAVE_EARTH,
    }
}

/// A body seen from above: face up, arms at its sides, head at the top
/// of the tile.
///
/// **The head is what makes it a person.** At sixteen texels a torso is
/// a rectangle and a leg is a bar; the one shape the eye reads instantly
/// is a head with hair round it, so it gets four rows of the sixteen --
/// a quarter of the picture for a fifth of a person, which is out of
/// proportion and correct.
///
/// It lies along the tile rather than across it so that a row of them --
/// a shaft where somebody died three times -- reads as three bodies
/// pointing the same way rather than as a pile.
fn generate_corpse_top() -> RgbaImage {
    const ROWS: [&str; 16] = [
        "................",
        "......hhhh......",
        ".....hssssh.....",
        ".....hssssh.....",
        "......hSSh......",
        "...ssttttttss...",
        "...SsttllttsS...",
        "...SsttllttsS...",
        "...SsttllttsS...",
        "...SsTTTTTTsS...",
        "...SsbbbbbbsS...",
        "....SppppppS....",
        ".....pppPppp....",
        ".....ppp.ppp....",
        "....BBBB.BBBB...",
        "....oooo.oooo...",
    ];
    paint_rows(&ROWS, corpse_colour)
}

/// The same body in profile, eight rows tall -- the height half a cell
/// is drawn at (`HALF_TILE`).
///
/// Head to the left, boots to the right, and the bottom rows are the
/// ground it is lying on: without them the figure floats, which at this
/// size reads as a sticker rather than as something fallen.
fn generate_corpse_side() -> RgbaImage {
    const ROWS: [&str; 8] = [
        "................",
        "..hhh...........",
        ".hsssttttt......",
        ".hsssTTTTTbppp..",
        "..SSSTTTTTbpppBB",
        "...SSTTTTTbpPPBB",
        "..dddddddddddoo.",
        "dddddddddddddddd",
    ];
    paint_rows(&ROWS, corpse_colour)
}

/// Bone, rag and earth: the letters of the two pictures of what is left.
fn remains_colour(cell: u8) -> [u8; 3] {
    // The skeletons in this game wear the boar's ivory (`BOAR_TUSK`), and
    // these are the same bones seen at the same size -- a second, whiter
    // white would say "a different material" about the same thing.
    const BONE: [u8; 3] = [226, 220, 198];
    const BONE_DARK: [u8; 3] = [170, 162, 138];
    // What is left of the clothes: the tunic's brown, greyed and dulled
    // most of the way to the earth. It is there so the bones are not
    // lying on nothing -- a rag under a skeleton is the difference
    // between "somebody died here" and "somebody left a skull here".
    const RAG: [u8; 3] = [92, 78, 58];
    match cell {
        b'B' => BONE,
        b'b' => BONE_DARK,
        b'r' => RAG,
        b'd' => GRAVE_EARTH_DARK,
        _ => GRAVE_EARTH,
    }
}

/// The skeleton from above: skull, ribs, spine, pelvis and the long
/// bones, in the arrangement the body was in.
///
/// Deliberately the *same layout* as `generate_corpse_top` -- head at the
/// top, feet at the bottom, the same width -- so that a player who comes
/// back late sees the thing they left, changed, rather than a new object
/// in its place. The ribs are the shape that says skeleton at this size,
/// so they get the middle of the tile and a gap between each pair.
fn generate_remains_top() -> RgbaImage {
    const ROWS: [&str; 16] = [
        "................",
        "....BBBBBBBB....",
        "...BBBBBBBBBB...",
        "...BBddBBddBB...",
        "...BBBBddBBBB...",
        "......BBBB......",
        "..bb.BBBBBB.bb..",
        "..bb.r.BB.r.bb..",
        "..bb.BBBBBB.bb..",
        "..bb.r.BB.r.bb..",
        "..bb.BBBBBB.bb..",
        "..bb.r.BB.r.bb..",
        "...rBBBBBBBBr...",
        "....BBB..BBB....",
        ".....BB..BB.....",
        "....rBB..BBr....",
    ];
    paint_rows(&ROWS, remains_colour)
}

/// ...and in profile: a low heap of bone with the skull at the head of
/// it, eight rows like the body it replaced.
fn generate_remains_side() -> RgbaImage {
    const ROWS: [&str; 8] = [
        "................",
        "................",
        "..BBB...........",
        ".BBBBBbBbBb.....",
        ".BBdBBBBBBBBrr..",
        "..BBBbBbBbBBrrBb",
        "..dddddddddddddd",
        "dddddddddddddddd",
    ];
    paint_rows(&ROWS, remains_colour)
}

// ---- the fir and the saxaul ----

/// Each wood's pictures: what is written, the oak picture it is repainted
/// from, the mean colour the repaint lands on, and whether the crown is
/// thinned. See `repaint_wood`.
const WOOD_REPAINTS: &[(&str, &str, [u8; 3], bool)] = &[
    // Fir: dark bark with a red cast, a pale resinous section, the palest
    // boards there are, and a crown the blue-black green of a spruce wood.
    ("terrain/fir_log_side.png", "terrain/log_side.png", [86, 62, 50], false),
    ("terrain/fir_log_top.png", "terrain/log_top.png", [168, 138, 92], false),
    ("terrain/fir_planks.png", "terrain/planks.png", [124, 100, 66], false),
    ("terrain/pegged_fir_planks.png", "terrain/pegged_planks.png", [123, 99, 65], false),
    ("plants/fir_needles.png", "plants/leaves.png", [42, 70, 56], false),
    // Saxaul: ashy grey bark, a dark dense section, grey-brown boards, and
    // a thin grey-green crown with the sky through it.
    ("terrain/saxaul_log_side.png", "terrain/log_side.png", [122, 112, 98], false),
    ("terrain/saxaul_log_top.png", "terrain/log_top.png", [112, 84, 62], false),
    ("terrain/saxaul_planks.png", "terrain/planks.png", [84, 72, 62], false),
    ("terrain/pegged_saxaul_planks.png", "terrain/pegged_planks.png", [83, 71, 61], false),
    ("plants/saxaul_leaves.png", "plants/leaves.png", [112, 132, 92], true),
];

/// One of the oak's pictures, in another wood's colour.
///
/// **A repaint, not a new drawing**: the pack's rule, and the reason is the
/// oak's pictures are drawn by hand -- the grain, the knots, the rings and
/// the sprays are the drawing, and a wood redrawn from noise stands in a
/// house beside the oak looking like a different game. So every texel keeps
/// its lightness *relative to the picture's own mean*, and only the colour
/// the mean lands on changes: a dark knot in oak is a dark knot in fir.
///
/// Rejected: *a hue rotation*. It keeps saturation, and the saxaul's point
/// is that it is grey; oak's browns turned by a hue are a green or a purple
/// wood, never an ashy one.
///
/// `sparse` thins a crown by a third, by a hash of the texel -- a saxaul's
/// twigs have the sky through them where an oak's leaves do not. Transparent
/// texels stay transparent either way, so the cut-out is the oak's.
fn repaint_wood(dir: &std::path::Path, source: &str, colour: [u8; 3], sparse: bool) -> RgbaImage {
    let img = match image::open(dir.join(source)) {
        Ok(picture) => image::imageops::resize(
            &picture.to_rgba8(),
            RESOLUTION,
            RESOLUTION,
            image::imageops::FilterType::Nearest,
        ),
        // No reference on disk: a flat field in the colour, which is a
        // placeholder and says so by being one.
        Err(_) => generate(colour, 12, source),
    };
    let lightness = |p: &Rgba<u8>| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
    let opaque: Vec<f32> = img.pixels().filter(|p| p[3] > 0).map(lightness).collect();
    let mean = (opaque.iter().sum::<f32>() / opaque.len().max(1) as f32).max(1.0);
    let seed = source.bytes().fold(0x5A7A_u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let mut out = RgbaImage::new(RESOLUTION, RESOLUTION);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] == 0 || (sparse && hash(seed, x, y).is_multiple_of(3)) {
            out.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            continue;
        }
        let k = lightness(p) / mean;
        out.put_pixel(
            x,
            y,
            Rgba([
                clamp_u8((colour[0] as f32 * k).round() as i32),
                clamp_u8((colour[1] as f32 * k).round() as i32),
                clamp_u8((colour[2] as f32 * k).round() as i32),
                p[3],
            ]),
        );
    }
    out
}
