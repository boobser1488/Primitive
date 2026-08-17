//! The hotbar: one slot per placeable block, showing that block's actual
//! texture.
//!
//! Geometry only -- this module builds screen-space quads and the
//! renderer draws them with `hotbar.wgsl`, which samples the same block
//! texture array the terrain uses. That's deliberate: an icon atlas
//! maintained separately from the block textures is a thing that drifts
//! out of date the first time someone re-skins a block.
//!
//! Coordinates are in NDC, authored as if the viewport were square; the
//! vertex shader divides x by the aspect ratio. Y is up (NDC), so the bar
//! sits at negative y.

use bytemuck::{Pod, Zeroable};

use primitive_shared::types::BlockId;

use crate::engine::texture::{TextureManager, FACE_SOUTH, FACE_TOP};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct HotbarVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    /// `UNTEXTURED` draws flat `tint` instead of sampling -- used for
    /// the slot frames and the selection box.
    pub tex_layer: u32,
    pub tint: [f32; 4],
}

/// Must match `UNTEXTURED` in hotbar.wgsl.
pub const UNTEXTURED: u32 = u32::MAX;

/// The bar holds every placeable block. Keys 1-9 reach the first nine
/// and 0 reaches the tenth, which is where the numeric row runs out --
/// past that the wheel is the only way to select, so the list is capped
/// here rather than growing a second row.
pub const MAX_SLOTS: usize = 10;
/// Six quads per slot at the very most (backdrop, icon, 4 frame edges).
pub const MAX_HOTBAR_VERTICES: usize = MAX_SLOTS * 8 * 6 + 64;

/// Slot geometry. Public because the HUD draws stack counts and the
/// health row relative to the bar, and two modules laying the same bar
/// out from two sets of numbers is how they end up half a slot apart.
pub const SLOT: f32 = 0.080;
const GAP: f32 = 0.012;
pub const BOTTOM: f32 = -0.94;

/// The bar has to be on screen, and the pack has to be at least as long
/// as the bar. Both are relations between constants, so the build is
/// where they should fail rather than the test suite.
const _: () = assert!(BOTTOM > -1.0 && BOTTOM + SLOT < 1.0);
const _: () = assert!(crate::logic::inventory::SLOTS >= MAX_SLOTS);
const BACKDROP: [f32; 4] = [0.05, 0.06, 0.09, 0.72];
/// The recess each slot sits in.
///
/// The bar used to be frames drawn straight over the world, so an empty
/// slot was a rectangle of whatever happened to be behind it -- against
/// a bright sky the frames vanished, and against a dark cave the icons
/// did. A cell of its own under each one costs a quad and makes the bar
/// legible over anything.
const CELL_TOP: [f32; 4] = [0.03, 0.035, 0.05, 0.82];
const CELL_BOTTOM: [f32; 4] = [0.09, 0.10, 0.13, 0.82];
/// ...and a little brighter under the slot in hand, so the selection
/// reads even where the frame is against something pale.
const CELL_SELECTED_TOP: [f32; 4] = [0.14, 0.13, 0.07, 0.88];
const CELL_SELECTED_BOTTOM: [f32; 4] = [0.24, 0.22, 0.11, 0.88];
const FRAME: [f32; 4] = [0.75, 0.78, 0.83, 0.9];
const FRAME_SELECTED: [f32; 4] = [1.0, 0.95, 0.55, 1.0];
const ICON_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const ICON_TINT_DIM: [f32; 4] = [0.72, 0.72, 0.72, 1.0];

fn push_quad(
    out: &mut Vec<HotbarVertex>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    tex_layer: u32,
    tint: [f32; 4],
) {
    // v = 0 is the top of the image, so the top edge of the quad (y1 in
    // NDC, where y is up) takes v = 0. Getting this backwards flips every
    // icon upside down -- the same trap as the block face UVs.
    let corners = [
        ([x0, y0], [0.0, 1.0]),
        ([x1, y0], [1.0, 1.0]),
        ([x1, y1], [1.0, 0.0]),
        ([x0, y0], [0.0, 1.0]),
        ([x1, y1], [1.0, 0.0]),
        ([x0, y1], [0.0, 0.0]),
    ];
    for (position, uv) in corners {
        out.push(HotbarVertex {
            position,
            uv,
            tex_layer,
            tint,
        });
    }
}

/// Horizontal centre of slot `index` of `count`.
/// The same quad, shaded from one colour at the top to another at the
/// bottom. The vertex carries its own tint and the hardware
/// interpolates it, so this costs exactly what a flat quad costs.
fn push_gradient(
    out: &mut Vec<HotbarVertex>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    top: [f32; 4],
    bottom: [f32; 4],
) {
    for (position, tint) in [
        ([x0, y0], bottom),
        ([x1, y0], bottom),
        ([x1, y1], top),
        ([x0, y0], bottom),
        ([x1, y1], top),
        ([x0, y1], top),
    ] {
        out.push(HotbarVertex {
            position,
            uv: [0.0, 0.0],
            tex_layer: UNTEXTURED,
            tint,
        });
    }
}

pub fn slot_centre(index: usize, count: usize) -> f32 {
    let pitch = SLOT + GAP;
    let total = pitch * count as f32 - GAP;
    let left = -total / 2.0;
    left + pitch * index as f32 + SLOT / 2.0
}

