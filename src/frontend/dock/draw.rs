//! Painting the dock over the game frame.

use super::cheat_menu::MenuLayout;
use super::controls_menu::ControlsLayout;
use super::layout::{DOCK_HEIGHT, DOCK_MARGIN, KNOB_RADIUS, Layout, TRACK_HEIGHT};
use super::{Dock, SpeedSetting};
use crate::frontend::draw::{Canvas, GLYPH_HEIGHT, Rect, TEXT_SCALE, text_width};

pub(super) const POWER_COLOR: u32 = 0x00D9_3B3B;
pub(super) const POWER_SYMBOL_COLOR: u32 = 0x00FF_FFFF;
pub(super) const DOCK_COLOR: u32 = 0x0020_2428;
pub(super) const DOCK_ALPHA: f32 = 0.88;
pub(super) const TRACK_COLOR: u32 = 0x0059_6168;
pub(super) const FILL_COLOR: u32 = 0x0046_B4FF;
pub(super) const MUTED_FILL_COLOR: u32 = 0x0090_9498;
pub(super) const KNOB_COLOR: u32 = 0x00F4_F6F8;
pub(super) const ICON_COLOR: u32 = 0x00F4_F6F8;
pub(super) const MUTE_SLASH_COLOR: u32 = 0x00FF_5A5A;

