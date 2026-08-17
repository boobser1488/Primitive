//! Breaking a block takes time, and you can see it happening.
//!
//! ## The state
//!
//! Holding the button accumulates progress against the targeted block's
//! hardness. Looking away, or letting go, throws that progress away --
//! there is deliberately no memory of a half-mined block, because a
//! player who returns to one and finds it half done cannot tell that
//! from a bug.
//!
//! ## The animation
//!
//! Two overlays, drawn by two different pipelines, because they are two
//! different kinds of thing:
//!
//! * **The selection outline** is flat untextured geometry -- four thin
//!   bars around each face -- and rides with the other players on the
//!   actor pipeline.
//! * **The cracks** are `break.0.png` .. `break.4.png` laid over the
//!   block's own texture: one quad per face, sampling the same texture
//!   array the terrain does, blended by the transparent pipeline (which
//!   tests depth without writing it, so the overlay sits on the surface
//!   instead of fighting it).
//!
//! Cracks used to be geometry too -- a star of dark bars per face. It
//! needed no art, and it looked like a star of dark bars. A texture is
//! what makes damage read as damage, and it costs nothing here: the
//! stages are five more layers in an array that already has a hundred,
//! so there is no second sampler, no second bind group and no shader of
//! their own.

use primitive_shared::types::{break_seconds_with, BlockId};

use crate::net::remote_players::ActorVertex;

/// Half-thickness of the selection outline.
const OUTLINE_THICKNESS: f32 = 0.012;
/// How far the selection outline floats off the block's surface.
///
/// Without it the outline and the block face are exactly coplanar and
/// z-fight; with too much it visibly detaches at a shallow angle. A few
/// thousandths of a block is comfortably inside a texel.
///
/// **Only the outline.** The cracks used to be lifted by this too, and
/// that is what "the damage sits on a cushion of air" was: a few
/// thousandths of a block is nothing seen head-on and a visible gap seen
/// along the face, because the further the surface recedes the more
/// screen distance those same thousandths cover. They are drawn exactly
/// on the face now and win the depth test by a bias in the pipeline
/// instead -- which moves the *comparison* rather than the geometry, so
/// there is nothing left to see a gap in. See `crack_pipeline`.
const SURFACE_OFFSET: f32 = 0.004;

const OUTLINE_COLOR: [f32; 3] = [0.03, 0.03, 0.04];

/// Progress against one block.
#[derive(Default)]
pub struct Mining {
    /// The cell being mined, and what is in it.
    target: Option<((i32, i32, i32), BlockId)>,
    /// 0..1.
    progress: f32,
}

impl Mining {
    pub fn new() -> Self {
        Self::default()
    }

