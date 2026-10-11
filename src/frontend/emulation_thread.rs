//! The emulation thread: runs the console flat out or paced by audio or the
//! wall clock, and publishes finished frames, audio and speed measurements to
//! the UI thread.

use super::audio_output::AudioSink;
use gba_emulator::apu::SAMPLE_RATE;
use gba_emulator::emulator::Emulator;
use gba_emulator::game_library;
use gba_emulator::ppu::{Pixel, SCREEN_HEIGHT, SCREEN_WIDTH};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);
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

/// [`Shared::state_request`] values.
pub(super) const STATE_SAVE: u8 = 1;
pub(super) const STATE_LOAD: u8 = 2;
/// Low bits of [`Shared::state_notice`].
pub(super) const STATE_SAVED: u32 = 1;
pub(super) const STATE_LOADED: u32 = 2;

/// State shared between the UI thread and the emulation thread.
pub(super) struct Shared {
    pub(super) buttons: AtomicU16,
    pub(super) running: AtomicBool,
    /// Set while the fast-forward key is held.
    pub(super) fast_forward: AtomicBool,
    /// Measured emulation speed as a multiple of real time (`f32` bits).
    pub(super) speed: AtomicU32,
    /// Fast-forward target as a multiple of real time; 0 means unthrottled.
    pub(super) fast_forward_target: AtomicU32,
    /// Set by the UI to ask the emulation thread to restart the game.
    pub(super) restart: AtomicBool,
    /// Number of restarts performed so far.
    pub(super) restarts: AtomicU32,
    /// Set by the UI to ask for a save state operation ([`STATE_SAVE`] or
    /// [`STATE_LOAD`]); cleared by the emulation thread.
    pub(super) state_request: AtomicU8,
    /// The last completed state operation as `count << 2 | kind`
    /// ([`STATE_SAVED`] or [`STATE_LOADED`]); the count lets the UI notice a
    /// repeat of the same operation.
    pub(super) state_notice: AtomicU32,
    /// Number of frames the emulation thread has published.
    pub(super) frames: AtomicU64,
    /// Latest finished frame as `0x00RRGGBB` pixels.
    pub(super) frame: Mutex<Vec<u32>>,
    pub(super) error: Mutex<Option<String>>,
}

impl Shared {
    pub(super) fn new() -> Self {
        Self {
            buttons: AtomicU16::new(0),
            running: AtomicBool::new(true),
            fast_forward: AtomicBool::new(false),
            speed: AtomicU32::new(1.0_f32.to_bits()),
            fast_forward_target: AtomicU32::new(0),
            restart: AtomicBool::new(false),
            restarts: AtomicU32::new(0),
            state_request: AtomicU8::new(0),
            state_notice: AtomicU32::new(0),
            frames: AtomicU64::new(0),
            frame: Mutex::new(vec![0; SCREEN_WIDTH * SCREEN_HEIGHT]),
            error: Mutex::new(None),
        }
    }
}

/// Runs the emulator, publishing frames and audio until told to stop.
pub(super) fn emulation_loop(
    mut emulator: Emulator,
    sink: Option<AudioSink>,
    shared: &Shared,
    save_path: &Path,
    state_path: &Path,
) {
    let mut state_operations = 0_u32;
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

        // Save states are taken and restored between frames, when the audio
        // has been collected and no scanline is half drawn.
        match shared.state_request.swap(0, Ordering::Relaxed) {
            STATE_SAVE => {
                if save_state(&emulator, state_path) {
                    state_operations += 1;
                    shared
                        .state_notice
                        .store(state_operations << 2 | STATE_SAVED, Ordering::Relaxed);
                }
            }
            STATE_LOAD => {
                if load_state(&mut emulator, state_path) {
                    if let Some(sink) = &sink {
                        sink.clear();
                    }
                    speed_window_start = Instant::now();
                    speed_window_frames = 0;
                    state_operations += 1;
                    shared
                        .state_notice
                        .store(state_operations << 2 | STATE_LOADED, Ordering::Relaxed);
                }
            }
            _ => {}
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

/// Writes a save state beside the ROM. Returns whether it was written.
fn save_state(emulator: &Emulator, state_path: &Path) -> bool {
    match game_library::write_save(state_path, &emulator.save_state()) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "could not write save state {}: {error}",
                state_path.display()
            );
            false
        }
    }
}

