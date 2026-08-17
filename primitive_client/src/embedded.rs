//! The assets, compiled into the binary.
//!
//! ## Why
//!
//! All of `assets/textures` is under ten kilobytes -- fifteen files, the
//! largest a 16x16 PNG. Against a seven-megabyte executable that is a
//! rounding error, and it buys something worth having: the game is one
//! file. No folder to keep beside it, no "it starts but every block is a
//! magenta checkerboard" when someone moves the executable out of the
//! zip, which is exactly what used to happen.
//!
//! ## The folder still wins
//!
//! Embedded copies are the *fallback*, not the source of truth. A file
//! on disk is preferred over the built-in one of the same name, so
//! `assets/textures/stone.png` next to the executable replaces the stone
//! texture without rebuilding anything. That is the whole of the
//! resource-pack story, and it costs one `if`.
//!
//! Adding a texture means adding a line here as well as to `blocks.toml`.
//! That is deliberate: `include_bytes!` needs a literal path, and the
//! alternative -- a build script walking the directory -- trades a
//! visible list for a hidden one, and turns a missing file into a
//! confusing build failure instead of a compile error naming the file.

/// `blocks.toml`, the map from block names to texture files.
pub const BLOCKS_TOML: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/blocks.toml"));

/// Every texture, by the filename `blocks.toml` refers to it by.
///
/// PNGs only -- `blocks.toml` itself has its own constant above.
pub const TEXTURES: &[(&str, &[u8])] = &[
    ("break.0.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/break.0.png"))),
    ("break.1.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/break.1.png"))),
    ("break.2.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/break.2.png"))),
    ("break.3.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/break.3.png"))),
    ("break.4.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/break.4.png"))),
    ("cobblestone.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/cobblestone.png"))),
    ("cactus.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/cactus.png"))),
    ("cactus_top.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/cactus_top.png"))),
    ("grass_mesh.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/grass_mesh.png"))),
    ("stick.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/stick.png"))),
    ("fiber.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/fiber.png"))),
    ("dirt.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/dirt.png"))),
    ("glowstone.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/glowstone.png"))),
    ("grass_side.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/grass_side.png"))),
    ("grass_top.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/grass_top.png"))),
    ("leaves.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/leaves.png"))),
    ("birch_log_side.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/birch_log_side.png"))),
    ("birch_log_top.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/birch_log_top.png"))),
    ("birch_leaves.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/birch_leaves.png"))),
    ("birch_planks.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/birch_planks.png"))),
    ("log_side.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/log_side.png"))),
    ("log_top.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/log_top.png"))),
    ("planks.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/planks.png"))),
    ("pebble.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/pebble.png"))),
    ("flint.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint.png"))),
    ("chest_side.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/chest_side.png"))),
    ("chest_top.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/chest_top.png"))),
    ("backpack_side.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/backpack_side.png"))),
    ("backpack_top.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/backpack_top.png"))),
    ("ash.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/ash.png"))),
    ("ash_item.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/ash_item.png"))),
    ("sky_clouds.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/sky_clouds.png"))),
    ("clay.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/clay.png"))),
    ("gravel.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/gravel.png"))),
    ("sand.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/sand.png"))),
    ("snow.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/snow.png"))),
    ("stone.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/stone.png"))),
    ("water.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/water.png"))),
    ("workbench_side.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/workbench_side.png"))),
    // Ore and metal. Nine files: four ores, four ingots and a lump of
    // coal.
    ("coal_ore.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/coal_ore.png"))),
    ("copper_ore.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/copper_ore.png"))),
    ("tin_ore.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/tin_ore.png"))),
    ("iron_ore.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/iron_ore.png"))),
    ("coal.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/coal.png"))),
    ("copper_ingot.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/copper_ingot.png"))),
    ("tin_ingot.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/tin_ingot.png"))),
    ("bronze_ingot.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/bronze_ingot.png"))),
    ("iron_ingot.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/iron_ingot.png"))),
    // The stone age: a flake, a haft, three heads and the three tools
    // they bind into. The metal picks that used to be here are gone with
    // their blocks -- their files are still on disk and no longer worth
    // carrying inside the binary.
    ("flint_flake.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint_flake.png"))),
    ("worked_stick.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/worked_stick.png"))),
    ("flint_knife_head.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint_knife_head.png"))),
    ("flint_axe_head.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint_axe_head.png"))),
    ("flint_pick_head.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint_pick_head.png"))),
    ("flint_knife.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint_knife.png"))),
    ("flint_axe.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint_axe.png"))),
    ("flint_pickaxe.png", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/flint_pickaxe.png"))),
];

/// The built-in copy of one texture, if there is one.
pub fn texture(filename: &str) -> Option<&'static [u8]> {
    TEXTURES
        .iter()
        .find(|(name, _)| *name == filename)
        .map(|(_, bytes)| *bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_texture_blocks_toml_names_is_embedded() {
        // The two lists are maintained by hand, and the failure mode of
        // them disagreeing is a game that runs with a checkerboard where
        // a block should be -- which is easy to ship without noticing.
        for line in BLOCKS_TOML.lines() {
            for piece in line.split('"') {
                if !piece.ends_with(".png") {
                    continue;
                }
                assert!(
                    texture(piece).is_some(),
                    "blocks.toml refers to {piece:?}, which is not embedded"
                );
            }
        }
    }

    #[test]
    fn the_window_icon_is_embedded_too() {
        // It is not in blocks.toml -- it is not a block -- so nothing
        // else would catch it going missing.
        assert!(texture(crate::engine::texture::ICON_FILE).is_some());
    }

    #[test]
    fn nothing_embedded_is_empty() {
        for (name, bytes) in TEXTURES {
            assert!(!bytes.is_empty(), "{name} is empty");
            assert_eq!(&bytes[1..4], b"PNG", "{name} is not a PNG");
        }
    }

    #[test]
    fn the_whole_lot_stays_small_enough_to_be_worth_embedding() {
        // The argument for doing this at all is that it is negligible.
        // If it stops being negligible, the argument stops holding.
        let total: usize = TEXTURES.iter().map(|(_, b)| b.len()).sum();
        assert!(total < 512 * 1024, "embedded assets have grown to {total} bytes");
    }
}
