//! What each key does, and how that survives a restart.
//!
//! ## Why the indirection
//!
//! Every action used to name its key inline -- `is_down(KeyCode::KeyW)`
//! scattered through the frame loop, `KeyCode::KeyI` in the event match.
//! That is fine right up until someone wants a different layout, at
//! which point the answer is "edit the source", and it is silently wrong
//! for anyone not on QWERTY: `KeyCode` is a *physical* position, so a
//! player on AZERTY walks forward with the key labelled Z.
//!
//! ## Why names rather than codes on disk
//!
//! `KeyCode` has no stable numeric form and no serde support, and a
//! settings file full of integers is a settings file nobody can edit by
//! hand. The table below maps both ways; anything not in it cannot be
//! bound, which is the point -- an unbindable key is better than a
//! binding that silently does nothing.

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use crate::ui::lang::{Language, Msg};

/// Something the player can bind a key to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sprint,
    /// Held to lay a *single layer* of loose material instead of a
    /// whole block. See `types::layer_placement`.
    Inventory,
    Drop,
    Respawn,
    ToggleFog,
    ToggleStats,
    /// Borderless fullscreen, on and off.
    ToggleFullscreen,
}

impl Action {
    /// Every action, in the order the controls screen lists them.
    pub const ALL: [Action; 12] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Jump,
        Action::Sprint,
        Action::Inventory,
        Action::Drop,
        Action::Respawn,
        Action::ToggleFog,
        Action::ToggleStats,
        Action::ToggleFullscreen,
    ];

    /// What the controls screen calls this action, in the language the
    /// player has chosen. The key *names* (SPACE, L SHIFT) stay as they
    /// are printed on the keyboard.
    pub fn label(self, language: Language) -> &'static str {
        language.text(self.msg())
    }

    /// The action's row in the language table.
    fn msg(self) -> Msg {
        match self {
            Action::Forward => Msg::WalkForward,
            Action::Back => Msg::WalkBack,
            Action::Left => Msg::StrafeLeft,
            Action::Right => Msg::StrafeRight,
            Action::Jump => Msg::Jump,
            Action::Sprint => Msg::Sprint,
            Action::Inventory => Msg::Inventory,
            Action::Drop => Msg::DropItem,
            Action::Respawn => Msg::Respawn,
            Action::ToggleFog => Msg::ToggleFog,
            Action::ToggleStats => Msg::ToggleStats,
            Action::ToggleFullscreen => Msg::Fullscreen,
        }
    }

    /// The field name used in the settings file.
    fn key(self) -> &'static str {
        match self {
            Action::Forward => "forward",
            Action::Back => "back",
            Action::Left => "left",
            Action::Right => "right",
            Action::Jump => "jump",
            Action::Sprint => "sprint",
            Action::Inventory => "inventory",
            Action::Drop => "drop",
            Action::Respawn => "respawn",
            Action::ToggleFog => "toggle_fog",
            Action::ToggleStats => "toggle_stats",
            Action::ToggleFullscreen => "toggle_fullscreen",
        }
    }

    fn default_key(self) -> KeyCode {
        match self {
            Action::Forward => KeyCode::KeyW,
            Action::Back => KeyCode::KeyS,
            Action::Left => KeyCode::KeyA,
            Action::Right => KeyCode::KeyD,
            Action::Jump => KeyCode::Space,
            Action::Sprint => KeyCode::ShiftLeft,
            Action::Inventory => KeyCode::KeyI,
            Action::Drop => KeyCode::KeyQ,
            Action::Respawn => KeyCode::KeyR,
            Action::ToggleFog => KeyCode::KeyF,
            Action::ToggleStats => KeyCode::F3,
            Action::ToggleFullscreen => KeyCode::F11,
        }
    }
}

/// The bindings, as they are stored and used.
///
/// A plain map keyed by the settings-file name, so an unknown entry in
/// the file is ignored rather than fatal and a missing one falls back to
/// the default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Keybinds {
    bound: std::collections::BTreeMap<String, String>,
}

impl Default for Keybinds {
    fn default() -> Self {
        let mut bound = std::collections::BTreeMap::new();
        for action in Action::ALL {
            bound.insert(
                action.key().to_string(),
                key_name(action.default_key()).to_string(),
            );
        }
        Self { bound }
    }
}

impl Keybinds {
    /// The key bound to an action, if it has one.
    ///
    /// `None` is a real state, not an error: binding a key that another
    /// action already held leaves that other action with nothing, and
    /// the controls screen says so. Pretending it fell back to its
    /// default would put two actions on one key again, which is the bug
    /// this returns an `Option` to avoid.
    ///
    /// A *missing* entry is different from an unbound one -- it means a
    /// settings file written before this action existed, and it gets the
    /// default.
    pub fn key(&self, action: Action) -> Option<KeyCode> {
        match self.bound.get(action.key()) {
            Some(name) if name == NONE => None,
            Some(name) => key_from_name(name),
            None => Some(action.default_key()),
        }
    }

