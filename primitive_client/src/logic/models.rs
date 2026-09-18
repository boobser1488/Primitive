//! The models, read from `assets/models` -- Blockbench projects.
//!
//! ## Why files and not tables
//!
//! An animal was a `const` table of boxes in `animal_model`, a bed a
//! table in `mesh`, and that was the right answer for as long as the only
//! people changing a model were reading Rust. A player asked to change
//! them in the tool people change boxes in, and a `.bbmodel` *is* a table
//! of boxes: a cube with a from, a to and a pivot, in sixteenths of a
//! block, which is the unit the tables were already written in.
//!
//! So `assets/models/animals/boar.bbmodel` is the boar, and there is **no
//! second copy in the code**. The tables were written out once by the
//! exporter (the boxes, the pictures and the gait of every one of them),
//! the game was drawn from both and compared vertex for vertex, and then
//! the tables were deleted.
//!
//! Rejected: keeping the tables as a fallback for a missing or broken
//! file. The file is compiled into the binary (`embedded::MODELS`), so it
//! cannot be missing; and a fallback that is right on the day it is
//! written and wrong the first time somebody moves an ear in Blockbench
//! is the silent second copy `obj_export` was written to avoid.
//!
//! ## The folder still wins
//!
//! As with textures, a file under `<assets>/models` beside the game is
//! preferred over the built-in one, so an edit shows up on the next start
//! without a rebuild. **A file that does not read is a warning, not a
//! crash**: the built-in model is drawn and the message names the file,
//! the line and what was wrong -- a half-finished edit should not cost a
//! player their world.
//!
//! ## Read once, and the hot path does not know
//!
//! `load` parses every file at start-up into exactly the structures the
//! tables were -- `animal_model::Part` and `mesh::PropBox` -- and leaks
//! them into a `OnceLock`. The mesher asks `animal` or `prop` and gets a
//! `&'static` slice, the same type a `const` gave it; what it pays is one
//! already-initialised `OnceLock` read per model, not per box. Anything
//! that asks before `load` (a test, `--export-models`) gets the built-in
//! files.
//!
//! ## What Blockbench does not know, and where it is written instead
//!
//! Blockbench knows boxes, pivots, groups and textures. The game also
//! needs which *picture* a face wears, how a part moves, and which boxes
//! show only when a block is loaded. All of it is carried by things
//! Blockbench already keeps -- a texture's `name`, a face's `uv`, a group's
//! `name` and `origin` -- and **no custom field**: an unknown field is a
//! promise about another program's save code, and a name is not.
//!
//! **An animal** (`animals/*.bbmodel`):
//!
//! * Every cube is inside one outer group, the *root*, whose `origin` is
//!   the animal's centre -- the point the server sends. The files stand
//!   the animal on y = 0, the way a model sits on Blockbench's grid, and
//!   the root's origin says how far up its middle is.
//! * **The picture a face wears is the square of the sheet its `uv`
//!   starts in** (four across and three down, `Skin::slot`): hide, head,
//!   head mirrored, face, snout, nose, ear, hoof, fur, tusk, antler, horn.
//!   `up`, `down` and `south` must agree -- that is the part's own skin;
//!   `north` is its front and `east`/`west` its two sides.
//! * **How a part moves is the first word of the innermost group it is
//!   in**: `still`, `head` (nods), `leg_front` and `leg_back` (swing in
//!   opposite phase), `folded` (a wing shown on the ground) and `wing` (a
//!   wing shown in the air, rolled about a hinge at the group's `origin`
//!   x). A part in no such group is `still`.
//! * **A cube's own `origin` is its joint**: where it swings from. Left on
//!   the middle of its top, it swings about its top.
//! * A cube may not be rotated: a living part turns only as it walks.
//!
//! **A piece of furniture or anything else of boxes**
//! (`furniture/*.bbmodel`, `misc/*.bbmodel`): from and to in sixteenths of
//! the cell, `0..16`, facing the way its placer looks from; every face of
//! a cube wears one texture, and the texture's `name` is the material:
//! `boards`, `pole`, `straw`, `wool`, `hide`, `stretched_hide`,
//! `stretched_leather`, `leather`, `timber`, `post`, `iron`, `fur`, `stone`,
//! `clay`, `chest`. A cube inside a group named `loaded` is drawn only when
//! the block carries its load (the rack's skin), and one inside a group
//! named `bare` only when it does not (a hide frame's loose cords).
//!
//! `assets/models/README.md` says the same for a player, shorter.
//!
//! ## What stayed in code, and why
//!
//! * **The player** (`player_model::PARTS`). Its six boxes are not only a
//!   drawing: their sizes are the layout of the skin sheet (`net` cuts the
//!   picture by them), `gen_placeholder_textures.rs` paints into the same
//!   net with its own copy of that arithmetic, a dead player's limbs are a
//!   second table (`player_model::limb`), and the figure's height is
//!   `PLAYER_HEIGHT`, which the collider and the anti-cheat read. A file
//!   that moved a head would move one of four things that have to move
//!   together, and the drawing would stop matching the picture on it.
//! * **Skeletons** (`animal_model::skeleton_parts`) are computed from the
//!   living model, so an edited file already changes them.
//! * **Models that are arithmetic on the block**: a barrel's contents, a
//!   door's swing, a jug's handle, a kiln's load, pottery by kind, a
//!   palm's fronds, a nest's eggs. Each is a function of the block's state
//!   or its neighbours, not a fixed set of boxes, and a file of the boxes
//!   one state happens to have would be a file that is wrong in all the
//!   others.
//! * **Held and dropped items** (`item_model`) are extruded from their
//!   16x16 picture: the picture *is* the model.

