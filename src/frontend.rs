use gba_emulator::emulator::Emulator;
use gba_emulator::mmu::ROM_START;
use gba_emulator::ppu::{Pixel, SCREEN_HEIGHT, SCREEN_WIDTH};
use minifb::{Key, Scale, Window, WindowOptions};
use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);

pub fn run(rom_path: &str) -> Result<(), Box<dyn Error>> {
    let mut emulator = Emulator::new(rom_path)?;
    emulator.initialize_gba_startup(ROM_START)?;

    let mut window = Window::new(
        "GBA Emulator",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        WindowOptions {
            scale: Scale::X4,
            ..WindowOptions::default()
        },
    )?;
    let mut display_buffer = vec![0; SCREEN_WIDTH * SCREEN_HEIGHT];
    let mut next_frame = Instant::now();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let mut instructions = 0_u64;

        // Run until the last visible scanline of the frame has been drawn.
        while !emulator.take_frame_ready() {
            let pc = emulator.cpu().registers[15];
            if let Err(error) = emulator.step() {
                return Err(format!(
                    "emulation stopped at PC {pc:#010X} after {instructions} instructions: {error}"
                )
                .into());
            }
            instructions += 1;
        }

        for (output, pixel) in display_buffer.iter_mut().zip(emulator.ppu().framebuffer()) {
            *output = pixel_to_rgb(*pixel);
        }
        window.update_with_buffer(&display_buffer, SCREEN_WIDTH, SCREEN_HEIGHT)?;

        next_frame += FRAME_DURATION;
        let now = Instant::now();
        if next_frame > now {
            thread::sleep(next_frame - now);
        } else {
            next_frame = now;
        }
    }

    Ok(())
}

fn pixel_to_rgb([red, green, blue, _]: Pixel) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_rgba_pixels_to_window_rgb_format() {
        assert_eq!(pixel_to_rgb([0x12, 0x34, 0x56, 0xFF]), 0x0012_3456);
    }
}
