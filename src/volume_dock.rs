//! A control dock (power, restart, full screen, volume, fast-forward speed) that slides up from
//! the bottom of the window whenever the mouse moves over it, and slides away
//! again once the mouse has been still for a moment or leaves the window.
//!
//! Interaction logic ([`VolumeDock::update`]) is separate from drawing
//! ([`VolumeDock::draw`]) so it can be unit tested without a window.

use crate::draw::{Canvas, GLYPH_HEIGHT, GLYPH_WIDTH, Rect, TEXT_SCALE};

/// Height of the strip along the bottom of the window that keeps the dock
/// open for as long as the mouse is anywhere in it.
const HOVER_ZONE: f32 = 110.0;
/// Frames of mouse stillness (about 2.5 s at 60 Hz) before the dock hides.
const HIDE_AFTER_IDLE_FRAMES: u32 = 150;
/// Fraction of the slide animation advanced per frame.
const REVEAL_STEP: f32 = 0.18;
const WHEEL_STEP: f32 = 0.05;

const DOCK_WIDTH: i32 = 663;
const RESTART_SIZE: i32 = 38;
const FULLSCREEN_SIZE: i32 = 38;
const POWER_SIZE: i32 = 38;
const POWER_COLOR: u32 = 0x00D9_3B3B;
const POWER_SYMBOL_COLOR: u32 = 0x00FF_FFFF;
const DOCK_HEIGHT: i32 = 60;
const DOCK_MARGIN: i32 = 18;
const ICON_SIZE: i32 = 30;
const TRACK_WIDTH: i32 = 55;
const TRACK_HEIGHT: i32 = 8;
const KNOB_RADIUS: i32 = 10;
const SPEED_BUTTON_WIDTH: i32 = 58;
const SPEED_BUTTON_HEIGHT: i32 = 34;
const SPEED_BUTTON_GAP: i32 = 6;
const DOCK_PADDING: i32 = 20;

const DOCK_COLOR: u32 = 0x0020_2428;
const DOCK_ALPHA: f32 = 0.88;
const TRACK_COLOR: u32 = 0x0059_6168;
const FILL_COLOR: u32 = 0x0046_B4FF;
const MUTED_FILL_COLOR: u32 = 0x0090_9498;
const KNOB_COLOR: u32 = 0x00F4_F6F8;
const ICON_COLOR: u32 = 0x00F4_F6F8;
const MUTE_SLASH_COLOR: u32 = 0x00FF_5A5A;

/// How fast the fast-forward key runs the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedSetting {
    X2,
    X4,
    X8,
    /// As fast as the machine allows.
    #[default]
    Max,
}

impl SpeedSetting {
    pub const ALL: [SpeedSetting; 4] = [Self::X2, Self::X4, Self::X8, Self::Max];