use std::path::Path;
use std::sync::OnceLock;

use primitive_shared::animals::Species;

use crate::engine::mesh::{Material, PropBox};
use crate::engine::texture::{SHEET_COLUMNS, SHEET_ROWS};
use crate::logic::animal_model::{Gait, Part, Skin, PART};
use crate::logic::bbmodel::{self, Document, Group, Json};
use crate::logic::obj_export::{sheet_of, skin_of};

/// A model of boxes that is not an animal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prop {
    BedHead,
    BedFoot,
    StrawBedHead,
    StrawBedFoot,
    Stool,
    Chair,
    Table,
    DryingRack,
    /// The rack of four cells: two along its ridge, two high.
    DryingRack2x2,
    Chest,
    Workbench,
    MasonBlock,
    PottersWheel,
    LeatherBench,
    /// The anvil: a bronze block on an oak stump. See `types::BLOCK_ANVIL`.
    Anvil,
    /// A bundle of sharpened poles stood in the ground, pointing out and up
    /// (`types::BLOCK_STAKE` with `STAKE_UPRIGHT`).
    Stake,
    /// Sharpened poles driven into the wall to the north, points out.
    StakeWall,
    /// A skin laced into a standing frame of poles (`types::BLOCK_HIDE_FRAME`).
    HideFrame,
}

impl Prop {
    pub(crate) const ALL: [Prop; 18] = [
        Prop::BedHead,
        Prop::BedFoot,
        Prop::StrawBedHead,
        Prop::StrawBedFoot,
        Prop::Stool,
        Prop::Chair,
        Prop::Table,
        Prop::DryingRack,
        Prop::DryingRack2x2,
        Prop::Chest,
        Prop::Workbench,
        Prop::MasonBlock,
        Prop::PottersWheel,
        Prop::LeatherBench,
        Prop::Anvil,
        Prop::Stake,
        Prop::StakeWall,
        Prop::HideFrame,
    ];

    /// Its file under `assets/models`.
    pub(crate) fn file(self) -> &'static str {
        match self {
            Prop::BedHead => "furniture/bed_head.bbmodel",
            Prop::BedFoot => "furniture/bed_foot.bbmodel",
            Prop::StrawBedHead => "furniture/straw_bed_head.bbmodel",
            Prop::StrawBedFoot => "furniture/straw_bed_foot.bbmodel",
            Prop::Stool => "furniture/stool.bbmodel",
            Prop::Chair => "furniture/chair.bbmodel",
            Prop::Table => "furniture/table.bbmodel",
            Prop::DryingRack => "misc/drying_rack.bbmodel",
            Prop::DryingRack2x2 => "misc/drying_rack_2x2.bbmodel",
            Prop::Chest => "furniture/chest.bbmodel",
            Prop::Workbench => "workstations/workbench.bbmodel",
            Prop::MasonBlock => "workstations/mason_block.bbmodel",
            Prop::PottersWheel => "workstations/potters_wheel.bbmodel",
            Prop::LeatherBench => "workstations/leather_bench.bbmodel",
            Prop::Anvil => "workstations/anvil.bbmodel",
            Prop::Stake => "misc/stake.bbmodel",
            Prop::StakeWall => "misc/stake_wall.bbmodel",
            Prop::HideFrame => "misc/hide_frame.bbmodel",
        }
    }
}

/// An animal's file under `assets/models`.
pub(crate) fn animal_file(species: Species) -> String {
    format!("animals/{}.bbmodel", species.name())
}

/// Every model, read.
pub(crate) struct Library {
    /// By `species as usize`.
    animals: Vec<&'static [Part]>,
    /// By `prop as usize`.
    props: Vec<&'static [PropBox]>,
}

