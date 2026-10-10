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

    pub fn render_frame(&mut self, memory: &MemoryBus) {
        let display_control = io_register(memory, DISPLAY_CONTROL);
        let background_mode = display_control & 0x7;
        let palette = &memory.palette_ram[..PALETTE_RAM_SIZE];
        let vram = &memory.vram[..VRAM_SIZE];
        let backdrop = rgb555(palette, 0);

        self.framebuffer.fill(backdrop);
        if background_mode != 0 {
            return;
        }

        let enabled_backgrounds = (0..4)
            .filter(|background| display_control & (1 << (8 + background)) != 0)
            .map(|background| {
                let control = io_register(memory, BG_CONTROL_BASE + background * 2);
                let horizontal_offset =
                    io_register(memory, BG_HORIZONTAL_OFFSET_BASE + background * 4) & 0x01FF;
                let vertical_offset =
                    io_register(memory, BG_VERTICAL_OFFSET_BASE + background * 4) & 0x01FF;
                (background, control, horizontal_offset, vertical_offset)
            })
            .collect::<Vec<_>>();

        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let mut selected_priority = u8::MAX;
                let mut selected_background = usize::MAX;
                let mut color = backdrop;

                for &(background, control, horizontal_offset, vertical_offset) in
                    &enabled_backgrounds
                {
                    let priority = (control & 0x3) as u8;
                    if priority > selected_priority
                        || (priority == selected_priority && background >= selected_background)
                    {
                        continue;
                    }
                    let Some(color_index) = background_pixel(
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
                    color = color_index;
                }
                self.framebuffer[y * SCREEN_WIDTH + x] = color;
            }
        }
    }
}

fn io_register(memory: &MemoryBus, offset: usize) -> u16 {
    u16::from_le_bytes([memory.io_regs[offset], memory.io_regs[offset + 1]])
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
}