    pub fn label(self) -> &'static str {
        match self {
            Self::X2 => "2X",
            Self::X4 => "4X",
            Self::X8 => "8X",
            Self::Max => "MAX",
        }
    }

    /// The target multiple of real time, or `None` for unthrottled.
    pub fn multiple(self) -> Option<u32> {
        match self {
            Self::X2 => Some(2),
            Self::X4 => Some(4),
            Self::X8 => Some(8),
            Self::Max => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    /// Cursor position in window pixels, or `None` when outside the window.
    pub position: Option<(f32, f32)>,
    pub left_down: bool,
    /// Vertical wheel movement since the last frame (positive = up).
    pub scroll: f32,
}

/// Element rectangles for a fully revealed dock in a `width`×`height` window.
#[derive(Clone, Copy, Debug)]
struct Layout {
    dock: Rect,
    icon: Rect,
    track: Rect,
    text_x: i32,
    text_y: i32,
    /// Separator between the volume and speed sections.
    divider_x: i32,
    speed_buttons: [Rect; 4],
    /// Separator after the power and restart buttons.
    restart_divider_x: i32,
    restart: Rect,
    fullscreen: Rect,
    power: Rect,
}

impl Layout {
    fn new(width: usize, height: usize) -> Self {
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

pub struct VolumeDock {
    volume: f32,
    muted: bool,
    /// 0.0 hidden .. 1.0 fully shown.
    reveal: f32,
    dragging: bool,
    was_down: bool,
    speed: SpeedSetting,
    last_position: Option<(f32, f32)>,
    idle_frames: u32,
    restart_requested: bool,
    quit_requested: bool,
    fullscreen_requested: bool,
}

impl VolumeDock {
    pub fn new(initial_volume: f32) -> Self {
        Self {
            volume: initial_volume.clamp(0.0, 1.0),
            muted: false,
            reveal: 0.0,
            dragging: false,
            was_down: false,
            speed: SpeedSetting::default(),
            last_position: None,
            idle_frames: HIDE_AFTER_IDLE_FRAMES,
            restart_requested: false,
            quit_requested: false,
            fullscreen_requested: false,
        }
    }

    /// True once per click of the full-screen button.
    pub fn take_fullscreen_request(&mut self) -> bool {
        std::mem::take(&mut self.fullscreen_requested)
    }

    /// True once per click of the power button (the app should exit).
    pub fn take_quit_request(&mut self) -> bool {
        std::mem::take(&mut self.quit_requested)
    }

    /// True once per click of the restart button.
    pub fn take_restart_request(&mut self) -> bool {
        std::mem::take(&mut self.restart_requested)
    }

    /// The selected fast-forward speed.
    pub fn speed(&self) -> SpeedSetting {
        self.speed
    }

    #[cfg(test)]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    #[cfg(test)]
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    #[cfg(test)]
    pub fn reveal(&self) -> f32 {
        self.reveal
    }

    /// Linear gain to apply to the audio. The slider is perceptual, so the
    /// gain follows a squared curve.
    pub fn gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume * self.volume
        }
    }

    /// Advances the slide animation and processes pointer input for one frame.
    pub fn update(&mut self, pointer: Pointer, width: usize, height: usize) {
        let layout = Layout::new(width, height);

        // The dock follows the mouse: any movement over the window shows it,
        // staying near the bottom keeps it, and stillness hides it again.
        if pointer.position.is_some() && pointer.position != self.last_position {
            self.idle_frames = 0;
        } else {
            self.idle_frames = self.idle_frames.saturating_add(1);
        }
        self.last_position = pointer.position;
        let in_hover_zone = pointer
            .position
            .is_some_and(|(_, y)| y >= height as f32 - HOVER_ZONE);
        let active = pointer.position.is_some()
            && (in_hover_zone || self.idle_frames < HIDE_AFTER_IDLE_FRAMES);
        let target = if active || self.dragging { 1.0 } else { 0.0 };
        let delta: f32 = target - self.reveal;
        self.reveal = (self.reveal + delta.clamp(-REVEAL_STEP, REVEAL_STEP)).clamp(0.0, 1.0);

        let pressed = pointer.left_down && !self.was_down;
        self.was_down = pointer.left_down;
        if !pointer.left_down {
            self.dragging = false;
        }

        // Ignore input while the dock is still sliding mostly out of view.
        if self.reveal < 0.5 {
            return;
        }
        if let Some((x, y)) = pointer.position {
            if pressed {
                if layout.power.contains(x, y) {
                    self.quit_requested = true;
                } else if layout.restart.contains(x, y) {
                    self.restart_requested = true;
                } else if layout.fullscreen.contains(x, y) {
                    self.fullscreen_requested = true;
                } else if layout.icon.inflated(6).contains(x, y) {
                    self.muted = !self.muted;
                } else if layout.track.inflated(KNOB_RADIUS).contains(x, y) {
                    self.dragging = true;
                    self.muted = false;
                } else if let Some(index) = layout
                    .speed_buttons
                    .iter()
                    .position(|button| button.contains(x, y))
                {
                    self.speed = SpeedSetting::ALL[index];
                }
            }
            if pointer.scroll != 0.0 && layout.dock.contains(x, y) {
                self.volume = (self.volume + pointer.scroll.signum() * WHEEL_STEP).clamp(0.0, 1.0);
                self.muted = false;
            }
        }
        if self.dragging {
            if let Some((x, _)) = pointer.position {
                self.volume =
                    ((x - layout.track.x as f32) / layout.track.width as f32).clamp(0.0, 1.0);
            }
        }
    }

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
            let width = label.chars().count() as i32 * (GLYPH_WIDTH + 1) * TEXT_SCALE - TEXT_SCALE;
            canvas.text(
                label,
                button.x + (button.width - width) / 2,
                button.y + (button.height - GLYPH_HEIGHT * TEXT_SCALE) / 2,
                text,
            );
        }

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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 960;
    const H: usize = 640;

    fn hover(x: f32, y: f32) -> Pointer {
        Pointer {
            position: Some((x, y)),
            ..Pointer::default()
        }
    }

    fn settle(dock: &mut VolumeDock, pointer: Pointer) {
        for _ in 0..20 {
            dock.update(pointer, W, H);
        }
    }

    #[test]
    fn dock_appears_when_the_mouse_moves_anywhere_and_hides_when_it_rests() {
        let mut dock = VolumeDock::new(0.5);
        // Never seen the mouse: hidden.
        settle(&mut dock, Pointer::default());
        assert_eq!(dock.reveal(), 0.0);

        // Moving over the middle of the screen shows it.
        for step in 0..20 {
            dock.update(hover(300.0 + step as f32, 200.0), W, H);
        }
        assert_eq!(dock.reveal(), 1.0);

        // A mouse that has stopped moving hides it after the idle delay.
        for _ in 0..(HIDE_AFTER_IDLE_FRAMES + 30) {
            dock.update(hover(320.0, 200.0), W, H);
        }
        assert_eq!(dock.reveal(), 0.0);

        // Moving again brings it straight back.
        dock.update(hover(330.0, 210.0), W, H);
        for _ in 0..10 {
            dock.update(hover(330.0, 210.0), W, H);
        }
        assert!(dock.reveal() > 0.9);
    }

    #[test]
    fn a_continuously_moving_mouse_keeps_the_dock_open() {
        let mut dock = VolumeDock::new(0.5);
        for frame in 0..(HIDE_AFTER_IDLE_FRAMES * 3) {
            dock.update(hover(100.0 + (frame % 200) as f32, 150.0), W, H);
        }
        assert_eq!(dock.reveal(), 1.0);
    }

    #[test]
    fn resting_near_the_bottom_or_dragging_keeps_the_dock_open_and_leaving_hides_it() {
        let mut dock = VolumeDock::new(0.5);
        // Stationary inside the bottom strip: stays.
        for _ in 0..(HIDE_AFTER_IDLE_FRAMES * 2) {
            dock.update(hover(480.0, H as f32 - 20.0), W, H);
        }
        assert_eq!(dock.reveal(), 1.0);

        // Leaving the window hides it immediately (animation aside).
        settle(&mut dock, Pointer::default());
        assert_eq!(dock.reveal(), 0.0);
    }

    #[test]
    fn restart_button_requests_a_restart_once_per_click() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        let layout = Layout::new(W, H);
        let (x, y) = (
            layout.restart.x as f32 + 10.0,
            layout.restart.y as f32 + 10.0,
        );
        assert!(!dock.take_restart_request());

        let press = Pointer {
            position: Some((x, y)),
            left_down: true,
            scroll: 0.0,
        };
        dock.update(press, W, H);
        assert!(dock.take_restart_request());
        // Holding the button does not fire again; neither does the release.
        dock.update(press, W, H);
        assert!(!dock.take_restart_request());
        dock.update(hover(x, y), W, H);
        assert!(!dock.take_restart_request());
        // A second click does.
        dock.update(press, W, H);
        assert!(dock.take_restart_request());
    }

    #[test]
    fn controls_are_ordered_power_restart_volume_speed_inside_the_dock() {
        let layout = Layout::new(W, H);
        let dock_left = layout.dock.x;
        let dock_right = layout.dock.x + layout.dock.width;
        assert!(layout.power.x >= dock_left);
        assert!(layout.restart.x > layout.power.x + layout.power.width);
        assert!(layout.fullscreen.x > layout.restart.x + layout.restart.width);
        assert!(layout.restart_divider_x > layout.fullscreen.x + layout.fullscreen.width);
        assert!(layout.icon.x > layout.restart_divider_x);
        // Volume text, then the speed divider, then the four speed buttons.
        let text_end = layout.text_x + 4 * (GLYPH_WIDTH + 1) * TEXT_SCALE;
        assert!(text_end < layout.divider_x);
        let first = layout.speed_buttons[0];
        let last = layout.speed_buttons[3];
        assert!(first.x > layout.divider_x);
        assert!(last.x + last.width < dock_right);
        assert!(dock_left >= 0 && dock_right <= W as i32);
    }

    #[test]
    fn power_button_requests_quit_once_per_click_and_is_red() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        let layout = Layout::new(W, H);
        let (x, y) = (layout.power.x as f32 + 10.0, layout.power.y as f32 + 10.0);
        assert!(!dock.take_quit_request());

        let press = Pointer {
            position: Some((x, y)),
            left_down: true,
            scroll: 0.0,
        };
        dock.update(press, W, H);
        assert!(dock.take_quit_request());
        dock.update(press, W, H);
        assert!(!dock.take_quit_request(), "holding does not repeat");
        // The power click is not a restart click.
        assert!(!dock.take_restart_request());

        let mut buffer = vec![0u32; W * H];
        dock.draw(&mut buffer, W, H);
        let probe = (layout.power.y as usize + 6) * W + layout.power.x as usize + 6;
        assert_eq!(buffer[probe], POWER_COLOR, "the button face is red");
        // The white symbol is drawn on it.
        let has_symbol = (0..layout.power.height as usize).any(|row| {
            (0..layout.power.width as usize).any(|column| {
                buffer[(layout.power.y as usize + row) * W + layout.power.x as usize + column]
                    == POWER_SYMBOL_COLOR
            })
        });
        assert!(has_symbol);
    }

    #[test]
    fn reveal_animates_over_several_frames() {
        let mut dock = VolumeDock::new(0.5);
        dock.update(hover(480.0, H as f32 - 5.0), W, H);
        assert!(dock.reveal() > 0.0 && dock.reveal() < 1.0);
    }

    #[test]
    fn clicking_and_dragging_the_track_sets_the_volume() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        let layout = Layout::new(W, H);
        let y = layout.track.y as f32 + 4.0;
        let at = |fraction: f32| layout.track.x as f32 + layout.track.width as f32 * fraction;

        let press = |x: f32| Pointer {
            position: Some((x, y)),
            left_down: true,
            scroll: 0.0,
        };
        dock.update(press(at(0.25)), W, H);
        assert!((dock.volume() - 0.25).abs() < 0.01);
        dock.update(press(at(0.8)), W, H);
        assert!((dock.volume() - 0.8).abs() < 0.01);

        // Dragging past the end, and well above the dock, clamps and keeps going.
        dock.update(
            Pointer {
                position: Some((at(1.5), 100.0)),
                left_down: true,
                scroll: 0.0,
            },
            W,
            H,
        );
        assert_eq!(dock.volume(), 1.0);
        assert_eq!(dock.reveal(), 1.0, "dock stays open while dragging");

        // Releasing ends the drag.
        dock.update(hover(at(0.1), y), W, H);
        assert_eq!(dock.volume(), 1.0);
    }

    #[test]
    fn clicking_the_icon_toggles_mute_and_gain() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        let icon = Layout::new(W, H).icon;
        let (x, y) = (icon.x as f32 + 5.0, icon.y as f32 + 5.0);
        let press = Pointer {
            position: Some((x, y)),
            left_down: true,
            scroll: 0.0,
        };

        dock.update(press, W, H);
        assert!(dock.is_muted());
        assert_eq!(dock.gain(), 0.0);
        // Holding the button does not toggle again.
        dock.update(press, W, H);
        assert!(dock.is_muted());
        dock.update(hover(x, y), W, H);
        dock.update(press, W, H);
        assert!(!dock.is_muted());
        assert!((dock.gain() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn wheel_over_the_dock_adjusts_volume_and_unmutes() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        let layout = Layout::new(W, H);
        let over = (layout.dock.x as f32 + 200.0, layout.dock.y as f32 + 30.0);
        dock.update(
            Pointer {
                position: Some(over),
                left_down: false,
                scroll: 1.0,
            },
            W,
            H,
        );
        assert!((dock.volume() - 0.55).abs() < 1e-6);
    }

    #[test]
    fn input_is_ignored_while_the_dock_is_mostly_hidden() {
        let mut dock = VolumeDock::new(0.5);
        let layout = Layout::new(W, H);
        let icon = (layout.icon.x as f32 + 5.0, layout.icon.y as f32 + 5.0);
        // First frame of hover: reveal is ~0.18, below the interaction threshold.
        dock.update(
            Pointer {
                position: Some(icon),
                left_down: true,
                scroll: 0.0,
            },
            W,
            H,
        );
        assert!(!dock.is_muted());
    }

    #[test]
    fn draw_leaves_the_frame_untouched_when_hidden_and_paints_when_shown() {
        let mut dock = VolumeDock::new(0.5);
        let mut buffer = vec![0x0010_2030u32; W * H];
        dock.draw(&mut buffer, W, H);
        assert!(buffer.iter().all(|&p| p == 0x0010_2030));

        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        dock.draw(&mut buffer, W, H);
        assert!(buffer.iter().any(|&p| p != 0x0010_2030));
        // Pixels well above the dock are unaffected.
        assert_eq!(buffer[50 * W + 50], 0x0010_2030);
    }

    #[test]
    fn speed_defaults_to_max_and_clicking_a_button_selects_it() {
        let mut dock = VolumeDock::new(0.5);
        assert_eq!(dock.speed(), SpeedSetting::Max);
        assert_eq!(dock.speed().multiple(), None);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));

        let layout = Layout::new(W, H);
        for (index, setting) in SpeedSetting::ALL.into_iter().enumerate() {
            let button = layout.speed_buttons[index];
            let click = Pointer {
                position: Some((button.x as f32 + 10.0, button.y as f32 + 10.0)),
                left_down: true,
                scroll: 0.0,
            };
            dock.update(click, W, H);
            assert_eq!(dock.speed(), setting);
            // Release before the next click.
            dock.update(hover(button.x as f32 + 10.0, button.y as f32 + 10.0), W, H);
        }
        assert_eq!(SpeedSetting::X2.multiple(), Some(2));
        assert_eq!(SpeedSetting::X4.multiple(), Some(4));
        assert_eq!(SpeedSetting::X8.multiple(), Some(8));
    }

    #[test]
    fn speed_buttons_fit_inside_the_dock_without_overlapping_the_volume_controls() {
        let layout = Layout::new(W, H);
        let dock_right = layout.dock.x + layout.dock.width;
        let mut previous_right = layout.divider_x;
        for button in layout.speed_buttons {
            assert!(
                button.x > previous_right,
                "buttons are ordered and separated"
            );
            previous_right = button.x + button.width;
        }
        assert!(previous_right < dock_right);
        // The volume percentage text ends before the divider.
        let text_end = layout.text_x + 4 * (GLYPH_WIDTH + 1) * TEXT_SCALE;
        assert!(text_end < layout.divider_x);
        assert!(layout.dock.x >= 0 && dock_right <= W as i32);
    }

    #[test]
    fn selected_speed_is_highlighted_and_changing_volume_leaves_it_alone() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        let layout = Layout::new(W, H);
        let button = layout.speed_buttons[1]; // 4X
        dock.update(
            Pointer {
                position: Some((button.x as f32 + 5.0, button.y as f32 + 5.0)),
                left_down: true,
                scroll: 0.0,
            },
            W,
            H,
        );
        assert_eq!(dock.speed(), SpeedSetting::X4);

        let mut buffer = vec![0u32; W * H];
        dock.draw(&mut buffer, W, H);
        let corner = |index: usize| {
            let b = layout.speed_buttons[index];
            buffer[(b.y as usize + 2) * W + b.x as usize + b.width as usize / 2]
        };
        assert_eq!(corner(1), FILL_COLOR, "selected button uses the accent");
        assert_ne!(corner(0), FILL_COLOR);
        assert_ne!(corner(3), FILL_COLOR);

        // Scrolling the wheel changes volume only.
        dock.update(
            Pointer {
                position: Some((layout.dock.x as f32 + 100.0, layout.dock.y as f32 + 30.0)),
                left_down: false,
                scroll: 1.0,
            },
            W,
            H,
        );
        assert_eq!(dock.speed(), SpeedSetting::X4);
    }

    #[test]
    fn fullscreen_button_requests_a_toggle_once_per_click() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, H as f32 - 30.0));
        let layout = Layout::new(W, H);
        let (x, y) = (
            layout.fullscreen.x as f32 + 10.0,
            layout.fullscreen.y as f32 + 10.0,
        );
        assert!(!dock.take_fullscreen_request());

        let press = Pointer {
            position: Some((x, y)),
            left_down: true,
            scroll: 0.0,
        };
        dock.update(press, W, H);
        assert!(dock.take_fullscreen_request());
        dock.update(press, W, H);
        assert!(!dock.take_fullscreen_request(), "holding does not repeat");
        // It is neither a restart nor a quit.
        assert!(!dock.take_restart_request());
        assert!(!dock.take_quit_request());
    }
}
