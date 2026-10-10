//! Background layers for one scanline: regular text layers, rotation/scaling
//! (affine) layers and the bitmap modes (GBATEK "LCD I/O BG Control" and
//! "BG Modes").

use super::{Pixel, background_pixel, rgb555, rgb555_value};

/// Visible area of the bitmap modes (mode 5 is smaller).
const BITMAP_FRAME_BYTES: usize = 0xA000;

#[derive(Clone, Copy)]
pub(super) enum LayerKind {
    Text {
        control: u16,
        horizontal_offset: u16,
        vertical_offset: u16,
    },
    /// Tiled rotation/scaling layer (BG2/BG3 in modes 1 and 2).
    Affine { control: u16, reference: Reference },
    /// Mode 3, 4 or 5 bitmap on BG2.
    Bitmap {
        mode: u16,
        frame_offset: usize,
        reference: Reference,
    },
}

/// Texture-space start point and per-pixel step (8.8 fixed point) for the
/// current scanline.
#[derive(Clone, Copy)]
pub(super) struct Reference {
    pub x: i32,
    pub y: i32,
    pub pa: i32,
    pub pc: i32,
}

impl Reference {
    fn texel(&self, screen_x: usize) -> (i32, i32) {
        let step = screen_x as i32;
        (
            self.x.wrapping_add(self.pa.wrapping_mul(step)) >> 8,
            self.y.wrapping_add(self.pc.wrapping_mul(step)) >> 8,
        )
    }
}

#[derive(Clone, Copy)]
pub(super) struct Layer {
    /// Background number 0..=3 (also its bit in layer masks).
    pub background: usize,
    pub priority: u8,
    pub kind: LayerKind,
    /// Horizontal and vertical mosaic block sizes when the layer has mosaic on.
    pub mosaic: Option<(usize, usize)>,
}

impl Layer {
    /// The color at screen position `x` on the line being drawn (`y`), or
    /// `None` when transparent.
    pub fn pixel(&self, x: usize, y: usize, vram: &[u8], palette: &[u8]) -> Option<Pixel> {
        let x = match self.mosaic {
            Some((horizontal, _)) => x - x % horizontal,
            None => x,
        };
        match self.kind {
            LayerKind::Text {
                control,
                horizontal_offset,
                vertical_offset,
            } => {
                let y = match self.mosaic {
                    Some((_, vertical)) => y - y % vertical,
                    None => y,
                };
                background_pixel(
                    &vram[..super::VRAM_SIZE],
                    palette,
                    control,
                    horizontal_offset,
                    vertical_offset,
                    x,
                    y,
                )
            }
            LayerKind::Affine { control, reference } => {
                affine_pixel(vram, palette, control, reference.texel(x))
            }
            LayerKind::Bitmap {
                mode,
                frame_offset,
                reference,
            } => bitmap_pixel(vram, palette, mode, frame_offset, reference.texel(x)),
        }
    }
}

fn affine_pixel(
    vram: &[u8],
    palette: &[u8],
    control: u16,
    (mut texel_x, mut texel_y): (i32, i32),
) -> Option<Pixel> {
    let size = 128_i32 << ((control >> 14) & 3);
    if control & (1 << 13) != 0 {
        // Display area overflow = wrap.
        texel_x = texel_x.rem_euclid(size);
        texel_y = texel_y.rem_euclid(size);
    } else if !(0..size).contains(&texel_x) || !(0..size).contains(&texel_y) {
        return None;
    }
    let (texel_x, texel_y) = (texel_x as usize, texel_y as usize);
    let tiles_per_row = size as usize / 8;
    let map_base = usize::from((control >> 8) & 0x1F) * 0x800;
    let tile = usize::from(*vram.get(map_base + (texel_y / 8) * tiles_per_row + texel_x / 8)?);
    let character_base = usize::from((control >> 2) & 3) * 0x4000;
    let index = *vram.get(character_base + tile * 64 + (texel_y % 8) * 8 + texel_x % 8)?;
    (index != 0).then(|| rgb555(palette, usize::from(index)))
}

fn bitmap_pixel(
    vram: &[u8],
    palette: &[u8],
    mode: u16,
    frame_offset: usize,
    (texel_x, texel_y): (i32, i32),
) -> Option<Pixel> {
    let (width, height) = if mode == 5 { (160, 128) } else { (240, 160) };
    if !(0..width).contains(&texel_x) || !(0..height).contains(&texel_y) {
        return None;
    }
    let pixel = texel_y as usize * width as usize + texel_x as usize;
    match mode {
        // 15-bit direct color; every pixel is opaque.
        3 | 5 => {
            let offset = frame_offset + pixel * 2;
            let value = u16::from_le_bytes([*vram.get(offset)?, *vram.get(offset + 1)?]);
            Some(rgb555_value(value))
        }
        // 8-bit palette index; index 0 is transparent.
        4 => {
            let index = *vram.get(frame_offset + pixel)?;
            (index != 0).then(|| rgb555(palette, usize::from(index)))
        }
        _ => None,
    }
}

/// VRAM offset of the displayed bitmap frame (DISPCNT bit 4 flips pages).
pub(super) fn bitmap_frame_offset(display_control: u16) -> usize {
    if display_control & (1 << 4) != 0 {
        BITMAP_FRAME_BYTES
    } else {
        0
    }
}