/// Restores the save state beside the ROM. Returns whether the console was
/// restored; on any failure the running game is left as it was.
fn load_state(emulator: &mut Emulator, state_path: &Path) -> bool {
    let bytes = match std::fs::read(state_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("no save state yet ({})", state_path.display());
            return false;
        }
        Err(error) => {
            eprintln!(
                "could not read save state {}: {error}",
                state_path.display()
            );
            return false;
        }
    };
    match emulator.load_state(&bytes) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "could not load save state {}: {error}",
                state_path.display()
            );
            false
        }
    }
}

/// Writes the backup image if the game changed it.
fn flush_save(emulator: &mut Emulator, save_path: &Path) {
    if let Some(image) = emulator.take_save_if_changed()
        && let Err(error) = game_library::write_save(save_path, &image)
    {
        eprintln!("could not write save {}: {error}", save_path.display());
    }
}

fn pixel_to_rgb([red, green, blue, _]: Pixel) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gba_emulator::mmu::ROM_START;
    use std::sync::Arc;

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
        let shared = Arc::new(Shared::new());
        shared.fast_forward.store(fast_forward, Ordering::Relaxed);
        shared.fast_forward_target.store(target, Ordering::Relaxed);
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                emulation_loop(
                    emulator,
                    sink,
                    &shared,
                    Path::new("/nonexistent-dir/never-written.sav"),
                    Path::new("/nonexistent-dir/never-written.state"),
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
        let shared = Arc::new(Shared::new());
        shared.speed.store(0, Ordering::Relaxed);
        let worker = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                emulation_loop(
                    emulator,
                    None,
                    &shared,
                    Path::new("/nonexistent-dir/x.sav"),
                    Path::new("/nonexistent-dir/x.state"),
                )
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
    fn state_requests_write_and_restore_a_state_and_report_each_once() {
        let path = spin_rom();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        let state_path = std::env::temp_dir().join(format!(
            "gba-frontend-state-test-{}.state",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&state_path);

        let shared = Arc::new(Shared::new());
        let worker = {
            let shared = Arc::clone(&shared);
            let state_path = state_path.clone();
            thread::spawn(move || {
                emulation_loop(
                    emulator,
                    None,
                    &shared,
                    Path::new("/nonexistent-dir/x.sav"),
                    &state_path,
                )
            })
        };
        let notice = |shared: &Shared| shared.state_notice.load(Ordering::Relaxed);
        let wait_for = |shared: &Shared, count: u32, kind: u32| {
            for _ in 0..200 {
                if notice(shared) == (count << 2 | kind) {
                    return true;
                }
                thread::sleep(Duration::from_millis(5));
            }
            false
        };

        // Restoring with no state yet reports nothing and changes nothing.
        shared.state_request.store(STATE_LOAD, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(120));
        assert_eq!(notice(&shared), 0);
        assert!(!state_path.exists());

        shared.state_request.store(STATE_SAVE, Ordering::Relaxed);
        assert!(wait_for(&shared, 1, STATE_SAVED), "save was not reported");
        assert!(state_path.exists());

        shared.state_request.store(STATE_LOAD, Ordering::Relaxed);
        assert!(wait_for(&shared, 2, STATE_LOADED), "load was not reported");

        shared.running.store(false, Ordering::Relaxed);
        worker.join().unwrap();
        assert!(shared.error.lock().unwrap().is_none());
        std::fs::remove_file(state_path).unwrap();
    }

    #[test]
    fn restart_request_resets_the_game_and_keeps_running() {
        let path = spin_rom();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        let shared = Arc::new(Shared::new());
        shared.fast_forward.store(true, Ordering::Relaxed);
        shared.speed.store(0, Ordering::Relaxed);
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
                    Path::new("/nonexistent-dir/x.state"),
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
