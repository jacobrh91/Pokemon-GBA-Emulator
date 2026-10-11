//! Keyboard bindings and the key-driven actions.

use super::dock::MenuNav;
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

/// Keys that act on the open cheat menu, and whether holding one repeats.
/// The movement keys and the arrow keys move; Space (or Enter) is the A
/// button and C the B button.
pub(super) const MENU_KEYS: [(Key, MenuNav, bool); 12] = [
    (Key::W, MenuNav::Up, true),
    (Key::Up, MenuNav::Up, true),
    (Key::S, MenuNav::Down, true),
    (Key::Down, MenuNav::Down, true),
    (Key::A, MenuNav::Left, true),
    (Key::Left, MenuNav::Left, true),
    (Key::D, MenuNav::Right, true),
    (Key::Right, MenuNav::Right, true),
    (Key::Space, MenuNav::Toggle, false),
    (Key::Enter, MenuNav::Toggle, false),
    (Key::C, MenuNav::Back, false),
    (Key::Backspace, MenuNav::Back, false),
];

/// Keeps the game's buttons from seeing the keys that drive the menu: no
/// button gets through while the menu is open, nor afterwards until every
/// game key has been released (so the press that closed it, or one held to
/// move around in it, is not taken by the game as its own).
#[derive(Debug, Default)]
pub(super) struct ButtonGate {
    held_back: bool,
}

impl ButtonGate {
    pub(super) fn apply(&mut self, menu_open: bool, pressed: Buttons) -> Buttons {
        if menu_open {
            self.held_back = true;
            return 0;
        }
        if self.held_back {
            if pressed == 0 {
                self.held_back = false;
            }
            return 0;
        }
        pressed
    }
}

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
    fn the_game_gets_no_buttons_while_the_menu_is_open_or_until_they_are_released() {
        let mut gate = ButtonGate::default();
        // Closed menu: buttons pass.
        assert_eq!(
            gate.apply(false, input::A | input::UP),
            input::A | input::UP
        );
        // Open: nothing passes, whatever is pressed.
        assert_eq!(gate.apply(true, input::UP), 0);
        assert_eq!(gate.apply(true, 0), 0);
        // Closed again with a key still held: still nothing...
        assert_eq!(gate.apply(false, input::B), 0);
        assert_eq!(gate.apply(false, input::B), 0);
        // ...until everything is released; then buttons pass again.
        assert_eq!(gate.apply(false, 0), 0);
        assert_eq!(gate.apply(false, input::B), input::B);
    }

    #[test]
    fn every_menu_key_is_distinct_and_the_game_keys_agree_with_them() {
        for (index, (key, ..)) in MENU_KEYS.iter().enumerate() {
            assert!(
                MENU_KEYS[index + 1..]
                    .iter()
                    .all(|(other, ..)| other != key),
                "{key:?} listed twice"
            );
        }
        // The same keys that are the game's buttons are the menu's actions.
        for (key, nav, _) in MENU_KEYS {
            let button = KEY_BINDINGS.iter().find(|(k, _)| *k == key).map(|b| b.1);
            let expected = match nav {
                MenuNav::Up => Some(input::UP),
                MenuNav::Down => Some(input::DOWN),
                MenuNav::Left => Some(input::LEFT),
                MenuNav::Right => Some(input::RIGHT),
                MenuNav::Toggle => Some(input::A),
                MenuNav::Back => Some(input::B),
            };
            if let Some(button) = button {
                assert_eq!(Some(button), expected, "{key:?}");
            }
        }
    }

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