    /// What to show on the controls screen.
    pub fn label(&self, action: Action) -> &'static str {
        match self.key(action) {
            Some(key) => key_name(key),
            None => NONE,
        }
    }

    /// Binds a key, taking it off whatever else had it.
    ///
    /// Stealing rather than refusing: a player rebinding forward to `E`
    /// when `E` is already something else means they want it on forward,
    /// and a refusal leaves them hunting for what is holding it. The
    /// action that lost its key falls back to its default, which is
    /// visible on the same screen.
    pub fn bind(&mut self, action: Action, key: KeyCode) {
        let name = key_name(key);
        if name == UNKNOWN {
            return;
        }
        for other in Action::ALL {
            if other != action && self.key(other) == Some(key) {
                // Explicitly unbound, not merely forgotten: a forgotten
                // entry falls back to its default, which is usually the
                // very key that was just taken.
                self.bound.insert(other.key().to_string(), NONE.to_string());
            }
        }
        self.bound.insert(action.key().to_string(), name.to_string());
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Drops entries that no longer name a real action or a real key, so
    /// a file edited by hand cannot leave a binding that does nothing.
    pub fn sanitize(&mut self) {
        self.bound.retain(|action, key| {
            Action::ALL.iter().any(|a| a.key() == action)
                && (key == NONE || key_from_name(key).is_some())
        });
    }
}

const UNKNOWN: &str = "?";
/// What an action with no key at all is written as.
const NONE: &str = "--";

/// Keys a player may bind, and what to call them.
///
/// Deliberately not exhaustive. Modifiers that the window manager eats,
/// and keys with no printed label, are worse than useless as bindings --
/// they look bound and do nothing.
const KEYS: &[(KeyCode, &str)] = &[
    (KeyCode::KeyA, "A"), (KeyCode::KeyB, "B"), (KeyCode::KeyC, "C"),
    (KeyCode::KeyD, "D"), (KeyCode::KeyE, "E"), (KeyCode::KeyF, "F"),
    (KeyCode::KeyG, "G"), (KeyCode::KeyH, "H"), (KeyCode::KeyI, "I"),
    (KeyCode::KeyJ, "J"), (KeyCode::KeyK, "K"), (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"), (KeyCode::KeyN, "N"), (KeyCode::KeyO, "O"),
    (KeyCode::KeyP, "P"), (KeyCode::KeyQ, "Q"), (KeyCode::KeyR, "R"),
    (KeyCode::KeyS, "S"), (KeyCode::KeyT, "T"), (KeyCode::KeyU, "U"),
    (KeyCode::KeyV, "V"), (KeyCode::KeyW, "W"), (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"), (KeyCode::KeyZ, "Z"),
    (KeyCode::Space, "SPACE"),
    (KeyCode::ShiftLeft, "L SHIFT"),
    (KeyCode::ShiftRight, "R SHIFT"),
    (KeyCode::ControlLeft, "L CTRL"),
    (KeyCode::ControlRight, "R CTRL"),
    (KeyCode::AltLeft, "L ALT"),
    (KeyCode::Tab, "TAB"),
    (KeyCode::CapsLock, "CAPS"),
    (KeyCode::ArrowUp, "UP"),
    (KeyCode::ArrowDown, "DOWN"),
    (KeyCode::ArrowLeft, "LEFT"),
    (KeyCode::ArrowRight, "RIGHT"),
    (KeyCode::F1, "F1"), (KeyCode::F2, "F2"), (KeyCode::F3, "F3"),
    (KeyCode::F4, "F4"), (KeyCode::F5, "F5"), (KeyCode::F6, "F6"),
    (KeyCode::F7, "F7"), (KeyCode::F8, "F8"),
    (KeyCode::F9, "F9"), (KeyCode::F10, "F10"), (KeyCode::F11, "F11"),
    (KeyCode::F12, "F12"),
];

pub fn key_name(key: KeyCode) -> &'static str {
    KEYS.iter()
        .find(|(code, _)| *code == key)
        .map(|(_, name)| *name)
        .unwrap_or(UNKNOWN)
}

pub fn key_from_name(name: &str) -> Option<KeyCode> {
    KEYS.iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(name))
        .map(|(code, _)| *code)
}

