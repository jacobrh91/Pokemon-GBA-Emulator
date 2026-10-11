//! Color special effects (BLDCNT/BLDALPHA/BLDY) and the per-layer pixels they
//! combine.

use super::{BLDALPHA, BLDCNT, BLDY, Pixel, register};

#[derive(Clone, Copy)]
pub(super) struct LayerPixel {
    /// `priority << 3 | order`; lower sorts in front.
    pub(super) sort_key: u8,
    /// One of the `1 << n` BG bits, [`OBJ_LAYER`] or [`BACKDROP_LAYER`].
    pub(super) layer: u8,
    pub(super) color: Pixel,
    pub(super) semi_transparent: bool,
}

/// Layer bit for the backdrop in BLDCNT target masks (bit 5).
pub(super) const BACKDROP_LAYER: u8 = 1 << 5;
/// Window-mask bit that allows color special effects (blending).
pub(super) const EFFECT_ENABLE: u8 = 1 << 5;

/// The color special effects unit (BLDCNT/BLDALPHA/BLDY).
pub(super) struct Blend {
    pub(super) effect: u8,
    pub(super) first_targets: u8,
    pub(super) second_targets: u8,
    pub(super) alpha_first: u32,
    pub(super) alpha_second: u32,
    pub(super) brightness: u32,
}

impl Blend {
    pub(super) fn from_registers(registers: &[u8]) -> Self {
        let control = register(registers, BLDCNT);
        let alpha = register(registers, BLDALPHA);
        Self {
            effect: ((control >> 6) & 3) as u8,
            first_targets: (control & 0x3F) as u8,
            second_targets: ((control >> 8) & 0x3F) as u8,
            alpha_first: u32::from(alpha & 0x1F).min(16),
            alpha_second: u32::from((alpha >> 8) & 0x1F).min(16),
            brightness: u32::from(register(registers, BLDY) & 0x1F).min(16),
        }
    }

    pub(super) fn apply(&self, top: LayerPixel, below: LayerPixel) -> Pixel {
        let second_matches = self.second_targets & below.layer != 0;
        // A semi-transparent sprite blends with whatever is below it, even
        // if BLDCNT selects no effect.
        if top.semi_transparent && second_matches {
            return self.alpha(top.color, below.color);
        }
        if self.first_targets & top.layer == 0 {
            return top.color;
        }
        match self.effect {
            1 if second_matches => self.alpha(top.color, below.color),
            2 => map_channels(top.color, |c| c + (((31 - c) * self.brightness) >> 4)),
            3 => map_channels(top.color, |c| c - ((c * self.brightness) >> 4)),
            _ => top.color,
        }
    }

    pub(super) fn alpha(&self, first: Pixel, second: Pixel) -> Pixel {
        let mix = |a: u32, b: u32| ((a * self.alpha_first + b * self.alpha_second) >> 4).min(31);
        let first = channels(first);
        let second = channels(second);
        expand([
            mix(first[0], second[0]),
            mix(first[1], second[1]),
            mix(first[2], second[2]),
        ])
    }
}

/// The 5-bit channel values behind an expanded 8-bit pixel.
pub(super) fn channels(pixel: Pixel) -> [u32; 3] {
    [
        u32::from(pixel[0] >> 3),
        u32::from(pixel[1] >> 3),
        u32::from(pixel[2] >> 3),
    ]
}

pub(super) fn expand(channels: [u32; 3]) -> Pixel {
    let widen = |value: u32| ((value << 3) | (value >> 2)) as u8;
    [
        widen(channels[0]),
        widen(channels[1]),
        widen(channels[2]),
        0xFF,
    ]
}

pub(super) fn map_channels(pixel: Pixel, f: impl Fn(u32) -> u32) -> Pixel {
    let [r, g, b] = channels(pixel);
    expand([f(r).min(31), f(g).min(31), f(b).min(31)])
}
