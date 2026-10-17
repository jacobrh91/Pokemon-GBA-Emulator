//! Painting the controls menu.

use super::{ControlsLayout, ControlsMenu, Notice, PADDING};
use crate::frontend::dock::draw::{DOCK_COLOR, FILL_COLOR, ICON_COLOR, TRACK_COLOR};
use crate::frontend::draw::{Canvas, GLYPH_HEIGHT, Rect, TEXT_SCALE, text_width};
use crate::frontend::input::{Control, key_label};

/// Colour of a message that something went wrong.
const WARNING_COLOR: u32 = 0x00FF_8A5A;
const TEXT_HEIGHT: i32 = GLYPH_HEIGHT * TEXT_SCALE;
pub(super) const HINT: &str = "CLICK A KEY TO CHANGE IT";
pub(super) const WAITING: &str = "PRESS A KEY";
pub(super) const RESET: &str = "RESET TO DEFAULTS";

/// The text of the line under the list.
pub(super) fn notice_text(notice: Notice, waiting: bool) -> (String, bool) {
    match notice {
        Notice::Refused => ("THAT KEY CANNOT BE USED".to_owned(), true),
        Notice::Swapped(other) => (format!("SWAPPED WITH {}", other.label()), false),
        Notice::None if waiting => ("ESCAPE CANCELS".to_owned(), false),
        Notice::None => (HINT.to_owned(), false),
    }
}

impl ControlsMenu {
    /// Paints the menu over the frame.
    pub(in crate::frontend::dock) fn draw(&self, canvas: &mut Canvas, layout: &ControlsLayout) {
        canvas.rounded_rect(layout.panel, 14, DOCK_COLOR, 0.95);
        canvas.text(
            "CONTROLS",
            layout.panel.x + PADDING,
            layout.title_y,
            FILL_COLOR,
        );
        canvas.rect(
            layout.panel.x + PADDING,
            layout.title_y + TEXT_HEIGHT + 10,
            layout.panel.width - 2 * PADDING,
            2,
            TRACK_COLOR,
        );
        for (control, rect) in Control::ALL.into_iter().zip(layout.keys) {
            let waiting = self.waiting() == Some(control);
            canvas.text(
                control.label(),
                layout.panel.x + PADDING,
                rect.y + (rect.height - TEXT_HEIGHT) / 2,
                ICON_COLOR,
            );
            let (fill, alpha, text_color) = if waiting {
                (FILL_COLOR, 1.0, DOCK_COLOR)
            } else {
                (TRACK_COLOR, 0.55, ICON_COLOR)
            };
            canvas.rounded_rect(rect, 9, fill, alpha);
            let label = if waiting {
                WAITING.to_owned()
            } else {
                key_label(self.bindings().key(control))
            };
            centred_text(canvas, rect, &label, text_color);
        }
        canvas.rounded_rect(layout.reset, 9, TRACK_COLOR, 0.55);
        centred_text(canvas, layout.reset, RESET, ICON_COLOR);
        let (text, warning) = notice_text(self.notice(), self.waiting().is_some());
        canvas.text(
            &text,
            layout.notice.0,
            layout.notice.1,
            if warning { WARNING_COLOR } else { ICON_COLOR },
        );
    }
}

fn centred_text(canvas: &mut Canvas, rect: Rect, text: &str, color: u32) {
    canvas.text(
        text,
        rect.x + (rect.width - text_width(text)) / 2,
        rect.y + (rect.height - TEXT_HEIGHT) / 2,
        color,
    );
}
