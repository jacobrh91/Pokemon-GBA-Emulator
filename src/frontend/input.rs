//! Keyboard bindings and the key-driven actions.

use gba_emulator::input::{self, Buttons};
use minifb::Key;

/// Keyboard layout: WASD = D-pad, Space = A, C = B, Q = L, E = R,
/// X = Start, V = Select.
pub(super) const KEY_BINDINGS: [(Key, Buttons); 10] = [
    (Key::Space, input::A),
    (Key::C, input::B),
    (Key::V, input::SELECT),
    (Key::X, input::START),
    (Key::D, input::RIGHT),
    (Key::A, input::LEFT),
    (Key::W, input::UP),
    (Key::S, input::DOWN),
    (Key::E, input::R),
    (Key::Q, input::L),
];

/// Save-state keys: 0 writes the state, 1 plays it back.
pub(super) const SAVE_STATE_KEY: Key = Key::Key0;
pub(super) const LOAD_STATE_KEY: Key = Key::Key1;

/// Tab opens and closes the cheat menu (the same as its dock button).
pub(super) const CHEAT_MENU_KEY: Key = Key::Tab;
/// The key of the console's B button also closes an open cheat menu, as B
/// backs out of menus in the games. The press still reaches the game.
pub(super) const CHEAT_MENU_BACK_KEY: Key = Key::C;

/// What a press of Escape should do.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum EscapeAction {
    None,
    /// In full screen, Escape returns to the window instead of quitting.
    LeaveFullscreen,
    Quit,
}

pub(super) fn escape_action(escape_pressed: bool, fullscreen: bool) -> EscapeAction {
    match (escape_pressed, fullscreen) {
        (false, _) => EscapeAction::None,
        (true, true) => EscapeAction::LeaveFullscreen,
        (true, false) => EscapeAction::Quit,
    }
}

pub(super) fn buttons_from_keys(is_down: impl Fn(Key) -> bool) -> Buttons {
    KEY_BINDINGS
        .iter()
        .filter(|(key, _)| is_down(*key))
        .fold(0, |buttons, (_, button)| buttons | button)
}

/// Holding either Shift key runs the game as fast as the machine allows.
pub(super) const FAST_FORWARD_KEYS: [Key; 2] = [Key::LeftShift, Key::RightShift];

pub(super) fn fast_forward_held(is_down: impl Fn(Key) -> bool) -> bool {
    FAST_FORWARD_KEYS.iter().any(|key| is_down(*key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_leaves_full_screen_and_only_quits_from_a_window() {
        assert_eq!(escape_action(false, false), EscapeAction::None);
        assert_eq!(escape_action(false, true), EscapeAction::None);
        assert_eq!(escape_action(true, true), EscapeAction::LeaveFullscreen);
        assert_eq!(escape_action(true, false), EscapeAction::Quit);
    }

    #[test]
    fn either_shift_key_holds_fast_forward() {
        assert!(fast_forward_held(|key| key == Key::LeftShift));
        assert!(fast_forward_held(|key| key == Key::RightShift));
        assert!(!fast_forward_held(|key| key == Key::Space));
        // Shift is not a game button, so holding it never presses one.
        assert_eq!(buttons_from_keys(|key| key == Key::LeftShift), 0);
    }

    #[test]
    fn keys_map_to_gba_buttons() {
        let held = [Key::Space, Key::X, Key::A, Key::E];
        let buttons = buttons_from_keys(|key| held.contains(&key));
        assert_eq!(buttons, input::A | input::START | input::LEFT | input::R);

        // Every binding maps to a distinct button, and the old arrow-key
        // layout is no longer bound.
        let all: Buttons = KEY_BINDINGS
            .iter()
            .fold(0, |mask, (_, button)| mask | button);
        assert_eq!(all, input::ALL);
        assert_eq!(
            buttons_from_keys(|key| key == Key::Left || key == Key::Enter),
            0
        );
        assert_eq!(buttons_from_keys(|_| false), 0);
        assert_eq!(buttons_from_keys(|_| true), input::ALL);
    }
}
