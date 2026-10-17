//! Keyboard bindings and the key-driven actions.

use super::dock::MenuNav;
use gba_emulator::input::{self, Buttons};
use minifb::Key;

/// Something a key can be bound to: a button of the console, the cheat menu,
/// or fast-forward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Control {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    L,
    R,
    Start,
    Select,
    CheatMenu,
    FastForward,
}

impl Control {
    pub(super) const ALL: [Control; 12] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::A,
        Self::B,
        Self::L,
        Self::R,
        Self::Start,
        Self::Select,
        Self::CheatMenu,
        Self::FastForward,
    ];

    /// The console button it presses, if it is one.
    pub(super) fn button(self) -> Option<Buttons> {
        Some(match self {
            Self::Up => input::UP,
            Self::Down => input::DOWN,
            Self::Left => input::LEFT,
            Self::Right => input::RIGHT,
            Self::A => input::A,
            Self::B => input::B,
            Self::L => input::L,
            Self::R => input::R,
            Self::Start => input::START,
            Self::Select => input::SELECT,
            Self::CheatMenu | Self::FastForward => return None,
        })
    }

    /// The name shown in the controls menu.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::A => "A",
            Self::B => "B",
            Self::L => "L",
            Self::R => "R",
            Self::Start => "START",
            Self::Select => "SELECT",
            Self::CheatMenu => "CHEAT MENU",
            Self::FastForward => "FAST FORWARD",
        }
    }

    /// The settings-file name, as in `key_up = w`.
    fn setting(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::A => "a",
            Self::B => "b",
            Self::L => "l",
            Self::R => "r",
            Self::Start => "start",
            Self::Select => "select",
            Self::CheatMenu => "cheat_menu",
            Self::FastForward => "fast_forward",
        }
    }

    fn default_key(self) -> Key {
        match self {
            Self::Up => Key::W,
            Self::Down => Key::S,
            Self::Left => Key::A,
            Self::Right => Key::D,
            Self::A => Key::Space,
            Self::B => Key::C,
            Self::L => Key::Q,
            Self::R => Key::E,
            Self::Start => Key::X,
            Self::Select => Key::V,
            Self::CheatMenu => Key::Tab,
            Self::FastForward => Key::LeftShift,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// The keys that can be bound, with the name each has in the settings file.
/// The menu shows the name in capitals with spaces for underscores. Escape,
/// 0, 1, F11 and Ctrl, Alt and Super are left out: they are the emulator's.
const BINDABLE_KEYS: &[(Key, &str)] = &[
    (Key::A, "a"),
    (Key::B, "b"),
    (Key::C, "c"),
    (Key::D, "d"),
    (Key::E, "e"),
    (Key::F, "f"),
    (Key::G, "g"),
    (Key::H, "h"),
    (Key::I, "i"),
    (Key::J, "j"),
    (Key::K, "k"),
    (Key::L, "l"),
    (Key::M, "m"),
    (Key::N, "n"),
    (Key::O, "o"),
    (Key::P, "p"),
    (Key::Q, "q"),
    (Key::R, "r"),
    (Key::S, "s"),
    (Key::T, "t"),
    (Key::U, "u"),
    (Key::V, "v"),
    (Key::W, "w"),
    (Key::X, "x"),
    (Key::Y, "y"),
    (Key::Z, "z"),
    (Key::Key2, "2"),
    (Key::Key3, "3"),
    (Key::Key4, "4"),
    (Key::Key5, "5"),
    (Key::Key6, "6"),
    (Key::Key7, "7"),
    (Key::Key8, "8"),
    (Key::Key9, "9"),
    (Key::F1, "f1"),
    (Key::F2, "f2"),
    (Key::F3, "f3"),
    (Key::F4, "f4"),
    (Key::F5, "f5"),
    (Key::F6, "f6"),
    (Key::F7, "f7"),
    (Key::F8, "f8"),
    (Key::F9, "f9"),
    (Key::F10, "f10"),
    (Key::F12, "f12"),
    (Key::Up, "up_arrow"),
    (Key::Down, "down_arrow"),
    (Key::Left, "left_arrow"),
    (Key::Right, "right_arrow"),
    (Key::Apostrophe, "apostrophe"),
    (Key::Backquote, "backquote"),
    (Key::Backslash, "backslash"),
    (Key::Comma, "comma"),
    (Key::Equal, "equals"),
    (Key::LeftBracket, "left_bracket"),
    (Key::Minus, "minus"),
    (Key::Period, "period"),
    (Key::RightBracket, "right_bracket"),
    (Key::Semicolon, "semicolon"),
    (Key::Slash, "slash"),
    (Key::Backspace, "backspace"),
    (Key::Delete, "delete"),
    (Key::End, "end"),
    (Key::Enter, "enter"),
    (Key::Home, "home"),
    (Key::Insert, "insert"),
    (Key::PageDown, "page_down"),
    (Key::PageUp, "page_up"),
    (Key::Space, "space"),
    (Key::CapsLock, "caps_lock"),
    (Key::NumPad0, "numpad_0"),
    (Key::NumPad1, "numpad_1"),
    (Key::NumPad2, "numpad_2"),
    (Key::NumPad3, "numpad_3"),
    (Key::NumPad4, "numpad_4"),
    (Key::NumPad5, "numpad_5"),
    (Key::NumPad6, "numpad_6"),
    (Key::NumPad7, "numpad_7"),
    (Key::NumPad8, "numpad_8"),
    (Key::NumPad9, "numpad_9"),
    (Key::NumPadDot, "numpad_dot"),
    (Key::NumPadSlash, "numpad_slash"),
    (Key::NumPadAsterisk, "numpad_asterisk"),
    (Key::NumPadMinus, "numpad_minus"),
    (Key::NumPadPlus, "numpad_plus"),
    (Key::NumPadEnter, "numpad_enter"),
    (Key::F13, "f13"),
    (Key::F14, "f14"),
    (Key::F15, "f15"),
    (Key::Tab, "tab"),
    (Key::LeftShift, "left_shift"),
    (Key::RightShift, "right_shift"),
    (Key::Menu, "menu"),
    (Key::Pause, "pause"),
    (Key::NumLock, "num_lock"),
    (Key::ScrollLock, "scroll_lock"),
];

/// The setting name of a key that can be bound.
fn key_name(key: Key) -> Option<&'static str> {
    BINDABLE_KEYS
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, name)| *name)
}

