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

use std::time::Instant;

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

/// How long a block takes to settle after the server says it is there.
///
/// **Short on purpose.** A builder places a row of blocks as fast as
/// they can tap, and an animation that outlasts the next placement
/// stacks into a flicker across the whole wall. A sixth of a second is
/// long enough to be seen and gone before the hand has moved.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(160);

/// How far outside its cell the outline starts before it draws in.
///
/// A tenth of a block. Larger reads as an explosion where a block was
/// laid; smaller is not an animation, it is a flicker. It shrinks to
/// the cell rather than growing out of it because what happened is a
/// thing arriving in a place, not a thing bursting out of one.
const SETTLE_OVERSHOOT: f32 = 0.1;

/// The colour it settles in: the outline's own, lightened.
///
/// Not white. A white flash on a dark wall at night is the brightest
/// thing on the screen for a sixth of a second, and a player laying a
/// hundred blocks would be looking at a strobe.
const SETTLE_COLOR: [f32; 3] = [0.62, 0.60, 0.54];

/// A box as its (min, max) corners, in world space.
type Corners = ([f32; 3], [f32; 3]);

/// Progress against one block.
#[derive(Default)]
pub struct Mining {
    /// The cell being mined, and what is in it.
    target: Option<((i32, i32, i32), BlockId)>,
    /// 0..1.
    progress: f32,
    /// The cell that has just been laid, what went in it, and when the
    /// server said so. See `note_placed`.
    settling: Option<((i32, i32, i32), BlockId, Instant)>,
    /// A cell this player has asked for and the server has not answered
    /// yet. See `await_placement`.
    awaiting: Option<((i32, i32, i32), BlockId)>,
    /// A cairn this player piled that the server has just agreed to, and
    /// that has not yet been asked a name. See `take_piled_cairn`.
    piled_cairn: Option<(i32, i32, i32)>,
    /// The box the outline and the cracks go on, and the cell it was fitted
    /// to, when the world round the target decides it. See `fit_outline`.
    outline: Option<((i32, i32, i32), Corners)>,
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

    /// Fits the outline and the cracks to the target as it stands in the
    /// world, once a frame after `update`.
    ///
    /// **A leaning palm is outlined where it leans.** The box the overlay
    /// used to draw is `geometry::block_target_box`, which sees one block and
    /// no world, and a piece of palm seen that way is an upright post over
    /// the middle of its cell -- a box of cracks beside the bark the player
    /// is hitting. `boxed` is the question asked with the world in reach
    /// (`geometry::block_box_for_aim_near`); anything it answers `None` for
    /// keeps the old box.
    pub fn fit_outline(&mut self, boxed: impl Fn((i32, i32, i32), BlockId) -> Option<Corners>) {
        self.outline = self.target.and_then(|(cell, block)| Some((cell, boxed(cell, block)?)));
    }

