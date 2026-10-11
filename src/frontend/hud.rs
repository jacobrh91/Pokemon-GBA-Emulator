//! On-screen status indicators drawn over the game.

use crate::frontend::draw::{Canvas, GLYPH_ADVANCE, GLYPH_HEIGHT, Rect, TEXT_SCALE, text_width};

const MARGIN: i32 = 14;
const PADDING_X: i32 = 12;
const HEIGHT: i32 = GLYPH_HEIGHT * TEXT_SCALE + 16;
const BACKGROUND: u32 = 0x0020_2428;
const BACKGROUND_ALPHA: f32 = 0.78;
const ICON_COLOR: u32 = 0x0046_B4FF;
const TEXT_COLOR: u32 = 0x00F4_F6F8;

/// Which save-state operation just completed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateNotice {
    /// A state was written: a floppy disk.
    Saved,
    /// A state was restored: a floppy disk with a circular arrow.
    Loaded,
}

/// Side of the rounded chip behind the save-state icon.
const STATE_CHIP: i32 = 52;
/// Pixels per unit of the 14x14-unit floppy-disk drawing.
const DISK_UNIT: i32 = 2;
const DISK_COLOR: u32 = 0x0046_B4FF;
const DISK_SHUTTER_COLOR: u32 = 0x00D5_DAE0;
const DISK_LABEL_COLOR: u32 = 0x00F4_F6F8;

/// Draws the save-state icon in the top-right corner: a floppy disk, with a
/// circular arrow badge when a state was restored instead of written.
pub fn draw_state_notice(buffer: &mut [u32], width: usize, height: usize, notice: StateNotice) {
    let chip = Rect {
        x: width as i32 - MARGIN - STATE_CHIP,
        y: MARGIN,
        width: STATE_CHIP,
        height: STATE_CHIP,
    };
    let mut canvas = Canvas::new(buffer, width, height, 0);
    canvas.rounded_rect(chip, 10, BACKGROUND, BACKGROUND_ALPHA);

    let unit = DISK_UNIT;
    let x = chip.x + (STATE_CHIP - 14 * unit) / 2;
    let y = chip.y + (STATE_CHIP - 14 * unit) / 2;
    // Body, with the top-right corner cut off.
    canvas.rect(x, y, 12 * unit, 14 * unit, DISK_COLOR);
    canvas.rect(x + 12 * unit, y + 2 * unit, 2 * unit, 12 * unit, DISK_COLOR);
    // Metal shutter with its slot, and the paper label.
    canvas.rect(x + 3 * unit, y, 7 * unit, 4 * unit, DISK_SHUTTER_COLOR);
    canvas.rect(x + 7 * unit, y + unit, 2 * unit, 2 * unit, DISK_COLOR);
    canvas.rect(
        x + 2 * unit,
        y + 8 * unit,
        10 * unit,
        6 * unit,
        DISK_LABEL_COLOR,
    );

    if notice == StateNotice::Loaded {
        let (cx, cy) = (x + 14 * unit - 1, y + 14 * unit - 1);
        canvas.circle(cx, cy, 11, BACKGROUND);
        canvas.circle(cx, cy, 9, DISK_COLOR);
        canvas.circular_arrow(cx, cy, 6, TEXT_COLOR);
    }
}

/// Formats a speed multiple for display: one decimal below 10x, whole
/// numbers above, and capped so the badge never grows unbounded.
pub fn format_speed(factor: f32) -> String {
    let factor = if factor.is_finite() {
        factor.max(0.0)
    } else {
        0.0
    };
    if factor >= 99.5 {
        "99x+".to_owned()
    } else if factor >= 9.95 {
        format!("{factor:.0}x")
    } else {
        format!("{factor:.1}x")
    }
}