    /// What the player is currently aimed at, if anything.
    pub fn target(&self) -> Option<(i32, i32, i32)> {
        self.target.map(|(cell, _)| cell)
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// Throws away any progress. Used when the world changes underneath
    /// the player -- a confirmed break, a respawn, opening the menu.
    pub fn reset(&mut self) {
        self.target = None;
        self.progress = 0.0;
    }

    /// Advances mining by one frame.
    ///
    /// `aim` is what the ray hit this frame, `holding` whether the break
    /// button is down, and `tool` whatever is in the selected slot.
    /// Returns the cell to break once it is finished, exactly once.
    ///
    /// The tool is passed in rather than remembered because it can
    /// change mid-swing -- a player may spin the wheel onto a pick with
    /// the button held -- and the honest answer is that the block being
    /// worked on gets easier from that frame on, which is what happens
    /// if the number is re-read every frame.
    pub fn update(
        &mut self,
        aim: Option<((i32, i32, i32), BlockId)>,
        holding: bool,
        dt: f32,
        tool: Option<BlockId>,
    ) -> Option<(i32, i32, i32)> {
        // Water, air, and anything this tool cannot get into, is not a
        // target at all, however long you hold the button on it.
        let minable = aim.filter(|(_, block)| break_seconds_with(*block, tool).is_some());
        if !holding || minable.is_none() {
            self.reset();
            return None;
        }

        // Aiming somewhere new starts over -- but still makes this
        // frame's progress, so holding the button never wastes the frame
        // the aim settled on.
        if minable != self.target {
            self.target = minable;
            self.progress = 0.0;
        }

        // `?` would do, and would read as "this is a lookup". It is
        // not: the lines above have already advanced the timer, and the
        // early return is a decision rather than a missing value.
        #[allow(clippy::question_mark)]
        let Some((cell, block)) = self.target else {
            return None;
        };
        #[allow(clippy::question_mark)]
        let Some(seconds) = break_seconds_with(block, tool) else {
            return None;
        };

        self.progress += dt / seconds.max(0.01);
        if self.progress < 1.0 {
            return None;
        }
        // Finished. Clear immediately so a held button does not send a
        // second break for the same cell before the server answers.
        self.reset();
        Some(cell)
    }

    /// Which stage of cracks the damage has reached, if any.
    ///
    /// Discrete rather than a smooth fade: stages read as damage
    /// accumulating, where a texture fading in reads as the block being
    /// shaded. The last stage holds until the block gives.
    pub fn break_stage(&self) -> Option<usize> {
        self.target?;
        if self.progress <= 0.0 {
            return None;
        }
        let stage = (self.progress * crate::engine::texture::BREAK_STAGES as f32) as usize;
        Some(stage.min(crate::engine::texture::BREAK_STAGES - 1))
    }

    /// Appends the selection outline to an actor mesh.
    ///
    /// Does nothing when there is no target, so the caller can call it
    /// unconditionally.
    pub fn build_overlay_into(&self, vertices: &mut Vec<ActorVertex>, indices: &mut Vec<u32>) {
        let Some((cell, block)) = self.target else {
            return;
        };
        // The box the ray actually stopped at, not the cell it is in.
        // A metre cube drawn around a blade of grass is a box around
        // mostly nothing, and it lies about what a click will hit.
        let Some((min, max)) =
            primitive_shared::geometry::block_target_box(block, cell.0, cell.1, cell.2)
        else {
            return;
        };
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];

        for face in 0..6 {
            outline_face(min, size, face, vertices, indices);
        }
    }

    /// Appends the crack overlay: the block's six faces, each covered by
    /// the `break.N.png` for the current stage.
    ///
    /// Separate from the outline because it is drawn by a different
    /// pipeline -- these are textured, blended quads that ride with the
    /// terrain shader, and the outline is flat untextured geometry.
    /// `layer` is the texture array layer for the stage; the caller
    /// looks it up, so this module needs nothing from the GPU.
    pub fn build_break_mesh_into(
        &self,
        layer: u32,
        vertices: &mut Vec<crate::engine::mesh::Vertex>,
        indices: &mut Vec<u32>,
    ) {
        let Some((cell, block)) = self.target else {
            return;
        };
        if self.break_stage().is_none() {
            return;
        }
        let origin = [cell.0 as f32, cell.1 as f32, cell.2 as f32];

        // **Cracks go on the shape, not around it.**
        //
        // Everything used to get the six faces of its target box, which
        // is right for a cube and wrong for the two things that are not
        // one. A tuft of grass is two crossed planes inside a cell it
        // very nearly fills, so its box is nearly a whole block: hitting
        // a blade drew a metre cube of cracks standing in the air around
        // it. A stone lying on the ground is a quad two centimetres
        // thick, so its box is a wafer, and the four side faces of that
        // wafer were the crack texture squashed into eight hundredths of
        // its height -- a smear along the ground.
        //
        // Both now take the same corners the mesher drew them with. See
        // `mesh::cross_planes` and `mesh::flat_quad`.
        if primitive_shared::types::is_cross(block) {
            for plane in crate::engine::mesh::cross_planes(origin) {
                // Once, not once per winding. A plane has no outside and
                // the pass does not cull, so the quad is drawn from
                // either side already -- the second copy was the same
                // triangles in the same place.
                //
                // Harmless while the cracks were blended over the block
                // and merely wasteful. Not harmless now that they
                // multiply: two copies multiply twice, and a tuft of
                // grass came out squared -- markedly darker than every
                // other block at the same damage.
                cracks_on_quad(plane, layer, vertices, indices);
            }
            return;
        }
        if primitive_shared::types::is_flat(block) {
            cracks_on_quad(
                crate::engine::mesh::flat_quad(origin, block),
                layer,
                vertices,
                indices,
            );
            return;
        }

        let Some((min, max)) =
            primitive_shared::geometry::block_target_box(block, cell.0, cell.1, cell.2)
        else {
            return;
        };
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        for face in 0..6 {
            cracks_on_face(min, size, face, layer, vertices, indices);
        }
    }
}