static LIBRARY: OnceLock<Library> = OnceLock::new();

/// Reads every model, preferring `<assets_dir>/models` to the built-in
/// copies. Called once, at start-up, before anything is drawn.
pub fn load(assets_dir: &Path) {
    let library = Library::read(Some(&assets_dir.join("models")));
    if LIBRARY.set(library).is_err() {
        eprintln!("warning: the models were drawn before they were loaded; the built-in ones stay");
    }
}

fn library() -> &'static Library {
    LIBRARY.get_or_init(|| Library::read(None))
}

/// The model of a species.
#[inline]
pub fn animal(species: Species) -> &'static [Part] {
    library().animals[species as usize]
}

/// The boxes of a prop.
#[inline]
pub(crate) fn prop(prop: Prop) -> &'static [PropBox] {
    library().props[prop as usize]
}

impl Library {
    /// Every model from `disk` where it has a file that reads, else the
    /// built-in one.
    pub(crate) fn read(disk: Option<&Path>) -> Library {
        let slots = Species::ALL.iter().map(|&s| s as usize).max().unwrap_or(0) + 1;
        let mut animals: Vec<&'static [Part]> = vec![&[]; slots];
        for &species in Species::ALL {
            let parts = read_one(disk, &animal_file(species), animal_parts);
            animals[species as usize] = Box::leak(parts.into_boxed_slice());
        }
        let props = Prop::ALL
            .iter()
            .map(|p| &*Box::leak(read_one(disk, p.file(), prop_boxes).into_boxed_slice()))
            .collect();
        Library { animals, props }
    }

    #[cfg(test)]
    pub(crate) fn animal(&self, species: Species) -> &'static [Part] {
        self.animals[species as usize]
    }

    #[cfg(test)]
    pub(crate) fn prop(&self, prop: Prop) -> &'static [PropBox] {
        self.props[prop as usize]
    }
}

fn read_one<T>(disk: Option<&Path>, file: &str, convert: fn(&Document) -> Result<T, String>) -> T {
    if let Some(path) = disk.map(|dir| dir.join(file)).filter(|path| path.is_file()) {
        let read = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| Document::parse(&text))
            .and_then(|doc| convert(&doc));
        match read {
            Ok(model) => return model,
            Err(e) => eprintln!("warning: {}: {e}; drawing the built-in model instead", path.display()),
        }
    }
    let text = crate::embedded::model(file).unwrap_or_else(|| panic!("{file} is not in embedded::MODELS"));
    Document::parse(text)
        .and_then(|doc| convert(&doc))
        .unwrap_or_else(|e| panic!("the built-in {file} does not read: {e}"))
}

/// A name from a file, for a field that is `&'static str` because the
/// tables it replaces were. Read once per start, so what leaks is a few
/// kilobytes, once.
fn leak(name: &str) -> &'static str {
    Box::leak(name.to_string().into_boxed_str())
}

/// An `f32` as the decimal a person wrote it as, widened.
///
/// `-0.3f32 as f64` is `-0.30000001192092896`, and a file of numbers like
/// that is unreadable. The shortest decimal that reads back as the same
/// `f32` is what was typed into the table, and reading it back narrows to
/// the same bits.
fn decimal(x: f32) -> f64 {
    format!("{x}").parse().unwrap_or(x as f64)
}

/// A number read from a file, narrowed to the `f32` a table held.
///
/// **Rounded to a millionth first**, as `bbmodel` writes it: a centre is
/// computed as the middle of a from and a to less the root's lift, and
/// a fish's head at y = 0 came back as -1.1e-16 -- nothing on screen, and a
/// different vertex to the bit from the table it was written from.
fn narrow(x: f64) -> f32 {
    // `+ 0.0` turns the negative zero that rounding -1.1e-16 gives into the
    // zero a table wrote: the same place, and not the same bits.
    ((x * 1e6).round() / 1e6 + 0.0) as f32
}

/// How many pixels one square of an animal's sheet is, in the shipped
/// pictures and so in the files' `uv`s.
const TILE: u32 = 16;

/// Where a skin's square is on the sheet, in `uv` units: left, top, right,
/// bottom.
fn tile_uv(skin: Skin) -> [f64; 4] {
    let slot = skin.slot() as u32;
    let (column, row) = (slot % SHEET_COLUMNS, slot / SHEET_COLUMNS);
    [column * TILE, row * TILE, (column + 1) * TILE, (row + 1) * TILE].map(|n| n as f64)
}

