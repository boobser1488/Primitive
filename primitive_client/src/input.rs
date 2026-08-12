use std::collections::HashSet;
use winit::keyboard::KeyCode;

use primitive_shared::types::{BlockId, PLACEABLE_BLOCKS};

#[derive(Default)]
pub struct InputState {
    pressed: HashSet<KeyCode>,
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    pub jump_pressed_this_frame: bool,
    pub mouse_grabbed: bool,
    /// Index into `PLACEABLE_BLOCKS` -- which block right-click places.
    pub hotbar_slot: usize,
}

impl InputState {
    pub fn set_key(&mut self, code: KeyCode, is_pressed: bool) {
        if is_pressed {
            if code == KeyCode::Space && !self.pressed.contains(&code) {
                self.jump_pressed_this_frame = true;
            }
            if let Some(slot) = hotbar_slot_for(code) {
                if slot < PLACEABLE_BLOCKS.len() {
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

    pub fn selected_block(&self) -> BlockId {
        PLACEABLE_BLOCKS
            .get(self.hotbar_slot)
            .copied()
            .unwrap_or(PLACEABLE_BLOCKS[0])
    }

    pub fn cycle_hotbar(&mut self, forward: bool) {
        let len = PLACEABLE_BLOCKS.len();
        self.hotbar_slot = if forward {
            (self.hotbar_slot + 1) % len
        } else {
            (self.hotbar_slot + len - 1) % len
        };
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
        self.jump_pressed_this_frame = false;
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
    }

    /// Call once per frame after consuming mouse_dx/dy and the jump flag.
    pub fn end_frame(&mut self) {
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
        self.jump_pressed_this_frame = false;
    }
}

fn hotbar_slot_for(code: KeyCode) -> Option<usize> {
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
    use primitive_shared::types::BLOCK_STONE;

    #[test]
    fn jump_only_fires_on_the_press_edge() {
        let mut input = InputState::default();
        input.set_key(KeyCode::Space, true);
        assert!(input.jump_pressed_this_frame);
        input.end_frame();
        // Holding the key down must not re-trigger without a release.
        input.set_key(KeyCode::Space, true);
        assert!(!input.jump_pressed_this_frame);
    }

    #[test]
    fn zero_selects_the_tenth_slot() {
        // The tenth block was reachable only with the wheel until 0 was
        // bound, which is not where a player looks for it.
        let mut input = InputState::default();
        input.set_key(KeyCode::Digit0, true);
        assert_eq!(input.hotbar_slot, 9);
        assert_eq!(input.selected_block(), PLACEABLE_BLOCKS[9]);
    }

    #[test]
    fn number_keys_pick_a_block() {
        let mut input = InputState::default();
        assert_eq!(input.selected_block(), BLOCK_STONE);
        input.set_key(KeyCode::Digit3, true);
        assert_eq!(input.hotbar_slot, 2);
        assert_eq!(input.selected_block(), PLACEABLE_BLOCKS[2]);
    }

    #[test]
    fn cycling_wraps_in_both_directions() {
        let mut input = InputState::default();
        input.cycle_hotbar(false);
        assert_eq!(input.hotbar_slot, PLACEABLE_BLOCKS.len() - 1);
        input.cycle_hotbar(true);
        assert_eq!(input.hotbar_slot, 0);
    }

    #[test]
    fn an_out_of_range_slot_never_panics() {
        let mut input = InputState::default();
        input.hotbar_slot = 999;
        assert_eq!(input.selected_block(), PLACEABLE_BLOCKS[0]);
    }
}
