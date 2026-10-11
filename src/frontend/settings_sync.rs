//! Keeps the per-ROM settings file up to date while the game runs.
//!
//! Each frame the UI reports the current dock and window state. Changes are
//! written at most once a second (volume drags change every frame) and again
//! on exit, so an abnormal exit loses at most a second of changes.

use gba_emulator::settings::{MAX_WINDOW_WIDTH, MIN_WINDOW_WIDTH, Settings};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const WRITE_INTERVAL: Duration = Duration::from_secs(1);
/// A window size must hold still this many frames before it is remembered, so
/// the in-between sizes of a full-screen or resize animation are never saved.
const STABLE_FRAMES: u32 = 30;

pub(super) struct SettingsSync {
    path: PathBuf,
    /// What the file on disk holds (or the loaded values).
    written: Settings,
    current: Settings,
    candidate_size: Option<(u32, u32)>,
    candidate_frames: u32,
    last_write: Instant,
}

impl SettingsSync {
    pub(super) fn new(path: PathBuf, loaded: Settings) -> Self {
        Self {
            path,
            written: loaded.clone(),
            current: loaded,
            candidate_size: None,
            candidate_frames: 0,
            last_write: Instant::now(),
        }
    }

    #[cfg(test)]
    pub(super) fn settings(&self) -> &Settings {
        &self.current
    }

    /// Records this frame's state. `windowed_size` is the window's content
    /// size in points, only meaningful while not full screen.
    pub(super) fn observe(
        &mut self,
        volume: f32,
        muted: bool,
        fast_forward: Option<u32>,
        fullscreen: bool,
        windowed_size: Option<(f32, f32)>,
    ) {
        self.current.volume = volume;
        self.current.muted = muted;
        self.current.fast_forward = fast_forward;
        self.current.fullscreen = fullscreen;
        match (fullscreen, windowed_size) {
            (false, Some((width, height))) => self.observe_size(width, height),
            _ => self.candidate_frames = 0,
        }
        if self.current != self.written && self.last_write.elapsed() >= WRITE_INTERVAL {
            self.write();
        }
    }

    /// Writes any unsaved changes now (on exit).
    pub(super) fn flush(&mut self) {
        if self.current != self.written {
            self.write();
        }
    }

    fn observe_size(&mut self, width: f32, height: f32) {
        let size = (width.round() as u32, height.round() as u32);
        if self.candidate_size == Some(size) {
            self.candidate_frames = self.candidate_frames.saturating_add(1);
        } else {
            self.candidate_size = Some(size);
            self.candidate_frames = 1;
        }
        let plausible = (MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH).contains(&size.0) && size.1 > 0;
        if self.candidate_frames >= STABLE_FRAMES && plausible {
            self.current.window_size = Some(size);
        }
    }

    fn write(&mut self) {
        self.last_write = Instant::now();
        // Whatever happens, do not retry until something changes again.
        self.written = self.current.clone();
        if let Err(error) = self.current.save(&self.path) {
            eprintln!("could not write settings {}: {error}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync() -> (SettingsSync, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "gba-sync-test-{}-{:?}.settings",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        (SettingsSync::new(path.clone(), Settings::default()), path)
    }

    #[test]
    fn changes_are_written_on_flush_and_read_back() {
        let (mut sync, path) = sync();
        sync.observe(0.25, true, Some(4), false, None);
        assert!(!path.exists(), "writes are rate limited");
        sync.flush();
        let saved = Settings::load(&path).unwrap();
        assert_eq!(saved.volume, 0.25);
        assert!(saved.muted);
        assert_eq!(saved.fast_forward, Some(4));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn nothing_is_written_when_nothing_changed() {
        let (mut sync, path) = sync();
        let defaults = Settings::default();
        sync.observe(defaults.volume, false, None, false, None);
        sync.flush();
        assert!(!path.exists());
    }

    #[test]
    fn a_window_size_is_remembered_only_once_it_holds_still() {
        let (mut sync, path) = sync();
        for _ in 0..STABLE_FRAMES - 1 {
            sync.observe(0.8, false, None, false, Some((1200.0, 800.0)));
        }
        assert_eq!(sync.settings().window_size, None);
        sync.observe(0.8, false, None, false, Some((1200.0, 800.0)));
        assert_eq!(sync.settings().window_size, Some((1200, 800)));
        // A moving size (an animation) does not replace it.
        for step in 0..100 {
            sync.observe(0.8, false, None, false, Some((1300.0 + step as f32, 866.0)));
        }
        assert_eq!(sync.settings().window_size, Some((1200, 800)));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn full_screen_does_not_overwrite_the_windowed_size() {
        let (mut sync, path) = sync();
        for _ in 0..STABLE_FRAMES {
            sync.observe(0.8, false, None, false, Some((960.0, 640.0)));
        }
        for _ in 0..STABLE_FRAMES * 2 {
            sync.observe(0.8, false, None, true, Some((1496.0, 934.0)));
        }
        assert_eq!(sync.settings().window_size, Some((960, 640)));
        assert!(sync.settings().fullscreen);
        let _ = std::fs::remove_file(path);
    }
}
