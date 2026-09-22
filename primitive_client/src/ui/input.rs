use std::collections::HashSet;
use crate::platform::Key as KeyCode;

use crate::logic::inventory::HOTBAR_SLOTS;

#[derive(Default)]
pub struct InputState {
    pressed: HashSet<KeyCode>,
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    /// Keys that went down this frame. Kept as a set rather than one
    /// jump flag because "was this action just pressed" is a question
    /// every rebindable action can ask, and hard-coding Space meant
    /// rebinding jump left the old key still jumping.
    pressed_this_frame: HashSet<KeyCode>,
    pub mouse_grabbed: bool,
    /// Which hotbar slot is selected. What is *in* that slot is the
    /// inventory's business -- it changes as the player mines, and input
    /// has no opinion about it.
    pub hotbar_slot: usize,
    /// Break is held down rather than clicked: mining takes time, so the
    /// frame loop needs to know the button is *still* down, not that it
    /// went down once.
    pub breaking: bool,
    /// The use button, held. **Tracked as a state and not only as a press**
    /// because the rod is wound back for as long as it is down: see
    /// `logic::fishing::Hold`.
    pub using: bool,
    /// Where a thumb is pushing, if there is a thumb.
    ///
    /// `(right, forward)`, each in -1..=1, and `None` on anything with
    /// a keyboard. Beside the keys rather than instead of them because
    /// the two are not the same kind of thing: four keys have one
    /// speed, and a thumb has every speed. Squashing the stick back
    /// into the keys would take away the only way to walk quietly up to
    /// a deer instead of charging at it.
    ///
    /// See `wish_direction`, which prefers this when it is there.
    pub stick: Option<(f32, f32)>,
}

impl InputState {
    pub fn set_key(&mut self, code: KeyCode, is_pressed: bool) {
        if is_pressed {
            if !self.pressed.contains(&code) {
                self.pressed_this_frame.insert(code);
            }
            if let Some(slot) = hotbar_slot_for(code) {
                if slot < HOTBAR_SLOTS {
                    self.hotbar_slot = slot;
                }
            }
            self.pressed.insert(code);
        } else {
            self.pressed.remove(&code);
        }
    }

    pub fn is_down(&self, code: KeyCode) -> bool {
        self.pressed.contains(&code)
    }

    /// Whether the key bound to an action is held.
    ///
    /// Everything that used to name a `KeyCode` inline goes through
    /// here, so a rebound key takes effect everywhere at once -- and so
    /// a player on a non-QWERTY layout is not walking forward with the
    /// key labelled Z.
    pub fn action_down(&self, binds: &crate::ui::keybinds::Keybinds, action: crate::ui::keybinds::Action) -> bool {
        binds.key(action).is_some_and(|key| self.is_down(key))
    }

    /// Whether the key bound to an action went down this frame.
    ///
    /// The press edge, not the held state: a jump has to fire once per
    /// press, or holding the key auto-hops.
    pub fn action_pressed(
        &self,
        binds: &crate::ui::keybinds::Keybinds,
        action: crate::ui::keybinds::Action,
    ) -> bool {
        binds
            .key(action)
            .is_some_and(|key| self.pressed_this_frame.contains(&key))
    }

    /// Moves the selection across the hotbar.
    ///
    /// Wraps over every slot *of the bar*, including empty ones.
    /// Skipping the empties would make the wheel land somewhere
    /// different depending on what the player happens to be carrying,
    /// and the number keys -- which cannot skip -- would then disagree
    /// with it.
    ///
    /// It used to wrap over every slot of the whole *inventory*, which
    /// is four times as many as the bar draws. Scrolling past the tenth
    /// selected a storage slot: the highlight vanished off the bar, and
    /// the server -- which clamps the selection to the bar -- then spent
    /// a different block from the one the client was checking.
    pub fn cycle_hotbar(&mut self, forward: bool) {
        let len = HOTBAR_SLOTS;
        self.hotbar_slot = if forward {
            (self.hotbar_slot + 1) % len
        } else {
            (self.hotbar_slot + len - 1) % len
        };
    }

    /// How hard the thumb is pushing, as a multiplier on walking speed.
    ///
    /// 1.0 on anything with a keyboard, because four keys have one
    /// speed and always did. On glass it is how far the stick is from
    /// its centre, which is what makes creeping up on an animal
    /// possible with a thumb.
    ///
    /// Never zero when the stick is held at all: `touch::Touch::stick`
    /// has already applied its dead zone and reports `(0, 0)` for a
    /// thumb that is merely resting, and a direction of zero stops the
    /// player without needing the speed to say so too.
    pub fn stick_speed(&self) -> f32 {
        match self.stick {
            Some((x, y)) => (x * x + y * y).sqrt().clamp(0.0, 1.0),
            None => 1.0,
        }
    }

    /// Whether the thumb is pushed far enough to mean "run".
    ///
    /// There is no sprint key to hold on a phone, and adding a fifth
    /// button for it would cost another piece of the world. Pushing the
    /// stick to its rim is what every game with a stick already means
    /// by running, so it is what this means too.
    pub fn stick_sprinting(&self) -> bool {
        // Not 1.0: a thumb cannot reliably reach the exact rim, and a
        // sprint that only works when it does is a sprint that feels
        // broken.
        const RIM: f32 = 0.85;
        self.stick.is_some() && self.stick_speed() >= RIM
    }

    pub fn accumulate_mouse(&mut self, dx: f32, dy: f32) {
        self.mouse_dx += dx;
        self.mouse_dy += dy;
    }

