//! **The frame the menus are drawn in.**
//!
//! Two things the world frame has no version of: the patch of world
//! standing behind the menus, and the editor for the controls on the
//! glass.
//!
//! ## The backdrop
//!
//! The one piece of world the client makes for itself. It exists only
//! while there is *no* session, and it is thrown away the moment there is
//! one -- see `logic::menu_scene` for how a place is chosen and why the
//! choice is reproducible from two printed numbers.
//!
//! **The menu's clock does not run.** The veil over the backdrop is
//! measured against how bright the scene can get, at dusk, so the time of
//! day is fixed (`menu_scene::TIME_OF_DAY`). It used to start at 0.3 --
//! mid-morning, for no reason anybody wrote down -- and from the player's
//! seat the menu's time of day simply kept changing between runs.

use std::time::{Duration, Instant};

use crate::engine::camera::Camera;
use crate::engine::renderer::GraphicsState;
use crate::engine::sky::Sky;
use crate::logic::menu_scene;
use crate::settings::ClientSettings;
use crate::ui::menu::Menu;
use crate::{menu_scene_roll, render_origin_for};

/// Builds, ticks or drops the backdrop, and says which place is behind
/// the menu -- which the fog and the bench line both need.
///
/// **Only while there is still no session.** The connection completes in
/// the same branch of the frame, so on the one frame a world opens this
/// would build a fresh backdrop over the top of it and put the menu's
/// time of day back: a player saw a world open at dusk and jump to
/// morning a second later, and the machine meshed a patch of world nobody
/// would ever see in the frame the real one started streaming.
#[allow(clippy::too_many_arguments)]
pub fn backdrop(
    menu_dt: f32,
    in_a_session: bool,
    settings: &ClientSettings,
    graphics: &mut GraphicsState,
    camera: &mut Camera,
    sky: &mut Sky,
    render_origin: &mut glam::Vec3,
    menu_scene: &mut Option<menu_scene::MenuScene>,
    menu_scene_asked_for: &mut Option<menu_scene::Place>,
    menu_scene_started: &mut Option<Instant>,
) -> Option<menu_scene::Place> {
    // **The backdrop, and the only place in the
    // client that makes world without being told
    // to.** Started the first frame the menu is on
    // screen with the setting on, rebuilt when the
    // player changes which place they want, and
    // dropped -- meshes and all -- when they switch
    // it off. The switch has to reach the card as
    // well as the flag: the geometry is the
    // renderer's now, so forgetting to clear it
    // would leave a shore behind a menu that says
    // the backdrop is off.
    //
    // **And only while there is still no session.**
    // The connection above completes *inside* this
    // block -- the `net.is_none()` that opened it
    // was tested at the top of the frame -- so on
    // the one frame a world opens, `menu_scene` has
    // just been cleared and `sky` has just been set
    // to the server's clock, and this would build a
    // fresh backdrop over the top of both and put
    // `menu_scene::TIME_OF_DAY` back. What a player
    // saw was a world that opened at dusk and
    // jumped to morning a second later, when the
    // first `TimeSync` landed and `Sky::on_time_sync`
    // snapped a delta too big to be drift. What the
    // machine did was mesh a patch of world nobody
    // would ever see, in the frame the real one
    // started streaming.
    if settings.menu_background && !in_a_session {
        if menu_scene.is_some()
            && *menu_scene_asked_for != settings.menu_background_place()
        {
            *menu_scene = None;
            graphics.clear_chunk_meshes();
        }
        let scene = match menu_scene.as_mut() {
            Some(scene) => scene,
            None => {
                *menu_scene_asked_for = settings.menu_background_place();
                // Dusk, and fixed: the veil over the
                // scene is measured against how
                // bright the scene can get, so the
                // menu's clock does not run. See
                // `menu_scene::TIME_OF_DAY`.
                *sky = Sky::new(menu_scene::TIME_OF_DAY, 900.0);
                let fresh = menu_scene::MenuScene::spawn(
                    *menu_scene_asked_for,
                    menu_scene_roll(),
                    graphics.textures.face_layers(),
                );
                // Printed because a backdrop that
                // came out wrong is otherwise a
                // report with nothing in it: the
                // place and the seed are between
                // them the whole of what was
                // chosen, and `look_for` is pure,
                // so those two numbers reproduce
                // the picture exactly.
                *menu_scene_started = Some(Instant::now());
                let spot = fresh.spot();
                println!(
                    "menu backdrop: {} at {}, {} in seed {}",
                    spot.place.name(),
                    spot.eye.x.round(),
                    spot.eye.z.round(),
                    spot.seed,
                );
                menu_scene.insert(fresh)
            }
        };
        scene.tick(menu_dt);
        // Landing a mesh is a copy to the card, so
        // it is rationed here exactly as it is for
        // the streamed world -- twenty-five of them
        // in the frame they all happen to be ready
        // in is a hitch on the one screen where
        // nothing else is going on.
        let landing = Instant::now();
        let budget =
            Duration::from_secs_f32(settings.mesh_budget_ms / 1000.0);
        while let Some(built) = scene.poll() {
            if built.buffers.indices.is_empty() {
                graphics.drop_chunk_mesh(built.pos);
            } else {
                graphics.set_chunk_mesh(built.pos, &built.buffers);
            }
            if landing.elapsed() >= budget {
                break;
            }
        }
        camera.position = scene.eye().as_dvec3();
        camera.yaw = scene.yaw();
        camera.pitch = scene.pitch();
        camera.aspect = graphics.aspect();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        *render_origin = render_origin_for(camera.position, *render_origin);
    } else if menu_scene.take().is_some() {
        graphics.clear_chunk_meshes();
        *menu_scene_asked_for = None;
    }
    menu_scene.as_ref().map(|s| s.spot().place)
}