fn key_from_name(name: &str) -> Option<Key> {
    BINDABLE_KEYS
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map(|(key, _)| *key)
}

/// What the key is called in the controls menu: `LEFT ARROW`.
pub(super) fn key_label(key: Key) -> String {
    key_name(key)
        .map_or_else(|| "?".to_owned(), |name| name.replace('_', " "))
        .to_ascii_uppercase()
}

/// What binding a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bound {
    /// The control has the key now.
    Done,
    /// The key was another control's, which took this control's old key.
    Swapped(Control),
    /// The key is not one that can be bound.
    Refused,
}

/// Which key presses which button. Every control has a key of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Bindings {
    keys: [Key; 12],
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            keys: Control::ALL.map(Control::default_key),
        }
    }
}

impl Bindings {
    pub(super) fn key(&self, control: Control) -> Key {
        self.keys[control.index()]
    }

    /// Binds `key` to `control`. A key that was another control's moves that
    /// control to this one's old key, so no two controls share a key.
    pub(super) fn bind(&mut self, control: Control, key: Key) -> Bound {
        if key_name(key).is_none() {
            return Bound::Refused;
        }
        let previous = self.key(control);
        let other = Control::ALL
            .into_iter()
            .find(|other| *other != control && self.key(*other) == key);
        self.keys[control.index()] = key;
        match other {
            Some(other) => {
                self.keys[other.index()] = previous;
                Bound::Swapped(other)
            }
            None => Bound::Done,
        }
    }

