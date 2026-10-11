//! Geometry of the dock's controls for a given window size.

use crate::frontend::draw::{GLYPH_HEIGHT, Rect, TEXT_SCALE};

pub(super) const DOCK_WIDTH: i32 = 663;
pub(super) const RESTART_SIZE: i32 = 38;
pub(super) const FULLSCREEN_SIZE: i32 = 38;
pub(super) const POWER_SIZE: i32 = 38;
pub(super) const DOCK_HEIGHT: i32 = 60;
pub(super) const DOCK_MARGIN: i32 = 18;
pub(super) const ICON_SIZE: i32 = 30;
pub(super) const TRACK_WIDTH: i32 = 55;
pub(super) const TRACK_HEIGHT: i32 = 8;
pub(super) const KNOB_RADIUS: i32 = 10;
pub(super) const SPEED_BUTTON_WIDTH: i32 = 58;
pub(super) const SPEED_BUTTON_HEIGHT: i32 = 34;
pub(super) const SPEED_BUTTON_GAP: i32 = 6;
pub(super) const DOCK_PADDING: i32 = 20;

/// Element rectangles for a fully revealed dock in a `width`×`height` window.
#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
    pub(super) dock: Rect,
    pub(super) icon: Rect,
    pub(super) track: Rect,
    pub(super) text_x: i32,
    pub(super) text_y: i32,
    /// Separator between the volume and speed sections.
    pub(super) divider_x: i32,
    pub(super) speed_buttons: [Rect; 4],
    /// Separator after the power and restart buttons.
    pub(super) restart_divider_x: i32,
    pub(super) restart: Rect,
    pub(super) fullscreen: Rect,
    pub(super) power: Rect,
}

impl Layout {
    pub(super) fn new(width: usize, height: usize) -> Self {
        let dock = Rect {
            x: (width as i32 - DOCK_WIDTH) / 2,
            y: height as i32 - DOCK_MARGIN - DOCK_HEIGHT,
            width: DOCK_WIDTH,
            height: DOCK_HEIGHT,
        };
        let center_y = dock.y + DOCK_HEIGHT / 2;
        // Left to right: power, restart, divider, then volume.
        let power = Rect {
            x: dock.x + DOCK_PADDING,
            y: center_y - POWER_SIZE / 2,
            width: POWER_SIZE,
            height: POWER_SIZE,
        };
        let restart = Rect {
            x: power.x + POWER_SIZE + 10,
            y: center_y - RESTART_SIZE / 2,
            width: RESTART_SIZE,
            height: RESTART_SIZE,
        };
        let fullscreen = Rect {
            x: restart.x + RESTART_SIZE + 10,
            y: center_y - FULLSCREEN_SIZE / 2,
            width: FULLSCREEN_SIZE,
            height: FULLSCREEN_SIZE,
        };
        let restart_divider_x = fullscreen.x + FULLSCREEN_SIZE + 13;
        let icon = Rect {
            x: restart_divider_x + 16,
            y: center_y - ICON_SIZE / 2,
            width: ICON_SIZE,
            height: ICON_SIZE,
        };
        let track = Rect {
            x: icon.x + ICON_SIZE + 22,
            y: center_y - TRACK_HEIGHT / 2,
            width: TRACK_WIDTH,
            height: TRACK_HEIGHT,
        };
        let buttons_width = 4 * SPEED_BUTTON_WIDTH + 3 * SPEED_BUTTON_GAP;
        let buttons_x = dock.x + DOCK_WIDTH - DOCK_PADDING - buttons_width;
        let speed_buttons = std::array::from_fn(|index| Rect {
            x: buttons_x + index as i32 * (SPEED_BUTTON_WIDTH + SPEED_BUTTON_GAP),
            y: center_y - SPEED_BUTTON_HEIGHT / 2,
            width: SPEED_BUTTON_WIDTH,
            height: SPEED_BUTTON_HEIGHT,
        });
        Self {
            dock,
            icon,
            track,
            text_x: track.x + TRACK_WIDTH + 26,
            text_y: center_y - (GLYPH_HEIGHT * TEXT_SCALE) / 2,
            divider_x: buttons_x - 14,
            speed_buttons,
            restart_divider_x,
            restart,
            fullscreen,
            power,
        }
    }
}
