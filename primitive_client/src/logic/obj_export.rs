//! The animals, written out as Wavefront `.obj`.
//!
//! ## Why this is in the game rather than beside it
//!
//! The models are a table of boxes in `animal_model` -- the same table
//! the mesher builds from -- and there is exactly one of it. An exporter
//! that re-typed the numbers into a script would be a second copy that
//! is right on the day it is written and wrong the first time somebody
//! moves a snout, and it would be wrong *silently*, which is the worst
//! way for a tool to be wrong.
//!
//! So this reads `animal_model::parts` and `Skin::picture`, and the file
//! it writes cannot disagree with what the game draws.
//!
//! ## What comes out
//!
//! One `.obj` and one `.mtl` per species, plus the pictures they name,
//! copied into a `textures/` folder beside them so the folder can be
//! handed to anybody and opened.
//!
//! * **One metre is one unit**, which is what every tool assumes.
//! * **The feet are on y = 0**, so the model stands on the grid rather
//!   than straddling it. In the game the position that crosses the wire
//!   is the animal's *middle* (see `Animal::state`), which is the right
//!   convention there and the wrong one for a file somebody is going to
//!   open in Blender.
//! * **It faces -Z**, its own forward, with no yaw applied and no gait:
//!   a model file wants the rest pose, not a frame of a walk cycle.
//! * **Groups are named** after the part -- `head`, `foreleg left` --
//!   because the whole point of the table is that a person can read it.
//! * **One material and one picture per animal.** The pieces of a wolf
//!   live on one sheet (see `texture::ANIMAL_SHEETS`), so the faces
//!   carry texture coordinates into that sheet and the file names it
//!   once. That is what a model wants: ten materials pointing at ten
//!   16x16 files is a thing to reassemble, not a thing to open.

use std::io::Write;
use std::path::{Path, PathBuf};

use primitive_shared::animals::Species;

use crate::engine::mesh::{face_uv, faces};
use crate::engine::texture::{ANIMAL_SHEETS, SHEET_COLUMNS, SHEET_ROWS};
use crate::logic::animal_model::{parts, Part, Skin, SCALE};

/// Which of the box's own faces looks forward, and which are its sides.
///
/// The mesher's face order is 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z, and an
/// animal faces -Z in its own space. The same three constants
/// `animal_model::append_part` uses, and for the same reason: a face
/// that picks the wrong skin is an eye on the back of a skull.
pub(crate) const FRONT_FACE: usize = 5;
pub(crate) const RIGHT_FACE: usize = 2;
pub(crate) const LEFT_FACE: usize = 3;

/// Writes every animal into `dir`, creating it if it is not there.
///
/// Returns what it wrote, so the caller can say so.
pub fn write_models(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    std::fs::create_dir_all(dir.join("textures"))?;
    let mut written = Vec::new();
    for &species in Species::ALL {
        written.push(write_species(dir, species)?);
    }
    Ok(written)
}

fn write_species(dir: &Path, species: Species) -> anyhow::Result<PathBuf> {
    let name = species.name();
    let obj_path = dir.join(format!("{name}.obj"));
    let mtl_name = format!("{name}.mtl");

    let parts = parts(species);
    // Sit it on the floor: the table is centred on the animal's middle,
    // and a file that opens half-buried is a file everybody has to move.
    let lift = -parts
        .iter()
        .map(|p| (p.at[1] - p.size[1] * 0.5) * SCALE)
        .fold(f32::INFINITY, f32::min);

    let mut obj = String::new();
    obj.push_str(&format!(
        "# {name}, exported from Primitive.\n\
         # One unit is one metre; the feet are on y = 0 and it faces -Z.\n\
         # Generated from the same table the game meshes from -- see\n\
         # `logic::animal_model`.\n\
         mtllib {mtl_name}\n\
         o {name}\n"
    ));

    // `.obj` indices are one-based and run across the whole file, so
    // they are counted as the faces are written rather than per group.
    let mut vertex = 1usize;
    // One material for the whole animal: everything it wears is on one
    // sheet, and the faces say where on it.
    obj.push_str(&format!("usemtl {name}\n"));

    for part in parts {
        obj.push_str(&format!("g {}\n", part.name.replace(' ', "_")));
        for (index, face) in faces().iter().enumerate() {
            let skin = skin_of(part, index);
            for corner in face.corners.iter() {
                let x = (part.at[0] + (corner[0] - 0.5) * part.size[0]) * SCALE;
                let y = (part.at[1] + (corner[1] - 0.5) * part.size[1]) * SCALE + lift;
                let z = (part.at[2] + (corner[2] - 0.5) * part.size[2]) * SCALE;
                obj.push_str(&format!("v {x:.5} {y:.5} {z:.5}\n"));
            }
            // Where this skin's tile sits on the animal's sheet, as a
            // rectangle in 0..1.
            let slot = skin.slot();
            let (column, row) = (
                slot as u32 % SHEET_COLUMNS,
                slot as u32 / SHEET_COLUMNS,
            );
            let (tile_u, tile_v) = (1.0 / SHEET_COLUMNS as f32, 1.0 / SHEET_ROWS as f32);
            for corner in face.corners.iter() {
                let [u, v] = face_uv(index, *corner);
                // Corner-to-corner across the tile, and then placed on
                // the sheet. `.obj` counts texture rows from the bottom
                // and the game counts them from the top, which is the
                // one conversion in this file that has to happen -- and
                // the one whose absence looks fine until you notice
                // every face is upside down.
                let su = (column as f32 + u) * tile_u;
                let sv = 1.0 - (row as f32 + v) * tile_v;
                obj.push_str(&format!("vt {su:.5} {sv:.5}\n"));
            }
            let n = normal_of(index);
            obj.push_str(&format!("vn {} {} {}\n", n[0], n[1], n[2]));
            obj.push_str(&format!(
                "f {a}/{a}/{n} {b}/{b}/{n} {c}/{c}/{n} {d}/{d}/{n}\n",
                a = vertex,
                b = vertex + 1,
                c = vertex + 2,
                d = vertex + 3,
                n = (vertex - 1) / 4 + 1,
            ));
            vertex += 4;
        }
    }

    std::fs::write(&obj_path, obj)?;
    write_material(dir, &dir.join(mtl_name), species)?;
    Ok(obj_path)
}

