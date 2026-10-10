use crate::mmu::MemoryBus;

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

pub struct Ppu {
    framebuffer: Vec<Pixel>,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            framebuffer: vec![[0; 4]; SCREEN_WIDTH * SCREEN_HEIGHT],
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
        if y >= SCREEN_HEIGHT {
            return;
        }
        let row = &mut self.framebuffer[y * SCREEN_WIDTH..(y + 1) * SCREEN_WIDTH];
        let display_control = register(registers, DISPLAY_CONTROL);
        // Forced blank shows white.
        if display_control & 0x80 != 0 {
            row.fill([0xFF, 0xFF, 0xFF, 0xFF]);
            return;
        }
        let palette = &memory.palette_ram[..PALETTE_RAM_SIZE];
        let vram = &memory.vram[..VRAM_SIZE];
        let backdrop = rgb555(palette, 0);
        row.fill(backdrop);
        // Which layers are regular text backgrounds depends on the video
        // mode: 0 has BG0-3, 1 has BG0-1 (BG2 is affine), and modes 2-5 have
        // none. Affine and bitmap layers are not rendered yet.
        let text_layers: u8 = match display_control & 0x7 {
            0 => 0b1111,
            1 => 0b0011,
            _ => 0,
        };

        let enabled_backgrounds = (0..4)
            .filter(|background| text_layers & (1 << background) != 0)
            .filter(|background| display_control & (1 << (8 + background)) != 0)
            .map(|background| {
                let control = register(registers, BG_CONTROL_BASE + background * 2);
                let horizontal_offset =
                    register(registers, BG_HORIZONTAL_OFFSET_BASE + background * 4) & 0x01FF;
                let vertical_offset =
                    register(registers, BG_VERTICAL_OFFSET_BASE + background * 4) & 0x01FF;
                (background, control, horizontal_offset, vertical_offset)
            })
            .collect::<Vec<_>>();

        let windows = Windows::for_line(registers, display_control, y);

        for (x, pixel) in row.iter_mut().enumerate() {
            let visible_layers = windows.layers_at(x);
            let mut selected_priority = u8::MAX;
            let mut selected_background = usize::MAX;
            let mut color = backdrop;

            for &(background, control, horizontal_offset, vertical_offset) in &enabled_backgrounds {
                if visible_layers & (1 << background) == 0 {
                    continue;
                }
                let priority = (control & 0x3) as u8;
                if priority > selected_priority
                    || (priority == selected_priority && background >= selected_background)
                {
                    continue;
                }
                let Some(candidate) = background_pixel(
                    vram,
                    palette,
                    control,
                    horizontal_offset,
                    vertical_offset,
                    x,
                    y,
                ) else {
                    continue;
                };
                selected_priority = priority;
                selected_background = background;
                color = candidate;
            }
            *pixel = color;
        }
    }
}

/// Window layout for one scanline. `layers_at` returns which BG layers
/// (bits 0-3) may draw at a given x; the backdrop is never windowed.
struct Windows {
    enabled: bool,
    /// Horizontal span for WIN0/WIN1 when the line is inside their vertical span.
    spans: [Option<(usize, usize)>; 2],
    inside: [u8; 2],
    outside: u8,
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
                (inside_control & 0x1F) as u8,
                ((inside_control >> 8) & 0x1F) as u8,
            ],
            outside: (outside_control & 0x1F) as u8,
        }
    }

    fn layers_at(&self, x: usize) -> u8 {
        if !self.enabled {
            return 0x0F;
        }
        // WIN0 has priority over WIN1, which has priority over "outside".
        for (span, layers) in self.spans.iter().zip(self.inside) {
            if let Some((left, right)) = span {
                if (*left..*right).contains(&x) {
                    return layers & 0x0F;
                }
            }
        }
        self.outside & 0x0F
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
    let color = u16::from_le_bytes([palette[offset], palette[offset + 1]]);
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
    fn mode_one_draws_text_layers_zero_and_one_only() {
        let mut memory = memory_bus();
        // Solid color-1 tile 1 on BG0; BG2 (affine in mode 1) must not draw.
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

        registers[1] = 0x04; // only BG2 on: affine, not rendered
        ppu.render_scanline(1, &registers, &memory);
        assert_eq!(
            ppu.framebuffer()[SCREEN_WIDTH],
            rgb555(&memory.palette_ram[..], 0)
        );
    }
}
