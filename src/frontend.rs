use crate::audio_output::{AudioOutput, AudioSink};
use crate::hud;
use crate::volume_dock::{Pointer, VolumeDock};
use crate::window_control;
use gba_emulator::apu::SAMPLE_RATE;
use gba_emulator::emulator::Emulator;
use gba_emulator::input::{self, Buttons};
use gba_emulator::library;
use gba_emulator::mmu::ROM_START;
use gba_emulator::ppu::{Pixel, SCREEN_HEIGHT, SCREEN_WIDTH};
use minifb::{Key, MouseMode, Scale, ScaleMode, Window, WindowOptions};
use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
/// The game is upscaled in software so overlays can be drawn at window resolution.
const DISPLAY_SCALE: usize = 4;
const WINDOW_WIDTH: usize = SCREEN_WIDTH * DISPLAY_SCALE;
const WINDOW_HEIGHT: usize = SCREEN_HEIGHT * DISPLAY_SCALE;
const DEFAULT_VOLUME: f32 = 0.8;

/// Keyboard layout: WASD = D-pad, Space = A, C = B, Q = L, E = R,
/// X = Start, V = Select.
const KEY_BINDINGS: [(Key, Buttons); 10] = [
    (Key::Space, input::A),
    (Key::C, input::B),
    (Key::V, input::SELECT),
    (Key::X, input::START),
    (Key::D, input::RIGHT),
    (Key::A, input::LEFT),
    (Key::W, input::UP),
    (Key::S, input::DOWN),
    (Key::E, input::R),
    (Key::Q, input::L),
];

/// What a press of Escape should do.
#[derive(Debug, PartialEq, Eq)]
enum EscapeAction {
    None,
    /// In full screen, Escape returns to the window instead of quitting.
    LeaveFullscreen,
    Quit,
}

fn escape_action(escape_pressed: bool, fullscreen: bool) -> EscapeAction {
    match (escape_pressed, fullscreen) {
        (false, _) => EscapeAction::None,
        (true, true) => EscapeAction::LeaveFullscreen,
        (true, false) => EscapeAction::Quit,
    }
}

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
/// How often the emulation thread checks for unsaved backup data.
const SAVE_CHECK_INTERVAL: Duration = Duration::from_millis(500);
/// While fast-forwarding, how often a frame is actually drawn and published
/// (the rest are emulated without rendering).
const FAST_FORWARD_PUBLISH_INTERVAL: Duration = Duration::from_millis(16);

/// Window over which the displayed speed multiple is averaged.
const SPEED_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
/// The console's frame rate (16.78 MHz / 280896 cycles).
const GBA_FRAMES_PER_SECOND: f64 = 59.7275;

/// Emulated speed as a multiple of real time for `frames` emulated in `elapsed`.
fn speed_factor(frames: u32, elapsed: Duration) -> f32 {
    if elapsed.is_zero() {
        return 0.0;
    }
    (f64::from(frames) / elapsed.as_secs_f64() / GBA_FRAMES_PER_SECOND) as f32
}

/// Holding either Shift key runs the game as fast as the machine allows.
const FAST_FORWARD_KEYS: [Key; 2] = [Key::LeftShift, Key::RightShift];

fn fast_forward_held(is_down: impl Fn(Key) -> bool) -> bool {
    FAST_FORWARD_KEYS.iter().any(|key| is_down(*key))
}

/// State shared between the UI thread and the emulation thread.
struct Shared {
    buttons: AtomicU16,
    running: AtomicBool,
    /// Set while the fast-forward key is held.
    fast_forward: AtomicBool,
    /// Measured emulation speed as a multiple of real time (`f32` bits).
    speed: AtomicU32,
    /// Fast-forward target as a multiple of real time; 0 means unthrottled.
    fast_forward_target: AtomicU32,
    /// Set by the UI to ask the emulation thread to restart the game.
    restart: AtomicBool,
    /// Number of restarts performed so far.
    restarts: AtomicU32,
    /// Number of frames the emulation thread has published.
    frames: AtomicU64,
    /// Latest finished frame as `0x00RRGGBB` pixels.
    frame: Mutex<Vec<u32>>,
    error: Mutex<Option<String>>,
}