    #[cfg(test)]
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The buttons whose keys are held.
    pub(super) fn buttons(&self, is_down: impl Fn(Key) -> bool) -> Buttons {
        Control::ALL
            .into_iter()
            .filter(|control| is_down(self.key(*control)))
            .filter_map(Control::button)
            .fold(0, |buttons, button| buttons | button)
    }

    /// Whether fast-forward is held. Until its key is changed, either Shift
    /// key does it.
    pub(super) fn fast_forward_held(&self, is_down: impl Fn(Key) -> bool) -> bool {
        let key = self.key(Control::FastForward);
        is_down(key) || (key == Key::LeftShift && is_down(Key::RightShift))
    }

    /// The bindings a settings file holds (`("up", "w")`); entries that name
    /// no control or no bindable key are skipped.
    pub(super) fn from_settings(entries: &[(String, String)]) -> Self {
        let mut bindings = Self::default();
        for (control, key) in entries {
            if let Some(control) = Control::ALL.into_iter().find(|c| c.setting() == control)
                && let Some(key) = key_from_name(key)
            {
                bindings.bind(control, key);
            }
        }
        bindings
    }

    /// The entries to save: the controls that differ from the defaults.
    pub(super) fn to_settings(&self) -> Vec<(String, String)> {
        Control::ALL
            .into_iter()
            .filter(|control| self.key(*control) != control.default_key())
            .filter_map(|control| {
                key_name(self.key(control))
                    .map(|name| (control.setting().to_owned(), name.to_owned()))
            })
            .collect()
    }

    /// The keys that act on the open cheat menu, and whether holding one
    /// repeats: the keys bound to the d-pad, A and B, and the arrow keys,
    /// Enter and Backspace unless they are bound to something else.
    pub(super) fn menu_keys(&self) -> Vec<(Key, MenuNav, bool)> {
        let bound = [
            (Control::Up, MenuNav::Up, true),
            (Control::Down, MenuNav::Down, true),
            (Control::Left, MenuNav::Left, true),
            (Control::Right, MenuNav::Right, true),
            (Control::A, MenuNav::Toggle, false),
            (Control::B, MenuNav::Back, false),
        ];
        let fixed = [
            (Key::Up, MenuNav::Up, true),
            (Key::Down, MenuNav::Down, true),
            (Key::Left, MenuNav::Left, true),
            (Key::Right, MenuNav::Right, true),
            (Key::Enter, MenuNav::Toggle, false),
            (Key::Backspace, MenuNav::Back, false),
        ];
        let mut keys: Vec<_> = bound
            .into_iter()
            .map(|(control, nav, repeats)| (self.key(control), nav, repeats))
            .collect();
        for entry in fixed {
            if !keys.iter().any(|(key, ..)| *key == entry.0) {
                keys.push(entry);
            }
        }
        keys
    }
}