/// Whether a key may be bound at all.
pub fn is_bindable(key: KeyCode) -> bool {
    key_name(key) != UNKNOWN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_layout_the_game_shipped_with() {
        let binds = Keybinds::default();
        assert_eq!(binds.key(Action::Forward), Some(KeyCode::KeyW));
        assert_eq!(binds.key(Action::Jump), Some(KeyCode::Space));
        assert_eq!(binds.key(Action::Sprint), Some(KeyCode::ShiftLeft));
        assert_eq!(binds.key(Action::Inventory), Some(KeyCode::KeyI));
    }

    #[test]
    fn every_action_has_a_key_and_every_key_has_a_name() {
        let binds = Keybinds::default();
        for action in Action::ALL {
            let key = binds.key(action).expect("every action starts bound");
            assert!(is_bindable(key), "{:?} has an unnameable key", action);
            assert_ne!(binds.label(action), UNKNOWN);
            for language in Language::ALL {
                assert!(!action.label(*language).is_empty());
            }
        }
    }

    #[test]
    fn no_two_actions_start_out_sharing_a_key() {
        let binds = Keybinds::default();
        for (index, a) in Action::ALL.iter().enumerate() {
            for b in Action::ALL.iter().skip(index + 1) {
                assert_ne!(
                    binds.key(*a),
                    binds.key(*b),
                    "{:?} and {:?} share a default key",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn names_survive_a_round_trip() {
        for (code, name) in KEYS {
            assert_eq!(key_from_name(name), Some(*code), "{name} did not come back");
            assert_eq!(key_name(*code), *name);
        }
    }

    #[test]
    fn binding_a_key_takes_it_off_whatever_had_it() {
        // Refusing instead would leave the player hunting for what is
        // holding the key they want.
        let mut binds = Keybinds::default();
        binds.bind(Action::Forward, KeyCode::KeyI);
        assert_eq!(binds.key(Action::Forward), Some(KeyCode::KeyI));
        assert_eq!(
            binds.key(Action::Inventory),
            None,
            "two actions ended up on one key"
        );
        assert_eq!(binds.label(Action::Inventory), NONE, "and the screen must say so");
    }

    #[test]
    fn an_unbindable_key_is_refused_rather_than_stored() {
        let mut binds = Keybinds::default();
        let before = binds.key(Action::Forward);
        binds.bind(Action::Forward, KeyCode::F24);
        assert_eq!(binds.key(Action::Forward), before, "an unnameable key was stored");
    }

    #[test]
    fn rebinding_something_to_its_own_key_is_harmless() {
        let mut binds = Keybinds::default();
        binds.bind(Action::Jump, KeyCode::Space);
        assert_eq!(binds.key(Action::Jump), Some(KeyCode::Space));
    }

    #[test]
    fn a_hand_edited_file_cannot_leave_a_dead_binding() {
        let mut binds = Keybinds::default();
        binds.bound.insert("forward".to_string(), "NOT A KEY".to_string());
        binds.bound.insert("fly".to_string(), "F".to_string());
        binds.sanitize();
        assert_eq!(
            binds.key(Action::Forward),
            Some(KeyCode::KeyW),
            "a nonsense key should fall back to the default"
        );
        assert!(!binds.bound.contains_key("fly"), "an unknown action survived");
    }

    #[test]
    fn every_action_is_bound_to_something_of_its_own() {
        // Two actions sharing a default key is a binding one of them
        // silently never gets, and the controls screen shows both of
        // them holding it.
        let mut seen = std::collections::HashSet::new();
        for action in Action::ALL {
            assert!(
                seen.insert(action.default_key()),
                "{} shares its default key",
                action.label(Language::English)
            );
            assert!(
                !action.key().is_empty(),
                "{} has no settings name",
                action.label(Language::English)
            );
        }
    }

    #[test]
    fn the_controls_screen_still_has_room_for_all_of_them() {
        // The controls list is drawn as one row per action inside a
        // fixed panel, with no scrolling: the rows simply run off the
        // bottom when there are too many, and the one that vanishes is
        // whichever was added last. See `menu::build_controls`, which
        // is where these numbers come from.
        const PANEL_HEIGHT: f32 = 0.76 - -0.62;
        let row = crate::ui::menu::controls_row_height();
        let used = 0.060 + Action::ALL.len() as f32 * (row + 0.010);
        assert!(
            used <= PANEL_HEIGHT,
            "{} actions need {used} of {PANEL_HEIGHT} at a row height of {row} --              the rows have shrunk as far as they can and this screen now needs to scroll",
            Action::ALL.len()
        );
        // ...and not by making the print unreadable.
        assert!(row >= 0.055, "the rows are too short to read");
    }

    #[test]
    fn resetting_restores_every_default() {
        let mut binds = Keybinds::default();
        binds.bind(Action::Forward, KeyCode::KeyT);
        binds.bind(Action::Jump, KeyCode::KeyG);
        binds.reset();
        for action in Action::ALL {
            assert_eq!(binds.key(action), Some(action.default_key()));
        }
    }

    #[test]
    fn settings_survive_being_written_and_read_back() {
        let mut binds = Keybinds::default();
        binds.bind(Action::Drop, KeyCode::KeyG);
        let text = toml::to_string(&binds).expect("serialise");
        let back: Keybinds = toml::from_str(&text).expect("parse");
        assert_eq!(back.key(Action::Drop), Some(KeyCode::KeyG));
        assert_eq!(back.key(Action::Forward), Some(KeyCode::KeyW));
    }
}