    /// Forgets every held key.
    ///
    /// Called when the cursor is released -- opening the pause menu, or
    /// alt-tabbing away. Key-up events don't arrive while the window
    /// isn't focused, so without this the player comes back still
    /// walking forward into whatever they were walking into.
    pub fn release_all(&mut self) {
        self.pressed.clear();
        self.pressed_this_frame.clear();
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
        // A thumb that was on the glass when the window went does not
        // send a touch-up either.
        self.stick = None;
        // Mouse-up does not arrive while the window is unfocused either,
        // so a player who alt-tabs mid-swing would come back still
        // mining whatever they were pointed at.
        self.breaking = false;
    }

    /// Call once per frame after consuming mouse_dx/dy and the jump flag.
    pub fn end_frame(&mut self) {
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
        self.pressed_this_frame.clear();
    }
}

/// The hotbar slot a number key names, if it names one.
///
/// Public because the inventory screen uses the same mapping for
/// "send what I am pointing at to slot 3", and two tables would be two
/// tables to keep in step.
pub fn hotbar_slot_for(code: KeyCode) -> Option<usize> {
    match code {
        KeyCode::Digit1 => Some(0),
        KeyCode::Digit2 => Some(1),
        KeyCode::Digit3 => Some(2),
        KeyCode::Digit4 => Some(3),
        KeyCode::Digit5 => Some(4),
        KeyCode::Digit6 => Some(5),
        KeyCode::Digit7 => Some(6),
        KeyCode::Digit8 => Some(7),
        KeyCode::Digit9 => Some(8),
        // The numeric row wraps to 0 for the tenth slot, the way the
        // key itself sits on the keyboard.
        KeyCode::Digit0 => Some(9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ui::keybinds::{Action, Keybinds};

    #[test]
    fn jump_only_fires_on_the_press_edge() {
        let binds = Keybinds::default();
        let mut input = InputState::default();
        input.set_key(binds.key(Action::Jump).unwrap(), true);
        assert!(input.action_pressed(&binds, Action::Jump));
        input.end_frame();
        // Holding the key down must not re-trigger without a release.
        input.set_key(binds.key(Action::Jump).unwrap(), true);
        assert!(!input.action_pressed(&binds, Action::Jump));
    }

    #[test]
    fn rebinding_moves_the_action_and_frees_the_old_key() {
        // The failure this guards against: a hard-coded key that keeps
        // working after the player has bound the action elsewhere.
        let mut binds = Keybinds::default();
        binds.bind(Action::Jump, KeyCode::KeyG);

        let mut input = InputState::default();
        input.set_key(KeyCode::Space, true);
        assert!(!input.action_pressed(&binds, Action::Jump), "the old key still jumps");
        assert!(!input.action_down(&binds, Action::Jump));

        input.set_key(KeyCode::KeyG, true);
        assert!(input.action_pressed(&binds, Action::Jump));
        assert!(input.action_down(&binds, Action::Jump));
    }

    #[test]
    fn zero_selects_the_tenth_slot() {
        // The tenth slot was reachable only with the wheel until 0 was
        // bound, which is not where a player looks for it.
        let mut input = InputState::default();
        input.set_key(KeyCode::Digit0, true);
        assert_eq!(input.hotbar_slot, 9);
    }

    #[test]
    fn number_keys_pick_a_slot() {
        let mut input = InputState::default();
        assert_eq!(input.hotbar_slot, 0);
        input.set_key(KeyCode::Digit3, true);
        assert_eq!(input.hotbar_slot, 2);
    }

    #[test]
    fn cycling_wraps_in_both_directions() {
        let mut input = InputState::default();
        input.cycle_hotbar(false);
        assert_eq!(input.hotbar_slot, HOTBAR_SLOTS - 1);
        input.cycle_hotbar(true);
        assert_eq!(input.hotbar_slot, 0);
    }

    #[test]
    fn cycling_visits_empty_slots_too() {
        // The wheel and the number keys have to agree on where slot 5
        // is, and the number keys cannot skip.
        let mut input = InputState::default();
        for expected in 1..HOTBAR_SLOTS {
            input.cycle_hotbar(true);
            assert_eq!(input.hotbar_slot, expected);
        }
    }

    #[test]
    fn the_wheel_never_leaves_the_bar() {
        // The bug: the wheel wrapped over all forty inventory slots, so
        // scrolling past the tenth selected storage. The highlight left
        // the bar entirely, and the server -- which clamps the selection
        // to the bar -- spent a different block from the one the client
        // had checked it was carrying.
        let mut input = InputState::default();
        for _ in 0..(crate::logic::inventory::SLOTS * 3) {
            input.cycle_hotbar(true);
            assert!(
                input.hotbar_slot < HOTBAR_SLOTS,
                "the wheel selected slot {}, which the bar does not draw",
                input.hotbar_slot
            );
        }
        for _ in 0..(crate::logic::inventory::SLOTS * 3) {
            input.cycle_hotbar(false);
            assert!(input.hotbar_slot < HOTBAR_SLOTS);
        }
    }

    #[test]
    fn the_bound_sprint_key_sprints() {
        let binds = Keybinds::default();
        let mut input = InputState::default();
        assert!(!input.action_down(&binds, Action::Sprint));
        input.set_key(binds.key(Action::Sprint).unwrap(), true);
        assert!(input.action_down(&binds, Action::Sprint));
        input.set_key(binds.key(Action::Sprint).unwrap(), false);
        assert!(!input.action_down(&binds, Action::Sprint));
    }

    #[test]
    fn losing_focus_stops_the_sprint_and_the_swing() {
        // Key-up never arrives while the window is unfocused, so a
        // player who alt-tabs mid-stride would come back still running.
        let binds = Keybinds::default();
        let mut input = InputState::default();
        input.set_key(binds.key(Action::Sprint).unwrap(), true);
        input.breaking = true;
        input.release_all();
        assert!(!input.action_down(&binds, Action::Sprint));
        assert!(!input.breaking);
    }
}