/// One face's plane: where its (0,0) corner sits and the two in-plane
/// axes, chosen so `u × v` is the outward normal.
///
/// That handedness is not cosmetic. The actor pipeline culls back faces,
/// so a quad wound the wrong way is invisible -- and with six faces to
/// get right, deriving the winding from a consistent basis is the only
/// way to avoid three of them silently disappearing.
fn face_basis(face: usize) -> ([f32; 3], [f32; 3], [f32; 3], [f32; 3]) {
    match face {
        // origin, u, v, normal
        0 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        1 => ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
        2 => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        3 => ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
        4 => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        _ => ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    }
}

/// Emits one rectangle lying in a block face's plane.
///
/// `corners` are in face-local (u, v) coordinates running 0..1 across the
/// face, listed counter-clockwise as seen from outside.
fn push_face_quad(
    origin: [f32; 3],
    size: [f32; 3],
    face: usize,
    corners: [(f32, f32); 4],
    color: [f32; 3],
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    let (face_origin, u, v, normal) = face_basis(face);
    let base = vertices.len() as u32;
    for (cu, cv) in corners {
        // The unit cube the basis describes, scaled onto the block's
        // own box. An outline that always drew a full cell would frame
        // the empty air above a layer of snow and around a blade of
        // grass, and it would lie about what a click is going to hit.
        let local = [
            face_origin[0] + u[0] * cu + v[0] * cv,
            face_origin[1] + u[1] * cu + v[1] * cv,
            face_origin[2] + u[2] * cu + v[2] * cv,
        ];
        vertices.push(ActorVertex {
            position: [
                origin[0] + local[0] * size[0] + normal[0] * SURFACE_OFFSET,
                origin[1] + local[1] * size[1] + normal[1] * SURFACE_OFFSET,
                origin[2] + local[2] * size[2] + normal[2] * SURFACE_OFFSET,
            ],
            color,
            normal,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// A frame of four thin bars around the edge of one face.
fn outline_face(
    origin: [f32; 3],
    size: [f32; 3],
    face: usize,
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    let t = OUTLINE_THICKNESS;
    let edges = [
        [(0.0, 0.0), (1.0, 0.0), (1.0, t), (0.0, t)],
        [(0.0, 1.0 - t), (1.0, 1.0 - t), (1.0, 1.0), (0.0, 1.0)],
        [(0.0, t), (t, t), (t, 1.0 - t), (0.0, 1.0 - t)],
        [(1.0 - t, t), (1.0, t), (1.0, 1.0 - t), (1.0 - t, 1.0 - t)],
    ];
    for edge in edges {
        push_face_quad(origin, size, face, edge, OUTLINE_COLOR, vertices, indices);
    }
}

/// One face's worth of crack texture, laid over the block.
/// Cracks over four corners given outright, in the order the mesher
/// emitted them, with the texture stretched across.
///
/// The counterpart of `cracks_on_face` for the shapes that are not
/// boxes -- see `build_break_mesh_into`. Exactly on the corners it is
/// given: the pipeline's depth bias is what keeps the quad in front of
/// what it lies on, so there is no offset to get wrong here.
fn cracks_on_quad(
    corners: [[f32; 3]; 4],
    layer: u32,
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    // Full brightness, for the same reason as `cracks_on_face`.
    let light = crate::engine::mesh::pack_light(15, 0, 3, 0);
    for (corner, uv) in corners
        .into_iter()
        .zip([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
    {
        vertices.push(crate::engine::mesh::Vertex::new(corner, uv, layer, light));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn cracks_on_face(
    origin: [f32; 3],
    size: [f32; 3],
    face: usize,
    layer: u32,
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    // The normal the basis also returns is unused here: the quad lies
    // in the face's plane rather than off it. See `SURFACE_OFFSET`.
    let (face_origin, u, v, _) = face_basis(face);
    let base = vertices.len() as u32;
    // Full brightness rather than the block's own light: a crack is
    // meant to be visible on the block you are hitting, and a block
    // being mined in a cave is exactly the case where the light there
    // is zero. The texture is near-black, so a lit overlay still reads
    // as damage rather than as a glow.
    let light = crate::engine::mesh::pack_light(15, 0, 3, face as u8);
    for (cu, cv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        vertices.push(crate::engine::mesh::Vertex::new(
            // Exactly on the face. See `SURFACE_OFFSET`.
            [
                origin[0] + (face_origin[0] + u[0] * cu + v[0] * cv) * size[0],
                origin[1] + (face_origin[1] + u[1] * cu + v[1] * cv) * size[1],
                origin[2] + (face_origin[2] + u[2] * cu + v[2] * cv) * size[2],
            ],
            // v = 0 is the top of the image, and the face basis runs v
            // upwards, so the two are flipped against each other.
            [cu, 1.0 - cv],
            layer,
            light,
        ));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
    use super::*;
    // Planks rather than stone: stone cannot be broken by hand at all now
// (see `types::break_seconds`), so the slowest thing a bare hand can
// still get through is worked wood.
use primitive_shared::types::{BLOCK_DIRT, BLOCK_PLANKS, BLOCK_WATER};

    const CELL: (i32, i32, i32) = (3, 4, 5);
    const OTHER: (i32, i32, i32) = (9, 4, 5);

    /// Mines until it breaks, capped so a bug cannot hang the test.
    fn mine_to_completion(mining: &mut Mining, cell: (i32, i32, i32), block: BlockId) -> Option<(i32, i32, i32)> {
        for _ in 0..10_000 {
            if let Some(broken) = mining.update(Some((cell, block)), true, 1.0 / 60.0, None) {
                return Some(broken);
            }
        }
        None
    }

    #[test]
    fn a_rock_face_waits_for_a_pick_and_then_gives_way() {
        use primitive_shared::types::{BLOCK_FLINT_AXE, BLOCK_FLINT_PICKAXE, BLOCK_STONE};
        // Bare-handed, stone is not a target at all: no progress, no
        // cracks, and no half-full bar to make the player think the
        // swing was doing something.
        let mut hands = Mining::new();
        for _ in 0..600 {
            assert_eq!(
                hands.update(Some((CELL, BLOCK_STONE)), true, 1.0 / 60.0, None),
                None
            );
        }
        assert_eq!(hands.progress(), 0.0);
        assert_eq!(hands.break_stage(), None);

        // With a flint pick it is ordinary work...
        let mut flint = Mining::new();
        let mut frames = 0;
        let broke = loop {
            frames += 1;
            if let Some(cell) = flint.update(
                Some((CELL, BLOCK_STONE)),
                true,
                1.0 / 60.0,
                Some(BLOCK_FLINT_PICKAXE),
            ) {
                break cell;
            }
            assert!(frames < 10_000, "a flint pick never got through stone");
        };
        assert_eq!(broke, CELL);
        assert!(frames > 1, "a flint pick went through rock instantly");

        // ...and the *wrong* tool is exactly as good as no tool. An axe
        // held against a rock face is a stone on a stick: no progress, no
        // cracks, nothing. The client has to agree with the server about
        // this or it would fill a progress bar the server then refuses.
        let mut axe = Mining::new();
        for _ in 0..600 {
            assert_eq!(
                axe.update(
                    Some((CELL, BLOCK_STONE)),
                    true,
                    1.0 / 60.0,
                    Some(BLOCK_FLINT_AXE)
                ),
                None
            );
        }
        assert_eq!(axe.progress(), 0.0);
    }

    #[test]
    fn an_axe_brings_a_standing_tree_down_and_a_pick_does_not() {
        use primitive_shared::types::{BLOCK_FLINT_AXE, BLOCK_FLINT_PICKAXE, BLOCK_LOG};
        let mut hands = Mining::new();
        for _ in 0..600 {
            assert_eq!(
                hands.update(Some((CELL, BLOCK_LOG)), true, 1.0 / 60.0, None),
                None,
                "a trunk came down to bare hands"
            );
        }
        let mut pick = Mining::new();
        for _ in 0..600 {
            assert_eq!(
                pick.update(
                    Some((CELL, BLOCK_LOG)),
                    true,
                    1.0 / 60.0,
                    Some(BLOCK_FLINT_PICKAXE)
                ),
                None,
                "a pickaxe felled a tree"
            );
        }
        let mut axe = Mining::new();
        assert!(
            (0..10_000).any(|_| axe
                .update(
                    Some((CELL, BLOCK_LOG)),
                    true,
                    1.0 / 60.0,
                    Some(BLOCK_FLINT_AXE)
                )
                .is_some()),
            "an axe never got through a trunk"
        );
    }

    #[test]
    fn swapping_to_a_pick_mid_swing_does_not_lose_the_swing() {
        // The tool is re-read every frame (see `update`), so picking one
        // up while the button is down keeps the progress already made
        // and simply gets faster. Throwing it away would be the more
        // "correct" model and would feel like the game punishing you for
        // improving your equipment.
        use primitive_shared::types::BLOCK_FLINT_PICKAXE;
        let mut mining = Mining::new();
        for _ in 0..10 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        }
        let before = mining.progress();
        assert!(before > 0.0);
        mining.update(
            Some((CELL, BLOCK_PLANKS)),
            true,
            1.0 / 60.0,
            Some(BLOCK_FLINT_PICKAXE),
        );
        assert!(mining.progress() > before);
    }

    #[test]
    fn a_block_does_not_break_instantly() {
        let mut mining = Mining::new();
        assert_eq!(
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None),
            None,
            "one frame should not break stone"
        );
        assert!(mining.progress() > 0.0, "no progress was made");
        assert!(mining.progress() < 1.0);
    }

    #[test]
    fn holding_long_enough_breaks_it_exactly_once() {
        let mut mining = Mining::new();
        assert_eq!(mine_to_completion(&mut mining, CELL, BLOCK_PLANKS), Some(CELL));
        // The button is still held, but the block is gone; without the
        // reset inside `update` this would fire again every frame until
        // the server's confirmation arrived.
        assert_eq!(mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None), None);
    }

    #[test]
    fn harder_blocks_take_longer() {
        let mut soft = Mining::new();
        let mut hard = Mining::new();
        // Ten frames, not twenty: breaking is twice as quick as it was,
        // and dirt now finishes inside a third of a second -- after
        // which its progress resets and the comparison is between a
        // second attempt and a first.
        for _ in 0..10 {
            soft.update(Some((CELL, BLOCK_DIRT)), true, 1.0 / 60.0, None);
            hard.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        }
        assert!(
            soft.progress() > hard.progress(),
            "dirt ({}) should mine faster than planks ({})",
            soft.progress(),
            hard.progress()
        );
    }

    #[test]
    fn letting_go_throws_the_progress_away() {
        let mut mining = Mining::new();
        for _ in 0..20 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        }
        assert!(mining.progress() > 0.0);
        mining.update(Some((CELL, BLOCK_PLANKS)), false, 1.0 / 60.0, None);
        assert_eq!(mining.progress(), 0.0);
        assert_eq!(mining.target(), None);
    }

    #[test]
    fn looking_at_a_different_block_starts_over() {
        let mut mining = Mining::new();
        for _ in 0..20 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        }
        let partway = mining.progress();
        assert!(partway > 0.0);

        mining.update(Some((OTHER, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        assert_eq!(mining.target(), Some(OTHER));
        assert!(
            mining.progress() < partway,
            "progress followed the aim across: {} then {}",
            partway,
            mining.progress()
        );
    }

    #[test]
    fn water_cannot_be_mined() {
        let mut mining = Mining::new();
        for _ in 0..600 {
            assert_eq!(mining.update(Some((CELL, BLOCK_WATER)), true, 1.0 / 60.0, None), None);
        }
        assert_eq!(mining.target(), None, "water should never become a target");
    }

    #[test]
    fn aiming_at_nothing_is_not_a_target() {
        let mut mining = Mining::new();
        assert_eq!(mining.update(None, true, 1.0 / 60.0, None), None);
        assert_eq!(mining.target(), None);
    }

    #[test]
    fn there_is_no_overlay_without_a_target() {
        let mining = Mining::new();
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(&mut v, &mut i);
        assert!(v.is_empty() && i.is_empty());
    }

    #[test]
    fn a_fresh_target_is_outlined_but_uncracked() {
        let mut mining = Mining::new();
        mining.update(Some((CELL, BLOCK_PLANKS)), true, 0.0, None);
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(&mut v, &mut i);
        // Six faces, four outline bars each, six indices per bar.
        assert_eq!(i.len(), 6 * 4 * 6, "expected only the outline");
        assert!(!v.is_empty());
    }

    #[test]
    fn the_cracks_deepen_as_the_block_gives_way() {
        let mut early = Mining::new();
        let mut late = Mining::new();
        early.update(Some((CELL, BLOCK_PLANKS)), true, 0.05, None);
        for _ in 0..100 {
            late.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        }
        assert!(late.progress() > early.progress());
        assert!(
            late.break_stage() > early.break_stage(),
            "a nearly broken block should be showing a later stage: {:?} then {:?}",
            early.break_stage(),
            late.break_stage()
        );
    }

    #[test]
    fn the_stage_stays_inside_the_textures_that_exist() {
        // `progress` is allowed to overshoot 1.0 by a frame before the
        // block gives, and indexing the texture table with it would be
        // an out-of-range layer -- which is a wrong texture at best.
        let mut mining = Mining::new();
        mining.update(Some((CELL, BLOCK_PLANKS)), true, 0.0, None);
        assert_eq!(mining.break_stage(), None, "an untouched block is not cracked");

        for progress in [0.01f32, 0.3, 0.7, 0.999, 1.0, 4.0] {
            // Set directly rather than mined up to: `update` would break
            // the block at 1.0 and clear the target, and what is being
            // checked here is the arithmetic, not the state machine.
            mining.progress = progress;
            let stage = mining.break_stage().expect("damage but no stage");
            assert!(
                stage < crate::engine::texture::BREAK_STAGES,
                "progress {progress} asked for stage {stage}"
            );
        }
    }

    #[test]
    fn cracks_follow_the_shape_rather_than_boxing_it() {
        // The bug: everything got the six faces of its *target box*.
        // A tuft of grass nearly fills its cell, so hitting a blade put
        // a metre cube of cracks in the air around it; a stone lying on
        // the ground is a wafer, so its four side faces were the crack
        // texture squashed into eight hundredths of its height.
        use primitive_shared::types::{BLOCK_PEBBLE, BLOCK_TALL_GRASS};

        let quads = |block| {
            let mut mining = Mining::new();
            // One frame only: grass and a loose stone come apart in a
            // fraction of a second, and `update` clears the target the
            // moment they do.
            mining.update(Some((CELL, block)), true, 1.0 / 60.0, None);
            assert!(mining.break_stage().is_some(), "no damage to draw");
            let (mut v, mut i) = (Vec::new(), Vec::new());
            mining.build_break_mesh_into(7, &mut v, &mut i);
            assert!(!v.is_empty(), "nothing was drawn at all");
            (v.len() / 4, v)
        };

        // Two crossed planes: two quads, not the six of a box -- and not
        // the four it used to be either. The second winding of each was
        // the same triangles in the same place, since the pass does not
        // cull, and two copies of a *multiplied* crack come out squared:
        // a tuft of grass was visibly darker at the same damage than
        // everything else in the world.
        let (grass_quads, grass) = quads(BLOCK_TALL_GRASS);
        assert_eq!(grass_quads, 2, "grass got a box instead of its planes");

        // One quad lying flat, not a six-sided shell around a wafer.
        let (pebble_quads, pebble) = quads(BLOCK_PEBBLE);
        assert_eq!(pebble_quads, 1, "a stone got a box");
        // ...and it is flat: every corner at the same height.
        let first = pebble[0].position[1];
        assert!(
            pebble.iter().all(|v| (v.position[1] - first).abs() < 1e-6),
            "the stone's cracks were not flat"
        );

        // The grass cracks stand where the grass does, which is the
        // whole point of sharing the shape: no corner outside the cell.
        let cell_origin = [CELL.0 as f32, CELL.1 as f32, CELL.2 as f32];
        for vertex in &grass {
            for axis in [0usize, 1, 2] {
                let local = vertex.position[axis] - cell_origin[axis];
                assert!(
                    (-0.01..=1.01).contains(&local),
                    "a crack corner left the cell on axis {axis}: {local}"
                );
            }
        }
    }

    #[test]
    fn the_crack_overlay_covers_every_face_of_the_block() {
        let mut mining = Mining::new();
        for _ in 0..100 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        }
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_break_mesh_into(7, &mut v, &mut i);
        assert_eq!(v.len(), 6 * 4, "one quad per face");
        assert_eq!(i.len(), 6 * 6);
        assert!(
            v.iter().all(|vertex| vertex.tex_layer() == 7),
            "the overlay sampled something other than the stage it was given"
        );
        // Every vertex sits *on* the block it is cracking, exactly --
        // not floating in front of it. An offset here is what made the
        // damage read as a decal hanging on a cushion of air when the
        // face was seen at an angle; the pipeline's depth bias does that
        // job now, and it moves the comparison rather than the corner.
        for vertex in &v {
            for axis in 0..3 {
                let cell = [CELL.0, CELL.1, CELL.2][axis] as f32;
                let local = vertex.position[axis] - cell;
                assert!(
                    (0.0..=1.0).contains(&local),
                    "a crack vertex left the block's own surface: {:?}",
                    vertex.position
                );
            }
        }

        // Nothing to draw before the first hit lands.
        let (mut v, mut i) = (Vec::new(), Vec::new());
        Mining::new().build_break_mesh_into(7, &mut v, &mut i);
        assert!(v.is_empty() && i.is_empty());
    }

    #[test]
    fn every_overlay_index_points_at_a_real_vertex() {
        // A stray index is a GPU-side crash rather than a wrong pixel,
        // so it is worth asserting rather than eyeballing.
        let mut mining = Mining::new();
        for _ in 0..100 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None);
        }
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(&mut v, &mut i);
        assert!(!i.is_empty());
        assert!(i.iter().all(|&index| (index as usize) < v.len()));
        assert_eq!(i.len() % 3, 0, "indices must form whole triangles");
    }

    #[test]
    fn the_overlay_sits_on_the_block_it_targets() {
        let mut mining = Mining::new();
        mining.update(Some((CELL, BLOCK_PLANKS)), true, 0.5, None);
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(&mut v, &mut i);

        for vertex in &v {
            for axis in 0..3 {
                let low = [CELL.0, CELL.1, CELL.2][axis] as f32 - 0.1;
                let high = low + 1.2;
                assert!(
                    vertex.position[axis] >= low && vertex.position[axis] <= high,
                    "overlay vertex {:?} is not on the target block",
                    vertex.position
                );
            }
        }
    }

    #[test]
    fn every_face_of_the_basis_is_right_handed() {
        // u x v must be the outward normal, or that face's quads are
        // wound backwards and the back-face cull eats them.
        for face in 0..6 {
            let (_, u, v, normal) = face_basis(face);
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            assert_eq!(
                cross, normal,
                "face {face}: u x v = {cross:?} but the normal is {normal:?}"
            );
        }
    }
}
