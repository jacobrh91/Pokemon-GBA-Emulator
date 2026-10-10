//! On-screen status indicators drawn over the game.

use crate::draw::{Canvas, GLYPH_HEIGHT, GLYPH_WIDTH, Rect, TEXT_SCALE};

const MARGIN: i32 = 14;
const PADDING_X: i32 = 12;
const HEIGHT: i32 = GLYPH_HEIGHT * TEXT_SCALE + 16;
const BACKGROUND: u32 = 0x0020_2428;
const BACKGROUND_ALPHA: f32 = 0.78;
const ICON_COLOR: u32 = 0x0046_B4FF;
const TEXT_COLOR: u32 = 0x00F4_F6F8;

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
    let glyph_advance = (GLYPH_WIDTH + 1) * TEXT_SCALE;
    let icon_width = 2 * glyph_advance;
    let text_width = label.chars().count() as i32 * glyph_advance - TEXT_SCALE;
    let badge = Rect {
        x: MARGIN,
        y: MARGIN,
        width: PADDING_X * 2 + icon_width + glyph_advance / 2 + text_width,
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
        assert!(buffer.iter().any(|&pixel| pixel == ICON_COLOR));
        assert!(buffer.iter().any(|&pixel| pixel == TEXT_COLOR));
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
