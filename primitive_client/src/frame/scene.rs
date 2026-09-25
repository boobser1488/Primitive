//! **The geometry that is rebuilt rather than streamed.**
//!
//! The animals, the dropped items, the other players, the set-down
//! things, the swinging chest lids, the particles and the player's own
//! arm: everything that *moves* every frame, regenerated from nothing
//! and uploaded again.
//!
//! **On a clock, not on every frame.** These were rebuilt every frame
//! when a frame was sixteen milliseconds; at the rates this now runs at
//! that is the same bobbing item rebuilt a thousand times a second, and
//! measurement put it at a fifth of the frame. A hundred and twenty times
//! a second is past what anyone can see -- see [`crate::DYNAMIC_REBUILD_HZ`].
//! A frame the render origin moved in is always a rebuild: the geometry
//! that exists was measured from somewhere else.
//!
//! The buffers themselves live in [`Dynamic`] and persist across frames,
//! on the CPU and on the card both. A fresh allocation per frame for
//! geometry that is the same size every frame is exactly the cost that
//! only shows up once the frame rate is high enough for it to matter.

use crate::engine::camera::Camera;
use crate::engine::renderer::{DynamicMesh, GraphicsState};
use crate::engine::sky::Sky;
use crate::engine::texture::FaceLayers;
use crate::engine::{self, item_model, mesh, particles as particle_pool};
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::inventory::Inventory;
use crate::logic::{entities, hand as hand_mod, mining as mining_mod};
use crate::net::remote_players::{self, RemotePlayers};
use crate::ui::input;
use crate::ui::{chest_screen, death, inventory_screen, station_screen};
use crate::{bump_version, figures_rebuild_due, mark_urgent, shown_in_hand, MeshQueueSet};
use primitive_shared::lighting::LightMap;
use primitive_shared::types::ChunkPos;
use std::collections::{HashMap, VecDeque};

/// **Every buffer the moving geometry is rebuilt into**, on the CPU and
/// on the card, kept from frame to frame.
///
/// Together in one type rather than nineteen locals threaded through the
/// frame: they are filled and uploaded as one step, they are all the same
/// kind of thing, and a rebuild that forgot one of them is a rebuild that
/// draws last frame's animals.
pub struct Dynamic {
    pub entity_mesh: DynamicMesh,
    pub particle_mesh: DynamicMesh,
    pub item_mesh: DynamicMesh,
    pub actor_mesh: DynamicMesh,
    pub break_mesh: DynamicMesh,
    pub hand_mesh: DynamicMesh,
    /// How many of `particle_mesh`'s indices go before the water. Kept
    /// with the mesh, because the mesh is not rebuilt every frame and the
    /// two have to describe the same buffer. See
    /// `Particles::behind_water_first`.
    pub particles_behind_water: u32,
    entity_vertices: Vec<mesh::Vertex>,
    entity_indices: Vec<u32>,
    /// Dropped items are sprites with a thickness rather than cubes, so
    /// they have their own vertex format and their own buffer. See
    /// `engine::item_model`.
    item_vertices: Vec<item_model::ItemVertex>,
    item_indices: Vec<u32>,
    particle_vertices: Vec<particle_pool::ParticleVertex>,
    particle_indices: Vec<u32>,
    actor_vertices: Vec<remote_players::ActorVertex>,
    actor_indices: Vec<u32>,
    /// The cracks on the block being mined. Their own buffer because they
    /// are textured and blended, where the outline around the same block
    /// is flat geometry on the actor pipeline.
    break_vertices: Vec<mesh::Vertex>,
    break_indices: Vec<u32>,
    /// The player's own arm. Its vertices are in view space rather than
    /// in the world -- see `logic::hand` -- which is why it is a buffer
    /// of its own rather than more geometry in the item one.
    hand_vertices: Vec<hand_mod::HandVertex>,
    hand_indices: Vec<u32>,
}

