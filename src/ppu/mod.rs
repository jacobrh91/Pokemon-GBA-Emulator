mod background;
mod blend;
mod sprites;
mod state;
mod windows;

use crate::mmu::MemoryBus;
use crate::mmu::registers::{
    AFFINE_BASE, BG_CONTROL_BASE, BG_HORIZONTAL_OFFSET_BASE, BG_VERTICAL_OFFSET_BASE, BLDALPHA,
    BLDCNT, BLDY, DISPCNT, MOSAIC, WINDOW_HORIZONTAL_BASE, WINDOW_VERTICAL_BASE, WININ, WINOUT,
};
use background::{Layer, LayerKind, Reference, bitmap_frame_offset};
use blend::{BACKDROP_LAYER, Blend, EFFECT_ENABLE, LayerPixel};
use sprites::SpriteLine;
use windows::{OBJ_LAYER, Windows};

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
pub type Pixel = [u8; 4];

const VRAM_SIZE: usize = 64 * 1024;
const PALETTE_RAM_SIZE: usize = 512;
const DISPCNT_WINDOW_0: u16 = 1 << 13;
const DISPCNT_WINDOW_1: u16 = 1 << 14;
const DISPCNT_OBJ_WINDOW: u16 = 1 << 15;
const DISPCNT_OBJ_ENABLE: u16 = 1 << 12;

pub struct Ppu {
    framebuffer: Vec<Pixel>,
    sprites: SpriteLine,
    /// Internal affine reference points (X, Y; 24.8) for BG2 and BG3,
    /// advanced by PB/PD after every scanline.
    affine_reference: [[i32; 2]; 2],
    /// Register values seen when the references were last loaded, to notice
    /// writes made between lines.
    affine_loaded_from: [Option<[i32; 2]>; 2],
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            framebuffer: vec![[0; 4]; SCREEN_WIDTH * SCREEN_HEIGHT],
            sprites: SpriteLine::default(),
            affine_reference: [[0; 2]; 2],
            affine_loaded_from: [None; 2],
        }
    }
}

impl Ppu {
    pub fn framebuffer(&self) -> &[Pixel] {
        &self.framebuffer
    }

    /// Renders a whole frame from the live register state.
    pub fn render_frame(&mut self, memory: &MemoryBus) {
        for y in 0..SCREEN_HEIGHT {
            self.render_scanline(y, &memory.io_regs[..], memory);
        }
    }

    /// Renders one line using `registers` (an `io_regs`-layout snapshot).
    pub fn render_scanline(&mut self, y: usize, registers: &[u8], memory: &MemoryBus) {
        self.render_scanline_with_events(y, registers, memory, [false; 2]);
    }

    /// Like [`Ppu::render_scanline`], also told which affine reference-point
    /// registers (BG2, BG3) were written since the previous line.
    pub fn render_scanline_with_events(
        &mut self,
        y: usize,
        registers: &[u8],
        memory: &MemoryBus,
        affine_reload: [bool; 2],
    ) {
        if y >= SCREEN_HEIGHT {
            return;
        }
        for (background, &reload) in affine_reload.iter().enumerate() {
            let source = affine_reference_registers(registers, background);
            let previous = self.affine_loaded_from[background];
            if y == 0 || reload || previous != Some(source) {
                self.affine_reference[background] = source;
                if previous.is_none() && y > 0 {
                    // First line seen is not line 0 (random access): behave as
                    // if the lines before it had been drawn.
                    let base = AFFINE_BASE + background * 0x10;
                    let rows = y as i32;
                    let pb = i32::from(register(registers, base + 2) as i16);
                    let pd = i32::from(register(registers, base + 6) as i16);
                    self.affine_reference[background][0] =
                        source[0].wrapping_add(pb.wrapping_mul(rows));
                    self.affine_reference[background][1] =
                        source[1].wrapping_add(pd.wrapping_mul(rows));
                }
            }
            self.affine_loaded_from[background] = Some(source);
        }
        self.draw_line(y, registers, memory);
        // Each line moves the reference point by (PB, PD).
        for (background, reference) in self.affine_reference.iter_mut().enumerate() {
            let base = AFFINE_BASE + background * 0x10;
            reference[0] =
                reference[0].wrapping_add(i32::from(register(registers, base + 2) as i16));
            reference[1] =
                reference[1].wrapping_add(i32::from(register(registers, base + 6) as i16));
        }
    }

