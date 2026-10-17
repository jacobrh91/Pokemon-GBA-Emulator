//! The controls menu: a panel that opens above the dock from its cog button,
//! where each console button, the cheat menu and fast-forward can be given
//! another key.
//!
//! Clicking a button's key makes the menu wait for the next key press, which
//! becomes that button's key (Escape cancels). A key that was another button's
//! swaps with it. Interaction is separate from drawing (`draw.rs`) so it can be
//! tested without a window.

mod draw;

use super::Pointer;
use crate::frontend::draw::Rect;
use crate::frontend::input::{Bindings, Bound, Control};
use minifb::Key;

pub(super) const PANEL_WIDTH: i32 = 440;
const ROWS: usize = Control::ALL.len();
const PADDING: i32 = 24;
const ROW_PITCH: i32 = 30;
const BUTTON_HEIGHT: i32 = 26;
const KEY_BUTTON_WIDTH: i32 = 200;
const RESET_WIDTH: i32 = 260;
/// From the panel's top to the centre of the first row.
const FIRST_ROW: i32 = 84;
const PANEL_HEIGHT: i32 = FIRST_ROW + (ROWS as i32 - 1) * ROW_PITCH + 112;

/// Where every control sits for a given window and dock.
#[derive(Clone, Copy, Debug)]
pub(super) struct ControlsLayout {
    pub(super) panel: Rect,
    pub(super) title_y: i32,
    /// The key buttons, in the order of [`Control::ALL`].
    pub(super) keys: [Rect; ROWS],
    pub(super) reset: Rect,
    /// Where the line of hints and messages starts.
    pub(super) notice: (i32, i32),
}

impl ControlsLayout {
    /// The panel sits just above the dock, aligned to its right edge and kept
    /// inside the window.
    pub(super) fn new(width: usize, dock: Rect) -> Self {
        let x = (dock.x + dock.width - PANEL_WIDTH)
            .min(width as i32 - PANEL_WIDTH - 12)
            .max(12);
        let y = (dock.y - 12 - PANEL_HEIGHT).max(8);
        let panel = Rect {
            x,
            y,
            width: PANEL_WIDTH,
            height: PANEL_HEIGHT,
        };
        let keys = std::array::from_fn(|index| Rect {
            x: x + PANEL_WIDTH - PADDING - KEY_BUTTON_WIDTH,
            y: y + FIRST_ROW + index as i32 * ROW_PITCH - BUTTON_HEIGHT / 2,
            width: KEY_BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
        });
        let after_rows = y + FIRST_ROW + (ROWS as i32 - 1) * ROW_PITCH + BUTTON_HEIGHT / 2;
        Self {
            panel,
            title_y: y + 20,
            keys,
            reset: Rect {
                x: x + PADDING,
                y: after_rows + 18,
                width: RESET_WIDTH,
                height: BUTTON_HEIGHT,
            },
            notice: (x + PADDING, after_rows + 18 + BUTTON_HEIGHT + 18),
        }
    }
}

/// What the line under the list says besides the hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    None,
    /// The key pressed cannot be bound.
    Refused,
    /// The key was this control's, which took the old key.
    Swapped(Control),
}

pub(super) struct ControlsMenu {
    open: bool,
    /// The control waiting for its new key.
    waiting: Option<Control>,
    notice: Notice,
    bindings: Bindings,
    changed: bool,
}

impl ControlsMenu {
    pub(super) fn new() -> Self {
        Self {
            open: false,
            waiting: None,
            notice: Notice::None,
            bindings: Bindings::default(),
            changed: false,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn open(&mut self) {
        self.open = true;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.waiting = None;
        self.notice = Notice::None;
    }

    pub(super) fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// The control that waits for a key, if any.
    pub(super) fn waiting(&self) -> Option<Control> {
        self.waiting.filter(|_| self.open)
    }

    pub(super) fn notice(&self) -> Notice {
        self.notice
    }

    pub(super) fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    /// Replaces the bindings (from saved settings) without reporting a change.
    pub(super) fn set_bindings(&mut self, bindings: Bindings) {
        self.bindings = bindings;
    }

    /// True once after the bindings were changed in the menu.
    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// B or Escape: stop waiting for a key, or else close the menu.
    pub(super) fn back(&mut self) {
        if self.waiting.take().is_some() {
            self.notice = Notice::None;
        } else {
            self.close();
        }
    }

    /// A key was pressed: it goes to the control that waits for one.
    pub(super) fn key_pressed(&mut self, key: Key) {
        let Some(control) = self.waiting() else {
            return;
        };
        match self.bindings.bind(control, key) {
            Bound::Refused => self.notice = Notice::Refused,
            Bound::Done => {
                self.waiting = None;
                self.notice = Notice::None;
                self.changed = true;
            }
            Bound::Swapped(other) => {
                self.waiting = None;
                self.notice = Notice::Swapped(other);
                self.changed = true;
            }
        }
    }

    /// Handles the pointer for one frame; returns whether it is over the
    /// panel (so the dock does not also act on the click).
    pub(super) fn update(
        &mut self,
        pointer: Pointer,
        pressed: bool,
        layout: &ControlsLayout,
    ) -> bool {
        let Some((x, y)) = pointer.position.filter(|_| self.open) else {
            return false;
        };
        if !layout.panel.contains(x, y) {
            return false;
        }
        if pressed {
            if let Some(index) = layout.keys.iter().position(|rect| rect.contains(x, y)) {
                self.waiting = Some(Control::ALL[index]);
                self.notice = Notice::None;
            } else if layout.reset.contains(x, y) {
                self.bindings = Bindings::default();
                self.waiting = None;
                self.notice = Notice::None;
                self.changed = true;
            } else {
                self.waiting = None;
                self.notice = Notice::None;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
