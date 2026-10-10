//! Object (sprite) rendering: OAM parsing, tile fetch, flips, affine
//! transforms and per-line composition inputs (GBATEK "LCD OBJ Overview").

use super::{SCREEN_WIDTH, rgb555};
use crate::mmu::MemoryBus;

const OAM_ENTRIES: usize = 128;
const OBJ_VRAM_BASE: usize = 0x1_0000;
const OBJ_VRAM_SIZE: usize = 0x8000;
const OBJ_PALETTE_BASE: usize = 0x200;
const TILE_BYTES: usize = 32;
/// Sprite pixel-fetch cycles available per scanline (DISPCNT bit 5 clear / set).
const LINE_CYCLES: i32 = 1210;
const LINE_CYCLES_HBLANK_FREE: i32 = 954;
/// Tile numbers below this are unusable in the bitmap modes (3-5).
const BITMAP_MODE_FIRST_TILE: usize = 512;

/// Pixel dimensions `(width, height)` indexed by `[shape][size]`.
const SPRITE_SIZES: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SpritePixel {
    /// 0 (front) ..= 3 (back); sprites win ties against backgrounds.
    pub priority: u8,
    pub color: super::Pixel,
    /// OBJ mode 1: takes part in alpha blending as the first target.
    pub semi_transparent: bool,
}

/// One scanline of composited sprite output.
pub(super) struct SpriteLine {
    pub pixels: [Option<SpritePixel>; SCREEN_WIDTH],
    /// Pixels covered by an OBJ-window sprite (mode 2).
    pub window: [bool; SCREEN_WIDTH],
}

impl Default for SpriteLine {
    fn default() -> Self {
        Self {
            pixels: [None; SCREEN_WIDTH],
            window: [false; SCREEN_WIDTH],
        }
    }
}

impl SpriteLine {
    pub fn clear(&mut self) {
        self.pixels.fill(None);
        self.window.fill(false);
    }

    /// Draws every sprite that intersects `line` into this buffer.
    pub fn render(
        &mut self,
        line: usize,
        display_control: u16,
        mosaic_register: u16,
        memory: &MemoryBus,
    ) {
        self.clear();
        let one_dimensional = display_control & (1 << 6) != 0;
        let bitmap_mode = (3..=5).contains(&(display_control & 7));
        let mosaic = (
            usize::from((mosaic_register >> 8) & 0xF) + 1,
            usize::from((mosaic_register >> 12) & 0xF) + 1,
        );
        let mut budget = if display_control & (1 << 5) != 0 {
            LINE_CYCLES_HBLANK_FREE
        } else {
            LINE_CYCLES
        };
        let oam = &memory.oam[..];
        let vram = &memory.vram[..];
        let palette = &memory.palette_ram[OBJ_PALETTE_BASE..OBJ_PALETTE_BASE + 0x200];

        for index in 0..OAM_ENTRIES {
            let attributes = Attributes::parse(oam, index);
            if attributes.hidden() {
                continue;
            }
            if budget <= 0 {
                break;
            }
            attributes.draw_line(
                line as i32,
                &LineContext {
                    one_dimensional,
                    bitmap_mode,
                    mosaic,
                },
                oam,
                vram,
                palette,
                &mut budget,
                self,
            );
        }
    }
}

/// Per-line state shared by every sprite.
struct LineContext {
    one_dimensional: bool,
    bitmap_mode: bool,
    /// Horizontal and vertical OBJ mosaic block sizes.
    mosaic: (usize, usize),
}

struct Attributes {
    y: i32,
    x: i32,
    affine: bool,
    double_size: bool,
    mosaic: bool,
    disabled: bool,
    shape_valid: bool,
    mode: u16,
    eight_bit: bool,
    horizontal_flip: bool,
    vertical_flip: bool,
    affine_group: usize,
    width: i32,
    height: i32,
    tile: usize,
    priority: u8,
    palette_bank: usize,
}

impl Attributes {
    fn parse(oam: &[u8], index: usize) -> Self {
        let word = |offset: usize| {
            u16::from_le_bytes([oam[index * 8 + offset], oam[index * 8 + offset + 1]])
        };
        let (attribute0, attribute1, attribute2) = (word(0), word(2), word(4));
        let affine = attribute0 & (1 << 8) != 0;
        // Shape 3 is prohibited; such sprites are skipped.
        let shape = usize::from((attribute0 >> 14) & 3);
        let size = usize::from((attribute1 >> 14) & 3);
        let (width, height) = SPRITE_SIZES.get(shape).map_or((0, 0), |sizes| sizes[size]);
        // X is a signed 9-bit value.
        let x = i32::from(attribute1 & 0x1FF);
        Self {
            y: i32::from(attribute0 & 0xFF),
            x: if x >= 256 { x - 512 } else { x },
            affine,
            double_size: affine && attribute0 & (1 << 9) != 0,
            mosaic: attribute0 & (1 << 12) != 0,
            disabled: !affine && attribute0 & (1 << 9) != 0,
            shape_valid: shape < 3,
            mode: (attribute0 >> 10) & 3,
            eight_bit: attribute0 & (1 << 13) != 0,
            horizontal_flip: !affine && attribute1 & (1 << 12) != 0,
            vertical_flip: !affine && attribute1 & (1 << 13) != 0,
            affine_group: usize::from((attribute1 >> 9) & 0x1F),
            width,
            height,
            tile: usize::from(attribute2 & 0x3FF),
            priority: ((attribute2 >> 10) & 3) as u8,
            palette_bank: usize::from(attribute2 >> 12),
        }
    }