/// Save-state keys: 0 writes the state, 1 plays it back.
pub(super) const SAVE_STATE_KEY: Key = Key::Key0;
pub(super) const LOAD_STATE_KEY: Key = Key::Key1;

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
    fn escape_leaves_full_screen_and_only_quits_from_a_window() {
        assert_eq!(escape_action(false, false), EscapeAction::None);
        assert_eq!(escape_action(false, true), EscapeAction::None);
        assert_eq!(escape_action(true, true), EscapeAction::LeaveFullscreen);
        assert_eq!(escape_action(true, false), EscapeAction::Quit);
    }

    #[test]
    fn either_shift_key_holds_fast_forward_until_its_key_is_changed() {
        let mut bindings = Bindings::default();
        assert!(bindings.fast_forward_held(|key| key == Key::LeftShift));
        assert!(bindings.fast_forward_held(|key| key == Key::RightShift));
        assert!(!bindings.fast_forward_held(|key| key == Key::Space));
        // Shift is not a game button, so holding it never presses one.
        assert_eq!(bindings.buttons(|key| key == Key::LeftShift), 0);
        // With another key, only that key does it.
        bindings.bind(Control::FastForward, Key::F);
        assert!(bindings.fast_forward_held(|key| key == Key::F));
        assert!(!bindings.fast_forward_held(|key| key == Key::RightShift));
        assert!(!bindings.fast_forward_held(|key| key == Key::LeftShift));
    }

    #[test]
    fn the_cheat_menu_and_fast_forward_press_no_console_button() {
        let bindings = Bindings::default();
        assert_eq!(bindings.key(Control::CheatMenu), Key::Tab);
        assert_eq!(bindings.buttons(|key| key == Key::Tab), 0);
        assert_eq!(Control::CheatMenu.button(), None);
        assert_eq!(Control::FastForward.button(), None);
        // Every other control is a distinct console button.
        let all = Control::ALL
            .iter()
            .filter_map(|c| c.button())
            .fold(0, |m, b| m | b);
        assert_eq!(all, input::ALL);
        // A button can take the cheat menu's key, and the two swap.
        let mut bindings = Bindings::default();
        assert_eq!(
            bindings.bind(Control::Start, Key::Tab),
            Bound::Swapped(Control::CheatMenu)
        );
        assert_eq!(bindings.key(Control::CheatMenu), Key::X);
        assert_eq!(bindings.buttons(|key| key == Key::Tab), input::START);
    }

    #[test]
    fn the_default_keys_map_to_gba_buttons() {
        let bindings = Bindings::default();
        let held = [Key::Space, Key::X, Key::A, Key::E];
        assert_eq!(
            bindings.buttons(|key| held.contains(&key)),
            input::A | input::START | input::LEFT | input::R
        );
        // Every control has its own key and its own button.
        assert_eq!(bindings.buttons(|_| true), input::ALL);
        assert_eq!(bindings.buttons(|_| false), 0);
        let keys: Vec<Key> = Control::ALL.iter().map(|c| bindings.key(*c)).collect();
        for (index, key) in keys.iter().enumerate() {
            assert!(!keys[index + 1..].contains(key), "{key:?} twice");
        }
        assert_eq!(
            bindings.buttons(|key| key == Key::Left || key == Key::Enter),
            0
        );
        assert!(bindings.is_default());
    }

    #[test]
    fn binding_a_key_moves_the_button_and_swaps_with_its_old_owner() {
        let mut bindings = Bindings::default();
        assert_eq!(bindings.bind(Control::A, Key::Z), Bound::Done);
        assert_eq!(bindings.key(Control::A), Key::Z);
        assert_eq!(
            bindings.buttons(|key| key == Key::Space),
            0,
            "the old key is free"
        );
        assert_eq!(bindings.buttons(|key| key == Key::Z), input::A);
        // W is Up's: Up takes A's key (Z), so the keys stay one per control.
        assert_eq!(
            bindings.bind(Control::A, Key::W),
            Bound::Swapped(Control::Up)
        );
        assert_eq!(bindings.key(Control::A), Key::W);
        assert_eq!(bindings.key(Control::Up), Key::Z);
        assert!(!bindings.is_default());
        // Binding the key a control already has changes nothing.
        let before = bindings.clone();
        assert_eq!(bindings.bind(Control::A, Key::W), Bound::Done);
        assert_eq!(bindings, before);
    }

    #[test]
    fn the_emulators_own_keys_cannot_be_bound() {
        let mut bindings = Bindings::default();
        for key in [
            Key::Escape,
            Key::Key0,
            Key::Key1,
            Key::F11,
            Key::LeftCtrl,
            Key::LeftAlt,
            Key::LeftSuper,
            Key::Unknown,
        ] {
            assert_eq!(bindings.bind(Control::A, key), Bound::Refused, "{key:?}");
        }
        assert!(bindings.is_default());
    }

    #[test]
    fn every_bindable_key_has_a_unique_name_and_a_drawable_label() {
        for (index, (key, name)) in BINDABLE_KEYS.iter().enumerate() {
            assert!(
                BINDABLE_KEYS[index + 1..]
                    .iter()
                    .all(|(other, other_name)| other != key && other_name != name),
                "{name} is listed twice"
            );
            assert_eq!(key_from_name(name), Some(*key));
            // The menu font has capitals, digits and spaces.
            assert!(
                key_label(*key)
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' '),
                "{name}"
            );
        }
        // Every default is bindable.
        for control in Control::ALL {
            assert!(key_name(control.default_key()).is_some(), "{control:?}");
        }
        assert_eq!(key_label(Key::Left), "LEFT ARROW");
        assert_eq!(key_label(Key::Space), "SPACE");
    }

    #[test]
    fn bindings_round_trip_through_the_settings_and_only_changes_are_saved() {
        assert!(Bindings::default().to_settings().is_empty());
        let mut bindings = Bindings::default();
        bindings.bind(Control::A, Key::Z);
        bindings.bind(Control::Start, Key::Enter);
        bindings.bind(Control::Left, Key::Left);
        let saved = bindings.to_settings();
        assert_eq!(saved.len(), 3);
        assert_eq!(Bindings::from_settings(&saved), bindings);
    }

    #[test]
    fn unusable_settings_entries_are_skipped_and_keys_stay_unique() {
        let entries = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect()
        };
        let bindings = Bindings::from_settings(&entries(&[
            ("a", "escape"),
            ("nonsense", "z"),
            ("b", "no_such_key"),
            ("start", "z"),
        ]));
        assert_eq!(bindings.key(Control::A), Key::Space, "escape is refused");
        assert_eq!(bindings.key(Control::B), Key::C);
        assert_eq!(bindings.key(Control::Start), Key::Z);
        // Two entries for one key: the later takes it, the earlier owner gets
        // the other's key.
        let bindings = Bindings::from_settings(&entries(&[("a", "z"), ("b", "z")]));
        assert_eq!(bindings.key(Control::B), Key::Z);
        assert_eq!(bindings.key(Control::A), Key::C);
    }

    #[test]
    fn the_cheat_menu_keys_follow_the_bindings() {
        let nav_of = |keys: &[(Key, MenuNav, bool)], key: Key| {
            keys.iter()
                .find(|(k, ..)| *k == key)
                .map(|(_, nav, _)| *nav)
        };
        let keys = Bindings::default().menu_keys();
        assert_eq!(nav_of(&keys, Key::W), Some(MenuNav::Up));
        assert_eq!(nav_of(&keys, Key::Space), Some(MenuNav::Toggle));
        assert_eq!(nav_of(&keys, Key::C), Some(MenuNav::Back));
        assert_eq!(nav_of(&keys, Key::Down), Some(MenuNav::Down));
        assert_eq!(nav_of(&keys, Key::Enter), Some(MenuNav::Toggle));
        assert_eq!(nav_of(&keys, Key::Backspace), Some(MenuNav::Back));
        assert_eq!(keys.len(), 12);

        let mut bindings = Bindings::default();
        bindings.bind(Control::Up, Key::I);
        bindings.bind(Control::Left, Key::Up);
        let keys = bindings.menu_keys();
        assert_eq!(nav_of(&keys, Key::I), Some(MenuNav::Up));
        assert_eq!(
            nav_of(&keys, Key::W),
            None,
            "W is the swapped control's now"
        );
        // The arrow key is bound to Left, so it does not also move up.
        assert_eq!(nav_of(&keys, Key::Up), Some(MenuNav::Left));
        for (index, (key, ..)) in keys.iter().enumerate() {
            assert!(keys[index + 1..].iter().all(|(other, ..)| other != key));
        }
    }
}
