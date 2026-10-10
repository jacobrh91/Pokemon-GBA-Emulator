use crate::audio_output::{AudioOutput, AudioSink};
use crate::volume_dock::{Pointer, VolumeDock};
use gba_emulator::apu::SAMPLE_RATE;
use gba_emulator::emulator::Emulator;
use gba_emulator::input::{self, Buttons};
use gba_emulator::mmu::ROM_START;
use gba_emulator::ppu::{Pixel, SCREEN_HEIGHT, SCREEN_WIDTH};
use minifb::{Key, MouseMode, Scale, Window, WindowOptions};
use std::error::Error;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
/// The game is upscaled in software so overlays can be drawn at window resolution.
const DISPLAY_SCALE: usize = 4;
const WINDOW_WIDTH: usize = SCREEN_WIDTH * DISPLAY_SCALE;
const WINDOW_HEIGHT: usize = SCREEN_HEIGHT * DISPLAY_SCALE;
const DEFAULT_VOLUME: f32 = 0.8;

/// Keyboard layout (mGBA's defaults): arrows = D-pad, X = A, Z = B,
/// A = L, S = R, Enter = Start, Backspace = Select.
const KEY_BINDINGS: [(Key, Buttons); 10] = [
    (Key::X, input::A),
    (Key::Z, input::B),
    (Key::Backspace, input::SELECT),
    (Key::Enter, input::START),
    (Key::Right, input::RIGHT),
    (Key::Left, input::LEFT),
    (Key::Up, input::UP),
    (Key::Down, input::DOWN),
    (Key::S, input::R),
    (Key::A, input::L),
];

fn buttons_from_keys(is_down: impl Fn(Key) -> bool) -> Buttons {
    KEY_BINDINGS
        .iter()
        .filter(|(key, _)| is_down(*key))
        .fold(0, |buttons, (_, button)| buttons | button)
}

/// Audio the emulation thread keeps queued ahead of the device. Emulation is
/// paced by this level rather than by the UI, so a stalled window (macOS Space
/// swipes, dragging, resizing) cannot starve the audio.
const TARGET_QUEUED_AUDIO_FRAMES: usize = SAMPLE_RATE as usize / 16;
/// Longest the emulation thread waits for the audio queue to drain per frame.
const MAX_AUDIO_WAIT: Duration = Duration::from_millis(40);

/// State shared between the UI thread and the emulation thread.
struct Shared {
    buttons: AtomicU16,
    running: AtomicBool,
    /// Number of frames the emulation thread has published.
    frames: AtomicU64,
    /// Latest finished frame as `0x00RRGGBB` pixels.
    frame: Mutex<Vec<u32>>,
    error: Mutex<Option<String>>,
}