pub fn run(rom_path: &str, save_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut emulator = Emulator::new(rom_path)?;
    emulator.initialize_gba_startup(ROM_START)?;
    if let Some(image) = library::read_save(save_path)? {
        emulator.load_save(&image);
        eprintln!("loaded save {}", save_path.display());
    }
    eprintln!(
        "backup memory: {:?}, save file {}",
        emulator.save_kind(),
        save_path.display()
    );

    let mut window = Window::new(
        "GBA Emulator",
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WindowOptions {
            scale: Scale::X1,
            // Resizable, with the picture scaled uniformly (bars, not
            // stretching, when the window's shape differs).
            resize: true,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )?;
    // Native extras minifb lacks: keep the window frame itself at 3:2 while
    // resizing, set a minimum size, and allow full screen.
    window_control::lock_aspect_ratio(
        &window,
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WINDOW_WIDTH / 2,
        WINDOW_HEIGHT / 2,
    );
    window_control::enable_fullscreen(&window);
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
        fast_forward: AtomicBool::new(false),
        speed: AtomicU32::new(1.0_f32.to_bits()),
        fast_forward_target: AtomicU32::new(0),
        restart: AtomicBool::new(false),
        restarts: AtomicU32::new(0),
        frames: AtomicU64::new(0),
        frame: Mutex::new(vec![0; SCREEN_WIDTH * SCREEN_HEIGHT]),
        error: Mutex::new(None),
    });
    let worker = {
        let shared = Arc::clone(&shared);
        let sink = audio.as_ref().map(AudioOutput::sink);
        let save_path = save_path.to_path_buf();
        thread::Builder::new()
            .name("emulation".into())
            .spawn(move || emulation_loop(emulator, sink, &shared, &save_path))?
    };

    let mut dock = VolumeDock::new(DEFAULT_VOLUME);
    let mut frame_pixels = vec![0; SCREEN_WIDTH * SCREEN_HEIGHT];
    let mut display_buffer = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
    let mut result = Ok(());

    while window.is_open() && shared.running.load(Ordering::Relaxed) {
        // Escape leaves full screen first; only from a window does it quit.
        let escape = window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No);
        match escape_action(escape, window_control::is_fullscreen(&window)) {
            EscapeAction::None => {}
            EscapeAction::LeaveFullscreen => window_control::toggle_fullscreen(&window),
            EscapeAction::Quit => break,
        }
        shared.buttons.store(
            buttons_from_keys(|key| window.is_key_down(key)),
            Ordering::Relaxed,
        );

        let fast_forwarding = fast_forward_held(|key| window.is_key_down(key));
        shared
            .fast_forward
            .store(fast_forwarding, Ordering::Relaxed);

        if let Ok(frame) = shared.frame.lock() {
            frame_pixels.copy_from_slice(&frame);
        }
        upscale(&frame_pixels, &mut display_buffer);

        // The picture is scaled into the window, so the mouse position has to
        // be mapped back to the 960x640 buffer the dock is drawn in. Ask the
        // window system for the cursor relative to the picture view itself:
        // minifb's own mouse values are computed against a rectangle that is
        // wrong in full screen and after leaving it, and it clips against a
        // window size it only refreshes when a resize drag ends.
        let buffer_size = (WINDOW_WIDTH, WINDOW_HEIGHT);
        let position = match window_control::pointer_in_picture_view(&window) {
            Some((pointer, view_size)) => {
                window_control::pointer_in_buffer(Some(pointer), view_size, buffer_size)
            }
            None => {
                let (width, height) = window.get_size();
                window_control::pointer_in_buffer(
                    window.get_mouse_pos(MouseMode::Pass),
                    (width as f32, height as f32),
                    buffer_size,
                )
            }
        };
        let pointer = Pointer {
            position,
            left_down: window.get_mouse_down(minifb::MouseButton::Left),
            scroll: window.get_scroll_wheel().map_or(0.0, |(_, y)| y),
        };
        dock.update(pointer, WINDOW_WIDTH, WINDOW_HEIGHT);
        if fast_forwarding {
            let speed = f32::from_bits(shared.speed.load(Ordering::Relaxed));
            hud::draw_fast_forward(&mut display_buffer, WINDOW_WIDTH, WINDOW_HEIGHT, speed);
        }
        dock.draw(&mut display_buffer, WINDOW_WIDTH, WINDOW_HEIGHT);
        shared
            .fast_forward_target
            .store(dock.speed().multiple().unwrap_or(0), Ordering::Relaxed);
        if dock.take_restart_request() {
            shared.restart.store(true, Ordering::Relaxed);
        }
        let fullscreen_shortcut = window.is_key_pressed(Key::F11, minifb::KeyRepeat::No)
            || (window.is_key_pressed(Key::F, minifb::KeyRepeat::No)
                && (window.is_key_down(Key::LeftSuper) || window.is_key_down(Key::RightSuper)));
        if dock.take_fullscreen_request() || fullscreen_shortcut {
            window_control::toggle_fullscreen(&window);
        }
        if dock.take_quit_request() {
            // Same path as Escape or closing the window: the emulation
            // thread is joined and the save flushed below.
            break;
        }
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
fn emulation_loop(
    mut emulator: Emulator,
    sink: Option<AudioSink>,
    shared: &Shared,
    save_path: &Path,
) {
    let mut next_frame = Instant::now();
    let mut last_save_check = Instant::now();
    let mut last_publish = Instant::now();
    let mut speed_window_start = Instant::now();
    let mut speed_window_frames = 0_u32;
    let mut was_throttled = false;
    let mut instructions = 0_u64;

    while shared.running.load(Ordering::Relaxed) {
        // A restart is applied between frames: persist the save, power-cycle
        // the console (the save is kept) and drop audio from the old run.
        if shared.restart.swap(false, Ordering::Relaxed) {
            flush_save(&mut emulator, save_path);
            if let Err(error) = emulator.reset() {
                if let Ok(mut slot) = shared.error.lock() {
                    *slot = Some(format!("could not restart the game: {error}"));
                }
                shared.running.store(false, Ordering::Relaxed);
                return;
            }
            if let Some(sink) = &sink {
                sink.clear();
            }
            shared.restarts.fetch_add(1, Ordering::Relaxed);
            speed_window_start = Instant::now();
            speed_window_frames = 0;
        }

        emulator.set_buttons(shared.buttons.load(Ordering::Relaxed));

        // Fast-forward emulates flat out but only draws the frames that will
        // be shown. Decided here, at a frame boundary.
        let fast_forward = shared.fast_forward.load(Ordering::Relaxed);
        let draw_frame = !fast_forward || last_publish.elapsed() >= FAST_FORWARD_PUBLISH_INTERVAL;
        emulator.set_rendering(draw_frame);

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
                flush_save(&mut emulator, save_path);
                return;
            }
            instructions += 1;
        }

        // Sped-up audio would only overflow the queue, so it is dropped.
        let samples = emulator.take_audio_samples();
        if let (Some(sink), false) = (&sink, fast_forward) {
            sink.push(&samples);
        }
        if draw_frame {
            last_publish = Instant::now();
            if let Ok(mut frame) = shared.frame.lock() {
                for (output, pixel) in frame.iter_mut().zip(emulator.ppu().framebuffer()) {
                    *output = pixel_to_rgb(*pixel);
                }
            }
        }

        shared.frames.fetch_add(1, Ordering::Relaxed);

        // Emulated speed as a multiple of the console's real-time rate.
        speed_window_frames += 1;
        let window = speed_window_start.elapsed();
        if window >= SPEED_SAMPLE_INTERVAL {
            let factor = speed_factor(speed_window_frames, window);
            shared.speed.store(factor.to_bits(), Ordering::Relaxed);
            speed_window_start = Instant::now();
            speed_window_frames = 0;
        }

        // Persist backup memory shortly after a game writes it.
        if last_save_check.elapsed() >= SAVE_CHECK_INTERVAL {
            last_save_check = Instant::now();
            flush_save(&mut emulator, save_path);
        }

        if fast_forward {
            let target = shared.fast_forward_target.load(Ordering::Relaxed);
            if target == 0 {
                // MAX: no pacing at all; also drop any wall-clock debt.
                next_frame = Instant::now();
            } else {
                // A fixed multiple of real time: each emulated frame owns
                // 1/target of a console frame of wall-clock time.
                if !was_throttled {
                    next_frame = Instant::now();
                }
                next_frame += FRAME_DURATION / target;
                let now = Instant::now();
                if next_frame > now {
                    thread::sleep(next_frame - now);
                } else {
                    // Cannot keep up (slow machine): run flat out, no debt.
                    next_frame = now;
                }
            }
            was_throttled = target != 0;
            continue;
        }
        was_throttled = false;

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
    // Shutting down: make sure the last writes reach the disk.
    flush_save(&mut emulator, save_path);
}