    /// The box the outline and the cracks are drawn on.
    fn target_box(&self, cell: (i32, i32, i32), block: BlockId) -> Option<Corners> {
        match self.outline {
            Some((fitted, boxed)) if fitted == cell => Some(boxed),
            _ => primitive_shared::geometry::block_target_box(block, cell.0, cell.1, cell.2),
        }
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
        quality: primitive_shared::quality::Quality,
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
        // **One swing, not one block.** A rock or a soil comes away a
        // quarter at a time now (`dig::swing_seconds`), so the bar fills
        // four times for one block and each filling sends one slice. The
        // total is the same second-count it always was, which is what
        // keeps a copper pick worth exactly what it was worth -- and the
        // arithmetic is in `dig` rather than here, because the server
        // bills the stamina off the same number.
        //
        // The target changes id with every slice, so the `minable !=
        // self.target` test above resets the bar for the next one by
        // itself: there is no per-slice state in this struct at all.
        #[allow(clippy::question_mark)]
        let Some(seconds) = primitive_shared::dig::swing_seconds_made(block, tool, quality) else {
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
    /// Notes that a cell has just become a block, so it can be drawn
    /// settling into place.
    ///
    /// **Told by the server, not by the click.** This client does not
    /// predict edits at all -- see the comment on `try_place_block`:
    /// the world changes when a `BlockUpdate` says it has, which is
    /// what keeps the view honest when the anti-cheat refuses one. An
    /// animation started on the tap would play just as happily for a
    /// block that was never placed, and the one time it lied would be
    /// the one time the player needed to know.
    ///
    /// Only one cell is remembered. A builder laying a row is placing
    /// them faster than the animation lasts, and a queue of them would
    /// draw a wall of pulsing outlines -- which is the opposite of
    /// "should not get in the way of building".
    pub fn note_placed(&mut self, cell: (i32, i32, i32), block: BlockId, at: Instant) {
        self.settling = Some((cell, block, at));
    }

    /// Remembers a cell this player has asked for.
    ///
    /// One, and the newest wins. A builder tapping down a row has the
    /// previous request answered before they finish the next, and a
    /// queue would only matter on a link slow enough that the animation
    /// is the least of it.
    pub fn await_placement(&mut self, cell: (i32, i32, i32), block: BlockId) {
        self.awaiting = Some((cell, block));
    }

    /// Starts the settle if this change is the one that was asked for.
    ///
    /// **Matched rather than assumed, and that is the whole of why this
    /// is not hung on the click.** The world changes for many reasons:
    /// water spreads, fire climbs, crops grow, sand falls, another
    /// player builds across the valley. Animating every cell that
    /// becomes a block would make a flowing river strobe. Animating the
    /// cell *this* player asked for, at the moment the server agrees,
    /// says the one thing worth saying -- "that went where you put it"
    /// -- and says nothing at all when the server refuses.
    /// `at` is handed in rather than read, for the same reason
    /// `build_settle_into` takes one: an animation a sixth of a second
    /// long whose start nobody can name is an animation nobody can
    /// write a test about.
    pub fn confirm_placement(
        &mut self,
        change: &primitive_shared::protocol::BlockChange,
        at: Instant,
    ) {
        let Some((cell, block)) = self.awaiting else {
            return;
        };
        if cell == (change.global_x, change.global_y, change.global_z)
            && block == change.block_id
        {
            self.awaiting = None;
            self.note_placed(cell, block, at);
            // **Asked when the server agrees, not on the tap**, for the
            // settle's reason above: a name typed for a cairn the anticheat
            // refused would be a mark on the map of a heap that is not
            // there.
            if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_CAIRN {
                self.piled_cairn = Some(cell);
            }
        }
    }

    /// The cairn this player has just piled, once: the frame opens the box
    /// that asks its name (`Chat::open_naming`).
    pub fn take_piled_cairn(&mut self) -> Option<(i32, i32, i32)> {
        self.piled_cairn.take()
    }

    /// How far along the settle is, 0..1, or `None` when there is
    /// nothing settling.
    fn settle_progress(&self, now: Instant) -> Option<f32> {
        let (_, _, at) = self.settling?;
        let elapsed = now.saturating_duration_since(at);
        if elapsed >= SETTLE {
            return None;
        }
        Some(elapsed.as_secs_f32() / SETTLE.as_secs_f32())
    }

    pub fn build_overlay_into(
        &self,
        origin: glam::Vec3,
        vertices: &mut Vec<ActorVertex>,
        indices: &mut Vec<u32>,
    ) {
        self.build_settle_into(origin, Instant::now(), vertices, indices);
        let Some((cell, block)) = self.target else {
            return;
        };
        // The box the ray actually stopped at, not the cell it is in.
        // A metre cube drawn around a blade of grass is a box around
        // mostly nothing, and it lies about what a click will hit.
        let Some((min, max)) = self.target_box(cell, block) else {
            return;
        };
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        let min = [min[0] - origin.x, min[1] - origin.y, min[2] - origin.z];

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
        origin: glam::Vec3,
        layer: u32,
        // The pictures' table, for the one shape that is read out of a
        // picture: a stone's thickness (`engine::relief`). The cracks have to
        // lie on the stone the mesher drew, and that stone is whatever
        // picture was loaded -- a resource pack's pebble included.
        layers: &crate::engine::texture::FaceLayers,
        // How far the lid of the chest under the crosshair is up, if the
        // frame is drawing it up (`ChunkManager::open_lids`). See
        // `model_faces` for why the cracks have to know.
        lid: Option<f32>,
        vertices: &mut Vec<crate::engine::mesh::Vertex>,
        indices: &mut Vec<u32>,
    ) {
        let Some((cell, block)) = self.target else {
            return;
        };
        if self.break_stage().is_none() {
            return;
        }
        // **A flat thing cracks where it lies**, lowered onto a lip under it
        // as the mesher draws it (`types::rest_drop`). The fitted box already
        // knows how far, having been asked with the world round it
        // (`fit_outline`), and a flat box starts at the floor it lies on.
        let drop = if primitive_shared::types::is_flat(block) {
            self.target_box(cell, block).map_or(0.0, |(min, _)| (cell.1 as f32 - min[1]).max(0.0))
        } else {
            0.0
        };
        let at = [
            cell.0 as f32 - origin.x,
            cell.1 as f32 - origin.y - drop,
            cell.2 as f32 - origin.z,
        ];

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
        // **A carcass cracks along the animal, not along its table row.**
        // The row is a slab three eighths high -- what the collider
        // stands on -- but what is drawn is the animal lying on its side
        // (`animal_model::build_fallen`). Cracks drawn on the row's box
        // lay *under* the model as a pale plate in the block's table
        // picture, and a player who hit a boar saw a light square appear
        // beneath it; so the cracks are drawn on the model's own quads,
        // from the same pose and the same yaw the mesher used.
        if let Some(species) = primitive_shared::animals::Species::of_carcass(block) {
            let mut quads = Vec::new();
            crate::logic::animal_model::fallen_quads(
                species,
                glam::Vec3::new(at[0] + 0.5, at[1], at[2] + 0.5),
                crate::logic::animal_model::carcass_yaw(cell.0, cell.1, cell.2),
                &mut quads,
            );
            for quad in quads {
                cracks_on_quad(quad, layer, vertices, indices);
            }
            return;
        }

        // ...and a dead player the same way, for the same reason: a body
        // is the figure lying on its side (`player_model::build_fallen`)
        // and its table row is the low slab underneath it. Not left to
        // `model_faces` below, which asks the *mesher* for a model's
        // quads: what a body is drawn with depends on the cell it lies
        // in, through the yaw hash, and that is an argument the mesher's
        // per-block builders do not take.
        if let Some(stage) = crate::logic::player_model::Dead::of(block) {
            let mut quads = Vec::new();
            crate::logic::player_model::fallen_quads(
                stage,
                glam::Vec3::new(at[0] + 0.5, at[1], at[2] + 0.5),
                crate::logic::animal_model::carcass_yaw(cell.0, cell.1, cell.2),
                &mut quads,
            );
            for quad in quads {
                cracks_on_quad(quad, layer, vertices, indices);
            }
            return;
        }

        if primitive_shared::types::is_cross(block) {
            for plane in crate::engine::mesh::cross_planes([cell.0, cell.1, cell.2], at, block) {
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
        // **A stone cracks on its own surface**, top and sides, and not on
        // the flat square it used to be. The exact surface rather than the
        // drawn one: the drawn one leans on the cut-out to hide spans standing
        // in the air, and a crack discards nothing -- it would have darkened
        // the grass beside the stone. See `engine::relief`.
        //
        // **At the picture's scale, not the block's**: the whole crack
        // picture across the stone's picture, as the flat quad had it. Cut by
        // position like a model's part (`cracks_on_model_face`), a stone a
        // quarter of a block across would show a corner of the pattern, and
        // for most stones that corner holds no crack until the last stage.
        if let Some(relief) = layers.relief(block) {
            use crate::engine::relief::Relief;
            let inset = primitive_shared::types::flat_inset(block);
            let turn = Relief::turn_of([cell.0, cell.1, cell.2]);
            // Full brightness, for the reason `cracks_on_face` gives.
            let light = crate::engine::mesh::pack_light(15, 0, 3, 0);
            for facet in &relief.exact {
                let (corners, _) = Relief::placed(facet, at, inset, turn);
                let base = vertices.len() as u32;
                for (corner, uv) in corners.into_iter().zip(facet.uv) {
                    vertices.push(crate::engine::mesh::Vertex::new(corner, [0.0, 0.0], layer, light).with_fine_uv(uv));
                }
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            return;
        }
        if primitive_shared::types::is_flat(block) {
            cracks_on_quad(
                crate::engine::mesh::flat_quad(at, block),
                layer,
                vertices,
                indices,
            );
            return;
        }

        // **A model cracks on its parts.** A table, a bed, a stool, a jug, a
        // barrel are boxes -- boards, legs, a blanket -- and they used to get
        // the cracks of the one box their row is aimed at. The pipeline
        // multiplies and tests depth against what is drawn, so on the parts
        // that box was *cut*: a leg two sixteenths wide showed a sliver of
        // the pattern, a board's edge a strip, and a small part never showed
        // the damage getting worse at all. Asked to choose between cutting
        // the pattern to the part and squeezing it onto it, the player chose
        // squeezing -- so each face ran the crack picture corner to corner.
        //
        // **And then asked for the opposite** ("не сжимай текстуру удаления
        // а обрезай ее"), having seen it: a crack pattern squeezed onto a
        // leg is sixteen texels of damage in two, a smear of dark lines at
        // eight times the density of the same damage on the seat beside it,
        // and the cracks stopped reading as one blow landing on one object.
        // So the crack is cut from its picture exactly as the wood under it
        // is (`cracks_on_model_face`): a thin part shows a sliver of the
        // pattern, and the sliver is at the same scale as everywhere else.
        // The cost the first choice was made to avoid is real and accepted:
        // a part too small to hold a crack shows the damage late.
        if let Some(faces) = model_faces(block, cell, at, lid) {
            for face in faces {
                cracks_on_model_face(face, layer, vertices, indices);
            }
            return;
        }

        let Some((min, max)) = self.target_box(cell, block) else {
            return;
        };
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        // Where the box sits in its own cell, for the crop.
        let within = [min[0] - cell.0 as f32, min[1] - cell.1 as f32, min[2] - cell.2 as f32];
        let min = [min[0] - origin.x, min[1] - origin.y, min[2] - origin.z];
        for face in 0..6 {
            cracks_on_face(min, size, within, face, layer, vertices, indices);
        }
    }
}

/// One face of a model as the mesher drew it: four corners, each with the
/// place in the block's picture it wears.
type ModelFace = [([f32; 3], [f32; 2]); 4];

/// The faces the mesher draws for `block` at `at`, if it draws it as boxes
/// rather than as a cube, a cross or a flat quad -- and `None` otherwise.
///
/// **The mesher's own functions, called again**, for `cross_planes`'s
/// reason: cracks drawn on a copy of a model's shape drift from the model
/// the first time either is edited. Two things the mesher knows are not
/// known here, and neither moves a face a player can see: whether a bed has
/// its other half beside it (the seam it leaves open is inside the bed), and
/// the light, which the crack does not use.
///
/// The picture table is the empty one, because a crack wants the corners
/// and not the pictures: every layer answers zero, and the geometry of no
/// model depends on which layer it wears. Branches are not here: which way
/// a piece reaches is read off its neighbours, which only the mesher has.
///
/// **A chest with its lid up is drawn in two pieces, and cracked in the same
/// two.** The frame draws an open lid itself and the chunk mesh leaves it out
/// (`mesh::Hinged`); asked for the whole chest, this drew the cracks on the
/// lid *shut* -- a lid of cracks lying across the open mouth of the box, over
/// the things the player was looking at inside it. `lid` is the swing the
/// frame is drawing, and the body and the lid come from the same two calls it
/// makes.
fn model_faces(block: BlockId, cell: (i32, i32, i32), at: [f32; 3], lid: Option<f32>) -> Option<Vec<ModelFace>> {
    use crate::engine::mesh;
    use primitive_shared::types as t;
    static LAYERS: std::sync::OnceLock<crate::engine::texture::FaceLayers> = std::sync::OnceLock::new();
    let layers = LAYERS.get_or_init(crate::engine::texture::FaceLayers::empty_for_test);
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    let kind = t::block_kind(block);
    if matches!(kind, t::BLOCK_DRYING_RACK | t::BLOCK_HIDE_FRAME) {
        mesh::rack_block(at, block, mesh::RackColumns::Lone, layers, 0xFF, &mut vertices, &mut indices);
    } else if kind == t::BLOCK_JUG {
        mesh::jug_block(at, block, layers, 0xFF, &mut vertices, &mut indices);
    } else if t::is_barrel(block) {
        mesh::barrel_block(at, block, layers, 0xFF, &mut vertices, &mut indices);
    } else if kind == t::BLOCK_BRACKET_FUNGUS {
        mesh::bracket_block(at, block, layers, 0xFF, &mut vertices, &mut indices);
    } else if primitive_shared::dripstone::is_dripstone(block) {
        mesh::dripstone_block(at, block, layers, 0xFF, &mut vertices, &mut indices);
    } else if matches!(kind, t::BLOCK_NEST | t::BLOCK_NEST_EGGS) {
        mesh::nest_block(at, block, layers, 0xFF, &mut vertices, &mut indices);
    } else if kind == t::BLOCK_CAIRN {
        mesh::cairn_block(at, block, layers, 0xFF, &mut vertices, &mut indices);
    } else if let Some(species) = t::species_in_bones(block) {
        crate::logic::animal_model::build_bones(
            species,
            glam::Vec3::new(at[0] + 0.5, at[1], at[2] + 0.5),
            crate::logic::animal_model::carcass_yaw(cell.0, cell.1, cell.2),
            layers,
            (15, 0),
            &mut vertices,
            &mut indices,
        );
    } else if let (true, Some(angle)) = (kind == t::BLOCK_CHEST, lid) {
        mesh::furniture_block_hinged(at, block, false, mesh::Hinged::Bodied, layers, 0xFF, &mut vertices, &mut indices);
        mesh::chest_lid_block(at, block, angle, layers, 0xFF, &mut vertices, &mut indices);
    } else if primitive_shared::lean_to::is_lean_to(block) {
        // A cell of a lean-to cracks as the box round the thatch it holds:
        // its model is the whole hut, drawn by one cell, and cracks over all
        // fifteen for a blow at one would be a hut breaking where it was not
        // hit.
        return None;
    } else if mesh::is_furniture(block) {
        mesh::furniture_block(at, block, false, layers, 0xFF, &mut vertices, &mut indices);
    } else {
        return None;
    }
    // Every model is written as quads of four corners and six indices. A
    // model that ever is not falls back to the aimed box rather than
    // pairing corners from two different faces.
    if vertices.len() % 4 != 0 || indices.len() != vertices.len() / 4 * 6 {
        return None;
    }
    Some(
        vertices
            .chunks_exact(4)
            .map(|q| [0, 1, 2, 3].map(|n| (q[n].position, q[n].uv())))
            .collect(),
    )
}

/// Cracks over one face of a model, cut from the crack picture by where the
/// face lies in space rather than by the picture it wears.
///
/// **Not the face's own picture coordinates**, though that was the first
/// try: a model's wood is not always cut -- a table top ten sixteenths
/// across wears the whole plank picture -- so cracks following the wood
/// were squeezed wherever the wood was. Read off position instead, the
/// crack is at one scale on every part: the face is measured in blocks
/// along its two widest axes, from the whole block below its lowest corner,
/// exactly as `cracks_on_face` does for a box. See the note in
/// `build_break_mesh_into` for why it used to be stretched instead.
fn cracks_on_model_face(face: ModelFace, layer: u32, vertices: &mut Vec<crate::engine::mesh::Vertex>, indices: &mut Vec<u32>) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for (corner, _) in face {
        for axis in 0..3 {
            lo[axis] = lo[axis].min(corner[axis]);
            hi[axis] = hi[axis].max(corner[axis]);
        }
    }
    // The axis the face spans least is the one it faces along; the other
    // two carry the picture, with the vertical one running down the image.
    let facing = (0..3)
        .min_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
        .unwrap_or(1);
    let (across, up) = match facing {
        0 => (2, 1),
        2 => (0, 1),
        _ => (0, 2),
    };
    let from = [lo[across].floor(), lo[up].floor()];
    let base = vertices.len() as u32;
    // Full brightness, for the reason `cracks_on_face` gives.
    let light = crate::engine::mesh::pack_light(15, 0, 3, 0);
    for (corner, _) in face {
        let u = corner[across] - from[0];
        let v = corner[up] - from[1];
        // Up the world is up the picture on a wall; on a floor either way
        // reads the same.
        let v = if up == 1 { 1.0 - v } else { v };
        // Fine, not whole cells: `Vertex::new` rounds a coordinate to a
        // count of pictures, which turned every part back into the whole
        // picture corner to corner.
        vertices.push(crate::engine::mesh::Vertex::new(corner, [0.0, 0.0], layer, light).with_fine_uv([u, v]));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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
        // Flat: an outline and its cracks carry no picture, and the
        // actor pipeline reads that off the texture coordinate. See
        // `ActorVertex::flat`.
        vertices.push(ActorVertex::flat(
            [
                origin[0] + local[0] * size[0] + normal[0] * SURFACE_OFFSET,
                origin[1] + local[1] * size[1] + normal[1] * SURFACE_OFFSET,
                origin[2] + local[2] * size[2] + normal[2] * SURFACE_OFFSET,
            ],
            color,
            normal,
        ));
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

/// The same four edges in a colour of the caller's choosing.
///
/// Split out rather than adding a parameter to `outline_face`, because
/// the crosshair's box is one colour by definition and the settle is
/// the only thing that fades.
fn outline_face_in(
    origin: [f32; 3],
    size: [f32; 3],
    face: usize,
    colour: [f32; 3],
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
        push_face_quad(origin, size, face, edge, colour, vertices, indices);
    }
}

impl Mining {
    /// Draws the block that has just been laid, shrinking into its cell.
    ///
    /// Separate from `build_overlay_into` so a test can hand it a clock
    /// rather than reading one: an animation whose only witness is a
    /// person watching for a sixth of a second is an animation nobody
    /// can check twice.
    fn build_settle_into(
        &self,
        origin: glam::Vec3,
        now: Instant,
        vertices: &mut Vec<ActorVertex>,
        indices: &mut Vec<u32>,
    ) {
        let Some(t) = self.settle_progress(now) else {
            return;
        };
        let Some((cell, block, _)) = self.settling else {
            return;
        };
        let Some((min, max)) =
            primitive_shared::geometry::block_target_box(block, cell.0, cell.1, cell.2)
        else {
            return;
        };
        // Eased so most of the travel is in the first half: a linear
        // shrink reads as a box sliding, and this has to read as a
        // thing coming to rest.
        let out = SETTLE_OVERSHOOT * (1.0 - t) * (1.0 - t);
        let size = [
            max[0] - min[0] + out * 2.0,
            max[1] - min[1] + out * 2.0,
            max[2] - min[2] + out * 2.0,
        ];
        let min = [
            min[0] - out - origin.x,
            min[1] - out - origin.y,
            min[2] - out - origin.z,
        ];
        // Fades as it closes, so the last frame of it is already gone
        // rather than snapping off.
        let fade = 1.0 - t;
        let colour = [
            SETTLE_COLOR[0] * fade,
            SETTLE_COLOR[1] * fade,
            SETTLE_COLOR[2] * fade,
        ];
        for face in 0..6 {
            outline_face_in(min, size, face, colour, vertices, indices);
        }
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

/// `within` is where the box's lowest corner sits inside its cell, 0..1 on
/// each axis.
///
/// **The picture is cut, not stretched**: each corner takes the crack
/// texel at its own place in the cell, so a layer of snow a quarter high
/// shows the bottom quarter of the cracks at full scale rather than the
/// whole picture squashed into a strip. "не сжимай текстуру удаления а
/// обрезай ее" -- a squashed crack is four times as dense one way as the
/// other, and reads as a scribble rather than as damage.
fn cracks_on_face(
    origin: [f32; 3],
    size: [f32; 3],
    within: [f32; 3],
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
        // The corner on the unit cube of the box, in the cell, and then
        // along each of the face's two axes -- which are unit axes of the
        // cell, so the dot product picks one coordinate out.
        let unit: [f32; 3] = std::array::from_fn(|i| face_origin[i] + u[i] * cu + v[i] * cv);
        let in_cell: [f32; 3] = std::array::from_fn(|i| within[i] + unit[i] * size[i]);
        let along = |axis: [f32; 3]| -> f32 { (0..3).map(|i| axis[i] * in_cell[i]).sum() };
        // v = 0 is the top of the image, and the face basis runs v
        // upwards, so the two are flipped against each other. Read off the
        // corner's place in the cell rather than in the box: see the note
        // above.
        let uv = [along(u).clamp(0.0, 1.0), (1.0 - along(v)).clamp(0.0, 1.0)];
        vertices.push(
            crate::engine::mesh::Vertex::new(
                // Exactly on the face. See `SURFACE_OFFSET`.
                std::array::from_fn(|i| origin[i] + unit[i] * size[i]),
                [0.0, 0.0],
                layer,
                light,
            )
            // A place in the picture, which `new` would round to a whole
            // one. See `Vertex::with_fine_uv`.
            .with_fine_uv(uv),
        );
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod settle_tests {
    use super::*;
    use primitive_shared::types::BLOCK_STONE;
    use primitive_shared::protocol::BlockChange;

    fn change(cell: (i32, i32, i32), block: BlockId) -> BlockChange {
        BlockChange {
            global_x: cell.0,
            global_y: cell.1,
            global_z: cell.2,
            block_id: block,
        }
    }

    fn quads(mining: &Mining, at: Instant) -> usize {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        mining.build_settle_into(glam::Vec3::ZERO, at, &mut vertices, &mut indices);
        vertices.len()
    }

    /// A block settles when the server agrees, not when it is asked for.
    ///
    /// **The client predicts no edit at all** -- the world changes on a
    /// `BlockUpdate` and not before, which is what keeps the view
    /// honest when the anti-cheat refuses one. An animation started on
    /// the tap would play just as brightly for a block that was never
    /// placed, and the once it lied would be the once it mattered.
    #[test]
    fn nothing_settles_until_the_server_says_the_block_is_there() {
        let now = Instant::now();
        let mut mining = Mining::new();
        let cell = (4, 5, 6);

        mining.await_placement(cell, BLOCK_STONE);
        assert_eq!(quads(&mining, now), 0, "the ask alone drew something");

        mining.confirm_placement(&change(cell, BLOCK_STONE), now);
        assert!(quads(&mining, now) > 0, "the confirmation drew nothing");
    }

    /// ...and the world going about its business does not.
    ///
    /// Water spreads, fire climbs, crops grow and sand falls, and every
    /// one of those turns a cell into a block. Animating all of them
    /// would make a flowing river strobe. The rule is that this player
    /// asked for *this* cell.
    #[test]
    fn a_block_appearing_somewhere_else_does_not_settle() {
        let now = Instant::now();
        let mut mining = Mining::new();
        mining.await_placement((4, 5, 6), BLOCK_STONE);

        mining.confirm_placement(&change((4, 5, 7), BLOCK_STONE), now);
        assert_eq!(quads(&mining, now), 0, "a neighbouring cell settled");

        // ...and neither does the right cell turning into the wrong
        // thing, which is what a refused edit corrected by the server
        // looks like from here.
        mining.confirm_placement(&change((4, 5, 6), BLOCK_STONE + 1), now);
        assert_eq!(quads(&mining, now), 0, "a different block settled");
    }

    /// It is over quickly, and it stops rather than thinning forever.
    ///
    /// A builder lays a row as fast as they can tap. An animation that
    /// outlasts the next placement stacks into a flicker across the
    /// whole wall, which is the opposite of "must not get in the way of
    /// building".
    #[test]
    fn the_settle_is_over_before_the_next_block_is_laid() {
        let start = Instant::now();
        let mut mining = Mining::new();
        mining.await_placement((0, 0, 0), BLOCK_STONE);
        mining.confirm_placement(&change((0, 0, 0), BLOCK_STONE), start);

        assert!(quads(&mining, start) > 0, "it never started");
        assert!(
            quads(&mining, start + SETTLE / 2) > 0,
            "it ended halfway through itself",
        );
        assert_eq!(
            quads(&mining, start + SETTLE),
            0,
            "it was still being drawn after it was over",
        );
        assert_eq!(
            quads(&mining, start + SETTLE * 4),
            0,
            "it came back",
        );
    }

    /// It closes on the block rather than opening out of it.
    ///
    /// What happened is a thing arriving in a place. A box growing
    /// outward reads as something bursting out of the ground, which is
    /// a different event and not the one that occurred.
    #[test]
    fn the_outline_draws_in_towards_the_block_it_is_around() {
        let start = Instant::now();
        let mut mining = Mining::new();
        mining.await_placement((0, 0, 0), BLOCK_STONE);
        mining.confirm_placement(&change((0, 0, 0), BLOCK_STONE), start);

        let span = |at: Instant| {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            mining.build_settle_into(glam::Vec3::ZERO, at, &mut vertices, &mut indices);
            let xs: Vec<f32> = vertices.iter().map(|v| v.position[0]).collect();
            xs.iter().cloned().fold(f32::MIN, f32::max)
                - xs.iter().cloned().fold(f32::MAX, f32::min)
        };

        let first = span(start);
        let later = span(start + SETTLE / 2);
        assert!(
            first > later,
            "the outline grew instead of closing: {first} then {later}",
        );
        assert!(
            first > 1.0,
            "it never started outside the cell it is settling into: {first}",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::quality::Quality;
    // Planks rather than stone: stone cannot be broken by hand at all now
// (see `types::break_seconds`), so the slowest thing a bare hand can
// still get through is worked wood.
use primitive_shared::types::{BLOCK_DIRT, BLOCK_PLANKS, BLOCK_WATER};

    const CELL: (i32, i32, i32) = (3, 4, 5);
    const OTHER: (i32, i32, i32) = (9, 4, 5);

    /// Mines until it breaks, capped so a bug cannot hang the test.
    fn mine_to_completion(mining: &mut Mining, cell: (i32, i32, i32), block: BlockId) -> Option<(i32, i32, i32)> {
        for _ in 0..10_000 {
            if let Some(broken) = mining.update(Some((cell, block)), true, 1.0 / 60.0, None, Quality::PLAIN) {
                return Some(broken);
            }
        }
        None
    }

    #[test]
    fn a_rock_face_waits_for_a_pick_and_then_gives_way() {
        use primitive_shared::types::{BLOCK_WEDGED_AXE, BLOCK_WEDGED_PICKAXE, BLOCK_STONE};
        // Bare-handed, stone is not a target at all: no progress, no
        // cracks, and no half-full bar to make the player think the
        // swing was doing something.
        let mut hands = Mining::new();
        for _ in 0..600 {
            assert_eq!(
                hands.update(Some((CELL, BLOCK_STONE)), true, 1.0 / 60.0, None, Quality::PLAIN),
                None
            );
        }
        assert_eq!(hands.progress(), 0.0);
        assert_eq!(hands.break_stage(), None);

        // With a wedged pick it is ordinary work...
        let mut flint = Mining::new();
        let mut frames = 0;
        let broke = loop {
            frames += 1;
            if let Some(cell) = flint.update(
                Some((CELL, BLOCK_STONE)),
                true,
                1.0 / 60.0,
                Some(BLOCK_WEDGED_PICKAXE),
                Quality::PLAIN,
            ) {
                break cell;
            }
            assert!(frames < 10_000, "a wedged pick never got through stone");
        };
        assert_eq!(broke, CELL);
        assert!(frames > 1, "a wedged pick went through rock instantly");

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
                    Some(BLOCK_WEDGED_AXE),
                    Quality::PLAIN,
                ),
                None
            );
        }
        assert_eq!(axe.progress(), 0.0);
    }

    #[test]
    fn an_axe_brings_a_standing_tree_down_and_a_pick_does_not() {
        use primitive_shared::types::{BLOCK_WEDGED_AXE, BLOCK_WEDGED_PICKAXE, BLOCK_LOG};
        let mut hands = Mining::new();
        for _ in 0..600 {
            assert_eq!(
                hands.update(Some((CELL, BLOCK_LOG)), true, 1.0 / 60.0, None, Quality::PLAIN),
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
                    Some(BLOCK_WEDGED_PICKAXE),
                    Quality::PLAIN,
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
                    Some(BLOCK_WEDGED_AXE),
                    Quality::PLAIN,
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
        use primitive_shared::types::BLOCK_WEDGED_PICKAXE;
        let mut mining = Mining::new();
        for _ in 0..10 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
        }
        let before = mining.progress();
        assert!(before > 0.0);
        mining.update(
            Some((CELL, BLOCK_PLANKS)),
            true,
            1.0 / 60.0,
            Some(BLOCK_WEDGED_PICKAXE),
            Quality::PLAIN,
        );
        assert!(mining.progress() > before);
    }

    #[test]
    fn a_block_does_not_break_instantly() {
        let mut mining = Mining::new();
        assert_eq!(
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN),
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
        assert_eq!(mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN), None);
    }

    #[test]
    fn a_rock_is_taken_apart_in_four_swings_and_the_four_cost_what_one_used_to() {
        // **The bar fills once per slice and the dig is the same length.**
        // A block that comes away in quarters (`dig::SLICES`) fills the bar
        // four times, each in a quarter of the seconds the whole block
        // always cost, and this is the arithmetic that says progression is
        // untouched: the number a copper pick improves on has not moved.
        //
        // The frames are counted rather than the seconds, so a change to
        // either end of `swing_seconds` shows up here as a count that is
        // not four times the slice.
        use primitive_shared::dig;
        use primitive_shared::types::{BLOCK_STONE, BLOCK_WEDGED_PICKAXE};
        const DT: f32 = 1.0 / 600.0;
        let pick = Some(BLOCK_WEDGED_PICKAXE);
        let frames_for = |block: BlockId| {
            let mut mining = Mining::new();
            let mut frames = 0;
            for _ in 0..200_000 {
                frames += 1;
                if mining.update(Some((CELL, block)), true, DT, pick, Quality::PLAIN).is_some() {
                    return frames;
                }
            }
            panic!("a swing at {block:#x} never finished");
        };
        let mut block = BLOCK_STONE;
        let mut swings = 0;
        let mut frames = 0;
        loop {
            swings += 1;
            frames += frames_for(block);
            match dig::next_bite(block, dig::Side::PosX) {
                Some(next) => block = next,
                None => break,
            }
        }
        assert_eq!(swings, usize::from(dig::SLICES), "stone came apart in {swings} swings");
        let whole = primitive_shared::types::break_seconds_with(BLOCK_STONE, pick).unwrap();
        let dug = frames as f32 * DT;
        assert!(
            (dug - whole).abs() < 0.05,
            "four swings took {dug}s and the block has always taken {whole}s"
        );
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
            soft.update(Some((CELL, BLOCK_DIRT)), true, 1.0 / 60.0, None, Quality::PLAIN);
            hard.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
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
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
        }
        assert!(mining.progress() > 0.0);
        mining.update(Some((CELL, BLOCK_PLANKS)), false, 1.0 / 60.0, None, Quality::PLAIN);
        assert_eq!(mining.progress(), 0.0);
        assert_eq!(mining.target(), None);
    }

    #[test]
    fn looking_at_a_different_block_starts_over() {
        let mut mining = Mining::new();
        for _ in 0..20 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
        }
        let partway = mining.progress();
        assert!(partway > 0.0);

        mining.update(Some((OTHER, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
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
            assert_eq!(mining.update(Some((CELL, BLOCK_WATER)), true, 1.0 / 60.0, None, Quality::PLAIN), None);
        }
        assert_eq!(mining.target(), None, "water should never become a target");
    }

    #[test]
    fn aiming_at_nothing_is_not_a_target() {
        let mut mining = Mining::new();
        assert_eq!(mining.update(None, true, 1.0 / 60.0, None, Quality::PLAIN), None);
        assert_eq!(mining.target(), None);
    }

    #[test]
    fn there_is_no_overlay_without_a_target() {
        let mining = Mining::new();
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(glam::Vec3::ZERO, &mut v, &mut i);
        assert!(v.is_empty() && i.is_empty());
    }

    #[test]
    fn a_fresh_target_is_outlined_but_uncracked() {
        let mut mining = Mining::new();
        mining.update(Some((CELL, BLOCK_PLANKS)), true, 0.0, None, Quality::PLAIN);
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(glam::Vec3::ZERO, &mut v, &mut i);
        // Six faces, four outline bars each, six indices per bar.
        assert_eq!(i.len(), 6 * 4 * 6, "expected only the outline");
        assert!(!v.is_empty());
    }

    #[test]
    fn the_cracks_deepen_as_the_block_gives_way() {
        let mut early = Mining::new();
        let mut late = Mining::new();
        early.update(Some((CELL, BLOCK_PLANKS)), true, 0.05, None, Quality::PLAIN);
        for _ in 0..100 {
            late.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
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
        mining.update(Some((CELL, BLOCK_PLANKS)), true, 0.0, None, Quality::PLAIN);
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
    fn the_crack_overlay_is_cut_to_each_part_of_a_model_at_one_scale() {
        // **Cut, not squeezed.** The player was once asked, chose squeezed,
        // saw it, and asked for it cut ("не сжимай текстуру удаления а
        // обрезай ее"). So a face wears as much of the crack picture as it
        // is large: a part half a block tall shows half the pattern, never
        // the whole of it pressed flat.
        use primitive_shared::types::BLOCK_TABLE;
        let mut mining = Mining::new();
        mining.update(Some((CELL, BLOCK_TABLE)), true, 1.0 / 60.0, None, Quality::PLAIN);
        assert!(mining.break_stage().is_some(), "no damage to draw");
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_break_mesh_into(glam::Vec3::ZERO, 7, &crate::engine::texture::FaceLayers::empty_for_test(), None, &mut v, &mut i);
        assert!(
            v.len() / 4 > 6,
            "the table got the six faces of a box instead of its parts: {} quads",
            v.len() / 4
        );
        assert_eq!(i.len(), v.len() / 4 * 6);
        let mut small = 0;
        for quad in v.chunks_exact(4) {
            assert!(quad.iter().all(|vertex| vertex.tex_layer() == 7), "a face sampled another layer");
            let extent = |axis: usize| {
                let values = quad.iter().map(|vertex| vertex.position[axis]);
                values.clone().fold(f32::MIN, f32::max) - values.fold(f32::MAX, f32::min)
            };
            let largest = (0..3).map(extent).fold(0.0, f32::max);
            let span: [f32; 2] = std::array::from_fn(|axis| {
                let values = quad.iter().map(|vertex| vertex.uv()[axis]);
                values.clone().fold(f32::MIN, f32::max) - values.fold(f32::MAX, f32::min)
            });
            // No stretch: neither way does the picture cover more of itself
            // than the face covers of a block (a sixteenth of slack, for the
            // texel the crop is snapped to).
            for worn in span {
                assert!(
                    worn <= largest + 1.0 / 16.0 + 1e-3,
                    "a face at most {largest:.3} across wears {worn:.3} of the cracks: squeezed"
                );
            }
            small += usize::from(largest > 0.0 && largest <= 0.5);
        }
        assert!(small > 0, "no part of the table was half a block or less, so nothing here could have been squeezed");
    }

    #[test]
    fn a_layer_of_snow_shows_the_bottom_of_the_cracks_rather_than_all_of_them_flattened() {
        use primitive_shared::types::{with_layers, BLOCK_SNOW};
        let snow = with_layers(BLOCK_SNOW, 2);
        let mut mining = Mining::new();
        mining.update(Some((CELL, snow)), true, 1.0 / 60.0, None, Quality::PLAIN);
        assert!(mining.break_stage().is_some(), "no damage to draw");
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_break_mesh_into(glam::Vec3::ZERO, 7, &crate::engine::texture::FaceLayers::empty_for_test(), None, &mut v, &mut i);
        assert!(!v.is_empty(), "the snow drew no cracks");
        let mut sides = 0;
        for quad in v.chunks_exact(4) {
            let ys = quad.iter().map(|vertex| vertex.position[1]);
            let height = ys.clone().fold(f32::MIN, f32::max) - ys.fold(f32::MAX, f32::min);
            if height < 1e-4 {
                continue;
            }
            sides += 1;
            let vs = quad.iter().map(|vertex| vertex.uv()[1]);
            let (lo, hi) = (vs.clone().fold(f32::MAX, f32::min), vs.fold(f32::MIN, f32::max));
            assert!((hi - lo - height).abs() < 1e-3, "a side {height} tall wears {lo}..{hi} of the cracks");
            // The bottom of the picture: v runs down the image.
            assert!((hi - 1.0).abs() < 1e-3, "the side wears {lo}..{hi}, not the picture's bottom");
        }
        assert_eq!(sides, 4, "a layer of snow has four sides");
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
            mining.update(Some((CELL, block)), true, 1.0 / 60.0, None, Quality::PLAIN);
            assert!(mining.break_stage().is_some(), "no damage to draw");
            let (mut v, mut i) = (Vec::new(), Vec::new());
            mining.build_break_mesh_into(glam::Vec3::ZERO, 7, &crate::engine::texture::FaceLayers::empty_for_test(), None, &mut v, &mut i);
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

        // A stone cracks on the stone: every face of the surface the mesher
        // stands up out of its picture (`engine::relief`), not the six faces
        // of a wafer -- and, since the stone got a thickness, not the one flat
        // quad either, which lay under the stone's top and was hidden by it.
        let (pebble_quads, pebble) = quads(BLOCK_PEBBLE);
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let relief = layers.relief(BLOCK_PEBBLE).expect("a pebble has a thickness");
        assert_eq!(pebble_quads, relief.exact.len(), "a stone cracked on something other than its surface");

        // A carcass cracks along the animal: one quad per face of every
        // part of the model, standing as tall as the animal is wide, and
        // never the six faces of the slab its table row describes.
        let boar = primitive_shared::types::BLOCK_CARCASS_BOAR;
        let (boar_quads, body) = quads(boar);
        let parts = crate::logic::animal_model::parts(primitive_shared::animals::Species::Boar).len();
        assert_eq!(boar_quads, parts * 6, "a carcass got a box instead of the animal");
        let low = body.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let high = body.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        assert!(high - low > 0.5, "the cracks lie flat under the animal: {} tall", high - low);
        // ...and inside what is aimed at, which is what a crack is the
        // inverse of: a crack corner outside the aim box is damage drawn
        // where the stone cannot be hit.
        let (lo, hi) = primitive_shared::geometry::block_box_for_aim(BLOCK_PEBBLE, CELL.0, CELL.1, CELL.2, false)
            .expect("a pebble can be aimed at");
        for vertex in &pebble {
            for axis in 0..3 {
                assert!(
                    (lo[axis] - 1e-4..=hi[axis] + 1e-4).contains(&vertex.position[axis]),
                    "a crack on the stone left its aim box on axis {axis}: {:?} outside {lo:?}..{hi:?}",
                    vertex.position
                );
            }
        }
        let tallest = pebble.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max) - CELL.1 as f32;
        assert!(tallest > 0.05, "the stone's cracks lie flat under a stone that stands up: {tallest}");

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
    fn the_cracks_on_an_open_chest_are_on_the_lid_where_the_lid_is() {
        // The report: the mining overlay drew the lid shut while it stood
        // open -- a lid of cracks lying across the open box. The cracks come
        // from the same two pieces the frame draws an open chest in.
        use primitive_shared::types::BLOCK_CHEST;
        let mut mining = Mining::new();
        for _ in 0..100 {
            mining.update(Some((CELL, BLOCK_CHEST)), true, 1.0 / 60.0, None, Quality::PLAIN);
        }
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let cracks = |lid: Option<f32>| {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            mining.build_break_mesh_into(glam::Vec3::ZERO, 7, &layers, lid, &mut v, &mut i);
            assert!(!v.is_empty(), "no cracks on a chest");
            v
        };
        // The same drawing the frame makes of the chest, for comparison.
        let drawn = |lid: Option<f32>| {
            let at = [CELL.0 as f32, CELL.1 as f32, CELL.2 as f32];
            let (mut v, mut i) = (Vec::new(), Vec::new());
            match lid {
                Some(angle) => {
                    crate::engine::mesh::furniture_block_hinged(at, BLOCK_CHEST, false, crate::engine::mesh::Hinged::Bodied, &layers, 0xFF, &mut v, &mut i);
                    crate::engine::mesh::chest_lid_block(at, BLOCK_CHEST, angle, &layers, 0xFF, &mut v, &mut i);
                }
                None => crate::engine::mesh::furniture_block(at, BLOCK_CHEST, false, &layers, 0xFF, &mut v, &mut i),
            }
            v
        };
        let top = |v: &[crate::engine::mesh::Vertex]| v.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        let open = crate::engine::mesh::LID_OPEN;
        let (shut, up) = (cracks(None), cracks(Some(open)));
        assert!((top(&shut) - top(&drawn(None))).abs() < 1e-4, "the shut chest is not cracked where it is drawn");
        assert!(
            (top(&up) - top(&drawn(Some(open)))).abs() < 1e-4,
            "the open lid is drawn up to {} and cracked up to {}",
            top(&drawn(Some(open))),
            top(&up)
        );
        assert!(top(&up) > top(&shut) + 0.1, "the cracks did not go up with the lid");
    }

    #[test]
    fn the_crack_overlay_covers_every_face_of_the_block() {
        let mut mining = Mining::new();
        for _ in 0..100 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
        }
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_break_mesh_into(glam::Vec3::ZERO, 7, &crate::engine::texture::FaceLayers::empty_for_test(), None, &mut v, &mut i);
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
        Mining::new().build_break_mesh_into(glam::Vec3::ZERO, 7, &crate::engine::texture::FaceLayers::empty_for_test(), None, &mut v, &mut i);
        assert!(v.is_empty() && i.is_empty());
    }

    #[test]
    fn every_overlay_index_points_at_a_real_vertex() {
        // A stray index is a GPU-side crash rather than a wrong pixel,
        // so it is worth asserting rather than eyeballing.
        let mut mining = Mining::new();
        for _ in 0..100 {
            mining.update(Some((CELL, BLOCK_PLANKS)), true, 1.0 / 60.0, None, Quality::PLAIN);
        }
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(glam::Vec3::ZERO, &mut v, &mut i);
        assert!(!i.is_empty());
        assert!(i.iter().all(|&index| (index as usize) < v.len()));
        assert_eq!(i.len() % 3, 0, "indices must form whole triangles");
    }

    #[test]
    fn the_overlay_sits_on_the_block_it_targets() {
        let mut mining = Mining::new();
        mining.update(Some((CELL, BLOCK_PLANKS)), true, 0.5, None, Quality::PLAIN);
        let (mut v, mut i) = (Vec::new(), Vec::new());
        mining.build_overlay_into(glam::Vec3::ZERO, &mut v, &mut i);

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

    /// The outline carries no picture.
    ///
    /// **Two kinds of geometry share the actor pipeline** since players
    /// started wearing a skin: a player, and this. The shader tells them
    /// apart by the texture coordinate, so an outline that arrived with
    /// a plausible-looking one would come out wearing a piece of
    /// somebody's sleeve. See `remote_players::UNTEXTURED`.
    #[test]
    fn the_block_outline_carries_no_picture() {
        let mut mining = Mining::default();
        mining.update(Some((CELL, BLOCK_DIRT)), true, 1.0 / 60.0, None, Quality::PLAIN);
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        mining.build_overlay_into(glam::Vec3::ZERO, &mut vertices, &mut indices);
        assert!(!vertices.is_empty(), "nothing was drawn round the target block");
        for v in &vertices {
            assert_eq!(
                v.uv,
                crate::net::remote_players::UNTEXTURED,
                "an outline corner asks for a texture",
            );
        }
    }
}