/// The group a gait is written as: its name and the x of its origin.
fn gait_group(gait: Gait) -> (&'static str, f64) {
    match gait {
        Gait::Still => ("still", 0.0),
        Gait::LegFront => ("leg_front", 0.0),
        Gait::LegBack => ("leg_back", 0.0),
        Gait::Head => ("head", 0.0),
        Gait::Folded => ("folded", 0.0),
        Gait::Wing(hinge) if hinge < 0 => ("wing left", hinge as f64),
        Gait::Wing(hinge) => ("wing right", hinge as f64),
    }
}

/// The gait a group means, if its name starts with one.
fn gait_named(group: &Group) -> Result<Option<Gait>, String> {
    Ok(Some(match group.name.split_whitespace().next().unwrap_or("") {
        "still" => Gait::Still,
        "head" => Gait::Head,
        "leg_front" => Gait::LegFront,
        "leg_back" => Gait::LegBack,
        "folded" => Gait::Folded,
        "wing" => {
            let x = group.origin[0];
            let hinge = x.round();
            if (x - hinge).abs() > 1e-6 || !(-128.0..=127.0).contains(&hinge) {
                return Err(format!(
                    "the group \"{}\" hinges its wing at x = {x}, and a hinge is a whole number of sixteenths",
                    group.name
                ));
            }
            Gait::Wing(hinge as i8)
        }
        _ => return Ok(None),
    }))
}

/// An animal as a Blockbench project.
///
/// `source` carries the sheet inside the file (`--export-models`); without
/// it the texture points at the sheet in `assets/textures`, which is what
/// the shipped files do.
pub(crate) fn animal_project(species: Species, parts: &[Part], source: Option<String>) -> Json {
    let seed = species as u32 + 1;
    // Stood on the grid, the way the `.obj` is: the file's floor is y = 0
    // and the table's origin was the animal's middle.
    let lift = -parts.iter().map(|p| decimal(p.at[1]) - decimal(p.size[1]) / 2.0).fold(f64::INFINITY, f64::min);
    let lift = (lift * 1e6).round() / 1e6;

    let mut elements = Vec::new();
    let mut groups: Vec<(Gait, Vec<Json>)> = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let uuid = bbmodel::uuid(seed, index);
        let at = part.at.map(decimal);
        let size = part.size.map(decimal);
        let from = [at[0] - size[0] / 2.0, at[1] - size[1] / 2.0 + lift, at[2] - size[2] / 2.0];
        let to = [from[0] + size[0], from[1] + size[1], from[2] + size[2]];
        // The joint: the top of the box unless the table named one -- the
        // point the game swings the part about (`Part::pivot`), so a person
        // animating it in Blockbench turns it where the game does.
        let origin = match part.pivot {
            None => [at[0], at[1] + size[1] / 2.0 + lift, at[2]],
            Some([y, z]) => [at[0], decimal(y) + lift, decimal(z)],
        };
        let faces = std::array::from_fn(|face| (tile_uv(skin_of(part, face)), 0));
        elements.push(bbmodel::element(part.name, &uuid, from, to, origin, faces));
        match groups.iter_mut().find(|(gait, _)| *gait == part.gait) {
            Some((_, children)) => children.push(bbmodel::text(&uuid)),
            None => groups.push((part.gait, vec![bbmodel::text(&uuid)])),
        }
    }
    let children = groups
        .into_iter()
        .enumerate()
        .map(|(index, (gait, children))| {
            let (name, x) = gait_group(gait);
            bbmodel::group(name, &bbmodel::uuid(seed, 0x1000 + index), [x, lift, 0.0], children)
        })
        .collect();
    let root = bbmodel::group(species.name(), &bbmodel::uuid(seed, 0xF0), [0.0, lift, 0.0], children);

    let sheet = sheet_of(species);
    let leaf = Path::new(sheet).file_name().and_then(|s| s.to_str()).unwrap_or("hide.png");
    let relative = format!("../../textures/{sheet}");
    let relative = source.is_none().then_some(relative.as_str());
    let texture = bbmodel::texture(leaf, &bbmodel::uuid(seed, 0xF1), relative, source);
    bbmodel::project(species.name(), [SHEET_COLUMNS * TILE, SHEET_ROWS * TILE], elements, vec![root], vec![texture])
}

