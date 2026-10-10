//! Small software drawing helpers shared by the on-screen overlays (volume
//! dock, fast-forward indicator): rectangles, circles, alpha blending and a
//! 3x5 bitmap font.

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub(crate) fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x as f32
            && x < (self.x + self.width) as f32
            && y >= self.y as f32
            && y < (self.y + self.height) as f32
    }

    pub(crate) fn inflated(&self, by: i32) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            width: self.width + by * 2,
            height: self.height + by * 2,
        }
    }
}

pub(crate) struct Canvas<'a> {
    buffer: &'a mut [u32],
    width: usize,
    height: usize,
    /// Extra downward shift used for the slide animation.
    offset_y: i32,
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(buffer: &'a mut [u32], width: usize, height: usize, offset_y: i32) -> Self {
        Self {
            buffer,
            width,
            height,
            offset_y,
        }
    }
}

impl Canvas<'_> {
    pub(crate) fn blend(&mut self, x: i32, y: i32, color: u32, alpha: f32) {
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

    pub(crate) fn rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: u32) {
        for row in y..y + height {
            for column in x..x + width {
                self.blend(column, row, color, 1.0);
            }
        }
    }

    pub(crate) fn rounded_rect(&mut self, rect: Rect, radius: i32, color: u32, alpha: f32) {
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

    /// Fills the triangle `a`, `b`, `c` (any winding).
    pub(crate) fn triangle(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: u32) {
        let min_x = a.0.min(b.0).min(c.0).floor() as i32;
        let max_x = a.0.max(b.0).max(c.0).ceil() as i32;
        let min_y = a.1.min(b.1).min(c.1).floor() as i32;
        let max_y = a.1.max(b.1).max(c.1).ceil() as i32;
        let edge = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| {
            (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
        };
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let point = (x as f32 + 0.5, y as f32 + 0.5);
                let (w0, w1, w2) = (edge(b, c, point), edge(c, a, point), edge(a, b, point));
                let inside =
                    (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
                if inside {
                    self.blend(x, y, color, 1.0);
                }
            }
        }
    }

    /// A clockwise circular arrow: an arc covering most of a circle whose
    /// arrowhead points back toward the start of the arc, as in a
    /// "refresh" or "restart" icon.
    pub(crate) fn circular_arrow(&mut self, cx: i32, cy: i32, radius: i32, color: u32) {
        // Angles in degrees, 0 = right, increasing clockwise on screen.
        const START: f32 = 25.0;
        const END: f32 = 318.0;
        let r = radius as f32;
        let mut angle = START;
        while angle <= END {
            let (sin, cos) = angle.to_radians().sin_cos();
            self.rect(
                (cx as f32 + r * cos).round() as i32 - 1,
                (cy as f32 + r * sin).round() as i32 - 1,
                3,
                3,
                color,
            );
            angle += 2.0;
        }
        // Arrowhead at the end of the arc, pointing along the clockwise
        // tangent (back toward the start).
        let (sin, cos) = END.to_radians().sin_cos();
        let end = (cx as f32 + r * cos, cy as f32 + r * sin);
        let tangent = (-sin, cos);
        let normal = (cos, sin);
        let (length, half_width) = (r * 0.85, r * 0.62);
        self.triangle(
            (end.0 + tangent.0 * length, end.1 + tangent.1 * length),
            (end.0 + normal.0 * half_width, end.1 + normal.1 * half_width),
            (end.0 - normal.0 * half_width, end.1 - normal.1 * half_width),
            color,
        );
    }

    /// The standard power symbol: a ring open at the top with a vertical bar
    /// through the opening.
    pub(crate) fn power_symbol(&mut self, cx: i32, cy: i32, radius: i32, color: u32) {
        // Angles in degrees, 0 = right, clockwise on screen, so 270 is the
        // top. Leave a gap of 40 degrees either side of it.
        const GAP: f32 = 40.0;
        let r = radius as f32;
        let mut angle = 270.0 + GAP;
        while angle <= 270.0 + 360.0 - GAP {
            let (sin, cos) = angle.to_radians().sin_cos();
            self.rect(
                (cx as f32 + r * cos).round() as i32 - 1,
                (cy as f32 + r * sin).round() as i32 - 1,
                3,
                3,
                color,
            );
            angle += 2.0;
        }
        self.rect(cx - 1, cy - radius - 3, 3, radius + 3, color);
    }

    pub(crate) fn circle(&mut self, cx: i32, cy: i32, radius: i32, color: u32) {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    self.blend(cx + dx, cy + dy, color, 1.0);
                }
            }
        }
    }

    /// Right-facing arc segment used for the speaker's sound waves.
    pub(crate) fn arc(&mut self, cx: i32, cy: i32, radius: i32, color: u32) {
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

    /// Draws `text` in the built-in 3x5 font at [`TEXT_SCALE`].
    pub(crate) fn text(&mut self, text: &str, x: i32, y: i32, color: u32) {
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
                                color,
                            );
                        }
                    }
                }
            }
            cursor += (GLYPH_WIDTH + 1) * TEXT_SCALE;
        }
    }
}

