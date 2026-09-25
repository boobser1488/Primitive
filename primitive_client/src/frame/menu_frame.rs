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

/// Whether a menu is on the glass, and therefore whether the thumb-
/// control editor has anything to say.
///
/// **A function rather than a condition written at the call site, and
/// that is the whole of the bug it was extracted from.** The editor's
/// phase used to be called from inside the frame's `if net.is_none()`
/// block -- the main-menu half of the loop -- which reads as "when a
/// menu is up" and is not: the pause menu is a menu over a world, with
/// `net` very much `Some`. A player in a world has no other way to the
/// editor than PAUSE, SETTINGS, BUTTONS, so the one route they can take
/// was the one route where nothing they did was kept.
///
/// Here it can be tested, and there is a test that says what it is for.
pub fn menu_is_up(in_a_world: bool, paused: bool) -> bool {
    !in_a_world || paused
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
///
/// ## Why `menu_is_up` is an argument rather than a condition at the
/// call site
///
/// **Because it was a condition at the call site, and it was the wrong
/// one.** This used to be called from inside the frame's `if
/// net.is_none()` block -- the main-menu half of the loop -- so none of
/// it happened while the pause menu stood over a world. A player in a
/// world reaches the editor the only way there is from in there: PAUSE,
/// SETTINGS, BUTTONS. There, nothing told the editor how big the glass
/// was (it kept the 1280x720 a `Menu` is born with, on a 2712x1220
/// phone), nothing copied what they dragged into the settings, nothing
/// resized the controls the game hit-tests, and nothing wrote the file.
/// The buttons moved on the arrangement screen and were in their old
/// places the moment the player pressed DONE: "настройки управления на
/// андроиде ни на что не влияют".
///
/// A condition inside a function is a condition a test can drive. A
/// condition around the call is one that needs the whole frame loop to
/// be running before it can be seen at all, which is why this one went
/// unseen -- there is no test in this repository that can start `run`.
///
/// `size` and `ui_scale` for the same reason: taking a `GraphicsState`
/// meant taking a graphics card, and a phase that needs a GPU to be
/// called is a phase nothing calls but the game.
// A frame phase takes the frame's state, and this one takes eight
// pieces of it -- the same count the phases beside it take, and for the
// same reason: the alternative is a struct that exists to be one
// argument, which hides which of them the phase actually writes to.
#[allow(clippy::too_many_arguments)]
pub fn arrangement(
    menu_is_up: bool,
    size: crate::platform::Size,
    ui_scale: f32,
    settings: &mut ClientSettings,
    menu: &mut Menu,
    touch: &mut crate::platform::touch::Touch,
    touch_layout: &mut crate::settings::TouchLayout,
    arrangement_unsaved: &mut bool,
) {
    // Nothing at all while the player is playing: the editor is not on
    // the glass, and the arrangement it holds is a stale copy of the
    // settings until the screen is opened again.
    if !menu_is_up {
        return;
    }
    // The arrangement screen is the one that needs
    // pixels: a thumb control is sized against the
    // physical screen, not against interface space.
    menu.set_screen_size(size.width, size.height);
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
            touch.resize(size, *touch_layout, ui_scale);
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
            match settings.save() {
                // One line, because a phone answers no other way.
                // Whether what a player dragged reached the file is a
                // question `adb logcat` can settle, and the whole
                // reason this went unnoticed is that nothing anywhere
                // said what had happened to it.
                Ok(()) => println!("the thumb controls were saved"),
                Err(e) => eprintln!("could not save the arrangement: {e}"),
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::touch::{Layout, Touch};
    use crate::platform::{Size, TouchPhase};
    use crate::settings::{Emits, TouchLayout};
    use crate::ui::menu::{Screen, ServerList};

    /// The phone this game is tested on, in its own pixels.
    fn phone() -> Size {
        Size::new(2712, 1220)
    }

    /// INTERFACE SIZE, at a setting that is neither the default nor an
    /// extreme -- the bar the controls are lifted clear of grows with
    /// it, so a layout checked at 1.0 is not checked at all.
    const SCALE: f32 = 1.5;

    /// Everything the frame loop holds about the controls, held the way
    /// the frame loop holds it: the settings, the menu, the controls the
    /// game hit-tests, and the copy of the arrangement the frame passes
    /// to them.
    struct Frame {
        settings: ClientSettings,
        menu: Menu,
        touch: Touch,
        touch_layout: TouchLayout,
        unsaved: bool,
    }

    impl Frame {
        fn new() -> Frame {
            let settings = ClientSettings::default();
            let touch_layout = settings.touch_layout;
            let mut frame = Frame {
                settings,
                menu: Menu::new(ServerList::default()),
                touch: Touch::default(),
                touch_layout,
                unsaved: false,
            };
            // What the world does every frame it has fingers on it.
            frame.touch.resize(phone(), frame.touch_layout, SCALE);
            frame
        }

        /// One frame of the phase, decided exactly as `run` decides it.
        fn frame(&mut self, in_a_world: bool, paused: bool) {
            arrangement(
                menu_is_up(in_a_world, paused),
                phone(),
                SCALE,
                &mut self.settings,
                &mut self.menu,
                &mut self.touch,
                &mut self.touch_layout,
                &mut self.unsaved,
            );
            // ...and what the world does with the arrangement it is
            // handed, which is the other half of "did it reach the
            // game": see the `touch_controls` block in `run`.
            self.touch.resize(phone(), self.touch_layout, SCALE);
        }
    }

    /// Where the game draws a control, from an arrangement.
    fn in_play(arrangement: TouchLayout, slot: usize) -> (f32, f32) {
        Layout::for_size(phone(), arrangement, SCALE, false).buttons[slot].centre
    }

    fn jump(arrangement: TouchLayout) -> usize {
        Layout::for_size(phone(), arrangement, SCALE, false)
            .buttons
            .iter()
            .position(|button| matches!(button.emits, Emits::Key(crate::platform::Key::Space)))
            .expect("the jump button is on the glass")
    }

    /// A point on the glass in the interface space the frame hands the
    /// menu -- the same conversion `frame::events` does for a finger.
    fn on_glass(px: (f32, f32)) -> (f32, f32) {
        crate::ui::widgets::cursor_to_ui(
            (px.0 as f64, px.1 as f64),
            (phone().width, phone().height),
            1.0,
        )
    }

    /// A button moved on the arrangement screen is moved in the world,
    /// with the world merely paused behind it.
    ///
    /// **The player's whole report.** The editor is reached from a world
    /// through the pause menu and from nowhere else, and the phase that
    /// carries what it holds into the settings and into the controls the
    /// game hit-tests was called only while there was no world at all.
    /// Everything done on that screen was dropped on the way out of it.
    #[test]
    fn a_button_moved_over_a_paused_world_is_moved_in_the_world() {
        let mut frame = Frame::new();
        let slot = jump(frame.settings.touch_layout);
        let was = in_play(frame.settings.touch_layout, slot);

        frame.menu.open(Screen::TouchControls);
        // The frame that opens the screen is the one that tells the
        // editor how big the glass is.
        frame.frame(true, true);

        // A thumb takes the button and carries it into the middle left
        // of the glass.
        let to = (phone().width as f32 * 0.35, phone().height as f32 * 0.42);
        assert!(
            frame
                .menu
                .arranging_touch(1, TouchPhase::Started, on_glass(was), SCALE),
            "a thumb in the middle of JUMP picked nothing up -- the editor \
was never told the size of the glass it is arranging",
        );
        frame
            .menu
            .arranging_touch(1, TouchPhase::Moved, on_glass(to), SCALE);
        frame
            .menu
            .arranging_touch(1, TouchPhase::Ended, on_glass(to), SCALE);
        frame.frame(true, true);

        // Where the game will draw it, and where a thumb will press it.
        let landed = in_play(frame.settings.touch_layout, slot);
        assert!(
            (landed.0 - to.0).abs() < 2.0 && (landed.1 - to.1).abs() < 2.0,
            "dropped at {to:?} and the world has it at {landed:?}",
        );
        assert_eq!(
            frame.touch.layout().buttons[slot].centre,
            landed,
            "the controls the game hit-tests were not rebuilt from the \
arrangement the player made",
        );
        assert_eq!(
            frame.touch.layout().button_at(landed.0, landed.1),
            Some(slot),
            "a thumb where the button is drawn pressed something else",
        );
        assert!(frame.unsaved, "nothing asked for the file to be written");
    }

    /// ...and it is still moved after the game is shut and opened again.
    ///
    /// Through the text the settings file really holds, because what
    /// would break here breaks in `serde` rather than in the struct.
    #[test]
    fn an_arrangement_made_over_a_paused_world_survives_the_settings_file() {
        let mut frame = Frame::new();
        let slot = jump(frame.settings.touch_layout);
        let was = in_play(frame.settings.touch_layout, slot);
        frame.menu.open(Screen::TouchControls);
        frame.frame(true, true);

        let to = (phone().width as f32 * 0.30, phone().height as f32 * 0.55);
        assert!(frame
            .menu
            .arranging_touch(2, TouchPhase::Started, on_glass(was), SCALE));
        frame
            .menu
            .arranging_touch(2, TouchPhase::Moved, on_glass(to), SCALE);
        frame
            .menu
            .arranging_touch(2, TouchPhase::Ended, on_glass(to), SCALE);
        frame.frame(true, true);

        let written = toml::to_string(&frame.settings).expect("the settings serialise");
        let read: ClientSettings = toml::from_str(&written).expect("and parse back");
        assert_eq!(
            in_play(read.touch_layout, slot),
            in_play(frame.settings.touch_layout, slot),
            "the arrangement did not survive the file",
        );
    }

    /// Leaving the screen hands the editor back what the player has,
    /// rather than what the game shipped.
    ///
    /// The editor holds a copy so that a drag can be abandoned; the copy
    /// has to be refreshed from the settings every frame the screen is
    /// shut, or the next opening starts from the shipped arrangement and
    /// the player's own is quietly replaced by it the moment they touch
    /// anything.
    #[test]
    fn opening_the_editor_again_starts_from_what_the_player_has() {
        let mut frame = Frame::new();
        let slot = jump(frame.settings.touch_layout);
        let was = in_play(frame.settings.touch_layout, slot);
        frame.menu.open(Screen::TouchControls);
        frame.frame(true, true);
        let to = (phone().width as f32 * 0.45, phone().height as f32 * 0.30);
        assert!(frame
            .menu
            .arranging_touch(3, TouchPhase::Started, on_glass(was), SCALE));
        frame
            .menu
            .arranging_touch(3, TouchPhase::Moved, on_glass(to), SCALE);
        frame
            .menu
            .arranging_touch(3, TouchPhase::Ended, on_glass(to), SCALE);
        frame.frame(true, true);
        let moved = frame.settings.touch_layout;

        // DONE, and then a few frames of the pause menu standing there.
        frame.menu.open(Screen::Settings);
        for _ in 0..3 {
            frame.frame(true, true);
        }
        assert!(
            frame.menu.arrangement().same_as(&moved),
            "the editor went back to the arrangement the game ships with",
        );
        assert!(
            frame.settings.touch_layout.same_as(&moved),
            "the settings lost the arrangement when the screen was left",
        );
        assert!(
            !frame.unsaved,
            "the file was not written on the way out of the screen",
        );
    }

    /// While the player is playing, the phase does nothing at all.
    ///
    /// It is called every frame now rather than from one branch of the
    /// loop, so "every frame" has to be free: no copying, no rebuilding
    /// of the controls under a thumb that is using them, and above all
    /// no writing of the settings file mid-stride.
    #[test]
    fn nothing_happens_to_the_controls_while_the_player_is_playing() {
        let mut frame = Frame::new();
        let slot = jump(frame.settings.touch_layout);
        let was = in_play(frame.settings.touch_layout, slot);
        frame.menu.open(Screen::TouchControls);
        frame.frame(true, true);
        assert!(frame
            .menu
            .arranging_touch(4, TouchPhase::Started, on_glass(was), SCALE));
        let to = (phone().width as f32 * 0.5, phone().height as f32 * 0.5);
        frame
            .menu
            .arranging_touch(4, TouchPhase::Moved, on_glass(to), SCALE);
        frame
            .menu
            .arranging_touch(4, TouchPhase::Ended, on_glass(to), SCALE);

        // ...and the player resumes before the phase has run: what the
        // editor holds stays in the editor.
        let before = frame.settings.touch_layout;
        for _ in 0..3 {
            frame.frame(true, false);
        }
        assert!(
            frame.settings.touch_layout.same_as(&before),
            "the editor reached into the settings of a game being played",
        );
        assert!(!frame.unsaved, "a file was asked for mid-stride");
    }

    /// The rule itself: a menu over a world is a menu.
    #[test]
    fn the_pause_menu_counts_as_a_menu_and_a_world_being_played_does_not() {
        assert!(menu_is_up(false, false), "the main menu");
        assert!(menu_is_up(true, true), "the pause menu over a world");
        assert!(!menu_is_up(true, false), "a world being played");
    }
}