impl Dynamic {
    pub fn new(graphics: &GraphicsState) -> Self {
        Self {
            entity_mesh: graphics.new_dynamic_mesh(),
            particle_mesh: graphics.new_dynamic_mesh(),
            item_mesh: graphics.new_dynamic_mesh(),
            actor_mesh: graphics.new_dynamic_mesh(),
            break_mesh: graphics.new_dynamic_mesh(),
            hand_mesh: graphics.new_dynamic_mesh(),
            particles_behind_water: 0,
            entity_vertices: Vec::new(),
            entity_indices: Vec::new(),
            item_vertices: Vec::new(),
            item_indices: Vec::new(),
            particle_vertices: Vec::new(),
            particle_indices: Vec::new(),
            actor_vertices: Vec::new(),
            actor_indices: Vec::new(),
            break_vertices: Vec::new(),
            break_indices: Vec::new(),
            hand_vertices: Vec::new(),
            hand_indices: Vec::new(),
        }
    }
}

/// One frame of the moving geometry, and the lamp volume that lights it.
#[allow(clippy::too_many_arguments)]
pub fn build(
    rebuild_due: bool,
    since_rebuild: f32,
    render_origin: glam::Vec3,
    world_ready: bool,
    loading: Option<f32>,
    paused: bool,
    face_layers: &FaceLayers,
    light: &LightMap,
    camera: &Camera,
    sky: &Sky,
    inventory: &Inventory,
    input: &input::InputState,
    entities: &entities::Entities,
    remote_players: &RemotePlayers,
    particles: &particle_pool::Particles,
    critters: &engine::critters::Critters,
    breeze: &engine::breeze::Breeze,
    mining: &mining_mod::Mining,
    hand: &hand_mod::Hand,
    death: &death::DeathScreen,
    inventory_screen: &inventory_screen::InventoryScreen,
    chest_screen: &chest_screen::ChestScreen,
    station_screen: &station_screen::StationScreen,
    journal: &crate::ui::journal::Journal,
    chunks: &mut ChunkManager,
    graphics: &mut GraphicsState,
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    chunk_versions: &mut HashMap<ChunkPos, u64>,
    dynamic: &mut Dynamic,
) {
    // Entities are drawn with the terrain pipeline, so
    // they get the same textures, lighting and fog as
    // the blocks they came from.
    // Written into buffers that persist between frames
    // rather than freshly allocated ones -- both on the
    // CPU and on the GPU. See `write_dynamic_mesh`.
    if rebuild_due {
    dynamic.entity_vertices.clear();
    dynamic.entity_indices.clear();
    dynamic.item_vertices.clear();
    dynamic.item_indices.clear();
    if !entities.is_empty() {
        entities.build_meshes_into(
            render_origin,
            face_layers,
            light,
            Some(&graphics.textures),
            &mut dynamic.entity_vertices,
            &mut dynamic.entity_indices,
            &mut dynamic.item_vertices,
            &mut dynamic.item_indices,
        );
    }
    // ...and what other players are carrying, into the
    // same two buffers. A pick in somebody's hand is
    // the same sprite as a pick on the ground and wants
    // the same pass; the actor pipeline the *body* is
    // drawn with samples the player skin and cannot
    // reach the block atlas at all. See
    // `remote_players::build_held_items_into`.
    // ...and every thing set down by hand, lying where it
    // was put. Asked of the chunks rather than the entities:
    // it is a cell in the world, not a stack the server moves.
    entities::build_set_down_into(
        chunks.set_down_laid(),
        render_origin,
        face_layers,
        light,
        Some(&graphics.textures),
        &mut dynamic.entity_vertices,
        &mut dynamic.entity_indices,
        &mut dynamic.item_vertices,
        &mut dynamic.item_indices,
    );
    remote_players::build_held_items_into(
        remote_players,
        render_origin,
        face_layers,
        light,
        Some(&graphics.textures),
        &mut dynamic.entity_vertices,
        &mut dynamic.entity_indices,
        &mut dynamic.item_vertices,
        &mut dynamic.item_indices,
    );
    // ...and the lid of every chest somebody has open, which
    // is here for the set-down items' reason and one more:
    // it *moves*. A lid swings for a third of a second
    // (`mesh::LID_SWING_SECONDS`), and a chunk meshed again
    // on every frame of that is forty rebuilds of sixteen
    // thousand cells to turn four boxes. It is drawn on this
    // clock, so it is stepped on this clock: the angle drawn
    // is the angle the swing has reached.
    for cell in chunks.advance_lids(since_rebuild) {
        // Rested shut: the mesh takes the lid back. Until
        // that mesh lands the frame goes on drawing it,
        // shut (`note_meshed_lids`); urgent all the same,
        // because it is a chest somebody is standing at.
        let pos = ChunkPos::from_world(cell.0, cell.2);
        bump_version(chunk_versions, pos);
        mark_urgent(urgent, dirty_set, pos);
    }
    for (cell, block, swing) in chunks.open_lids() {
        let corner = glam::DVec3::new(f64::from(cell.0), f64::from(cell.1), f64::from(cell.2));
        let (sky, block_light) = entities::sampled_light(corner, light);
        let at = (corner - render_origin.as_dvec3()).as_vec3();
        engine::mesh::chest_lid_block(
            [at.x, at.y, at.z],
            block,
            swing,
            face_layers,
            sky | (block_light << 4),
            &mut dynamic.entity_vertices,
            &mut dynamic.entity_indices,
        );
    }
    graphics.write_dynamic_mesh(
        &mut dynamic.entity_mesh,
        &dynamic.entity_vertices,
        &dynamic.entity_indices,
    );

    // ...and every particle in the world, into a buffer
    // and a pass of its own.
    //
    // Not the entity buffer any more: a particle wears
    // one *texel* of a block rather than the whole
    // picture of it, and the terrain vertex has two bits
    // of texture coordinate. See `engine::particles`.
    //
    // Measured from the render origin like everything
    // else in the world. It used to be uploaded in world
    // coordinates, "because the shader uses `view_proj`
    // directly" -- and `view_proj` is built around the
    // origin, so every particle was drawn the player's
    // own position further on: blood seventy blocks up
    // in the sky. See `Particles::build_into`.
    dynamic.particle_vertices.clear();
    dynamic.particle_indices.clear();
    let (billboard_right, billboard_up) =
        engine::particles::billboard_axes(camera);
    particles.build_into(
        render_origin,
        billboard_right,
        billboard_up,
        face_layers,
        light,
        &mut dynamic.particle_vertices,
        &mut dynamic.particle_indices,
    );
    // Those with a water surface between them and the eye
    // go first and are drawn before the water. See
    // `Particles::behind_water_first`.
    dynamic.particles_behind_water =
        particles.behind_water_first(chunks, camera.position.as_vec3(), &mut dynamic.particle_indices);
    // The small life rides the same buffer and the same
    // pass: a wing is a soft quad like a flake of snow.
    critters.build_into(
        render_origin,
        camera.right_horizontal(),
        glam::Vec3::Y,
        face_layers,
        light,
        &mut dynamic.particle_vertices,
        &mut dynamic.particle_indices,
    );
    // ...and so does the wind, laid out along the particles'
    // own axes: a streak seen from above must not be a hairline.
    breeze.build_into(
        render_origin,
        billboard_right,
        billboard_up,
        face_layers,
        light,
        &mut dynamic.particle_vertices,
        &mut dynamic.particle_indices,
    );
    graphics.write_dynamic_mesh(
        &mut dynamic.particle_mesh,
        &dynamic.particle_vertices,
        &dynamic.particle_indices,
    );
    graphics.write_dynamic_mesh(&mut dynamic.item_mesh, &dynamic.item_vertices, &dynamic.item_indices);

    // The player's own hand, and what is in it. Hidden
    // behind anything that has taken over the screen:
    // a menu, a container, the death screen, or the
    // loading bar with no world behind it yet. An arm
    // waving over the inventory is worse than no arm.
    dynamic.hand_vertices.clear();
    dynamic.hand_indices.clear();
    let shown_in_hand = shown_in_hand(inventory.block_in(input.hotbar_slot));
    hand.build_into(
        world_ready
            && loading.is_none()
            && !paused
            && !death.is_open()
            && !inventory_screen.open
            && !chest_screen.is_open()
            && !station_screen.is_open()
            && !journal.is_open(),
        shown_in_hand,
        face_layers,
        Some(&graphics.textures),
        // Lit by the cell the player's own head is in,
        // the same way a dropped item is lit by the cell
        // it lies in.
        entities::sampled_light(camera.position, light),
        // **The same clock the campfire runs on**, so a
        // torch in the hand and a fire in the ring are
        // in step. Read from the sky's own elapsed
        // seconds, which is what the shader animates the
        // hearth by -- a second clock here would drift
        // against it, and two fires flickering out of
        // step is exactly the sort of thing a player
        // sees without being able to say what is wrong.
        //
        // The *pictures* are the torch's own, and that
        // is not a change of mind about sharing: a
        // hearth's fire is drawn to fill a cell and a
        // torch's has to sit on four texels of fibre.
        // See `Textures::torch_flame_layer`.
        shown_in_hand
            .filter(|&block| primitive_shared::types::is_lit_torch(block))
            .map(|_| {
                let frames = crate::engine::texture::FLAME_FRAMES;
                let step = (sky.elapsed() * crate::engine::texture::FLAME_FPS)
                    as u32
                    % frames;
                graphics.textures.torch_flame_layer() + step
            }),
        &mut dynamic.hand_vertices,
        &mut dynamic.hand_indices,
    );
    graphics.write_dynamic_mesh(&mut dynamic.hand_mesh, &dynamic.hand_vertices, &dynamic.hand_indices);
    } // rebuild_due

    // Other players on every frame they are drawn, off the
    // clock above: see `figures_rebuild_due`.
    if figures_rebuild_due(rebuild_due, !remote_players.is_empty()) {
    dynamic.actor_vertices.clear();
    dynamic.actor_indices.clear();
    remote_players::build_actor_mesh_into(
        remote_players,
        // Every dead player's body in the loaded world is
        // drawn here too, in their own skin: see
        // `player_model::append_lying`.
        Some(chunks),
        render_origin,
        light,
        &mut dynamic.actor_vertices,
        &mut dynamic.actor_indices,
    );
    // The block outline and its cracks are untextured
    // lit triangles, which is exactly what the actor
    // pipeline already draws -- so they ride along in
    // the same buffer rather than needing a pass of
    // their own.
    mining.build_overlay_into(render_origin, &mut dynamic.actor_vertices, &mut dynamic.actor_indices);
    graphics.write_dynamic_mesh(&mut dynamic.actor_mesh, &dynamic.actor_vertices, &dynamic.actor_indices);

    dynamic.break_vertices.clear();
    dynamic.break_indices.clear();
    if let Some(stage) = mining.break_stage() {
        // The lid as the frame is drawing it, so the cracks
        // go on the lid where it is. See `mining::model_faces`.
        let lid = mining.target().and_then(|cell| {
            chunks.open_lids().find(|&(at, _, _)| at == cell).map(|(_, _, swing)| swing)
        });
        mining.build_break_mesh_into(
            render_origin,
            graphics.textures.break_layer(stage),
            face_layers,
            lid,
            &mut dynamic.break_vertices,
            &mut dynamic.break_indices,
        );
    }
    graphics.write_dynamic_mesh(
        &mut dynamic.break_mesh,
        &dynamic.break_vertices,
        &dynamic.break_indices,
    );
    } // figures_rebuild_due

    // The fires' shadows walk the blocks round the player,
    // read again only when the eye has walked out of the
    // middle of the last lot or a chunk under it has a new
    // mesh. See `engine::lamp_shadow`.
    if let Some(min) = graphics.lamp_volume_wanted(camera.position) {
        // The plants as the player asked for their shadows,
        // so the walk and the map agree about a crown.
        let volume = engine::lamp_shadow::Volume::gather_for(min, graphics.plant_shadows(), |gx, gz, y0, out| {
            match chunks.column(gx, gz) {
                Some(column) => {
                    for (dy, slot) in out.iter_mut().enumerate() {
                        *slot = column.block(y0 + dy as i32);
                    }
                }
                None => out.fill(primitive_shared::types::BLOCK_AIR),
            }
        });
        graphics.set_lamp_volume(volume);
    }
}