/// The picture one face of one part wears.
///
/// The same three-way choice `animal_model::append_part` makes, because
/// an exported model that dressed its faces differently from the drawn
/// one would be a different animal.
pub(crate) fn skin_of(part: &Part, face: usize) -> Skin {
    match (face, part.front, part.sides) {
        (FRONT_FACE, Some(front), _) => front,
        (RIGHT_FACE, _, Some((right, _))) => right,
        (LEFT_FACE, _, Some((_, left))) => left,
        _ => part.skin,
    }
}

/// The outward normal of one of the six faces, in the mesher's order.
fn normal_of(face: usize) -> [i32; 3] {
    match face {
        0 => [0, 1, 0],
        1 => [0, -1, 0],
        2 => [1, 0, 0],
        3 => [-1, 0, 0],
        4 => [0, 0, 1],
        _ => [0, 0, -1],
    }
}

/// The one picture an animal wears, as a filename.
pub(crate) fn sheet_of(species: Species) -> &'static str {
    // Two species can wear one sheet -- see `texture::sheet_index`.
    ANIMAL_SHEETS[crate::engine::texture::sheet_index(species)]
}

fn write_material(dir: &Path, path: &Path, species: Species) -> anyhow::Result<()> {
    let name = species.name();
    let file = sheet_of(species);
    let leaf = Path::new(file)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("hide.png");

    let mut mtl = std::fs::File::create(path)?;
    writeln!(mtl, "# The one material {name} wears: its sheet.")?;
    writeln!(mtl, "\nnewmtl {name}")?;
    // Flat white with the picture on it: the game's own shading is light
    // levels and ambient occlusion, neither of which is a material
    // property, so anything else here would be this file inventing a
    // look the game does not have.
    writeln!(mtl, "Kd 1.000 1.000 1.000")?;
    writeln!(mtl, "Ka 0.000 0.000 0.000")?;
    writeln!(mtl, "Ks 0.000 0.000 0.000")?;
    writeln!(mtl, "d 1.0")?;
    writeln!(mtl, "illum 1")?;
    writeln!(mtl, "map_Kd textures/{leaf}")?;

    // The sheet itself, copied so the folder stands on its own. From
    // disk if it is there and from the binary if it is not -- the same
    // order `texture::load_sheet` uses, and for the same reason: a
    // single-file install has no folder to read.
    let target = dir.join("textures").join(leaf);
    let on_disk = Path::new("assets/textures").join(file);
    if on_disk.exists() {
        std::fs::copy(&on_disk, &target)?;
    } else if let Some(bytes) = crate::embedded::texture(file) {
        std::fs::write(&target, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_animal_comes_out_as_a_closed_set_of_boxes() {
        let dir = std::env::temp_dir().join("primitive-obj-test");
        let _ = std::fs::remove_dir_all(&dir);
        let written = write_models(&dir).expect("exported");
        assert_eq!(written.len(), Species::ALL.len());

        for (&species, path) in Species::ALL.iter().zip(&written) {
            let text = std::fs::read_to_string(path).expect("readable");
            let boxes = parts(species).len();
            // Six faces a box, four corners a face.
            assert_eq!(
                text.lines().filter(|l| l.starts_with("v ")).count(),
                boxes * 6 * 4,
                "{} has the wrong number of corners",
                species.name()
            );
            assert_eq!(
                text.lines().filter(|l| l.starts_with("f ")).count(),
                boxes * 6,
                "{} has the wrong number of faces",
                species.name()
            );
            // Every group in the table is named in the file.
            for part in parts(species) {
                assert!(
                    text.contains(&format!("g {}", part.name.replace(' ', "_"))),
                    "{} is missing its {}",
                    species.name(),
                    part.name
                );
            }
            // ...and it stands on the floor rather than straddling it.
            let lowest = text
                .lines()
                .filter_map(|l| l.strip_prefix("v "))
                .filter_map(|l| l.split_whitespace().nth(1))
                .filter_map(|y| y.parse::<f32>().ok())
                .fold(f32::INFINITY, f32::min);
            assert!(lowest.abs() < 1e-4, "{} floats at {lowest}", species.name());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_face_wears_the_picture_the_game_would_draw_on_it() {
        // The exporter makes the same three-way choice the mesher does.
        // If it stops doing so, an exported wolf has an eye on the back
        // of its skull and nothing else says so.
        for &species in Species::ALL {
            for part in parts(species) {
                if let Some(front) = part.front {
                    assert_eq!(skin_of(part, FRONT_FACE), front);
                }
                if let Some((right, left)) = part.sides {
                    assert_eq!(skin_of(part, RIGHT_FACE), right);
                    assert_eq!(skin_of(part, LEFT_FACE), left);
                }
                // The top of a head is never the eyed side view.
                assert_eq!(skin_of(part, 0), part.skin);
            }
        }
    }
}