pub fn run(rom_path: &str) -> Result<(), Box<dyn Error>> {
    let mut emulator = Emulator::new(rom_path)?;
    emulator.initialize_gba_startup(ROM_START)?;

    let mut window = Window::new(
        "GBA Emulator",
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WindowOptions {
            scale: Scale::X1,
            ..WindowOptions::default()
        },
    )?;
    // The window runs at the display's pace; emulation runs on its own thread.
    window.set_target_fps(60);

    let audio = match AudioOutput::new() {
        Ok(audio) => Some(audio),
        Err(error) => {
            eprintln!("audio disabled: {error}");
            None
        }
    };

    let shared = Arc::new(Shared {
        buttons: AtomicU16::new(0),
        running: AtomicBool::new(true),
        frames: AtomicU64::new(0),
        frame: Mutex::new(vec![0; SCREEN_WIDTH * SCREEN_HEIGHT]),
        error: Mutex::new(None),
    });
    let worker = {
        let shared = Arc::clone(&shared);
        let sink = audio.as_ref().map(AudioOutput::sink);
        thread::Builder::new()
            .name("emulation".into())
            .spawn(move || emulation_loop(emulator, sink, &shared))?
    };

    let mut dock = VolumeDock::new(DEFAULT_VOLUME);
    let mut frame_pixels = vec![0; SCREEN_WIDTH * SCREEN_HEIGHT];
    let mut display_buffer = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
    let mut result = Ok(());

    while window.is_open()
        && !window.is_key_down(Key::Escape)
        && shared.running.load(Ordering::Relaxed)
    {
        shared.buttons.store(
            buttons_from_keys(|key| window.is_key_down(key)),
            Ordering::Relaxed,
        );

        if let Ok(frame) = shared.frame.lock() {
            frame_pixels.copy_from_slice(&frame);
        }
        upscale(&frame_pixels, &mut display_buffer);

        let pointer = Pointer {
            position: window.get_mouse_pos(MouseMode::Discard),
            left_down: window.get_mouse_down(minifb::MouseButton::Left),
            scroll: window.get_scroll_wheel().map_or(0.0, |(_, y)| y),
        };
        dock.update(pointer, WINDOW_WIDTH, WINDOW_HEIGHT);
        dock.draw(&mut display_buffer, WINDOW_WIDTH, WINDOW_HEIGHT);
        if let Some(audio) = &audio {
            audio.set_gain(dock.gain());
        }
        if let Err(error) = window.update_with_buffer(&display_buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
        {
            result = Err(error.into());
            break;
        }
    }

    shared.running.store(false, Ordering::Relaxed);
    let _ = worker.join();
    if let Some(message) = shared.error.lock().ok().and_then(|mut error| error.take()) {
        return Err(message.into());
    }
    result
}

/// Runs the emulator, publishing frames and audio until told to stop.
fn emulation_loop(mut emulator: Emulator, sink: Option<AudioSink>, shared: &Shared) {
    let mut next_frame = Instant::now();
    let mut instructions = 0_u64;

    while shared.running.load(Ordering::Relaxed) {
        emulator.set_buttons(shared.buttons.load(Ordering::Relaxed));

        // Run until the last visible scanline of the frame has been drawn.
        while !emulator.take_frame_ready() {
            let pc = emulator.cpu().registers[15];
            if let Err(error) = emulator.step() {
                if let Ok(mut slot) = shared.error.lock() {
                    *slot = Some(format!(
                        "emulation stopped at PC {pc:#010X} after {instructions} instructions: {error}"
                    ));
                }
                shared.running.store(false, Ordering::Relaxed);
                return;
            }
            instructions += 1;
        }

        let samples = emulator.take_audio_samples();
        if let Some(sink) = &sink {
            sink.push(&samples);
        }
        if let Ok(mut frame) = shared.frame.lock() {
            for (output, pixel) in frame.iter_mut().zip(emulator.ppu().framebuffer()) {
                *output = pixel_to_rgb(*pixel);
            }
        }

        shared.frames.fetch_add(1, Ordering::Relaxed);

        match &sink {
            // Audio-paced: sleep while the device still has plenty queued.
            Some(sink) => {
                let deadline = Instant::now() + MAX_AUDIO_WAIT;
                while shared.running.load(Ordering::Relaxed)
                    && sink.queued_frames() > TARGET_QUEUED_AUDIO_FRAMES
                    && Instant::now() < deadline
                {
                    thread::sleep(Duration::from_millis(1));
                }
                next_frame = Instant::now();
            }
            // No audio device: fall back to wall-clock pacing.
            None => {
                next_frame += FRAME_DURATION;
                let now = Instant::now();
                if next_frame > now {
                    thread::sleep(next_frame - now);
                } else {
                    next_frame = now;
                }
            }
        }
    }
}

/// Nearest-neighbour upscale of the 240x160 frame to the window buffer.
fn upscale(framebuffer: &[u32], output: &mut [u32]) {
    for (y, row) in framebuffer.chunks_exact(SCREEN_WIDTH).enumerate() {
        let first = y * DISPLAY_SCALE * WINDOW_WIDTH;
        for (x, rgb) in row.iter().enumerate() {
            output[first + x * DISPLAY_SCALE..first + (x + 1) * DISPLAY_SCALE].fill(*rgb);
        }
        for line in 1..DISPLAY_SCALE {
            output.copy_within(first..first + WINDOW_WIDTH, first + line * WINDOW_WIDTH);
        }
    }
}

fn pixel_to_rgb([red, green, blue, _]: Pixel) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin_rom() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("gba-frontend-test-{}-{id}.gba", std::process::id()));
        // ARM `b .` forever.
        std::fs::write(&path, 0xEAFF_FFFEu32.to_le_bytes()).unwrap();
        path
    }

    fn run_loop_for(sink: Option<AudioSink>, millis: u64) -> u64 {
        let path = spin_rom();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        let shared = Arc::new(Shared {
            buttons: AtomicU16::new(0),
            running: AtomicBool::new(true),
            frames: AtomicU64::new(0),
            frame: Mutex::new(vec![0; SCREEN_WIDTH * SCREEN_HEIGHT]),
            error: Mutex::new(None),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || emulation_loop(emulator, sink, &shared))
        };
        thread::sleep(Duration::from_millis(millis));
        shared.running.store(false, Ordering::Relaxed);
        worker.join().unwrap();
        assert!(shared.error.lock().unwrap().is_none());
        shared.frames.load(Ordering::Relaxed)
    }

    #[test]
    fn emulation_thread_advances_without_any_ui_activity() {
        // With no audio device it is paced at ~60 fps from the wall clock.
        let frames = run_loop_for(None, 400);
        assert!((10..=40).contains(&frames), "{frames} frames");
    }

    #[test]
    fn audio_pacing_keeps_emulation_running_when_the_device_stops_draining() {
        // Nobody drains the queue (a stalled output): the loop must not block
        // forever, it throttles to one frame per bounded wait.
        let sink = AudioSink::detached();
        let frames = run_loop_for(Some(sink.clone()), 600);
        assert!(frames >= 5, "{frames} frames");
        assert!(sink.queued_frames() > 0);
    }

    #[test]
    fn keys_map_to_gba_buttons() {
        let held = [Key::X, Key::Enter, Key::Left, Key::S];
        let buttons = buttons_from_keys(|key| held.contains(&key));
        assert_eq!(buttons, input::A | input::START | input::LEFT | input::R);
        assert_eq!(buttons_from_keys(|_| false), 0);
        assert_eq!(buttons_from_keys(|_| true), input::ALL);
    }

    #[test]
    fn upscale_replicates_each_pixel_into_a_square_block() {
        let mut frame = vec![0u32; SCREEN_WIDTH * SCREEN_HEIGHT];
        frame[SCREEN_WIDTH + 2] = 0x0012_3456;
        let mut output = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
        upscale(&frame, &mut output);
        for dy in 0..DISPLAY_SCALE {
            for dx in 0..DISPLAY_SCALE {
                let index = (DISPLAY_SCALE + dy) * WINDOW_WIDTH + 2 * DISPLAY_SCALE + dx;
                assert_eq!(output[index], 0x0012_3456);
            }
        }
        assert_eq!(
            output[(DISPLAY_SCALE) * WINDOW_WIDTH + 2 * DISPLAY_SCALE - 1],
            0
        );
    }

    #[test]
    fn converts_rgba_pixels_to_window_rgb_format() {
        assert_eq!(pixel_to_rgb([0x12, 0x34, 0x56, 0xFF]), 0x0012_3456);
    }
}
