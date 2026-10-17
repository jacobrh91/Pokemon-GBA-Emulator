mod audio_output;
mod dock;
mod draw;
mod emulation_thread;
mod hud;
mod input;
mod load_jitter;
mod settings_sync;
mod window_control;

use audio_output::AudioOutput;
use dock::{Dock, MenuNav, Pointer, SpeedSetting};
use emulation_thread::{STATE_LOAD, STATE_LOADED, STATE_SAVE, STATE_SAVED, Shared, emulation_loop};
use gba_emulator::cheats::Cheats;
use gba_emulator::cheats::options::CheatOptions;
use gba_emulator::emulator::Emulator;
use gba_emulator::game_library;
use gba_emulator::mmu::ROM_START;
use gba_emulator::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use gba_emulator::settings::Settings;
use input::{
    Bindings, ButtonGate, Control, EscapeAction, LOAD_STATE_KEY, SAVE_STATE_KEY, escape_action,
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
    let emulator = start_emulator(rom_path, save_path, &settings)?;
    let cheat_scan = emulator.cheat_scan();

    let mut window = open_window(&settings)?;

    let audio = match AudioOutput::new() {
        Ok(audio) => Some(audio),
        Err(error) => {
            eprintln!("audio disabled: {error}");
            None
        }
    };

    // What the cheats file holds, read before the emulator moves to its
    // thread.
    let file_wild_species = emulator.cheats().wild_species();
    let file_codes = emulator.cheats().write_count() + emulator.cheats().master_line_count();
    let wild_hook = emulator.wild_species_hook_address();

    let shared = Arc::new(Shared::new());
    shared
        .load_jitter_frames
        .store(settings.state_jitter_frames, Ordering::Relaxed);
    let worker = {
        let shared = Arc::clone(&shared);
        let sink = audio.as_ref().map(AudioOutput::sink);
        let save_path = save_path.to_path_buf();
        let state_path = state_path.clone();
        thread::Builder::new()
            .name("emulation".into())
            .spawn(move || emulation_loop(emulator, sink, &shared, &save_path, &state_path))?
    };

    let mut cheat_options = settings.cheat_options;
    preselect_file_wild_species(&mut cheat_options, file_wild_species, wild_hook);
    let mut dock = Dock::new(settings.volume);
    dock.set_file_codes_available(file_codes);
    dock.set_cheat_options(cheat_options);
    shared.set_cheat_options(cheat_options);
    if std::env::var_os("GBA_EMULATOR_SMOKE_UI").is_some() {
        // Used by scripts/check_codegen.sh: show every piece of UI so the
        // release binary's drawing code is exercised without a mouse.
        dock.open_everything_for_smoke_test();
    }
    dock.set_bindings(Bindings::from_settings(&settings.key_bindings));
    dock.set_muted(settings.muted);
    dock.set_speed(SpeedSetting::from_multiple(settings.fast_forward));
    let mut restore_fullscreen = settings.fullscreen;
    let mut sync = SettingsSync::new(settings_path, settings);
    sync.record_cheat_scan(cheat_scan);
    let mut frames_shown = 0_u32;
    let mut button_gate = ButtonGate::default();
    let mut last_state_notice = 0_u32;
    let mut state_notice_frames = 0_u32;
    let mut frame_pixels = vec![0; SCREEN_WIDTH * SCREEN_HEIGHT];
    let mut display_buffer = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
    let mut result = Ok(());

    while window.is_open() && shared.running.load(Ordering::Relaxed) {
        // Escape leaves full screen first; only from a window does it quit.
        let escape = window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No);
        if escape && dock.menu_open() {
            // An open cheat menu takes Escape first, as the B button does:
            // it leaves an open list, or else closes the menu.
            dock.menu_navigate(MenuNav::Back);
        } else {
            match escape_action(escape, window_control::is_fullscreen(&window)) {
                EscapeAction::None => {}
                EscapeAction::LeaveFullscreen => window_control::toggle_fullscreen(&window),
                EscapeAction::Quit => break,
            }
        }
        if dock.controls_waiting().is_some() {
            // The controls menu is waiting for a key: it takes the next press,
            // and the game and the other shortcuts see nothing.
            for key in window.get_keys_pressed(minifb::KeyRepeat::No) {
                dock.controls_key_pressed(key);
            }
            shared.buttons.store(0, Ordering::Relaxed);
        } else {
            handle_keys(&window, &mut dock, &shared, &mut button_gate);
        }

        let fast_forwarding = dock
            .bindings()
            .fast_forward_held(|key| window.is_key_down(key));
        shared
            .fast_forward
            .store(fast_forwarding, Ordering::Relaxed);

        if let Ok(frame) = shared.frame.lock() {
            frame_pixels.copy_from_slice(&frame);
        }
        upscale(&frame_pixels, &mut display_buffer);

        let pointer = pointer_state(&window);
        dock.update(pointer, WINDOW_WIDTH, WINDOW_HEIGHT);
        if let Some(options) = dock.take_cheat_options_change() {
            shared.set_cheat_options(options);
        }
        if let Some(bindings) = dock.take_bindings_change() {
            sync.record_key_bindings(bindings.to_settings());
        }

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
            dock.cheat_options(),
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

/// Opens the ROM with its cheats file and save, and reports them on the terminal.
fn start_emulator(
    rom_path: &str,
    save_path: &Path,
    settings: &Settings,
) -> Result<Emulator, Box<dyn Error>> {
    // The settings remember where the cheats hook in this ROM, so the
    // signature scan only runs the first time (or after the ROM changes).
    let mut emulator = Emulator::with_cheat_scan(rom_path, settings.cheat_scan.as_ref())?;
    emulator.initialize_gba_startup(ROM_START)?;
    let cheats_path = Cheats::path_for_rom(Path::new(rom_path));
    match Cheats::load(&cheats_path) {
        Ok(cheats) => {
            if cheats.write_count() > 0 || cheats.master_line_count() > 0 {
                let when = match cheats.hook_address() {
                    Some(address) => format!("when the game reaches {address:#010X}"),
                    None => "every frame".to_owned(),
                };
                eprintln!(
                    "cheats {}: {} code write(s), applied {when} once the \"CHEATS FILE\" row of the cheat menu (Tab) is ticked",
                    cheats_path.display(),
                    cheats.write_count(),
                );
            }
            for warning in cheats.warnings() {
                eprintln!("cheats {}: {warning}", cheats_path.display());
            }
            emulator.set_cheats(cheats);
        }
        Err(error) => eprintln!("could not read cheats {}: {error}", cheats_path.display()),
    }
    if let Some(image) = game_library::read_save(save_path)? {
        emulator.load_save(&image);
        eprintln!("loaded save {}", save_path.display());
    }
    eprintln!(
        "backup memory: {:?}, save file {}",
        emulator.save_kind(),
        save_path.display()
    );
    Ok(emulator)
}

/// Opens the window at the size it had last time (kept at the 3:2 ratio).
fn open_window(settings: &Settings) -> Result<Window, Box<dyn Error>> {
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
    Ok(window)
}

/// The cheats file's `wild_species` line preselects the menu's species (when
/// none was saved); the row's checkbox is what turns it on.
fn preselect_file_wild_species(
    options: &mut CheatOptions,
    file_species: Option<u16>,
    wild_hook: Option<u32>,
) {
    let Some(species) = file_species.filter(|_| options.wild_species.is_none()) else {
        return;
    };
    options.wild_species = Some(species);
    let name = gba_emulator::cheats::species::name(species).unwrap_or("?");
    match wild_hook {
        Some(address) => eprintln!(
            "cheats: the cheats file sets the wild species to {name} ({species}); tick its checkbox in the cheat menu (Tab) to use it (found CreateWildMon at {address:#010X})"
        ),
        None => eprintln!(
            "cheats: the cheats file sets the wild species to {name}, but this ROM is not a FireRed, LeafGreen or Emerald build whose wild-encounter function can be found"
        ),
    }
}

/// The mouse mapped back to the 960x640 buffer the dock is drawn in, using the
/// window system's position (minifb's is wrong in full screen).
fn pointer_state(window: &Window) -> Pointer {
    let buffer_size = (WINDOW_WIDTH, WINDOW_HEIGHT);
    let position = match window_control::pointer_in_picture_view(window) {
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
    Pointer {
        position,
        // The window system's button state also covers clicks the window never
        // received because it was not key yet (just after toggling full
        // screen); the position check keeps clicks elsewhere out.
        left_down: window.get_mouse_down(minifb::MouseButton::Left)
            || (position.is_some() && window_control::left_button_down().unwrap_or(false)),
        scroll: window.get_scroll_wheel().map_or(0.0, |(_, y)| y),
    }
}

/// Acts on the keys: the cheat menu, the save state, the cheat menu's own
/// navigation while it is open, and the game's buttons.
fn handle_keys(window: &Window, dock: &mut Dock, shared: &Shared, button_gate: &mut ButtonGate) {
    if window.is_key_pressed(
        dock.bindings().key(Control::CheatMenu),
        minifb::KeyRepeat::No,
    ) {
        dock.toggle_menu();
    }
    if window.is_key_pressed(SAVE_STATE_KEY, minifb::KeyRepeat::No) {
        shared.state_request.store(STATE_SAVE, Ordering::Relaxed);
    }
    if window.is_key_pressed(LOAD_STATE_KEY, minifb::KeyRepeat::No) {
        shared.state_request.store(STATE_LOAD, Ordering::Relaxed);
    }
    // While the cheat menu is open the movement keys drive it, and the game
    // sees no buttons.
    let menu_keys = dock.bindings().menu_keys();
    if dock.cheat_menu_open() {
        for (key, nav, repeats) in menu_keys.iter().copied() {
            let repeat = if repeats {
                minifb::KeyRepeat::Yes
            } else {
                minifb::KeyRepeat::No
            };
            if window.is_key_pressed(key, repeat) {
                dock.menu_navigate(nav);
            }
        }
    }
    dock.menu_hold_toggle(
        dock.cheat_menu_open()
            && menu_keys
                .iter()
                .any(|(key, nav, _)| *nav == MenuNav::Toggle && window.is_key_down(*key)),
    );
    shared.buttons.store(
        button_gate.apply(
            dock.menu_open(),
            dock.bindings().buttons(|key| window.is_key_down(key)),
        ),
        Ordering::Relaxed,
    );
}

/// Nearest-neighbour upscale of the 240x160 frame to the window buffer. Kept
/// out of line like the drawing primitives (see `draw.rs`): constant window
/// dimensions inlined into `run` have tripped an arm64 miscompile.
#[inline(never)]
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