pub(crate) const GLYPH_WIDTH: i32 = 3;
pub(crate) const GLYPH_HEIGHT: i32 = 5;
pub(crate) const TEXT_SCALE: i32 = 3;

/// 3x5 bitmap glyphs for digits and `% . x X M A >`, one byte per row (low 3 bits
/// used).
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
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        'x' => [0b000, 0b101, 0b010, 0b101, 0b000],
        '>' => [0b100, 0b010, 0b001, 0b010, 0b100],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(buffer: &[u32]) -> usize {
        buffer.iter().filter(|&&pixel| pixel != 0).count()
    }

    #[test]
    fn triangle_fills_its_interior_regardless_of_winding() {
        let (width, height) = (40, 40);
        let mut clockwise = vec![0u32; width * height];
        let mut counter = vec![0u32; width * height];
        let (a, b, c) = ((5.0, 5.0), (35.0, 5.0), (5.0, 35.0));
        Canvas::new(&mut clockwise, width, height, 0).triangle(a, b, c, 1);
        Canvas::new(&mut counter, width, height, 0).triangle(a, c, b, 1);

        assert_eq!(clockwise, counter);
        // Area is 450; pixel-centre sampling lands close to it.
        let filled = painted(&clockwise);
        assert!((430..=470).contains(&filled), "{filled}");
        assert_eq!(clockwise[10 * width + 10], 1, "interior");
        assert_eq!(clockwise[30 * width + 30], 0, "outside the hypotenuse");
    }

    #[test]
    fn circular_arrow_has_an_open_ring_and_an_arrowhead() {
        let (width, height) = (60, 60);
        let mut buffer = vec![0u32; width * height];
        Canvas::new(&mut buffer, width, height, 0).circular_arrow(30, 30, 16, 1);

        // Left, top and bottom of the ring are drawn...
        assert_eq!(buffer[30 * width + 14], 1);
        assert_eq!(buffer[14 * width + 30], 1);
        assert_eq!(buffer[46 * width + 30], 1);
        // ...the centre is empty, and there is a gap on the right where the
        // arrowhead meets the start of the arc.
        assert_eq!(buffer[30 * width + 30], 0);
        let gap = (29..=31).all(|y| buffer[y * width + 46] == 0);
        assert!(gap, "open gap on the right-hand side");
        // The head adds weight beyond the plain ring.
        assert!(painted(&buffer) > 100);
    }

    #[test]
    fn power_symbol_is_a_ring_open_at_the_top_with_a_bar_through_the_gap() {
        let (width, height) = (60, 60);
        let mut buffer = vec![0u32; width * height];
        Canvas::new(&mut buffer, width, height, 0).power_symbol(30, 32, 14, 1);

        // Ring at the sides and bottom; centre empty.
        assert_eq!(buffer[32 * width + 16], 1);
        assert_eq!(buffer[32 * width + 44], 1);
        assert_eq!(buffer[46 * width + 30], 1);
        assert_eq!(
            buffer[32 * width + 24],
            0,
            "inside the ring, left of the bar"
        );
        // The bar runs from above the ring down toward the centre...
        assert_eq!(buffer[16 * width + 30], 1);
        assert_eq!(buffer[28 * width + 30], 1);
        // ...and the ring itself is open on either side of it at the top.
        assert_eq!(buffer[18 * width + 24], 0);
        assert_eq!(buffer[18 * width + 36], 0);
    }
}