/// An animal from its project. See the module notes for what each thing
/// in the file means.
pub(crate) fn animal_parts(doc: &Document) -> Result<Vec<Part>, String> {
    let tile = [doc.resolution[0] / SHEET_COLUMNS as f64, doc.resolution[1] / SHEET_ROWS as f64];
    let mut parts = Vec::with_capacity(doc.elements.len());
    for element in &doc.elements {
        let name = &element.name;
        let root = match element.groups.first() {
            Some(root) if gait_named(root)?.is_none() => root,
            _ => {
                return Err(format!(
                    "\"{name}\" is not inside the animal's own group, whose origin is the animal's centre"
                ))
            }
        };
        if element.rotation.iter().any(|&r| r != 0.0) {
            return Err(format!("\"{name}\" is rotated, and a living part only turns as it walks"));
        }
        let lift = root.origin[1];
        let size: [f64; 3] = std::array::from_fn(|a| element.to[a] - element.from[a]);
        if size.iter().any(|&s| s < 0.0) {
            return Err(format!("\"{name}\" has a \"to\" below its \"from\""));
        }
        let at = [
            (element.from[0] + element.to[0]) / 2.0,
            (element.from[1] + element.to[1]) / 2.0 - lift,
            (element.from[2] + element.to[2]) / 2.0,
        ];

        let mut skins = [Skin::Hide; 6];
        for (face, skin) in skins.iter_mut().enumerate() {
            let Some(drawn) = &element.faces[face] else {
                return Err(format!("\"{name}\" has no {} face", bbmodel::FACE_NAMES[face]));
            };
            let [x1, y1, x2, y2] = drawn.uv;
            // The square the face starts in. A hair in, so a uv written as
            // 15.9999 is not taken for the square to its left.
            let column = ((x1.min(x2) + 1e-3) / tile[0]).floor();
            let row = ((y1.min(y2) + 1e-3) / tile[1]).floor();
            let slot = row * SHEET_COLUMNS as f64 + column;
            *skin = usize::try_from(slot as i64)
                .ok()
                .filter(|_| column >= 0.0 && column < SHEET_COLUMNS as f64)
                .and_then(|slot| Skin::ALL.get(slot).copied())
                .ok_or_else(|| {
                    format!("the {} face of \"{name}\" is off the sheet", bbmodel::FACE_NAMES[face])
                })?;
        }
        let skin = skins[0];
        if skins[1] != skin || skins[4] != skin {
            return Err(format!(
                "\"{name}\" wears {skin:?} on top but {:?} underneath and {:?} behind; only its north (front), \
                 east and west faces may wear something else",
                skins[1], skins[4]
            ));
        }
        let front = (skins[5] != skin).then_some(skins[5]);
        let sides = (skins[2] != skin || skins[3] != skin).then_some((skins[2], skins[3]));

        let mut gait = Gait::Still;
        for group in element.groups.iter().rev() {
            if let Some(named) = gait_named(group)? {
                gait = named;
                break;
            }
        }

        let top = at[1] + size[1] / 2.0;
        let joint = [element.origin[1] - lift, element.origin[2]];
        let pivot = if (joint[0] - top).abs() < 1e-6 && (joint[1] - at[2]).abs() < 1e-6 {
            None
        } else {
            Some(joint.map(narrow))
        };

        parts.push(Part {
            name: leak(name),
            at: at.map(narrow),
            size: size.map(narrow),
            skin,
            front,
            sides,
            gait,
            pivot,
            ..PART
        });
    }
    if parts.is_empty() {
        return Err("there are no boxes in it".into());
    }
    Ok(parts)
}

/// Where a face of a prop's box sits on a sixteen-texel picture: the piece
/// under it, which is what the game draws on a material (`push_box`). For
/// Blockbench's preview only -- the game does not read it back.
#[cfg_attr(not(test), allow(dead_code))]
fn prop_uv(from: [f64; 3], to: [f64; 3], face: usize) -> [f64; 4] {
    match face {
        0 | 1 => [from[0], from[2], to[0], to[2]],
        2 | 3 => [from[2], 16.0 - to[1], to[2], 16.0 - from[1]],
        _ => [from[0], 16.0 - to[1], to[0], 16.0 - from[1]],
    }
}

