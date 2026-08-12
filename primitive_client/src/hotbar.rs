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

use primitive_shared::types::{BlockId, PLACEABLE_BLOCKS};

use crate::texture::{TextureManager, FACE_SOUTH, FACE_TOP};

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
pub const MAX_HOTBAR_VERTICES: usize = MAX_SLOTS * 6 * 6 + 64;

const SLOT: f32 = 0.080;
const GAP: f32 = 0.012;
const BOTTOM: f32 = -0.94;
const BACKDROP: [f32; 4] = [0.05, 0.06, 0.09, 0.72];
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
fn slot_centre(index: usize, count: usize) -> f32 {
    let pitch = SLOT + GAP;
    let total = pitch * count as f32 - GAP;
    let left = -total / 2.0;
    left + pitch * index as f32 + SLOT / 2.0
}

/// Which face of a block to show as its icon.
///
/// The side face, not the top: for grass, the side shows the green strip
/// over dirt, which is what makes it recognisable at icon size. Blocks
/// with no side texture fall back to the top face.
fn icon_face(textures: &TextureManager, block: BlockId) -> usize {
    let side = textures.layer_for_face(block, FACE_SOUTH);
    if side == 0 {
        FACE_TOP
    } else {
        FACE_SOUTH
    }
}

/// Builds the whole bar. `selected` is an index into `PLACEABLE_BLOCKS`.
pub fn build(textures: &TextureManager, selected: usize) -> Vec<HotbarVertex> {
    let blocks = PLACEABLE_BLOCKS;
    let count = blocks.len().min(MAX_SLOTS);
    let mut out = Vec::with_capacity(MAX_HOTBAR_VERTICES);

    let pitch = SLOT + GAP;
    let total = pitch * count as f32 - GAP;
    let pad = 0.014;
    push_quad(
        &mut out,
        -total / 2.0 - pad,
        BOTTOM - pad,
        total / 2.0 + pad,
        BOTTOM + SLOT + pad,
        UNTEXTURED,
        BACKDROP,
    );

    for (index, &block) in blocks.iter().take(count).enumerate() {
        let centre = slot_centre(index, count);
        let x0 = centre - SLOT / 2.0;
        let x1 = centre + SLOT / 2.0;
        let y0 = BOTTOM;
        let y1 = BOTTOM + SLOT;
        let is_selected = index == selected;

        // Frame: four thin quads rather than a filled rect behind the
        // icon, so the selection reads as an outline at any size.
        let frame_colour = if is_selected { FRAME_SELECTED } else { FRAME };
        let t = if is_selected { 0.008 } else { 0.004 };
        push_quad(&mut out, x0 - t, y0 - t, x1 + t, y0, UNTEXTURED, frame_colour);
        push_quad(&mut out, x0 - t, y1, x1 + t, y1 + t, UNTEXTURED, frame_colour);
        push_quad(&mut out, x0 - t, y0, x0, y1, UNTEXTURED, frame_colour);
        push_quad(&mut out, x1, y0, x1 + t, y1, UNTEXTURED, frame_colour);

        let layer = textures.layer_for_face(block, icon_face(textures, block));
        let tint = if is_selected { ICON_TINT } else { ICON_TINT_DIM };
        push_quad(&mut out, x0, y0, x1, y1, layer, tint);
    }

    out
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
        assert!(BOTTOM > -1.0 && BOTTOM + SLOT < 1.0);
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
    fn every_placeable_block_gets_a_slot() {
        // Regression: the bar was fixed at nine while the block list
        // grew to ten, so the last block could be selected with the
        // wheel but was never drawn.
        assert!(
            PLACEABLE_BLOCKS.len() <= MAX_SLOTS,
            "{} placeable blocks but only {MAX_SLOTS} slots",
            PLACEABLE_BLOCKS.len()
        );
        let expected = PLACEABLE_BLOCKS.len();
        // 1 backdrop + per slot: 4 frame quads + 1 icon = 5.
        let quads = 1 + expected * 5;
        assert!(quads * 6 <= MAX_HOTBAR_VERTICES);
    }

    #[test]
    fn a_full_bar_still_fits_on_a_square_window() {
        let count = MAX_SLOTS;
        let half = slot_centre(count - 1, count) + SLOT / 2.0;
        assert!(half < 1.0, "a full hotbar is wider than the viewport: {half}");
    }
}
