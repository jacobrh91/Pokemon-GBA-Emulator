mod background;
mod sprites;

use crate::mmu::MemoryBus;
use background::{Layer, LayerKind, Reference, bitmap_frame_offset};
use sprites::SpriteLine;

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
pub type Pixel = [u8; 4];

const VRAM_SIZE: usize = 64 * 1024;
const PALETTE_RAM_SIZE: usize = 512;
const DISPLAY_CONTROL: usize = 0x000;
const BG_CONTROL_BASE: usize = 0x008;
const BG_HORIZONTAL_OFFSET_BASE: usize = 0x010;
const BG_VERTICAL_OFFSET_BASE: usize = 0x012;
const WINDOW_HORIZONTAL_BASE: usize = 0x040;
const WINDOW_VERTICAL_BASE: usize = 0x044;
const WINDOW_INSIDE_CONTROL: usize = 0x048;
const WINDOW_OUTSIDE_CONTROL: usize = 0x04A;
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
        for background in 0..2 {
            let source = affine_reference_registers(registers, background);
            let previous = self.affine_loaded_from[background];
            if y == 0 || affine_reload[background] || previous != Some(source) {
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
        let display_control = register(registers, DISPLAY_CONTROL);
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
            if visible_layers & OBJ_LAYER != 0 {
                if let Some(sprite) = self.sprites.pixels[x] {
                    layers[count] = Some(LayerPixel {
                        sort_key: sprite.priority << 3,
                        layer: OBJ_LAYER,
                        color: sprite.color,
                        semi_transparent: sprite.semi_transparent,
                    });
                    count += 1;
                }
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

#[derive(Clone, Copy)]
struct LayerPixel {
    /// `priority << 3 | order`; lower sorts in front.
    sort_key: u8,
    /// One of the `1 << n` BG bits, [`OBJ_LAYER`] or [`BACKDROP_LAYER`].
    layer: u8,
    color: Pixel,
    semi_transparent: bool,
}

/// Layer bit for the backdrop in BLDCNT target masks (bit 5).
const BACKDROP_LAYER: u8 = 1 << 5;
/// Window-mask bit that allows color special effects (blending).
const EFFECT_ENABLE: u8 = 1 << 5;

const BLDCNT: usize = 0x050;
const BLDALPHA: usize = 0x052;
const BLDY: usize = 0x054;

/// The color special effects unit (BLDCNT/BLDALPHA/BLDY).
struct Blend {
    effect: u8,
    first_targets: u8,
    second_targets: u8,
    alpha_first: u32,
    alpha_second: u32,
    brightness: u32,
}

impl Blend {
    fn from_registers(registers: &[u8]) -> Self {
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

    fn apply(&self, top: LayerPixel, below: LayerPixel) -> Pixel {
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

    fn alpha(&self, first: Pixel, second: Pixel) -> Pixel {
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
fn channels(pixel: Pixel) -> [u32; 3] {
    [
        u32::from(pixel[0] >> 3),
        u32::from(pixel[1] >> 3),
        u32::from(pixel[2] >> 3),
    ]
}

fn expand(channels: [u32; 3]) -> Pixel {
    let widen = |value: u32| ((value << 3) | (value >> 2)) as u8;
    [
        widen(channels[0]),
        widen(channels[1]),
        widen(channels[2]),
        0xFF,
    ]
}

fn map_channels(pixel: Pixel, f: impl Fn(u32) -> u32) -> Pixel {
    let [r, g, b] = channels(pixel);
    expand([f(r).min(31), f(g).min(31), f(b).min(31)])
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

/// BG2 matrix/reference registers start here; BG3's are 0x10 further.
const AFFINE_BASE: usize = 0x020;
const MOSAIC: usize = 0x04C;

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

/// Bit 4 of a window layer mask: the OBJ layer (bits 0-3 are BG0-3).
const OBJ_LAYER: u8 = 1 << 4;

/// Window layout for one scanline. `layers_at` returns which BG layers
/// (bits 0-3) may draw at a given x; the backdrop is never windowed.
struct Windows {
    enabled: bool,
    /// Horizontal span for WIN0/WIN1 when the line is inside their vertical span.
    spans: [Option<(usize, usize)>; 2],
    inside: [u8; 2],
    outside: u8,
    object_window_enabled: bool,
    /// Layer enables inside OBJ-window sprites (WINOUT high byte).
    object_window: u8,
}

impl Windows {
    fn for_line(registers: &[u8], display_control: u16, y: usize) -> Self {
        let window_enabled = [
            display_control & DISPCNT_WINDOW_0 != 0,
            display_control & DISPCNT_WINDOW_1 != 0,
        ];
        let enabled =
            window_enabled.iter().any(|&on| on) || display_control & DISPCNT_OBJ_WINDOW != 0;
        let inside_control = register(registers, WINDOW_INSIDE_CONTROL);
        let outside_control = register(registers, WINDOW_OUTSIDE_CONTROL);

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
    fn layers_at(&self, x: usize, in_object_window: bool) -> u8 {
        if !self.enabled {
            return 0x3F;
        }
        // WIN0 beats WIN1, which beats the OBJ window, which beats "outside".
        for (span, layers) in self.spans.iter().zip(self.inside) {
            if let Some((left, right)) = span {
                if (*left..*right).contains(&x) {
                    return layers;
                }
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
fn window_range(value: u16, limit: usize) -> (usize, usize) {
    let start = usize::from(value >> 8);
    let mut end = usize::from(value & 0xFF);
    if start > limit || start > end || end > limit {
        end = limit;
    }
    (start, end)
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
mod tests {
    use super::*;
    use crate::mmu::{IO_REG_START, PALETTE_RAM_START, VRAM_START};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);

    fn memory_bus() -> MemoryBus {
        let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "gba-emulator-ppu-test-{}-{id}.gba",
            std::process::id()
        ));
        fs::write(&path, [0; 4]).unwrap();
        let bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        bus
    }

    #[test]
    fn mode_zero_renders_4bpp_tiles_and_transparent_backdrop() {
        let mut memory = memory_bus();
        memory.write_u16(IO_REG_START, 1 << 8).unwrap();
        memory.write_u16(IO_REG_START + 8, 0).unwrap();
        memory.write_u16(VRAM_START, 1).unwrap();
        memory.write_u8(VRAM_START + 32, 0x21).unwrap();
        memory.write_u16(PALETTE_RAM_START, 0x7C00).unwrap();
        memory.write_u16(PALETTE_RAM_START + 2, 0x001F).unwrap();
        memory.write_u16(PALETTE_RAM_START + 4, 0x03E0).unwrap();

        let mut ppu = Ppu::default();
        ppu.render_frame(&memory);
        let frame = ppu.framebuffer();

        assert_eq!(frame.len(), SCREEN_WIDTH * SCREEN_HEIGHT);
        assert_eq!(frame[0], [255, 0, 0, 255]);
        assert_eq!(frame[1], [0, 255, 0, 255]);
        assert_eq!(frame[2], [0, 0, 255, 255]);
    }

    #[test]
    fn mode_zero_composites_backgrounds_by_priority_then_layer_number() {
        let mut memory = memory_bus();
        memory.write_u16(IO_REG_START, (1 << 8) | (1 << 9)).unwrap();
        memory.write_u16(IO_REG_START + 8, 1).unwrap();
        memory.write_u16(IO_REG_START + 10, 0x0104).unwrap();
        memory.write_u16(VRAM_START, 1).unwrap();
        memory.write_u16(VRAM_START + 0x800, 1 | (1 << 12)).unwrap();
        memory.write_u8(VRAM_START + 32, 0x11).unwrap();
        memory.write_u8(VRAM_START + 0x4000 + 32, 0x11).unwrap();
        memory.write_u16(PALETTE_RAM_START + 2, 0x001F).unwrap();
        memory.write_u16(PALETTE_RAM_START + 34, 0x03E0).unwrap();

        let mut ppu = Ppu::default();
        ppu.render_frame(&memory);

        assert_eq!(ppu.framebuffer()[0], [0, 255, 0, 255]);
    }

    #[test]
    fn mode_zero_renders_8bpp_tiles_with_horizontal_flip_and_scroll() {
        let mut memory = memory_bus();
        memory.write_u16(IO_REG_START, 1 << 8).unwrap();
        memory.write_u16(IO_REG_START + 8, 1 << 7).unwrap();
        memory.write_u16(IO_REG_START + 0x10, 1).unwrap();
        memory.write_u16(VRAM_START, 1 | (1 << 10)).unwrap();
        for pixel in 0..8 {
            memory
                .write_u8(VRAM_START + 64 + pixel, (pixel + 1) as u8)
                .unwrap();
        }
        memory.write_u16(PALETTE_RAM_START + 7 * 2, 0x001F).unwrap();

        let mut ppu = Ppu::default();
        ppu.render_frame(&memory);

        assert_eq!(ppu.framebuffer()[0], [255, 0, 0, 255]);
    }

    #[test]
    fn scanlines_use_the_registers_latched_for_that_line() {
        let mut memory = memory_bus();
        // BG0, 4bpp, tile 1 at map (0,0); tile 1 row 0 = color 1, row 1 = color 2.
        memory.write_u16(IO_REG_START, 1 << 8).unwrap();
        memory.write_u16(VRAM_START, 1).unwrap();
        memory.write_u32(VRAM_START + 32, 0x1111_1111).unwrap();
        memory.write_u32(VRAM_START + 36, 0x2222_2222).unwrap();
        memory.write_u16(PALETTE_RAM_START + 2, 0x001F).unwrap();
        memory.write_u16(PALETTE_RAM_START + 4, 0x03E0).unwrap();

        let mut ppu = Ppu::default();
        let mut registers = [0u8; 0x60];
        registers[1] = 1; // DISPCNT: BG0 on
        // Line 0 with BG0VOFS = 0 samples tile row 0; with VOFS = 1, row 1.
        ppu.render_scanline(0, &registers, &memory);
        registers[0x12] = 1;
        ppu.render_scanline(1, &registers, &memory);
        assert_eq!(ppu.framebuffer()[0], rgb555(&memory.palette_ram[..], 1));
        // Line 1 + VOFS 1 = source row 2: tile row 2 is blank -> backdrop.
        assert_eq!(
            ppu.framebuffer()[SCREEN_WIDTH],
            rgb555(&memory.palette_ram[..], 0)
        );
        registers[0x12] = 0;
        ppu.render_scanline(1, &registers, &memory);
        assert_eq!(
            ppu.framebuffer()[SCREEN_WIDTH],
            rgb555(&memory.palette_ram[..], 2)
        );
    }

    #[test]
    fn forced_blank_renders_white() {
        let mut memory = memory_bus();
        memory.write_u16(IO_REG_START, 0x0080).unwrap();
        let mut ppu = Ppu::default();
        ppu.render_frame(&memory);
        assert!(ppu.framebuffer().iter().all(|p| *p == [0xFF; 4]));
    }

    #[test]
    fn window_masks_backgrounds_outside_the_window_rectangle() {
        let mut memory = memory_bus();
        // BG0 filled with tile 1 (solid color 1); backdrop color 0 stays black.
        for entry in 0..(32 * 32) {
            memory.write_u16(VRAM_START + entry * 2, 1).unwrap();
        }
        for row in 0..8 {
            memory
                .write_u32(VRAM_START + 0x4000 + 32 + row * 4, 0x1111_1111)
                .unwrap();
        }
        memory.write_u16(PALETTE_RAM_START + 2, 0x001F).unwrap();

        let mut ppu = Ppu::default();
        let mut registers = [0u8; 0x60];
        registers[0] = 0; // mode 0
        registers[1] = 0x01 | 0x20; // BG0 on, WIN0 on
        registers[8] = 0x04; // BG0CNT: char base 1
        // WIN0: x 40..80, y 20..60, BG0 shown inside, nothing outside.
        registers[0x40..0x42].copy_from_slice(&((40u16 << 8) | 80).to_le_bytes());
        registers[0x44..0x46].copy_from_slice(&((20u16 << 8) | 60).to_le_bytes());
        registers[0x48] = 0x01;
        registers[0x4A] = 0x00;

        ppu.render_scanline(10, &registers, &memory);
        ppu.render_scanline(30, &registers, &memory);
        let red = rgb555(&memory.palette_ram[..], 1);
        let black = rgb555(&memory.palette_ram[..], 0);
        assert_eq!(ppu.framebuffer()[10 * SCREEN_WIDTH + 50], black);
        assert_eq!(ppu.framebuffer()[30 * SCREEN_WIDTH + 39], black);
        assert_eq!(ppu.framebuffer()[30 * SCREEN_WIDTH + 40], red);
        assert_eq!(ppu.framebuffer()[30 * SCREEN_WIDTH + 79], red);
        assert_eq!(ppu.framebuffer()[30 * SCREEN_WIDTH + 80], black);
    }

    #[test]
    fn window_ranges_clamp_to_the_screen() {
        assert_eq!(window_range((10 << 8) | 20, SCREEN_WIDTH), (10, 20));
        assert_eq!(
            window_range((10 << 8) | 250, SCREEN_WIDTH),
            (10, SCREEN_WIDTH)
        );
        assert_eq!(
            window_range((30 << 8) | 20, SCREEN_WIDTH),
            (30, SCREEN_WIDTH)
        );
    }

    #[test]
    fn mode_one_has_text_layers_zero_and_one_but_no_layer_three() {
        let mut memory = memory_bus();
        // Solid color-1 tile 1 on BG0; BG3 does not exist in mode 1.
        for entry in 0..(32 * 32) {
            memory.write_u16(VRAM_START + entry * 2, 1).unwrap();
        }
        for row in 0..8 {
            memory
                .write_u32(VRAM_START + 0x4000 + 32 + row * 4, 0x1111_1111)
                .unwrap();
        }
        memory.write_u16(PALETTE_RAM_START + 2, 0x001F).unwrap();

        let mut ppu = Ppu::default();
        let mut registers = [0u8; 0x60];
        registers[0] = 1; // mode 1
        registers[1] = 0x01; // BG0 on
        registers[8] = 0x04; // BG0CNT char base 1
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(ppu.framebuffer()[0], rgb555(&memory.palette_ram[..], 1));

        registers[1] = 0x08; // only BG3 on: not available in mode 1
        ppu.render_scanline(1, &registers, &memory);
        assert_eq!(
            ppu.framebuffer()[SCREEN_WIDTH],
            rgb555(&memory.palette_ram[..], 0)
        );
    }

    // ---- sprites ----------------------------------------------------------

    use crate::mmu::OAM_START;

    const OBJ_TILES: u32 = VRAM_START + 0x1_0000;
    const OBJ_PALETTE: u32 = PALETTE_RAM_START + 0x200;

    fn set_sprite(memory: &mut MemoryBus, index: u32, attr0: u16, attr1: u16, attr2: u16) {
        let base = OAM_START + index * 8;
        memory.write_u16(base, attr0).unwrap();
        memory.write_u16(base + 2, attr1).unwrap();
        memory.write_u16(base + 4, attr2).unwrap();
    }

    /// Hides every sprite so unused OAM (all zero) does not draw 8x8 tiles.
    fn hide_all_sprites(memory: &mut MemoryBus) {
        for index in 0..128 {
            memory.write_u16(OAM_START + index * 8, 1 << 9).unwrap();
        }
    }

    /// Fills one 4bpp 8x8 tile with a single color index.
    fn solid_tile(memory: &mut MemoryBus, tile: u32, color: u8) {
        for offset in 0..32 {
            memory
                .write_u8(OBJ_TILES + tile * 32 + offset, color | (color << 4))
                .unwrap();
        }
    }

    fn sprite_palette(memory: &mut MemoryBus, color_index: u32, rgb555: u16) {
        memory
            .write_u16(OBJ_PALETTE + color_index * 2, rgb555)
            .unwrap();
    }

    fn registers_with(display_control: u16) -> [u8; 0x60] {
        let mut registers = [0u8; 0x60];
        registers[0..2].copy_from_slice(&display_control.to_le_bytes());
        registers
    }

    fn color_at(ppu: &Ppu, x: usize, y: usize) -> Pixel {
        ppu.framebuffer()[y * SCREEN_WIDTH + x]
    }

    fn pal(memory: &MemoryBus, color_index: usize) -> Pixel {
        rgb555(&memory.palette_ram[0x200..0x400], color_index)
    }

    const OBJ_ON_1D: u16 = (1 << 12) | (1 << 6);
    const OBJ_ON_2D: u16 = 1 << 12;

    #[test]
    fn sprite_draws_at_its_position_with_transparent_zero_pixels() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 1, 0x001F);
        solid_tile(&mut memory, 0, 1);
        // Make pixel (0,0) of the tile transparent.
        memory.write_u8(OBJ_TILES, 0x10).unwrap();
        set_sprite(&mut memory, 0, 20, 10, 0); // y=20, x=10, 8x8

        let mut ppu = Ppu::default();
        let registers = registers_with(OBJ_ON_1D);
        for y in 19..30 {
            ppu.render_scanline(y, &registers, &memory);
        }
        let red = pal(&memory, 1);
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        assert_eq!(color_at(&ppu, 10, 20), backdrop, "transparent texel");
        assert_eq!(color_at(&ppu, 11, 20), red);
        assert_eq!(color_at(&ppu, 10, 21), red);
        assert_eq!(color_at(&ppu, 17, 27), red);
        assert_eq!(color_at(&ppu, 18, 27), backdrop, "past the right edge");
        assert_eq!(color_at(&ppu, 10, 19), backdrop, "above the sprite");
        assert_eq!(color_at(&ppu, 10, 28), backdrop, "below the sprite");
    }

    #[test]
    fn flips_mirror_the_sprite_within_its_box() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 1, 0x001F);
        sprite_palette(&mut memory, 2, 0x03E0);
        // Tile: left half color 1, right half color 2, top row only.
        for row in 0..8 {
            memory.write_u32(OBJ_TILES + row * 4, 0x2222_1111).unwrap();
        }
        set_sprite(&mut memory, 0, 0, 0, 0);
        set_sprite(&mut memory, 1, 20, 1 << 12, 0); // hflip at y=20, x=0

        let mut ppu = Ppu::default();
        let registers = registers_with(OBJ_ON_1D);
        ppu.render_scanline(0, &registers, &memory);
        ppu.render_scanline(20, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), pal(&memory, 1));
        assert_eq!(color_at(&ppu, 7, 0), pal(&memory, 2));
        assert_eq!(color_at(&ppu, 0, 20), pal(&memory, 2));
        assert_eq!(color_at(&ppu, 7, 20), pal(&memory, 1));
    }

    #[test]
    fn one_and_two_dimensional_mapping_pick_different_tiles() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        for (tile, color) in [(0, 1), (1, 2), (2, 3), (32, 4), (33, 5)] {
            solid_tile(&mut memory, tile, color);
            sprite_palette(&mut memory, u32::from(color), u16::from(color) * 0x0421);
        }
        // 16x16 sprite (square, size 1) at the top-left.
        set_sprite(&mut memory, 0, 0, 1 << 14, 0);

        let mut ppu = Ppu::default();
        // 1D: tiles 0,1 on the first tile row, 2,3 on the second.
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        ppu.render_scanline(8, &registers_with(OBJ_ON_1D), &memory);
        assert_eq!(color_at(&ppu, 0, 0), pal(&memory, 1));
        assert_eq!(color_at(&ppu, 8, 0), pal(&memory, 2));
        assert_eq!(color_at(&ppu, 0, 8), pal(&memory, 3));

        // 2D: the second tile row starts 32 tiles later.
        ppu.render_scanline(0, &registers_with(OBJ_ON_2D), &memory);
        ppu.render_scanline(8, &registers_with(OBJ_ON_2D), &memory);
        assert_eq!(color_at(&ppu, 8, 0), pal(&memory, 2));
        assert_eq!(color_at(&ppu, 0, 8), pal(&memory, 4));
        assert_eq!(color_at(&ppu, 8, 8), pal(&memory, 5));
    }

    #[test]
    fn eight_bit_sprites_index_the_full_256_color_object_palette() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 200, 0x7C00);
        for offset in 0..64 {
            memory.write_u8(OBJ_TILES + offset, 200).unwrap();
        }
        set_sprite(&mut memory, 0, 1 << 13, 0, 0);

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        assert_eq!(color_at(&ppu, 3, 0), pal(&memory, 200));
    }

    #[test]
    fn palette_bank_selects_a_16_color_sub_palette() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 3 * 16 + 1, 0x03E0);
        solid_tile(&mut memory, 0, 1);
        set_sprite(&mut memory, 0, 0, 0, 3 << 12);

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        assert_eq!(color_at(&ppu, 0, 0), pal(&memory, 3 * 16 + 1));
    }

    #[test]
    fn sprite_priority_is_compared_with_background_priority() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        // BG0: solid color 1 (green) with priority 1.
        for entry in 0..(32 * 32) {
            memory.write_u16(VRAM_START + entry * 2, 1).unwrap();
        }
        for row in 0..8 {
            memory
                .write_u32(VRAM_START + 0x4000 + 32 + row * 4, 0x1111_1111)
                .unwrap();
        }
        memory.write_u16(PALETTE_RAM_START + 2, 0x03E0).unwrap();
        sprite_palette(&mut memory, 1, 0x001F);
        solid_tile(&mut memory, 0, 1);

        let mut registers = registers_with(OBJ_ON_1D | 0x0100);
        registers[8] = 0x04 | 1; // BG0: char base 1, priority 1

        let mut ppu = Ppu::default();
        let green = rgb555(&memory.palette_ram[..], 1);
        let red = pal(&memory, 1);
        for (sprite_priority, expected) in [(0, red), (1, red), (2, green)] {
            set_sprite(&mut memory, 0, 0, 0, sprite_priority << 10);
            ppu.render_scanline(0, &registers, &memory);
            assert_eq!(
                color_at(&ppu, 0, 0),
                expected,
                "sprite priority {sprite_priority}"
            );
        }
    }

    #[test]
    fn lower_priority_value_wins_then_lower_oam_index() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        for color in 1..=3u8 {
            sprite_palette(&mut memory, u32::from(color), u16::from(color) * 0x0421);
            solid_tile(&mut memory, u32::from(color), color);
        }
        // All three overlap at x=0,y=0.
        set_sprite(&mut memory, 0, 0, 0, (1 << 10) | 1); // index 0, priority 1, tile 1
        set_sprite(&mut memory, 1, 0, 0, (1 << 10) | 2); // index 1, priority 1, tile 2
        set_sprite(&mut memory, 2, 0, 0, 3); // index 2, priority 0, tile 3

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        assert_eq!(color_at(&ppu, 0, 0), pal(&memory, 3), "priority 0 beats 1");

        set_sprite(&mut memory, 2, 0, 0, (2 << 10) | 3); // now priority 2
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        assert_eq!(
            color_at(&ppu, 0, 0),
            pal(&memory, 1),
            "lowest index wins ties"
        );
    }

    #[test]
    fn y_wraps_and_negative_x_clips_at_the_screen_edge() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 1, 0x001F);
        solid_tile(&mut memory, 0, 1);
        // y = 252 wraps so rows 0..4 show the sprite's lower half;
        // x = 508 is -4 as a signed 9-bit value.
        set_sprite(&mut memory, 0, 252, 508, 0);

        let mut ppu = Ppu::default();
        let registers = registers_with(OBJ_ON_1D);
        for y in 0..6 {
            ppu.render_scanline(y, &registers, &memory);
        }
        let red = pal(&memory, 1);
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        assert_eq!(color_at(&ppu, 0, 0), red);
        assert_eq!(color_at(&ppu, 3, 3), red);
        assert_eq!(color_at(&ppu, 4, 0), backdrop, "only 4 columns remain");
        assert_eq!(color_at(&ppu, 0, 4), backdrop, "only 4 rows wrap");
    }

    #[test]
    fn disabled_sprites_and_a_cleared_obj_enable_bit_draw_nothing() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 1, 0x001F);
        solid_tile(&mut memory, 0, 1);
        set_sprite(&mut memory, 0, 1 << 9, 0, 0); // disabled (non-affine bit 9)
        let backdrop = rgb555(&memory.palette_ram[..], 0);

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        assert_eq!(color_at(&ppu, 0, 0), backdrop);

        set_sprite(&mut memory, 0, 0, 0, 0);
        ppu.render_scanline(0, &registers_with(1 << 6), &memory); // OBJ off
        assert_eq!(color_at(&ppu, 0, 0), backdrop);
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        assert_eq!(color_at(&ppu, 0, 0), pal(&memory, 1));
    }

    #[test]
    fn affine_sprites_rotate_and_double_size_expands_the_box() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 1, 0x001F);
        sprite_palette(&mut memory, 2, 0x03E0);
        // 8x8 tile: top half color 1, bottom half color 2.
        for row in 0..8 {
            let value = if row < 4 { 0x1111_1111 } else { 0x2222_2222 };
            memory.write_u32(OBJ_TILES + row * 4, value).unwrap();
        }
        // Affine group 0: identity.
        for (slot, value) in [0x0100i16, 0, 0, 0x0100].into_iter().enumerate() {
            memory
                .write_u16(OAM_START + 6 + slot as u32 * 8, value as u16)
                .unwrap();
        }
        // Sprite 4 (group 1 params live in entries 4-7): rotate 90 degrees:
        // pa=0 pb=-1.0? use pa=0, pb=1.0, pc=-1.0, pd=0 in 8.8.
        for (slot, value) in [0i16, 0x0100, -0x0100, 0].into_iter().enumerate() {
            memory
                .write_u16(OAM_START + 32 + 6 + slot as u32 * 8, value as u16)
                .unwrap();
        }
        // Sprite 0: affine, group 0, at (0,0). Sprite 8 uses group 1 at (40,0).
        set_sprite(&mut memory, 0, 1 << 8, 0, 0);
        set_sprite(&mut memory, 8, (1 << 8) | 0, 40 | (1 << 9), 0);

        let mut ppu = Ppu::default();
        let registers = registers_with(OBJ_ON_1D);
        ppu.render_scanline(0, &registers, &memory);
        ppu.render_scanline(7, &registers, &memory);
        assert_eq!(
            color_at(&ppu, 0, 0),
            pal(&memory, 1),
            "identity: top is color 1"
        );
        assert_eq!(
            color_at(&ppu, 0, 7),
            pal(&memory, 2),
            "identity: bottom is color 2"
        );

        // Rotated 90 degrees: the vertical split becomes a horizontal one.
        ppu.render_scanline(4, &registers, &memory);
        let left = color_at(&ppu, 40, 4);
        let right = color_at(&ppu, 47, 4);
        assert_ne!(left, right, "rotation turns top/bottom into left/right");

        // Double-size (non-rotated group) reaches 16 pixels wide: group 1 with
        // identity would; here just confirm the box is wider than the 8px tile.
        let mut wide = Ppu::default();
        set_sprite(&mut memory, 8, (1 << 8) | (1 << 9), 40, 0);
        for slot in 0..4 {
            let value = if slot == 0 || slot == 3 { 0x0100i16 } else { 0 };
            memory
                .write_u16(OAM_START + 32 + 6 + slot as u32 * 8, value as u16)
                .unwrap();
        }
        wide.render_scanline(8, &registers, &memory);
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        assert_ne!(color_at(&wide, 44, 8), backdrop, "inside the doubled box");
        assert_eq!(color_at(&wide, 40, 8), backdrop, "outside the 8x8 texture");
    }

    #[test]
    fn obj_window_sprites_mask_layers_using_the_winout_high_byte() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        // BG0 solid green.
        for entry in 0..(32 * 32) {
            memory.write_u16(VRAM_START + entry * 2, 1).unwrap();
        }
        for row in 0..8 {
            memory
                .write_u32(VRAM_START + 0x4000 + 32 + row * 4, 0x1111_1111)
                .unwrap();
        }
        memory.write_u16(PALETTE_RAM_START + 2, 0x03E0).unwrap();
        solid_tile(&mut memory, 0, 1);
        // Sprite 0 is an OBJ-window sprite (mode 2) at x=16.
        set_sprite(&mut memory, 0, 2 << 10, 16, 0);

        // OBJ window on + BG0 + OBJ. WINOUT: outside shows BG0 (bit 0);
        // the OBJ window shows nothing (high byte 0).
        let mut registers = registers_with(OBJ_ON_1D | 0x0100 | (1 << 15));
        registers[8] = 0x04;
        registers[0x4A] = 0x01;
        registers[0x4B] = 0x00;

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        let green = rgb555(&memory.palette_ram[..], 1);
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        assert_eq!(color_at(&ppu, 0, 0), green, "outside: BG0 visible");
        assert_eq!(color_at(&ppu, 16, 0), backdrop, "inside OBJ window: hidden");
        assert_eq!(color_at(&ppu, 23, 0), backdrop);
        assert_eq!(color_at(&ppu, 24, 0), green);
    }

    // ---- blending ---------------------------------------------------------

    /// BG0 solid color index 1 (priority 0) over BG1 solid color index 2.
    fn two_background_scene(memory: &mut MemoryBus) {
        // BG0: screen block 0, char base 1 (tile 1 = color 1).
        // BG1: screen block 1, char base 1 (tile 2 = color 2).
        for entry in 0..(32 * 32) {
            memory.write_u16(VRAM_START + entry * 2, 1).unwrap();
            memory.write_u16(VRAM_START + 0x800 + entry * 2, 2).unwrap();
        }
        for row in 0..8 {
            memory
                .write_u32(VRAM_START + 0x4000 + 32 + row * 4, 0x1111_1111)
                .unwrap();
            memory
                .write_u32(VRAM_START + 0x4000 + 64 + row * 4, 0x2222_2222)
                .unwrap();
        }
        // color 1 = pure red (31,0,0); color 2 = pure blue (0,0,31).
        memory.write_u16(PALETTE_RAM_START + 2, 0x001F).unwrap();
        memory.write_u16(PALETTE_RAM_START + 4, 0x7C00).unwrap();
    }

    fn blend_registers(bldcnt: u16, bldalpha: u16, bldy: u16) -> [u8; 0x60] {
        let mut registers = registers_with(0x0300); // BG0 + BG1
        registers[8..10].copy_from_slice(&(0x04u16).to_le_bytes()); // BG0: char 1, block 0
        registers[10..12].copy_from_slice(&(0x04u16 | (1 << 8)).to_le_bytes()); // BG1: block 1
        registers[0x50..0x52].copy_from_slice(&bldcnt.to_le_bytes());
        registers[0x52..0x54].copy_from_slice(&bldalpha.to_le_bytes());
        registers[0x54..0x56].copy_from_slice(&bldy.to_le_bytes());
        registers
    }

    #[test]
    fn alpha_blending_mixes_the_top_layer_with_the_one_below() {
        let mut memory = memory_bus();
        two_background_scene(&mut memory);
        // BG0 is first target, BG1 second target; EVA = 8/16, EVB = 8/16.
        let registers = blend_registers(0x0001 | (1 << 6) | 0x0200, 8 | (8 << 8), 0);
        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);

        // Half red + half blue in 5-bit space: r = 15, b = 15.
        assert_eq!(color_at(&ppu, 0, 0), expand([15, 0, 15]));
    }

    #[test]
    fn alpha_needs_the_layer_below_to_be_a_second_target() {
        let mut memory = memory_bus();
        two_background_scene(&mut memory);
        // Second target selects BG2 only, so BG1 below is not blended.
        let registers = blend_registers(0x0001 | (1 << 6) | 0x0400, 8 | (8 << 8), 0);
        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), expand([31, 0, 0]));
    }

    #[test]
    fn brightness_decrease_darkens_only_selected_first_targets() {
        let mut memory = memory_bus();
        two_background_scene(&mut memory);
        // Darken BG0 by 8/16.
        let registers = blend_registers(0x0001 | (3 << 6), 0, 8);
        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), expand([31 - (31 * 8 >> 4), 0, 0]));

        // BG0 not selected: unchanged.
        let registers = blend_registers(0x0002 | (3 << 6), 0, 8);
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), expand([31, 0, 0]));
    }

    #[test]
    fn brightness_increase_moves_toward_white_and_full_ev_saturates() {
        let mut memory = memory_bus();
        two_background_scene(&mut memory);
        let mut ppu = Ppu::default();

        let registers = blend_registers(0x0001 | (2 << 6), 0, 8);
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), expand([31, 15, 15]));

        // EVY above 16 clamps to 16: pure white.
        let registers = blend_registers(0x0001 | (2 << 6), 0, 31);
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), expand([31, 31, 31]));
    }

    #[test]
    fn backdrop_can_be_a_blend_target() {
        let mut memory = memory_bus();
        two_background_scene(&mut memory);
        // Backdrop pure green; only BG0 enabled and blended onto the backdrop.
        memory.write_u16(PALETTE_RAM_START, 0x03E0).unwrap();
        let mut registers = blend_registers(0x0001 | (1 << 6) | 0x2000, 8 | (8 << 8), 0);
        registers[1] = 0x01; // BG0 only
        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), expand([15, 15, 0]));
    }

    #[test]
    fn window_effect_bit_disables_blending_inside_the_window() {
        let mut memory = memory_bus();
        two_background_scene(&mut memory);
        let mut registers = blend_registers(0x0001 | (3 << 6), 0, 16);
        registers[1] = 0x23; // BG0 + BG1 + WIN0
        registers[0x40..0x42].copy_from_slice(&((0u16 << 8) | 100).to_le_bytes());
        registers[0x44..0x46].copy_from_slice(&((0u16 << 8) | 160).to_le_bytes());
        registers[0x48] = 0x03; // inside WIN0: BG0 + BG1, effects off
        registers[0x4A] = 0x23; // outside: BG0 + BG1 + effects on

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(
            color_at(&ppu, 50, 0),
            expand([31, 0, 0]),
            "inside: not darkened"
        );
        assert_eq!(
            color_at(&ppu, 150, 0),
            expand([0, 0, 0]),
            "outside: fully darkened"
        );
    }

    #[test]
    fn semi_transparent_sprites_blend_with_the_layer_beneath() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        two_background_scene(&mut memory);
        sprite_palette(&mut memory, 1, 0x03E0); // green
        solid_tile(&mut memory, 0, 1);
        // Mode 1 (semi-transparent) sprite at the origin.
        set_sprite(&mut memory, 0, 1 << 10, 0, 0);

        // BG1 (blue) is the second target; no BLDCNT effect selected.
        let mut registers = blend_registers(0x0200, 8 | (8 << 8), 0);
        registers[0..2].copy_from_slice(&(0x0300 | OBJ_ON_1D).to_le_bytes());
        // Remove BG0 so BG1 sits directly below the sprite.
        registers[0] &= !0;
        registers[1] = 0x12;

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), expand([0, 15, 15]));
        assert_eq!(
            color_at(&ppu, 20, 0),
            expand([0, 0, 31]),
            "beyond the sprite"
        );
    }

    // ---- affine backgrounds, bitmap modes, mosaic, sprite budget ----------

    fn put16(registers: &mut [u8], offset: usize, value: u16) {
        registers[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(registers: &mut [u8], offset: usize, value: u32) {
        registers[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// Mode 2, BG2 only, identity matrix, 128x128, char base 1, map block 0.
    fn affine_registers() -> [u8; 0x60] {
        let mut registers = registers_with(0x0402); // mode 2 + BG2
        put16(&mut registers, 0x0C, 0x0004); // BG2CNT: char base 1
        put16(&mut registers, 0x20, 0x0100); // PA
        put16(&mut registers, 0x26, 0x0100); // PD
        registers
    }

    /// Solid 8bpp BG tile `tile` with palette index `color` (char base 1).
    fn affine_tile(memory: &mut MemoryBus, tile: u32, color: u8) {
        for offset in 0..64 {
            memory
                .write_u8(VRAM_START + 0x4000 + tile * 64 + offset, color)
                .unwrap();
        }
    }

    fn bg_color(memory: &mut MemoryBus, index: u32, rgb555: u16) {
        memory
            .write_u16(PALETTE_RAM_START + index * 2, rgb555)
            .unwrap();
    }

    fn bg_pal(memory: &MemoryBus, index: usize) -> Pixel {
        rgb555(&memory.palette_ram[..], index)
    }

    #[test]
    fn affine_background_maps_screen_pixels_through_the_matrix() {
        let mut memory = memory_bus();
        affine_tile(&mut memory, 1, 5);
        affine_tile(&mut memory, 2, 6);
        bg_color(&mut memory, 5, 0x001F);
        bg_color(&mut memory, 6, 0x03E0);
        memory.write_u8(VRAM_START, 1).unwrap(); // map (0,0) = tile 1
        memory.write_u8(VRAM_START + 1, 2).unwrap(); // map (1,0) = tile 2

        let mut ppu = Ppu::default();
        let mut registers = affine_registers();
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 0), bg_pal(&memory, 5));
        assert_eq!(color_at(&ppu, 7, 0), bg_pal(&memory, 5));
        assert_eq!(color_at(&ppu, 8, 0), bg_pal(&memory, 6));

        // 2x horizontal stretch (PA = 0.5 texels per pixel): tile 2 starts at x=16.
        put16(&mut registers, 0x20, 0x0080);
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(color_at(&ppu, 15, 0), bg_pal(&memory, 5));
        assert_eq!(color_at(&ppu, 16, 0), bg_pal(&memory, 6));
    }

    #[test]
    fn affine_overflow_wraps_only_when_the_wrap_bit_is_set() {
        let mut memory = memory_bus();
        affine_tile(&mut memory, 3, 5);
        bg_color(&mut memory, 5, 0x001F);
        // Last tile of the first map row (128px wide = 16 tiles).
        memory.write_u8(VRAM_START + 15, 3).unwrap();

        let mut ppu = Ppu::default();
        let mut registers = affine_registers();
        // Reference X = -8 pixels: screen x 0..8 samples texels -8..0.
        put32(&mut registers, 0x28, (-8i32 * 256) as u32 & 0x0FFF_FFFF);
        let backdrop = rgb555(&memory.palette_ram[..], 0);

        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(
            color_at(&ppu, 0, 0),
            backdrop,
            "no wrap: outside is transparent"
        );

        put16(&mut registers, 0x0C, 0x0004 | (1 << 13));
        ppu.render_scanline(0, &registers, &memory);
        assert_eq!(
            color_at(&ppu, 0, 0),
            bg_pal(&memory, 5),
            "wraps to the last tile"
        );
    }

    #[test]
    fn affine_reference_point_advances_by_pb_and_pd_each_line() {
        let mut memory = memory_bus();
        affine_tile(&mut memory, 1, 5);
        bg_color(&mut memory, 5, 0x001F);
        // Only the map row for texel rows 8..16 uses tile 1.
        memory.write_u8(VRAM_START + 16, 1).unwrap();

        let mut ppu = Ppu::default();
        let registers = affine_registers();
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        for y in 0..10 {
            ppu.render_scanline(y, &registers, &memory);
        }
        assert_eq!(color_at(&ppu, 0, 7), backdrop);
        assert_eq!(color_at(&ppu, 0, 8), bg_pal(&memory, 5));
        assert_eq!(color_at(&ppu, 0, 9), bg_pal(&memory, 5));
    }

    #[test]
    fn writing_the_reference_point_mid_frame_restarts_the_internal_counter() {
        let mut memory = memory_bus();
        affine_tile(&mut memory, 1, 5);
        bg_color(&mut memory, 5, 0x001F);
        memory.write_u8(VRAM_START, 1).unwrap(); // only texel rows 0..8

        let mut ppu = Ppu::default();
        let mut registers = affine_registers();
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        for y in 0..12 {
            ppu.render_scanline(y, &registers, &memory);
        }
        assert_eq!(color_at(&ppu, 0, 11), backdrop, "advanced past the tile");

        // An HBlank effect rewrites BG2Y (to texel row 4) before line 12.
        put32(&mut registers, 0x2C, 4 << 8);
        ppu.render_scanline(12, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 12), bg_pal(&memory, 5));
        // ...and the counter keeps advancing from the new value: rows 5..8.
        for y in 13..16 {
            ppu.render_scanline(y, &registers, &memory);
            assert_eq!(color_at(&ppu, 0, y), bg_pal(&memory, 5), "line {y}");
        }
        ppu.render_scanline(16, &registers, &memory);
        assert_eq!(
            color_at(&ppu, 0, 16),
            backdrop,
            "texel row 8 is past the tile"
        );

        // Rewriting the *same* value is only visible through the bus's write
        // event, which restarts the counter.
        ppu.render_scanline_with_events(17, &registers, &memory, [false, false]);
        assert_eq!(color_at(&ppu, 0, 17), backdrop);
        ppu.render_scanline_with_events(18, &registers, &memory, [true, false]);
        assert_eq!(color_at(&ppu, 0, 18), bg_pal(&memory, 5));
    }

    #[test]
    fn bitmap_modes_read_direct_color_and_paletted_pixels() {
        let mut memory = memory_bus();
        let backdrop = rgb555(&memory.palette_ram[..], 0);

        // Each mode gets a fresh PPU: lines are drawn out of order here, and
        // the affine reference counter is per-frame state.
        // Mode 3: 15-bit color at (3, 2); identity matrix.
        let mut ppu = Ppu::default();
        let mut registers = registers_with(0x0403);
        put16(&mut registers, 0x20, 0x0100);
        put16(&mut registers, 0x26, 0x0100);
        memory
            .write_u16(VRAM_START + (2 * 240 + 3) * 2, 0x7C00)
            .unwrap();
        ppu.render_scanline(2, &registers, &memory);
        assert_eq!(color_at(&ppu, 3, 2), expand([0, 0, 31]));
        // Direct color is opaque even when it is black.
        assert_eq!(color_at(&ppu, 0, 2), [0, 0, 0, 0xFF]);

        // Mode 4: 8-bit indices, 0 transparent; page flip selects 0xA000.
        let mut ppu = Ppu::default();
        bg_color(&mut memory, 9, 0x03E0);
        memory.write_u8(VRAM_START + 240 + 5, 9).unwrap(); // page 0, (5,1)
        memory.write_u8(VRAM_START + 0xA000 + 240 + 6, 9).unwrap(); // page 1, (6,1)
        let mut registers = registers_with(0x0404);
        put16(&mut registers, 0x20, 0x0100);
        put16(&mut registers, 0x26, 0x0100);
        ppu.render_scanline(1, &registers, &memory);
        assert_eq!(color_at(&ppu, 5, 1), bg_pal(&memory, 9));
        assert_eq!(color_at(&ppu, 6, 1), backdrop);
        registers[0] |= 0x10; // display page 1
        let mut ppu = Ppu::default(); // re-drawing line 1 needs a fresh frame
        ppu.render_scanline(1, &registers, &memory);
        assert_eq!(color_at(&ppu, 5, 1), backdrop);
        assert_eq!(color_at(&ppu, 6, 1), bg_pal(&memory, 9));

        // Mode 5: 160x128; pixels beyond that are transparent.
        let mut ppu = Ppu::default();
        let mut registers = registers_with(0x0405);
        put16(&mut registers, 0x20, 0x0100);
        put16(&mut registers, 0x26, 0x0100);
        memory
            .write_u16(VRAM_START + (4 * 160 + 7) * 2, 0x03E0)
            .unwrap();
        ppu.render_scanline(4, &registers, &memory);
        assert_eq!(color_at(&ppu, 7, 4), expand([0, 31, 0]));
        assert_eq!(color_at(&ppu, 160, 4), backdrop);
        ppu.render_scanline(130, &registers, &memory);
        assert_eq!(color_at(&ppu, 0, 130), backdrop);
    }

    #[test]
    fn background_mosaic_repeats_the_first_pixel_of_each_block() {
        let mut memory = memory_bus();
        // 4bpp text BG0, char base 1: one tile whose columns are colors 1..8.
        for entry in 0..(32 * 32) {
            memory.write_u16(VRAM_START + entry * 2, 1).unwrap();
        }
        for row in 0..8u32 {
            memory
                .write_u32(VRAM_START + 0x4000 + 32 + row * 4, 0x8765_4321)
                .unwrap();
        }
        for color in 1..=8u32 {
            bg_color(&mut memory, color, (color as u16) * 0x0421);
        }

        let mut registers = registers_with(0x0100);
        put16(&mut registers, 8, 0x0004 | (1 << 6)); // char base 1, mosaic on
        put16(&mut registers, 0x4C, 3); // BG mosaic: 4 wide, 1 tall
        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        // Columns 0..4 repeat column 0 (color 1); 4..8 repeat column 4 (color 5).
        for x in 0..4 {
            assert_eq!(color_at(&ppu, x, 0), bg_pal(&memory, 1), "x={x}");
        }
        for x in 4..8 {
            assert_eq!(color_at(&ppu, x, 0), bg_pal(&memory, 5), "x={x}");
        }
    }

    #[test]
    fn sprites_stop_when_the_line_cycle_budget_runs_out() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 1, 0x001F);
        for tile in 0..64 {
            solid_tile(&mut memory, tile, 1);
        }
        // 18 64x64 sprites (cost 64 each = 1152 of 1210) at x=0, then a 19th
        // that only fits 58 columns, then a 20th that gets nothing.
        for index in 0..18 {
            set_sprite(&mut memory, index, 0, 3 << 14, 0);
        }
        set_sprite(&mut memory, 18, 0, (3 << 14) | 100, 0);
        set_sprite(&mut memory, 19, 0, (3 << 14) | 180, 0);

        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D), &memory);
        let red = pal(&memory, 1);
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        assert_eq!(color_at(&ppu, 63, 0), red);
        assert_eq!(color_at(&ppu, 100 + 57, 0), red, "last affordable column");
        assert_eq!(color_at(&ppu, 100 + 58, 0), backdrop, "budget exhausted");
        assert_eq!(color_at(&ppu, 190, 0), backdrop, "later sprite not drawn");

        // With H-blank interval free the budget is smaller (954).
        ppu.render_scanline(0, &registers_with(OBJ_ON_1D | (1 << 5)), &memory);
        assert_eq!(color_at(&ppu, 100, 0), backdrop);
    }

    #[test]
    fn bitmap_modes_cannot_use_the_first_512_sprite_tiles() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        sprite_palette(&mut memory, 1, 0x001F);
        solid_tile(&mut memory, 0, 1);
        solid_tile(&mut memory, 512, 1);
        set_sprite(&mut memory, 0, 0, 0, 0);
        set_sprite(&mut memory, 1, 0, 16, 512);

        let mut ppu = Ppu::default();
        let registers = registers_with(OBJ_ON_1D | 3);
        ppu.render_scanline(0, &registers, &memory);
        let backdrop = rgb555(&memory.palette_ram[..], 0);
        assert_eq!(color_at(&ppu, 0, 0), backdrop, "tile 0 unusable in mode 3");
        assert_eq!(color_at(&ppu, 16, 0), pal(&memory, 1), "tile 512 is usable");
    }

    #[test]
    fn sprite_mosaic_repeats_blocks_using_the_obj_mosaic_size() {
        let mut memory = memory_bus();
        hide_all_sprites(&mut memory);
        for color in 1..=8u32 {
            sprite_palette(&mut memory, color, (color as u16) * 0x0421);
        }
        for row in 0..8 {
            memory.write_u32(OBJ_TILES + row * 4, 0x8765_4321).unwrap();
        }
        set_sprite(&mut memory, 0, 1 << 12, 0, 0); // mosaic sprite

        let mut registers = registers_with(OBJ_ON_1D);
        put16(&mut registers, 0x4C, 3 << 8); // OBJ mosaic 4 wide, 1 tall
        let mut ppu = Ppu::default();
        ppu.render_scanline(0, &registers, &memory);
        for x in 0..4 {
            assert_eq!(color_at(&ppu, x, 0), pal(&memory, 1), "x={x}");
        }
        assert_eq!(color_at(&ppu, 4, 0), pal(&memory, 5));
    }
}