/// A prop as a Blockbench project.
///
/// Nothing in the game writes one -- the shipped files came from the one-off
/// export, and a player edits them in Blockbench -- but it is the inverse
/// the reader is held to (`a_model_read_back_from_its_file_is_the_model_that_was_written`),
/// and the way to write a new prop out of boxes built in code.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn prop_project(prop: Prop, boxes: &[PropBox]) -> Json {
    let seed = 0x100 + prop as u32;
    let mut materials: Vec<Material> = Vec::new();
    for piece in boxes {
        if !materials.contains(&piece.material) {
            materials.push(piece.material);
        }
    }
    let mut elements = Vec::new();
    let mut outliner = Vec::new();
    let mut loaded = Vec::new();
    let mut bare = Vec::new();
    let mut inside = Vec::new();
    for (index, piece) in boxes.iter().enumerate() {
        let uuid = bbmodel::uuid(seed, index);
        let from = piece.from.map(decimal);
        let to = piece.to.map(decimal);
        let texture = materials.iter().position(|&m| m == piece.material).unwrap_or(0);
        let faces = std::array::from_fn(|face| (prop_uv(from, to, face), texture));
        let centre = std::array::from_fn(|a| (from[a] + to[a]) / 2.0);
        let element = match piece.tilt {
            None => bbmodel::element(piece.name, &uuid, from, to, centre, faces),
            Some(tilt) => {
                let mut rotation = [0.0; 3];
                rotation[tilt.axis] = decimal(tilt.degrees);
                bbmodel::turned(
                    bbmodel::element(piece.name, &uuid, from, to, tilt.origin.map(decimal), faces),
                    rotation,
                )
            }
        };
        elements.push(element);
        if piece.loaded {
            loaded.push(bbmodel::text(&uuid));
        } else if piece.bare {
            bare.push(bbmodel::text(&uuid));
        } else if piece.inside {
            inside.push(bbmodel::text(&uuid));
        } else {
            outliner.push(bbmodel::text(&uuid));
        }
    }
    if !loaded.is_empty() {
        outliner.push(bbmodel::group("loaded", &bbmodel::uuid(seed, 0xF0), [8.0, 0.0, 8.0], loaded));
    }
    if !bare.is_empty() {
        outliner.push(bbmodel::group("bare", &bbmodel::uuid(seed, 0xF1), [8.0, 0.0, 8.0], bare));
    }
    if !inside.is_empty() {
        outliner.push(bbmodel::group("inside", &bbmodel::uuid(seed, 0xF2), [8.0, 0.0, 8.0], inside));
    }
    let textures = materials
        .iter()
        .enumerate()
        .map(|(index, m)| bbmodel::texture(m.name(), &bbmodel::uuid(seed, 0x2000 + index), None, None))
        .collect();
    let name = Path::new(prop.file()).file_stem().and_then(|s| s.to_str()).unwrap_or("model");
    bbmodel::project(name, [16, 16], elements, outliner, textures)
}