    fn draw_line(&mut self, y: usize, registers: &[u8], memory: &MemoryBus) {
        let display_control = register(registers, DISPCNT);
        let row_range = y * SCREEN_WIDTH..(y + 1) * SCREEN_WIDTH;
        // Forced blank shows white.
        if display_control & 0x80 != 0 {
            self.framebuffer[row_range].fill([0xFF, 0xFF, 0xFF, 0xFF]);
            return;
        }
        // Sprites are rendered first so the compositor can consult them.
        if display_control & DISPCNT_OBJ_ENABLE != 0 {
            let mosaic = register(registers, MOSAIC);
            self.sprites.render(y, display_control, mosaic, memory);
        } else {
            self.sprites.clear();
        }
        let affine_reference = self.affine_reference;
        let row = &mut self.framebuffer[row_range];
        let palette = &memory.palette_ram[..PALETTE_RAM_SIZE];
        let vram = &memory.vram[..];
        let backdrop = rgb555(palette, 0);
        row.fill(backdrop);
        let mode = display_control & 0x7;
        let mosaic_register = register(registers, MOSAIC);
        let background_mosaic = (
            usize::from(mosaic_register & 0xF) + 1,
            usize::from((mosaic_register >> 4) & 0xF) + 1,
        );

        let mut enabled_backgrounds: Vec<Layer> = Vec::with_capacity(4);
        for background in 0..4 {
            if display_control & (1 << (8 + background)) == 0 {
                continue;
            }
            let control = register(registers, BG_CONTROL_BASE + background * 2);
            let mosaic = (control & (1 << 6) != 0).then_some(background_mosaic);
            let kind = match (mode, background) {
                (0, _) | (1, 0 | 1) => LayerKind::Text {
                    control,
                    horizontal_offset: register(
                        registers,
                        BG_HORIZONTAL_OFFSET_BASE + background * 4,
                    ) & 0x01FF,
                    vertical_offset: register(registers, BG_VERTICAL_OFFSET_BASE + background * 4)
                        & 0x01FF,
                },
                (1, 2) | (2, 2 | 3) => LayerKind::Affine {
                    control,
                    reference: line_reference(&affine_reference, background, registers, y, mosaic),
                },
                (3..=5, 2) => LayerKind::Bitmap {
                    mode,
                    frame_offset: if mode == 3 {
                        0
                    } else {
                        bitmap_frame_offset(display_control)
                    },
                    reference: line_reference(&affine_reference, background, registers, y, mosaic),
                },
                _ => continue,
            };
            enabled_backgrounds.push(Layer {
                background,
                priority: (control & 0x3) as u8,
                kind,
                mosaic,
            });
        }

        let windows = Windows::for_line(registers, display_control, y);
        let blend = Blend::from_registers(registers);

        for (x, pixel) in row.iter_mut().enumerate() {
            let visible_layers = windows.layers_at(x, self.sprites.window[x]);

            // Gather every visible, opaque layer pixel at x. Ordering is by
            // priority, then OBJ before BG0..3 on ties; the backdrop is last.
            let mut layers: [Option<LayerPixel>; 6] = [None; 6];
            let mut count = 0;
            if visible_layers & OBJ_LAYER != 0
                && let Some(sprite) = self.sprites.pixels[x]
            {
                layers[count] = Some(LayerPixel {
                    sort_key: sprite.priority << 3,
                    layer: OBJ_LAYER,
                    color: sprite.color,
                    semi_transparent: sprite.semi_transparent,
                });
                count += 1;
            }
            for layer in &enabled_backgrounds {
                if visible_layers & (1 << layer.background) == 0 {
                    continue;
                }
                if let Some(color) = layer.pixel(x, y, vram, palette) {
                    layers[count] = Some(LayerPixel {
                        sort_key: (layer.priority << 3) | (layer.background as u8 + 1),
                        layer: 1 << layer.background,
                        color,
                        semi_transparent: false,
                    });
                    count += 1;
                }
            }

            let mut top = None::<LayerPixel>;
            let mut below = None::<LayerPixel>;
            for candidate in layers[..count].iter().flatten() {
                match top {
                    Some(current) if candidate.sort_key >= current.sort_key => {
                        if below.is_none_or(|second| candidate.sort_key < second.sort_key) {
                            below = Some(*candidate);
                        }
                    }
                    _ => {
                        below = top;
                        top = Some(*candidate);
                    }
                }
            }
            let backdrop_pixel = LayerPixel {
                sort_key: u8::MAX,
                layer: BACKDROP_LAYER,
                color: backdrop,
                semi_transparent: false,
            };
            let top = top.unwrap_or(backdrop_pixel);
            let below = below.unwrap_or(backdrop_pixel);

            *pixel = if visible_layers & EFFECT_ENABLE != 0 {
                blend.apply(top, below)
            } else {
                top.color
            };
        }
    }
}