/// Draws the fast-forward badge (a `>>` icon and the speed multiple) in the
/// top-left corner of the frame.
pub fn draw_fast_forward(buffer: &mut [u32], width: usize, height: usize, factor: f32) {
    let label = format_speed(factor);
    let glyph_advance = GLYPH_ADVANCE;
    let icon_width = text_width(">>");
    let label_width = text_width(&label);
    let badge = Rect {
        x: MARGIN,
        y: MARGIN,
        width: PADDING_X * 2 + icon_width + glyph_advance / 2 + label_width,
        height: HEIGHT,
    };

    let mut canvas = Canvas::new(buffer, width, height, 0);
    canvas.rounded_rect(badge, 10, BACKGROUND, BACKGROUND_ALPHA);
    let text_y = badge.y + (HEIGHT - GLYPH_HEIGHT * TEXT_SCALE) / 2;
    canvas.text(">>", badge.x + PADDING_X, text_y, ICON_COLOR);
    canvas.text(
        &label,
        badge.x + PADDING_X + icon_width + glyph_advance / 2,
        text_y,
        TEXT_COLOR,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_is_formatted_compactly() {
        assert_eq!(format_speed(1.0), "1.0x");
        assert_eq!(format_speed(3.74), "3.7x");
        assert_eq!(format_speed(9.96), "10x");
        assert_eq!(format_speed(12.4), "12x");
        assert_eq!(format_speed(250.0), "99x+");
        assert_eq!(format_speed(f32::NAN), "0.0x");
        assert_eq!(format_speed(-2.0), "0.0x");
    }

    #[test]
    fn badge_is_drawn_in_the_top_left_and_nowhere_else() {
        let (width, height) = (960, 640);
        let background = 0x0010_2030u32;
        let mut buffer = vec![background; width * height];
        draw_fast_forward(&mut buffer, width, height, 3.7);

        let changed: Vec<usize> = buffer
            .iter()
            .enumerate()
            .filter(|(_, pixel)| **pixel != background)
            .map(|(index, _)| index)
            .collect();
        assert!(!changed.is_empty());
        let (min_x, max_x) = changed.iter().fold((usize::MAX, 0), |(lo, hi), i| {
            (lo.min(i % width), hi.max(i % width))
        });
        let max_y = changed.iter().map(|i| i / width).max().unwrap();
        assert!(min_x >= MARGIN as usize);
        assert!(max_x < 200, "badge stays compact: {max_x}");
        assert!(max_y < 80, "badge stays in the top-left corner: {max_y}");
        // The icon uses the accent color somewhere.
        assert!(buffer.contains(&ICON_COLOR));
        assert!(buffer.contains(&TEXT_COLOR));
    }

    fn changed_pixels(notice: StateNotice) -> Vec<usize> {
        let (width, height) = (960, 640);
        let mut buffer = vec![0u32; width * height];
        draw_state_notice(&mut buffer, width, height, notice);
        buffer
            .iter()
            .enumerate()
            .filter(|(_, pixel)| **pixel != 0)
            .map(|(index, _)| index)
            .collect()
    }

    #[test]
    fn state_icon_sits_in_the_top_right_corner_only() {
        let width = 960;
        for notice in [StateNotice::Saved, StateNotice::Loaded] {
            let changed = changed_pixels(notice);
            assert!(!changed.is_empty());
            let min_x = changed.iter().map(|i| i % width).min().unwrap();
            let max_x = changed.iter().map(|i| i % width).max().unwrap();
            let max_y = changed.iter().map(|i| i / width).max().unwrap();
            assert!(min_x >= width - 100, "icon hugs the right edge: {min_x}");
            assert!(max_x < width - MARGIN as usize + 1);
            assert!(max_y < 100, "icon stays in the top corner: {max_y}");
        }
    }

    #[test]
    fn restoring_adds_a_circular_arrow_badge_to_the_disk() {
        let (width, height) = (960, 640);
        let draw = |notice| {
            let mut buffer = vec![0u32; width * height];
            draw_state_notice(&mut buffer, width, height, notice);
            buffer
        };
        let saved = draw(StateNotice::Saved);
        let loaded = draw(StateNotice::Loaded);
        let differing: Vec<usize> = (0..saved.len())
            .filter(|i| saved[*i] != loaded[*i])
            .collect();
        assert!(!differing.is_empty(), "the two icons must look different");
        // The badge sits on the disk's lower-right corner, inside the chip.
        let chip_x = width - MARGIN as usize - STATE_CHIP as usize;
        let chip_y = MARGIN as usize;
        for index in differing {
            let (x, y) = (index % width, index / width);
            assert!(x >= chip_x + STATE_CHIP as usize / 2 - 4, "badge x {x}");
            assert!(y >= chip_y + STATE_CHIP as usize / 2 - 4, "badge y {y}");
            assert!(x < chip_x + STATE_CHIP as usize && y < chip_y + STATE_CHIP as usize);
        }
    }

    #[test]
    fn wider_labels_make_a_wider_badge() {
        let (width, height) = (960, 640);
        let widest = |factor: f32| {
            let mut buffer = vec![0u32; width * height];
            draw_fast_forward(&mut buffer, width, height, factor);
            buffer
                .iter()
                .enumerate()
                .filter(|(_, pixel)| **pixel != 0)
                .map(|(index, _)| index % width)
                .max()
                .unwrap()
        };
        // "3.7x" has four characters, "12x" three.
        assert!(widest(3.7) > widest(12.0));
    }
}