/// Which texture to show for a block in the pack.
///
/// A block that has asked for a picture of its own gets it -- see
/// `ITEM_SLOT`. Otherwise a face, and the side face rather than the top:
/// for grass, the side shows the green strip over dirt, which is what
/// makes it recognisable at icon size. Blocks with no side texture fall
/// back to the top.
fn icon_layer(textures: &TextureManager, block: BlockId) -> u32 {
    if let Some(layer) = textures.layer_for_item(block) {
        return layer;
    }
    let side = textures.layer_for_face(block, FACE_SOUTH);
    if side == 0 {
        textures.layer_for_face(block, FACE_TOP)
    } else {
        side
    }
}

/// Builds the whole bar from what the player is actually carrying,
/// appending to a list the caller keeps between frames -- so a rebuild
/// reuses the allocation instead of making a fresh one.
///
/// The bar always shows its ten slots -- an empty one is a frame with
/// nothing in it, so the number keys keep pointing at the same places as
/// the inventory fills and empties. Only the *contents* come and go.
pub fn build_into(
    textures: &TextureManager,
    inventory: &crate::logic::inventory::Inventory,
    selected: usize,
    out: &mut Vec<HotbarVertex>,
) {
    let count = MAX_SLOTS;

    let pitch = SLOT + GAP;
    let total = pitch * count as f32 - GAP;
    let pad = 0.014;
    push_quad(
        out,
        -total / 2.0 - pad,
        BOTTOM - pad,
        total / 2.0 + pad,
        BOTTOM + SLOT + pad,
        UNTEXTURED,
        BACKDROP,
    );

    for index in 0..count {
        let centre = slot_centre(index, count);
        let x0 = centre - SLOT / 2.0;
        let x1 = centre + SLOT / 2.0;
        let y0 = BOTTOM;
        let y1 = BOTTOM + SLOT;
        let is_selected = index == selected;

        // The recess, under everything else in the slot.
        let (top, bottom) = if is_selected {
            (CELL_SELECTED_TOP, CELL_SELECTED_BOTTOM)
        } else {
            (CELL_TOP, CELL_BOTTOM)
        };
        push_gradient(out, x0, y0, x1, y1, top, bottom);

        // Frame: four thin quads rather than a filled rect behind the
        // icon, so the selection reads as an outline at any size.
        let frame_colour = if is_selected { FRAME_SELECTED } else { FRAME };
        let t = if is_selected { 0.008 } else { 0.004 };
        push_quad(out, x0 - t, y0 - t, x1 + t, y0, UNTEXTURED, frame_colour);
        push_quad(out, x0 - t, y1, x1 + t, y1 + t, UNTEXTURED, frame_colour);
        push_quad(out, x0 - t, y0, x0, y1, UNTEXTURED, frame_colour);
        push_quad(out, x1, y0, x1 + t, y1, UNTEXTURED, frame_colour);

        // An empty slot draws its frame and nothing else. Drawing a
        // greyed-out block instead would suggest the player has one.
        let Some(block) = inventory.block_in(index) else {
            continue;
        };
        let layer = icon_layer(textures, block);
        let tint = if is_selected { ICON_TINT } else { ICON_TINT_DIM };
        push_quad(out, x0, y0, x1, y1, layer, tint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_is_centred_horizontally() {
        let count = MAX_SLOTS;
        let first = slot_centre(0, count);
        let last = slot_centre(count - 1, count);
        assert!(
            (first + last).abs() < 1e-5,
            "slots should be symmetric about x=0, got {first} and {last}"
        );
    }

    #[test]
    fn slots_do_not_overlap() {
        let count = MAX_SLOTS;
        for i in 1..count {
            let gap = slot_centre(i, count) - slot_centre(i - 1, count) - SLOT;
            assert!(gap > 0.0, "slots {} and {i} overlap", i - 1);
        }
    }

    #[test]
    fn the_bar_fits_on_screen() {
        // NDC runs -1..1; with the aspect divide, x shrinks on a wide
        // window, so checking the square case is the worst case.
        let count = MAX_SLOTS;
        let half = slot_centre(count - 1, count) + SLOT / 2.0;
        assert!(half < 1.0, "hotbar is wider than the viewport: {half}");
    }

    #[test]
    fn icon_quads_are_not_upside_down() {
        // The top edge of the quad (larger y in NDC) must carry v = 0,
        // the top of the image.
        let mut quad = Vec::new();
        push_quad(&mut quad, -0.1, -0.1, 0.1, 0.1, 3, ICON_TINT);
        for vertex in &quad {
            let expected_v = if vertex.position[1] > 0.0 { 0.0 } else { 1.0 };
            assert_eq!(
                vertex.uv[1], expected_v,
                "vertex at y={} should have v={expected_v}",
                vertex.position[1]
            );
        }
    }

    #[test]
    fn a_full_bar_fits_in_the_vertex_budget() {
        // 1 backdrop + per slot: 4 frame quads + 1 icon = 5.
        let quads = 1 + MAX_SLOTS * 5;
        assert!(
            quads * 6 <= MAX_HOTBAR_VERTICES,
            "{MAX_SLOTS} full slots need {} vertices but the budget is {MAX_HOTBAR_VERTICES}",
            quads * 6
        );
    }

    #[test]
    fn the_hotbar_matches_the_inventory_it_draws() {
        // The bar draws the inventory's *hotbar* slots -- the first ten
        // of them. If the two disagreed, the number keys would point
        // somewhere the player cannot see.
        assert_eq!(MAX_SLOTS, crate::logic::inventory::HOTBAR_SLOTS);
    }

    #[test]
    fn a_full_bar_still_fits_on_a_square_window() {
        let count = MAX_SLOTS;
        let half = slot_centre(count - 1, count) + SLOT / 2.0;
        assert!(half < 1.0, "a full hotbar is wider than the viewport: {half}");
    }
}