/// Reference point and matrix columns for the line `y` of BG2 (index 0) or
/// BG3 (index 1). With vertical mosaic the start point is the one belonging
/// to the first line of the mosaic block.
fn line_reference(
    affine_reference: &[[i32; 2]; 2],
    background: usize,
    registers: &[u8],
    y: usize,
    mosaic: Option<(usize, usize)>,
) -> Reference {
    let index = background - 2;
    let base = AFFINE_BASE + index * 0x10;
    let signed = |offset: usize| i32::from(register(registers, base + offset) as i16);
    let rows_back = mosaic.map_or(0, |(_, vertical)| (y % vertical) as i32);
    let [reference_x, reference_y] = affine_reference[index];
    Reference {
        x: reference_x.wrapping_sub(rows_back.wrapping_mul(signed(2))),
        y: reference_y.wrapping_sub(rows_back.wrapping_mul(signed(6))),
        pa: signed(0),
        pc: signed(4),
    }
}

/// Reads the 28-bit signed reference point registers (X, Y) of BG2/BG3.
fn affine_reference_registers(registers: &[u8], background: usize) -> [i32; 2] {
    let base = AFFINE_BASE + background * 0x10 + 8;
    let read = |offset: usize| {
        let value = u32::from_le_bytes([
            registers[offset],
            registers[offset + 1],
            registers[offset + 2],
            registers[offset + 3],
        ]);
        // Sign-extend from bit 27.
        ((value << 4) as i32) >> 4
    };
    [read(base), read(base + 4)]
}

fn register(registers: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([registers[offset], registers[offset + 1]])
}

fn rgb555(palette: &[u8], color_index: usize) -> Pixel {
    let offset = color_index * 2;
    rgb555_value(u16::from_le_bytes([palette[offset], palette[offset + 1]]))
}

fn rgb555_value(color: u16) -> Pixel {
    let red = (color & 0x1F) as u8;
    let green = ((color >> 5) & 0x1F) as u8;
    let blue = ((color >> 10) & 0x1F) as u8;
    [
        (red << 3) | (red >> 2),
        (green << 3) | (green >> 2),
        (blue << 3) | (blue >> 2),
        0xFF,
    ]
}

fn background_pixel(
    vram: &[u8],
    palette: &[u8],
    control: u16,
    horizontal_offset: u16,
    vertical_offset: u16,
    screen_x: usize,
    screen_y: usize,
) -> Option<Pixel> {
    let size = (control >> 14) & 0x3;
    let width = if size & 1 == 0 { 256 } else { 512 };
    let height = if size & 2 == 0 { 256 } else { 512 };
    let source_x = (screen_x + usize::from(horizontal_offset)) % width;
    let source_y = (screen_y + usize::from(vertical_offset)) % height;
    let tile_x = source_x / 8;
    let tile_y = source_y / 8;
    let screenblocks_per_row = width / 256;
    let screenblock = (tile_y / 32) * screenblocks_per_row + tile_x / 32;
    let screen_base = usize::from((control >> 8) & 0x1F) * 0x800;
    let map_offset = screen_base + screenblock * 0x800 + ((tile_y % 32) * 32 + tile_x % 32) * 2;
    let map_entry = u16::from_le_bytes([*vram.get(map_offset)?, *vram.get(map_offset + 1)?]);

    let mut pixel_x = source_x % 8;
    let mut pixel_y = source_y % 8;
    if map_entry & (1 << 10) != 0 {
        pixel_x = 7 - pixel_x;
    }
    if map_entry & (1 << 11) != 0 {
        pixel_y = 7 - pixel_y;
    }

    let color_256 = control & (1 << 7) != 0;
    let tile_index = usize::from(map_entry & 0x03FF);
    let character_base = usize::from((control >> 2) & 0x3) * 0x4000;
    let color_index = if color_256 {
        let tile_offset = character_base + tile_index * 64 + pixel_y * 8 + pixel_x;
        usize::from(*vram.get(tile_offset)?)
    } else {
        let tile_offset = character_base + tile_index * 32 + pixel_y * 4 + pixel_x / 2;
        let packed_pixels = *vram.get(tile_offset)?;
        let local_color = if pixel_x & 1 == 0 {
            packed_pixels & 0x0F
        } else {
            packed_pixels >> 4
        };
        if local_color == 0 {
            return None;
        }
        let palette_bank = usize::from((map_entry >> 12) & 0xF);
        palette_bank * 16 + usize::from(local_color)
    };
    if color_256 && color_index == 0 {
        return None;
    }
    Some(rgb555(palette, color_index))
}

#[cfg(test)]
mod tests;