/// Writes the backup image if the game changed it.
fn flush_save(emulator: &mut Emulator, save_path: &Path) {
    if let Some(image) = emulator.take_save_if_changed() {
        if let Err(error) = library::write_save(save_path, &image) {
            eprintln!("could not write save {}: {error}", save_path.display());
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
        run_loop(sink, millis, false)
    }

    fn run_loop(sink: Option<AudioSink>, millis: u64, fast_forward: bool) -> u64 {
        run_loop_at(sink, millis, fast_forward, 0)
    }

    fn run_loop_at(sink: Option<AudioSink>, millis: u64, fast_forward: bool, target: u32) -> u64 {
        let path = spin_rom();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        let shared = Arc::new(Shared {
            buttons: AtomicU16::new(0),
            running: AtomicBool::new(true),
            fast_forward: AtomicBool::new(fast_forward),
            speed: AtomicU32::new(1.0_f32.to_bits()),
            fast_forward_target: AtomicU32::new(target),
            restart: AtomicBool::new(false),
            restarts: AtomicU32::new(0),
            frames: AtomicU64::new(0),
            frame: Mutex::new(vec![0; SCREEN_WIDTH * SCREEN_HEIGHT]),
            error: Mutex::new(None),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                emulation_loop(
                    emulator,
                    sink,
                    &shared,
                    Path::new("/nonexistent-dir/never-written.sav"),
                )
            })
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
    fn speed_factor_is_frames_per_second_over_the_console_rate() {
        // 60 frames in a second is almost exactly real time on a GBA.
        let normal = speed_factor(60, Duration::from_secs(1));
        assert!((normal - 1.0).abs() < 0.01, "{normal}");
        let fast = speed_factor(240, Duration::from_millis(1000));
        assert!((fast - 4.02).abs() < 0.02, "{fast}");
        assert_eq!(speed_factor(10, Duration::ZERO), 0.0);
    }

    #[test]
    fn emulation_loop_publishes_a_speed_measurement() {
        let path = spin_rom();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        let shared = Arc::new(Shared {
            buttons: AtomicU16::new(0),
            running: AtomicBool::new(true),
            fast_forward: AtomicBool::new(false),
            speed: AtomicU32::new(0),
            fast_forward_target: AtomicU32::new(0),
            restart: AtomicBool::new(false),
            restarts: AtomicU32::new(0),
            frames: AtomicU64::new(0),
            frame: Mutex::new(vec![0; SCREEN_WIDTH * SCREEN_HEIGHT]),
            error: Mutex::new(None),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                emulation_loop(emulator, None, &shared, Path::new("/nonexistent-dir/x.sav"))
            })
        };
        thread::sleep(Duration::from_millis(800));
        shared.running.store(false, Ordering::Relaxed);
        worker.join().unwrap();

        // Wall-clock paced at one console frame per frame: about real time.
        let measured = f32::from_bits(shared.speed.load(Ordering::Relaxed));
        assert!((0.5..=1.6).contains(&measured), "measured {measured}x");
    }

    #[test]
    fn restart_request_resets_the_game_and_keeps_running() {
        let path = spin_rom();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        let shared = Arc::new(Shared {
            buttons: AtomicU16::new(0),
            running: AtomicBool::new(true),
            fast_forward: AtomicBool::new(true),
            speed: AtomicU32::new(0),
            fast_forward_target: AtomicU32::new(0),
            restart: AtomicBool::new(false),
            restarts: AtomicU32::new(0),
            frames: AtomicU64::new(0),
            frame: Mutex::new(vec![0; SCREEN_WIDTH * SCREEN_HEIGHT]),
            error: Mutex::new(None),
        });
        let sink = AudioSink::detached();
        sink.push(&[[1, 1]; 100]);
        let worker = {
            let shared = Arc::clone(&shared);
            let sink = sink.clone();
            thread::spawn(move || {
                emulation_loop(
                    emulator,
                    Some(sink),
                    &shared,
                    Path::new("/nonexistent-dir/x.sav"),
                )
            })
        };
        thread::sleep(Duration::from_millis(100));
        assert_eq!(shared.restarts.load(Ordering::Relaxed), 0);

        shared.restart.store(true, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(150));
        let after_first = shared.restarts.load(Ordering::Relaxed);
        let frames_then = shared.frames.load(Ordering::Relaxed);
        shared.restart.store(true, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(150));
        let frames_now = shared.frames.load(Ordering::Relaxed);

        shared.running.store(false, Ordering::Relaxed);
        worker.join().unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(after_first, 1);
        assert_eq!(shared.restarts.load(Ordering::Relaxed), 2);
        assert!(
            frames_now > frames_then,
            "emulation continues after a restart"
        );
        assert!(shared.error.lock().unwrap().is_none());
        assert_eq!(sink.queued_frames(), 0, "old audio was discarded");
    }

    #[test]
    fn fast_forward_target_throttles_to_a_multiple_of_real_time() {
        // Frames emulated in 600 ms with no audio device (wall-clock pacing).
        let normal = run_loop_at(None, 600, false, 0) as f64;
        let two = run_loop_at(None, 600, true, 2) as f64;
        let four = run_loop_at(None, 600, true, 4) as f64;
        let max = run_loop_at(None, 600, true, 0) as f64;

        // Real time is ~36 frames in 600 ms; allow generous scheduler slack.
        assert!((20.0..=50.0).contains(&normal), "normal {normal}");
        assert!(
            two > normal * 1.5 && two < normal * 2.6,
            "2x {two} vs {normal}"
        );
        assert!(four > two * 1.5 && four < two * 2.6, "4x {four} vs {two}");
        assert!(max > four * 1.5, "MAX {max} vs {four}");
    }

    #[test]
    fn escape_leaves_full_screen_and_only_quits_from_a_window() {
        assert_eq!(escape_action(false, false), EscapeAction::None);
        assert_eq!(escape_action(false, true), EscapeAction::None);
        assert_eq!(escape_action(true, true), EscapeAction::LeaveFullscreen);
        assert_eq!(escape_action(true, false), EscapeAction::Quit);
    }

    #[test]
    fn either_shift_key_holds_fast_forward() {
        assert!(fast_forward_held(|key| key == Key::LeftShift));
        assert!(fast_forward_held(|key| key == Key::RightShift));
        assert!(!fast_forward_held(|key| key == Key::Space));
        // Shift is not a game button, so holding it never presses one.
        assert_eq!(buttons_from_keys(|key| key == Key::LeftShift), 0);
    }

    #[test]
    fn fast_forward_runs_unthrottled_and_drops_audio() {
        // Pre-fill the queue above the pacing target so normal play must
        // wait on it every frame (bounded at 40 ms), and nothing drains it.
        let full_queue = |sink: &AudioSink| {
            sink.push(&vec![[0, 0]; TARGET_QUEUED_AUDIO_FRAMES + 1000]);
            sink.queued_frames()
        };

        let paced_sink = AudioSink::detached();
        let paced_start = full_queue(&paced_sink);
        let paced_frames = run_loop(Some(paced_sink.clone()), 500, false);
        assert!(
            paced_sink.queued_frames() > paced_start,
            "normal play queues audio"
        );

        let fast_sink = AudioSink::detached();
        let fast_start = full_queue(&fast_sink);
        let fast_frames = run_loop(Some(fast_sink.clone()), 500, true);
        assert_eq!(
            fast_sink.queued_frames(),
            fast_start,
            "fast-forward discards audio instead of queueing it"
        );
        // Paced play waits ~40 ms per frame here (about 12 frames in 500 ms);
        // fast-forward never waits.
        assert!(paced_frames <= 14, "{paced_frames} paced frames");
        assert!(
            fast_frames > paced_frames,
            "{fast_frames} fast vs {paced_frames} paced frames"
        );
    }

    #[test]
    fn keys_map_to_gba_buttons() {
        let held = [Key::Space, Key::X, Key::A, Key::E];
        let buttons = buttons_from_keys(|key| held.contains(&key));
        assert_eq!(buttons, input::A | input::START | input::LEFT | input::R);

        // Every binding maps to a distinct button, and the old arrow-key
        // layout is no longer bound.
        let all: Buttons = KEY_BINDINGS
            .iter()
            .fold(0, |mask, (_, button)| mask | button);
        assert_eq!(all, input::ALL);
        assert_eq!(
            buttons_from_keys(|key| key == Key::Left || key == Key::Enter),
            0
        );
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

    #[test]
    fn changed_backup_memory_is_flushed_once_and_reloaded() {
        static NEXT: AtomicU64 = AtomicU64::new(100);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("gba-save-test-{}-{id}", std::process::id()));
        let rom = directory.join("game.gba");
        std::fs::create_dir_all(&directory).unwrap();
        let mut image = vec![0u8; 0x200];
        image[0x100..0x10C].copy_from_slice(b"FLASH1M_V103");
        std::fs::write(&rom, image).unwrap();
        let save = directory.join("saves/game.sav");

        let mut emulator = Emulator::new(rom.to_str().unwrap()).unwrap();
        // Unlock + program one byte through the cartridge bus.
        let bus = emulator.memory_mut();
        for (address, value) in [
            (0x5555, 0xAA),
            (0x2AAA, 0x55),
            (0x5555, 0xA0),
            (0x0042, 0x7E),
        ] {
            bus.write_u8(0x0E00_0000 + address, value).unwrap();
        }

        flush_save(&mut emulator, &save);
        let written = std::fs::read(&save).unwrap();
        assert_eq!(written.len(), 128 * 1024);
        assert_eq!(written[0x42], 0x7E);

        // Nothing changed since: no rewrite.
        std::fs::remove_file(&save).unwrap();
        flush_save(&mut emulator, &save);
        assert!(!save.exists());

        let mut reloaded = Emulator::new(rom.to_str().unwrap()).unwrap();
        reloaded.load_save(&written);
        assert_eq!(reloaded.memory().read_u8(0x0E00_0042).unwrap(), 0x7E);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
