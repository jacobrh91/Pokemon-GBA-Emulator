//! A volume control that slides up from the bottom of the window while the
//! mouse hovers there and slides away again when it leaves.
//!
//! Interaction logic ([`VolumeDock::update`]) is separate from drawing
//! ([`VolumeDock::draw`]) so it can be unit tested without a window.

/// Height of the hover strip along the bottom of the window that reveals the dock.
const HOVER_ZONE: f32 = 110.0;
/// Fraction of the slide animation advanced per frame.
const REVEAL_STEP: f32 = 0.18;
const WHEEL_STEP: f32 = 0.05;

const DOCK_WIDTH: i32 = 440;
const DOCK_HEIGHT: i32 = 60;
const DOCK_MARGIN: i32 = 18;
const ICON_SIZE: i32 = 30;
const TRACK_WIDTH: i32 = 250;
const TRACK_HEIGHT: i32 = 8;
const KNOB_RADIUS: i32 = 10;

const DOCK_COLOR: u32 = 0x0020_2428;
const DOCK_ALPHA: f32 = 0.88;
const TRACK_COLOR: u32 = 0x0059_6168;
const FILL_COLOR: u32 = 0x0046_B4FF;
const MUTED_FILL_COLOR: u32 = 0x0090_9498;
const KNOB_COLOR: u32 = 0x00F4_F6F8;
const ICON_COLOR: u32 = 0x00F4_F6F8;
const MUTE_SLASH_COLOR: u32 = 0x00FF_5A5A;

#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    /// Cursor position in window pixels, or `None` when outside the window.
    pub position: Option<(f32, f32)>,
    pub left_down: bool,
    /// Vertical wheel movement since the last frame (positive = up).
    pub scroll: f32,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Rect {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x as f32
            && x < (self.x + self.width) as f32
            && y >= self.y as f32
            && y < (self.y + self.height) as f32
    }

    fn inflated(&self, by: i32) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            width: self.width + by * 2,
            height: self.height + by * 2,
        }
    }
}

/// Element rectangles for a fully revealed dock in a `width`×`height` window.
#[derive(Clone, Copy, Debug)]
struct Layout {
    dock: Rect,
    icon: Rect,
    track: Rect,
    text_x: i32,
    text_y: i32,
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
        let icon = Rect {
            x: dock.x + 20,
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
        Self {
            dock,
            icon,
            track,
            text_x: track.x + TRACK_WIDTH + 26,
            text_y: center_y - (GLYPH_HEIGHT * TEXT_SCALE) / 2,
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
}

impl VolumeDock {
    pub fn new(initial_volume: f32) -> Self {
        Self {
            volume: initial_volume.clamp(0.0, 1.0),
            muted: false,
            reveal: 0.0,
            dragging: false,
            was_down: false,
        }
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

        let hovering = pointer
            .position
            .is_some_and(|(_, y)| y >= height as f32 - HOVER_ZONE);
        let target = if hovering || self.dragging { 1.0 } else { 0.0 };
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
                if layout.icon.inflated(6).contains(x, y) {
                    self.muted = !self.muted;
                } else if layout.track.inflated(KNOB_RADIUS).contains(x, y) {
                    self.dragging = true;
                    self.muted = false;
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
        let mut canvas = Canvas {
            buffer,
            width,
            height,
            offset_y: slide,
        };

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

        // Percentage.
        let percent = if self.muted {
            0
        } else {
            (self.volume * 100.0).round() as u32
        };
        canvas.text(&format!("{percent}%"), layout.text_x, layout.text_y);
    }
}

struct Canvas<'a> {
    buffer: &'a mut [u32],
    width: usize,
    height: usize,
    /// Extra downward shift used for the slide animation.
    offset_y: i32,
}

impl Canvas<'_> {
    fn blend(&mut self, x: i32, y: i32, color: u32, alpha: f32) {
        let y = y + self.offset_y;
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let pixel = &mut self.buffer[y as usize * self.width + x as usize];
        if alpha >= 1.0 {
            *pixel = color;
            return;
        }
        let mix = |shift: u32| {
            let dst = ((*pixel >> shift) & 0xFF) as f32;
            let src = ((color >> shift) & 0xFF) as f32;
            (dst + (src - dst) * alpha).round() as u32
        };
        *pixel = (mix(16) << 16) | (mix(8) << 8) | mix(0);
    }

    fn rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: u32) {
        for row in y..y + height {
            for column in x..x + width {
                self.blend(column, row, color, 1.0);
            }
        }
    }

    fn rounded_rect(&mut self, rect: Rect, radius: i32, color: u32, alpha: f32) {
        let radius = radius.min(rect.width / 2).min(rect.height / 2);
        for row in 0..rect.height {
            for column in 0..rect.width {
                // Distance outside the nearest corner circle, if any.
                let dx = (radius - 1 - column)
                    .max(column - (rect.width - radius))
                    .max(0);
                let dy = (radius - 1 - row).max(row - (rect.height - radius)).max(0);
                if dx * dx + dy * dy <= radius * radius {
                    self.blend(rect.x + column, rect.y + row, color, alpha);
                }
            }
        }
    }

    fn circle(&mut self, cx: i32, cy: i32, radius: i32, color: u32) {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    self.blend(cx + dx, cy + dy, color, 1.0);
                }
            }
        }
    }

    /// Right-facing arc segment used for the speaker's sound waves.
    fn arc(&mut self, cx: i32, cy: i32, radius: i32, color: u32) {
        for dy in -radius..=radius {
            for dx in 1..=radius {
                let distance = ((dx * dx + dy * dy) as f32).sqrt();
                // Keep a 55 degree wedge around the horizontal axis.
                if (distance - radius as f32).abs() < 1.2 && dy.abs() as f32 <= distance * 0.82 {
                    self.rect(cx + dx, cy + dy, 2, 2, color);
                }
            }
        }
    }

    fn text(&mut self, text: &str, x: i32, y: i32) {
        let mut cursor = x;
        for character in text.chars() {
            if let Some(glyph) = glyph(character) {
                for (row, bits) in glyph.iter().enumerate() {
                    for column in 0..GLYPH_WIDTH {
                        if bits & (1 << (GLYPH_WIDTH - 1 - column)) != 0 {
                            self.rect(
                                cursor + column * TEXT_SCALE,
                                y + row as i32 * TEXT_SCALE,
                                TEXT_SCALE,
                                TEXT_SCALE,
                                ICON_COLOR,
                            );
                        }
                    }
                }
            }
            cursor += (GLYPH_WIDTH + 1) * TEXT_SCALE;
        }
    }
}

const GLYPH_WIDTH: i32 = 3;
const GLYPH_HEIGHT: i32 = 5;
const TEXT_SCALE: i32 = 3;

/// 3x5 bitmap glyphs for digits and `%`, one byte per row (low 3 bits used).
fn glyph(character: char) -> Option<[u8; 5]> {
    Some(match character {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        _ => return None,
    })
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
    fn dock_is_hidden_until_the_bottom_strip_is_hovered() {
        let mut dock = VolumeDock::new(0.5);
        settle(&mut dock, hover(480.0, 200.0));
        assert_eq!(dock.reveal(), 0.0);

        settle(&mut dock, hover(480.0, H as f32 - 20.0));
        assert_eq!(dock.reveal(), 1.0);

        settle(&mut dock, hover(480.0, 200.0));
        assert_eq!(dock.reveal(), 0.0);
        settle(&mut dock, Pointer::default());
        assert_eq!(dock.reveal(), 0.0);
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
}