/// **The thumb controls, edited while the player watches.**
///
/// Moving a control and only seeing it land after leaving the screen is
/// arranging blind, so the editor's arrangement is pushed into the
/// settings and into `touch` every frame it differs. `resize` is free
/// when nothing changed -- it compares the arrangement it was last given
/// -- so this costs nothing on every other frame.
///
/// The file is written once, on the way out of the screen, rather than on
/// every frame of a drag: a settings file rewritten sixty times a second
/// while a thumb moves is a lot of disk for one decision.
pub fn arrangement(
    settings: &mut ClientSettings,
    menu: &mut Menu,
    graphics: &GraphicsState,
    touch: &mut crate::platform::touch::Touch,
    touch_layout: &mut crate::settings::TouchLayout,
    arrangement_unsaved: &mut bool,
) {
    // The arrangement screen is the one that needs
    // pixels: a thumb control is sized against the
    // physical screen, not against interface space.
    menu.set_screen_size(graphics.size.width, graphics.size.height);
    // **Applied while the player watches.** Moving a
    // control and only seeing it land after leaving
    // the screen is arranging blind. `resize` is
    // free when nothing changed -- it compares the
    // arrangement it was last given -- so this costs
    // nothing on every other frame.
    if menu.is_arranging() {
        let wanted = menu.arrangement();
        if !wanted.same_as(&settings.touch_layout) {
            settings.touch_layout = wanted;
            *touch_layout = wanted;
            *touch_layout = wanted;
            touch.resize(graphics.size, *touch_layout, graphics.ui_scale());
            *arrangement_unsaved = true;
        }
    } else {
        // **Kept in step while the screen is shut**,
        // so that opening it starts from what the
        // player actually has rather than from what
        // the game shipped. Done here rather than in
        // the action that opens the screen, because
        // the menu has no settings of its own to
        // read and handing it a stale copy once is
        // how an editor comes to be editing a
        // layout nobody is using.
        menu.begin_arranging(settings.touch_layout);
        if *arrangement_unsaved {
            // Written down once, on the way out,
            // rather than on every frame of a drag:
            // a settings file rewritten sixty times
            // a second while a thumb moves is a lot
            // of disk for one decision.
            *arrangement_unsaved = false;
            if let Err(e) = settings.save() {
                eprintln!("could not save the arrangement: {e}");
            }
        }
    }
}