    fn hidden(&self) -> bool {
        self.disabled || self.mode == 3 || !self.shape_valid
    }

    /// Reads a signed 8.8 affine parameter. Parameters are stored in the
    /// unused fourth halfword of each group of four OAM entries.
    fn affine_parameter(&self, oam: &[u8], which: usize) -> i32 {
        let offset = self.affine_group * 32 + which * 8 + 6;
        i32::from(i16::from_le_bytes([oam[offset], oam[offset + 1]]))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_line(
        &self,
        line: i32,
        context: &LineContext,
        oam: &[u8],
        vram: &[u8],
        palette: &[u8],
        budget: &mut i32,
        output: &mut SpriteLine,
    ) {
        let (box_width, box_height) = if self.double_size {
            (self.width * 2, self.height * 2)
        } else {
            (self.width, self.height)
        };
        // Y wraps at 256, so a sprite near the bottom can reappear at the top.
        if (line - self.y) & 0xFF >= box_height {
            return;
        }
        // Mosaic repeats the first row of each block of lines.
        let sample_line = if self.mosaic {
            line - line % context.mosaic.1 as i32
        } else {
            line
        };
        let row = ((sample_line - self.y) & 0xFF).min(box_height - 1);

        // Fetch cost: one cycle per pixel, or 10 + 2 per pixel when rotated.
        let cost = if self.affine {
            10 + 2 * box_width
        } else {
            box_width
        };
        let drawable_columns = if cost <= *budget {
            box_width
        } else if self.affine {
            ((*budget - 10) / 2).max(0)
        } else {
            *budget
        };
        *budget -= cost.min(*budget);

        let matrix = self.affine.then(|| {
            [
                self.affine_parameter(oam, 0),
                self.affine_parameter(oam, 1),
                self.affine_parameter(oam, 2),
                self.affine_parameter(oam, 3),
            ]
        });

        for column in 0..drawable_columns {
            let screen_x = self.x + column;
            if !(0..SCREEN_WIDTH as i32).contains(&screen_x) {
                continue;
            }
            let screen_x = screen_x as usize;
            let column = if self.mosaic {
                let horizontal = context.mosaic.0;
                let quantized = (screen_x - screen_x % horizontal) as i32;
                (quantized - self.x).max(0)
            } else {
                column
            };

            // Map the box pixel to a texture pixel.
            let (mut texture_x, mut texture_y) = match matrix {
                Some([pa, pb, pc, pd]) => {
                    let dx = column - box_width / 2;
                    let dy = row - box_height / 2;
                    (
                        ((pa * dx + pb * dy) >> 8) + self.width / 2,
                        ((pc * dx + pd * dy) >> 8) + self.height / 2,
                    )
                }
                None => (column, row),
            };
            if !(0..self.width).contains(&texture_x) || !(0..self.height).contains(&texture_y) {
                continue;
            }
            if self.horizontal_flip {
                texture_x = self.width - 1 - texture_x;
            }
            if self.vertical_flip {
                texture_y = self.height - 1 - texture_y;
            }

            let Some(color_index) = self.texel(texture_x, texture_y, context, vram) else {
                continue;
            };

            if self.mode == 2 {
                // OBJ-window sprites only define a mask; they draw nothing.
                output.window[screen_x] = true;
                continue;
            }
            let better =
                output.pixels[screen_x].is_none_or(|existing| self.priority < existing.priority);
            if better {
                let palette_index = if self.eight_bit {
                    color_index
                } else {
                    self.palette_bank * 16 + color_index
                };
                output.pixels[screen_x] = Some(SpritePixel {
                    priority: self.priority,
                    color: rgb555(palette, palette_index),
                    semi_transparent: self.mode == 1,
                });
            }
        }
    }

    /// Fetches one texel as a palette index; `None` when transparent.
    fn texel(&self, x: i32, y: i32, context: &LineContext, vram: &[u8]) -> Option<usize> {
        let (tile_x, tile_y) = ((x / 8) as usize, (y / 8) as usize);
        let (pixel_x, pixel_y) = ((x % 8) as usize, (y % 8) as usize);
        // 8bpp tiles take two 32-byte slots, which scales the tile stride.
        let slots_per_tile = if self.eight_bit { 2 } else { 1 };
        let row_stride = if context.one_dimensional {
            (self.width as usize / 8) * slots_per_tile
        } else {
            32
        };
        let tile_number = self.tile + tile_y * row_stride + tile_x * slots_per_tile;
        if context.bitmap_mode && tile_number < BITMAP_MODE_FIRST_TILE {
            return None;
        }
        let tile_offset = tile_number * TILE_BYTES;
        let at = |inner: usize| vram[OBJ_VRAM_BASE + (tile_offset + inner) % OBJ_VRAM_SIZE];

        let value = if self.eight_bit {
            usize::from(at(pixel_y * 8 + pixel_x))
        } else {
            let packed = at(pixel_y * 4 + pixel_x / 2);
            usize::from(if pixel_x % 2 == 0 {
                packed & 0xF
            } else {
                packed >> 4
            })
        };
        (value != 0).then_some(value)
    }
}