/// A prop from its project.
pub(crate) fn prop_boxes(doc: &Document) -> Result<Vec<PropBox>, String> {
    let mut boxes = Vec::with_capacity(doc.elements.len());
    for element in &doc.elements {
        let name = &element.name;
        // **Turned about one axis or none** (`mesh::Swing` says why not
        // three), and about its own `origin`, as Blockbench turns it.
        let turned: Vec<usize> = (0..3).filter(|&a| element.rotation[a] != 0.0).collect();
        let tilt = match turned[..] {
            [] => None,
            [axis] => Some(crate::engine::mesh::Tilt {
                axis,
                origin: element.origin.map(narrow),
                degrees: narrow(element.rotation[axis]),
            }),
            _ => {
                return Err(format!(
                    "\"{name}\" is turned about more than one axis, and a box of a block turns about one:                      the order Blockbench turns three in is not in the file"
                ))
            }
        };
        let mut textures = element.faces.iter().map(|face| face.as_ref().and_then(|f| f.texture));
        let first = textures.next().flatten();
        if textures.any(|t| t != first) {
            return Err(format!("\"{name}\" wears more than one texture, and a box is made of one material"));
        }
        let texture = first
            .and_then(|t| doc.textures.get(t))
            .ok_or_else(|| format!("\"{name}\" has a face with no texture"))?;
        let material = Material::ALL.iter().copied().find(|m| m.name() == texture).ok_or_else(|| {
            let known: Vec<&str> = Material::ALL.iter().map(|m| m.name()).collect();
            format!("\"{name}\" wears \"{texture}\", which is none of the materials: {}", known.join(", "))
        })?;
        boxes.push(PropBox {
            name: leak(name),
            from: element.from.map(narrow),
            to: element.to.map(narrow),
            material,
            loaded: element.groups.iter().any(|g| g.name.split_whitespace().next() == Some("loaded")),
            bare: element.groups.iter().any(|g| g.name.split_whitespace().next() == Some("bare")),
            inside: element.groups.iter().any(|g| g.name.split_whitespace().next() == Some("inside")),
            tilt,
        });
    }
    if boxes.is_empty() {
        return Err("there are no boxes in it".into());
    }
    Ok(boxes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::mesh::Vertex;
    use crate::logic::animal_model::build_parts;

    fn animal_mesh(model: &[Part], species: Species, walked: f32, speed: f32) -> Vec<Vertex> {
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let (mut v, mut i) = (Vec::new(), Vec::new());
        let motion = crate::logic::animal_model::Motion { walked, speed, ..Default::default() };
        build_parts(model, species, glam::Vec3::new(3.0, 60.0, -2.0), 0.7, motion, &layers, (15, 0), &mut v, &mut i);
        v
    }

    fn prop_mesh(boxes: &[PropBox], loaded: bool) -> Vec<Vertex> {
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let (mut v, mut i) = (Vec::new(), Vec::new());
        crate::engine::mesh::push_prop([1.0, 2.0, 3.0], boxes, 1, loaded, crate::engine::mesh::Hinged::Whole, |_| 0, 0, &layers, 0xA7, &mut v, &mut i);
        v
    }

    fn bytes(v: &[Vertex]) -> &[u8] {
        bytemuck::cast_slice(v)
    }

    /// Standing, walking, running and -- for a bird -- flying: every gait
    /// a part can be drawn in, so a pivot or a hinge that moved shows.
    const PACES: [(f32, f32); 4] = [(0.0, 0.0), (0.4, 3.0), (1.3, 4.5), (2.2, 9.0)];

    fn same_animal(a: &[Part], b: &[Part], species: Species) -> bool {
        PACES.iter().all(|&(walked, speed)| {
            bytes(&animal_mesh(a, species, walked, speed)) == bytes(&animal_mesh(b, species, walked, speed))
        })
    }

    #[test]
    fn every_file_in_assets_models_is_built_in_and_every_built_in_model_reads() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/models");
        let mut on_disk = Vec::new();
        for folder in ["animals", "furniture", "misc", "workstations"] {
            for entry in std::fs::read_dir(root.join(folder)).expect("the folder is there") {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "bbmodel") {
                    on_disk.push(format!("{folder}/{}", path.file_name().unwrap().to_string_lossy()));
                }
            }
        }
        on_disk.sort();
        let mut built_in: Vec<String> = crate::embedded::MODELS.iter().map(|(name, _)| name.to_string()).collect();
        built_in.sort();
        assert_eq!(on_disk, built_in, "embedded::MODELS and assets/models disagree");

        // Reading panics on a built-in file that does not read, naming it.
        let library = Library::read(None);
        for &species in Species::ALL {
            assert!(!library.animal(species).is_empty(), "{species:?} has no model");
        }
        for prop in Prop::ALL {
            assert!(!library.prop(prop).is_empty(), "{prop:?} has no model");
        }
    }

    /// The writer and the reader are exact inverses, to the bit of every
    /// vertex -- which is what let the tables be written out once and
    /// deleted, and what keeps `--export-models` honest.
    #[test]
    fn a_model_read_back_from_its_file_is_the_model_that_was_written() {
        for &species in Species::ALL {
            let model = animal(species);
            let text = animal_project(species, model, None).pretty();
            let read = animal_parts(&Document::parse(&text).unwrap()).unwrap();
            assert!(same_animal(model, &read, species), "{species:?} changed on the way through its file");
            let names: Vec<&str> = read.iter().map(|p| p.name).collect();
            assert_eq!(names, model.iter().map(|p| p.name).collect::<Vec<_>>());
        }
        for p in Prop::ALL {
            let model = prop(p);
            let text = prop_project(p, model).pretty();
            let read = prop_boxes(&Document::parse(&text).unwrap()).unwrap();
            for loaded in [false, true] {
                assert!(bytes(&prop_mesh(model, loaded)) == bytes(&prop_mesh(&read, loaded)), "{p:?}");
            }
        }
    }

    /// The boar's file with its body a quarter of a block taller.
    fn taller_boar() -> String {
        let mut json = Json::parse(crate::embedded::model("animals/boar.bbmodel").unwrap()).unwrap();
        let Some(Json::List(elements)) = json.get_mut("elements") else { panic!("no elements") };
        let body = elements.iter_mut().find(|e| e.get("name").and_then(Json::as_str) == Some("body")).unwrap();
        let Some(Json::List(to)) = body.get_mut("to") else { panic!("no to") };
        let Json::Number(y) = &mut to[1] else { panic!("no y") };
        *y += 4.0;
        json.pretty()
    }

    fn top(v: &[Vertex]) -> f32 {
        v.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max)
    }

    #[test]
    fn a_model_file_edited_beside_the_game_changes_what_is_drawn() {
        let dir = std::env::temp_dir().join(format!("primitive-models-edit-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("animals")).unwrap();
        std::fs::write(dir.join("animals/boar.bbmodel"), taller_boar()).unwrap();
        let edited = Library::read(Some(&dir));
        let _ = std::fs::remove_dir_all(&dir);

        let (before, after) = (animal(Species::Boar), edited.animal(Species::Boar));
        let rise = top(&animal_mesh(after, Species::Boar, 0.0, 0.0)) - top(&animal_mesh(before, Species::Boar, 0.0, 0.0));
        assert!((rise - 0.25).abs() < 1e-3, "four sixteenths on the body raised the boar by {rise}");
        // Nothing but the file that was there changed.
        assert!(same_animal(animal(Species::Deer), edited.animal(Species::Deer), Species::Deer));
    }

    #[test]
    fn a_model_file_that_does_not_read_is_drawn_as_the_built_in_one() {
        let dir = std::env::temp_dir().join(format!("primitive-models-broken-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("animals")).unwrap();
        std::fs::create_dir_all(dir.join("furniture")).unwrap();
        // Half a save, and a stool made of something nobody has heard of.
        let boar = taller_boar();
        std::fs::write(dir.join("animals/boar.bbmodel"), &boar[..boar.len() / 2]).unwrap();
        let stool = crate::embedded::model("furniture/stool.bbmodel").unwrap().replace("\"post\"", "\"marble\"");
        std::fs::write(dir.join("furniture/stool.bbmodel"), &stool).unwrap();
        let library = Library::read(Some(&dir));
        let _ = std::fs::remove_dir_all(&dir);

        assert!(same_animal(animal(Species::Boar), library.animal(Species::Boar), Species::Boar));
        assert!(bytes(&prop_mesh(prop(Prop::Stool), false)) == bytes(&prop_mesh(library.prop(Prop::Stool), false)));
        // ...and what was wrong is said in words a person can act on.
        let error = prop_boxes(&Document::parse(&stool).unwrap()).unwrap_err();
        assert!(error.contains("\"marble\"") && error.contains("boards"), "{error}");
        let error = Document::parse(&boar[..boar.len() / 2]).unwrap_err();
        assert!(error.starts_with("line "), "{error}");
    }

    #[test]
    fn a_face_wears_the_square_of_the_sheet_its_uv_starts_in() {
        for (slot, skin) in Skin::ALL.into_iter().enumerate() {
            assert_eq!(skin.slot(), slot, "Skin::ALL is out of slot order at {skin:?}");
            let [x1, y1, x2, y2] = tile_uv(skin);
            assert!(x2 <= (SHEET_COLUMNS * TILE) as f64 && y2 <= (SHEET_ROWS * TILE) as f64, "{skin:?} runs off the sheet");
            assert_eq!((x2 - x1, y2 - y1), (TILE as f64, TILE as f64));
        }
    }

    #[test]
    fn a_wing_is_a_group_whose_origin_is_its_hinge_and_a_leg_a_group_by_its_name() {
        let gull = animal(Species::Gull);
        assert!(gull.iter().any(|p| matches!(p.gait, Gait::Wing(h) if h != 0)), "the gull lost its hinged wings");
        assert!(gull.iter().any(|p| p.gait == Gait::Folded));
        let boar = animal(Species::Boar);
        assert!(boar.iter().any(|p| p.gait == Gait::LegFront) && boar.iter().any(|p| p.gait == Gait::LegBack));
        let bear = animal(Species::Bear);
        assert!(bear.iter().any(|p| p.pivot.is_some()), "the bear's paws no longer swing from its shoulders");
    }

    /// Blockbench 5 keeps a group's name and origin in a top-level `groups`
    /// list and only uuids in the outliner, and the shipped gull was saved
    /// that way: its wings have to come out hinged and folded, not still.
    #[test]
    fn a_blockbench_five_file_keeps_its_groups_out_of_the_outliner_and_still_moves() {
        let text = crate::embedded::model("animals/gull.bbmodel").unwrap();
        let parts = animal_parts(&Document::parse(text).unwrap()).unwrap();
        assert!(parts.iter().any(|p| matches!(p.gait, Gait::Wing(h) if h != 0)), "the gull's wings lost their hinge");
        assert!(parts.iter().any(|p| p.gait == Gait::Folded), "the gull's folded wings are still");
        let five = r#"{"elements": [{"name": "leg", "uuid": "e", "from": [0, 0, 0], "to": [1, 2, 1], "origin": [0.5, 2, 0.5],
            "faces": {"up": {"uv": [0, 0, 16, 16]}, "down": {"uv": [0, 0, 16, 16]}, "east": {"uv": [0, 0, 16, 16]},
            "west": {"uv": [0, 0, 16, 16]}, "south": {"uv": [0, 0, 16, 16]}, "north": {"uv": [0, 0, 16, 16]}}}],
            "resolution": {"width": 64, "height": 48},
            "groups": [{"name": "hare", "uuid": "r", "origin": [0, 1, 0]}, {"name": "leg_back", "uuid": "g", "origin": [0, 1, 0]}],
            "outliner": [{"uuid": "r", "children": [{"uuid": "g", "children": ["e"]}]}]}"#;
        let parts = animal_parts(&Document::parse(five).unwrap()).unwrap();
        assert_eq!(parts[0].gait, Gait::LegBack);
        assert_eq!(parts[0].at, [0.5, 0.0, 0.5], "the root's origin was not taken as the centre");
    }
}