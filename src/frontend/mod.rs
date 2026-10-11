mod audio_output;
mod dock;
mod draw;
mod emulation_thread;
mod hud;
mod input;
mod settings_sync;
mod window_control;

use audio_output::AudioOutput;
use dock::{Dock, Pointer, SpeedSetting};
use emulation_thread::{STATE_LOAD, STATE_LOADED, STATE_SAVE, STATE_SAVED, Shared, emulation_loop};
use gba_emulator::emulator::Emulator;
use gba_emulator::game_library;
use gba_emulator::mmu::ROM_START;
use gba_emulator::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use gba_emulator::settings::Settings;
use input::{
    EscapeAction, LOAD_STATE_KEY, SAVE_STATE_KEY, buttons_from_keys, escape_action,
    fast_forward_held,
};
use minifb::{Key, MouseMode, Scale, ScaleMode, Window, WindowOptions};
use settings_sync::SettingsSync;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

/// The game is upscaled in software so overlays can be drawn at window resolution.
const DISPLAY_SCALE: usize = 4;
const WINDOW_WIDTH: usize = SCREEN_WIDTH * DISPLAY_SCALE;
const WINDOW_HEIGHT: usize = SCREEN_HEIGHT * DISPLAY_SCALE;
/// How long the save-state icon stays up (about 1.3 s at 60 Hz).
const STATE_NOTICE_FRAMES: u32 = 80;
/// Frames to wait after the window opens before restoring full screen.
const RESTORE_FULLSCREEN_AFTER_FRAMES: u32 = 3;

pub fn run(rom_path: &str, save_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut emulator = Emulator::new(rom_path)?;
    emulator.initialize_gba_startup(ROM_START)?;
    if let Some(image) = game_library::read_save(save_path)? {
        emulator.load_save(&image);
        eprintln!("loaded save {}", save_path.display());
    }
    eprintln!(
        "backup memory: {:?}, save file {}",
        emulator.save_kind(),
        save_path.display()
    );

    let settings_path = Settings::path_for_rom(Path::new(rom_path));
    // The save state (one slot) lives beside the ROM, like the settings.
    let state_path = Path::new(rom_path).with_extension("state");
    let settings = match Settings::load(&settings_path) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!(
                "could not read settings {}: {error}",
                settings_path.display()
            );
            Settings::default()
        }
    };
    // Reopen at the size the window had last time (kept at the 3:2 ratio).
    let (initial_width, initial_height) =
        settings
            .window_size
            .map_or((WINDOW_WIDTH, WINDOW_HEIGHT), |(width, _)| {
                let width = width as usize;
                (
                    width,
                    (width * WINDOW_HEIGHT + WINDOW_WIDTH / 2) / WINDOW_WIDTH,
                )
            });

    let mut window = Window::new(
        "GBA Emulator",
        initial_width,
        initial_height,
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

    let shared = Arc::new(Shared::new());
    let worker = {
        let shared = Arc::clone(&shared);
        let sink = audio.as_ref().map(AudioOutput::sink);
        let save_path = save_path.to_path_buf();
        let state_path = state_path.clone();
        thread::Builder::new()
            .name("emulation".into())
            .spawn(move || emulation_loop(emulator, sink, &shared, &save_path, &state_path))?
    };

    let mut dock = Dock::new(settings.volume);
    dock.set_muted(settings.muted);
    dock.set_speed(SpeedSetting::from_multiple(settings.fast_forward));
    let mut restore_fullscreen = settings.fullscreen;
    let mut sync = SettingsSync::new(settings_path, settings);
    let mut frames_shown = 0_u32;
    let mut last_state_notice = 0_u32;
    let mut state_notice_frames = 0_u32;
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
        if window.is_key_pressed(SAVE_STATE_KEY, minifb::KeyRepeat::No) {
            shared.state_request.store(STATE_SAVE, Ordering::Relaxed);
        }
        if window.is_key_pressed(LOAD_STATE_KEY, minifb::KeyRepeat::No) {
            shared.state_request.store(STATE_LOAD, Ordering::Relaxed);
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
            // The window system's button state also covers clicks the window
            // never received because it was not key yet (just after toggling
            // full screen); the position check keeps clicks elsewhere out.
            left_down: window.get_mouse_down(minifb::MouseButton::Left)
                || (position.is_some() && window_control::left_button_down().unwrap_or(false)),
            scroll: window.get_scroll_wheel().map_or(0.0, |(_, y)| y),
        };
        dock.update(pointer, WINDOW_WIDTH, WINDOW_HEIGHT);

        // Show the floppy-disk icon for a moment after a state is saved or
        // restored (the emulation thread reports completed operations).
        let notice = shared.state_notice.load(Ordering::Relaxed);
        if notice != last_state_notice {
            last_state_notice = notice;
            state_notice_frames = STATE_NOTICE_FRAMES;
        }
        if state_notice_frames > 0 {
            state_notice_frames -= 1;
            let kind = if notice & 3 == STATE_LOADED {
                hud::StateNotice::Loaded
            } else {
                hud::StateNotice::Saved
            };
            debug_assert!(notice & 3 == STATE_LOADED || notice & 3 == STATE_SAVED);
            hud::draw_state_notice(&mut display_buffer, WINDOW_WIDTH, WINDOW_HEIGHT, kind);
        }
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
        frames_shown = frames_shown.saturating_add(1);
        if restore_fullscreen && frames_shown >= RESTORE_FULLSCREEN_AFTER_FRAMES {
            restore_fullscreen = false;
            window_control::toggle_fullscreen(&window);
        }
        sync.observe(
            dock.volume(),
            dock.is_muted(),
            dock.speed().multiple(),
            window_control::is_fullscreen(&window),
            Some(window_control::content_size(&window)),
        );
        if let Err(error) = window.update_with_buffer(&display_buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
        {
            result = Err(error.into());
            break;
        }
    }

    sync.flush();
    shared.running.store(false, Ordering::Relaxed);
    let _ = worker.join();
    if let Some(message) = shared.error.lock().ok().and_then(|mut error| error.take()) {
        return Err(message.into());
    }
    result
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

#[cfg(test)]
mod tests;
