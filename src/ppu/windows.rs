//! Window layout for a scanline: which layers may draw at each x
//! (WIN0/WIN1/OBJ window/outside).

use super::*;

/// Bit 4 of a window layer mask: the OBJ layer (bits 0-3 are BG0-3).
pub(super) const OBJ_LAYER: u8 = 1 << 4;

/// Window layout for one scanline. `layers_at` returns which BG layers
/// (bits 0-3) may draw at a given x; the backdrop is never windowed.
pub(super) struct Windows {
    pub(super) enabled: bool,
    /// Horizontal span for WIN0/WIN1 when the line is inside their vertical span.
    pub(super) spans: [Option<(usize, usize)>; 2],
    pub(super) inside: [u8; 2],
    pub(super) outside: u8,
    pub(super) object_window_enabled: bool,
    /// Layer enables inside OBJ-window sprites (WINOUT high byte).
    pub(super) object_window: u8,
}

impl Windows {
    pub(super) fn for_line(registers: &[u8], display_control: u16, y: usize) -> Self {
        let window_enabled = [
            display_control & DISPCNT_WINDOW_0 != 0,
            display_control & DISPCNT_WINDOW_1 != 0,
        ];
        let enabled =
            window_enabled.iter().any(|&on| on) || display_control & DISPCNT_OBJ_WINDOW != 0;
        let inside_control = register(registers, WININ);
        let outside_control = register(registers, WINOUT);

        let mut spans = [None; 2];
        for (window, span) in spans.iter_mut().enumerate() {
            if !window_enabled[window] {
                continue;
            }
            let horizontal = register(registers, WINDOW_HORIZONTAL_BASE + window * 2);
            let vertical = register(registers, WINDOW_VERTICAL_BASE + window * 2);
            let (left, right) = window_range(horizontal, SCREEN_WIDTH);
            let (top, bottom) = window_range(vertical, SCREEN_HEIGHT);
            if (top..bottom).contains(&y) {
                *span = Some((left, right));
            }
        }
        Self {
            enabled,
            spans,
            inside: [
                (inside_control & 0x3F) as u8,
                ((inside_control >> 8) & 0x3F) as u8,
            ],
            outside: (outside_control & 0x3F) as u8,
            object_window_enabled: display_control & DISPCNT_OBJ_WINDOW != 0,
            object_window: ((outside_control >> 8) & 0x3F) as u8,
        }
    }

    /// Layers (bits 0-3 BG0-3, bit 4 OBJ) allowed to draw at `x`; bit 5 says
    /// whether color special effects apply there.
    /// `in_object_window` is whether an OBJ-window sprite covers the pixel.
    pub(super) fn layers_at(&self, x: usize, in_object_window: bool) -> u8 {
        if !self.enabled {
            return 0x3F;
        }
        // WIN0 beats WIN1, which beats the OBJ window, which beats "outside".
        for (span, layers) in self.spans.iter().zip(self.inside) {
            if let Some((left, right)) = span
                && (*left..*right).contains(&x)
            {
                return layers;
            }
        }
        if self.object_window_enabled && in_object_window {
            return self.object_window;
        }
        self.outside
    }
}

/// Decodes a WINxH/WINxV register (`start << 8 | end`). An end past the
/// screen or before the start clamps to the screen edge, as in mGBA.
pub(super) fn window_range(value: u16, limit: usize) -> (usize, usize) {
    let start = usize::from(value >> 8);
    let mut end = usize::from(value & 0xFF);
    if start > limit || start > end || end > limit {
        end = limit;
    }
    (start, end)
}