impl Dock {
    /// Composites the dock over `buffer` (`0x00RRGGBB` pixels, row-major).
    pub fn draw(&self, buffer: &mut [u32], width: usize, height: usize) {
        if self.reveal <= 0.0 {
            return;
        }
        let layout = Layout::new(width, height);
        let eased = 1.0 - (1.0 - self.reveal).powi(3);
        let slide = ((1.0 - eased) * (DOCK_HEIGHT + DOCK_MARGIN) as f32).round() as i32;
        let mut canvas = Canvas::new(buffer, width, height, slide);

        canvas.rounded_rect(layout.dock, 14, DOCK_COLOR, DOCK_ALPHA);

        // Speaker icon.
        let icon = layout.icon;
        let (ix, iy) = (icon.x, icon.y);
        canvas.rect(ix + 2, iy + 10, 8, 10, ICON_COLOR);
        for column in 0..10 {
            let half = 5 + column;
            canvas.rect(ix + 10 + column, iy + 15 - half, 1, half * 2, ICON_COLOR);
        }
        if self.muted || self.volume <= 0.0 {
            for step in 0..14 {
                canvas.rect(ix + 16 + step, iy + 8 + step, 3, 3, MUTE_SLASH_COLOR);
                canvas.rect(ix + 16 + step, iy + 21 - step, 3, 3, MUTE_SLASH_COLOR);
            }
        } else {
            let waves = if self.volume > 0.66 {
                3
            } else if self.volume > 0.33 {
                2
            } else {
                1
            };
            for wave in 0..waves {
                let radius = 8 + wave * 5;
                canvas.arc(ix + 14, iy + 15, radius, ICON_COLOR);
            }
        }

        // Slider.
        let track = layout.track;
        let filled = (track.width as f32 * self.volume).round() as i32;
        let fill_color = if self.muted {
            MUTED_FILL_COLOR
        } else {
            FILL_COLOR
        };
        canvas.rounded_rect(track, TRACK_HEIGHT / 2, TRACK_COLOR, 1.0);
        if filled > 0 {
            canvas.rounded_rect(
                Rect {
                    width: filled.max(TRACK_HEIGHT),
                    ..track
                },
                TRACK_HEIGHT / 2,
                fill_color,
                1.0,
            );
        }
        canvas.circle(
            track.x + filled,
            track.y + TRACK_HEIGHT / 2,
            KNOB_RADIUS,
            KNOB_COLOR,
        );

        // Fast-forward speed selector.
        canvas.rect(
            layout.divider_x,
            layout.dock.y + 14,
            2,
            DOCK_HEIGHT - 28,
            TRACK_COLOR,
        );
        for (index, setting) in SpeedSetting::ALL.into_iter().enumerate() {
            let button = layout.speed_buttons[index];
            let selected = setting == self.speed;
            let (fill, text) = if selected {
                (FILL_COLOR, DOCK_COLOR)
            } else {
                (TRACK_COLOR, ICON_COLOR)
            };
            canvas.rounded_rect(button, 9, fill, if selected { 1.0 } else { 0.55 });
            let label = setting.label();
            let width = text_width(label);
            canvas.text(
                label,
                button.x + (button.width - width) / 2,
                button.y + (button.height - GLYPH_HEIGHT * TEXT_SCALE) / 2,
                text,
            );
        }

        // The CHEATS button (lit while its menu is open), at the left end.
        let menu_open = self.menu.is_open();
        canvas.rounded_rect(
            layout.cheats,
            9,
            if menu_open { FILL_COLOR } else { TRACK_COLOR },
            if menu_open { 1.0 } else { 0.55 },
        );
        canvas.text(
            "CHEATS",
            layout.cheats.x + (layout.cheats.width - text_width("CHEATS")) / 2,
            layout.cheats.y + (layout.cheats.height - GLYPH_HEIGHT * TEXT_SCALE) / 2,
            if menu_open { DOCK_COLOR } else { ICON_COLOR },
        );
        let controls_open = self.controls.is_open();
        canvas.rounded_rect(
            layout.settings,
            9,
            if controls_open {
                FILL_COLOR
            } else {
                TRACK_COLOR
            },
            if controls_open { 1.0 } else { 0.55 },
        );
        canvas.gear(
            layout.settings.x + layout.settings.width / 2,
            layout.settings.y + layout.settings.height / 2,
            11,
            if controls_open {
                DOCK_COLOR
            } else {
                ICON_COLOR
            },
        );
        canvas.rect(
            layout.cheats_divider_x,
            layout.dock.y + 14,
            2,
            DOCK_HEIGHT - 28,
            TRACK_COLOR,
        );

        // Power (red) and restart (circular arrow), left of the volume.
        canvas.rounded_rect(layout.power, 9, POWER_COLOR, 1.0);
        canvas.power_symbol(
            layout.power.x + layout.power.width / 2,
            layout.power.y + layout.power.height / 2 + 2,
            9,
            POWER_SYMBOL_COLOR,
        );
        canvas.rect(
            layout.restart_divider_x,
            layout.dock.y + 14,
            2,
            DOCK_HEIGHT - 28,
            TRACK_COLOR,
        );
        canvas.rounded_rect(layout.restart, 9, TRACK_COLOR, 0.55);
        canvas.circular_arrow(
            layout.restart.x + layout.restart.width / 2,
            layout.restart.y + layout.restart.height / 2,
            10,
            ICON_COLOR,
        );
        canvas.rounded_rect(layout.fullscreen, 9, TRACK_COLOR, 0.55);
        canvas.corner_brackets(
            layout.fullscreen.x + layout.fullscreen.width / 2,
            layout.fullscreen.y + layout.fullscreen.height / 2,
            9,
            7,
            ICON_COLOR,
        );

        // Percentage.
        let percent = if self.muted {
            0
        } else {
            (self.volume * 100.0).round() as u32
        };
        canvas.text(
            &format!("{percent}%"),
            layout.text_x,
            layout.text_y,
            ICON_COLOR,
        );

        // The menus open above the dock and do not slide with it.
        if self.controls.is_open() {
            let mut menu_canvas = Canvas::new(buffer, width, height, 0);
            self.controls
                .draw(&mut menu_canvas, &ControlsLayout::new(width, layout.dock));
        }
        if self.menu.is_open() {
            let mut menu_canvas = Canvas::new(buffer, width, height, 0);
            self.menu.draw(
                &mut menu_canvas,
                &MenuLayout::new(width, layout.dock, self.menu.has_file_row()),
            );
        }
    }
}
