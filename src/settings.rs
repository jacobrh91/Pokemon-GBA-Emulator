//! Per-ROM user settings, kept in a small `key = value` text file beside the
//! ROM so they are restored the next time the game is played.
//!
//! The format is deliberately forgiving: unknown keys, malformed lines and
//! out-of-range values are ignored (falling back to the default for that
//! setting), so an old or hand-edited file never prevents a game from
//! starting.

use crate::game_library;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_VOLUME: f32 = 0.8;
/// The window is kept at the console's 3:2 aspect ratio; these bound the
/// restored width (in points).
pub const MIN_WINDOW_WIDTH: u32 = 480;
pub const MAX_WINDOW_WIDTH: u32 = 7680;
/// Extension of the settings file, replacing the ROM's own.
const EXTENSION: &str = "settings";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Volume slider position, 0.0..=1.0.
    pub volume: f32,
    pub muted: bool,
    /// Whether the game was left in full screen.
    pub fullscreen: bool,
    /// Content size `(width, height)` of the window when not full screen, or
    /// `None` to use the default size.
    pub window_size: Option<(u32, u32)>,
    /// Fast-forward target as a multiple of real time; `None` is unthrottled.
    pub fast_forward: Option<u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: DEFAULT_VOLUME,
            muted: false,
            fullscreen: false,
            window_size: None,
            fast_forward: None,
        }
    }
}

impl Settings {
    /// Where the settings for `rom` live: beside it, with a `.settings`
    /// extension.
    pub fn path_for_rom(rom: &Path) -> PathBuf {
        rom.with_extension(EXTENSION)
    }

    /// Reads the settings file; a missing file yields the defaults.
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// Writes the settings file atomically.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        game_library::write_save(path, self.to_text().as_bytes())
    }

    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        let (mut width, mut height) = (None, None);
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "volume" => {
                    if let Ok(volume) = value.parse::<f32>()
                        && volume.is_finite()
                    {
                        settings.volume = volume.clamp(0.0, 1.0);
                    }
                }
                "muted" => {
                    if let Ok(muted) = value.parse() {
                        settings.muted = muted;
                    }
                }
                "fullscreen" => {
                    if let Ok(fullscreen) = value.parse() {
                        settings.fullscreen = fullscreen;
                    }
                }
                "window_width" => width = value.parse::<u32>().ok(),
                "window_height" => height = value.parse::<u32>().ok(),
                "fast_forward" => match value.to_ascii_lowercase().as_str() {
                    "max" => settings.fast_forward = None,
                    "2x" => settings.fast_forward = Some(2),
                    "4x" => settings.fast_forward = Some(4),
                    "8x" => settings.fast_forward = Some(8),
                    _ => {}
                },
                _ => {}
            }
        }
        if let (Some(width), Some(height)) = (width, height)
            && (MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH).contains(&width)
            && height > 0
        {
            settings.window_size = Some((width, height));
        }
        settings
    }

    pub fn to_text(&self) -> String {
        let mut text = String::from("# GBA emulator settings for this game\n");
        text.push_str(&format!("volume = {:.2}\n", self.volume));
        text.push_str(&format!("muted = {}\n", self.muted));
        text.push_str(&format!("fullscreen = {}\n", self.fullscreen));
        if let Some((width, height)) = self.window_size {
            text.push_str(&format!("window_width = {width}\n"));
            text.push_str(&format!("window_height = {height}\n"));
        }
        let speed = match self.fast_forward {
            Some(multiple) => format!("{multiple}x"),
            None => "max".to_string(),
        };
        text.push_str(&format!("fast_forward = {speed}\n"));
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_text() {
        let settings = Settings {
            volume: 0.35,
            muted: true,
            fullscreen: true,
            window_size: Some((1200, 800)),
            fast_forward: Some(4),
        };
        assert_eq!(Settings::parse(&settings.to_text()), settings);
        assert_eq!(
            Settings::parse(&Settings::default().to_text()),
            Settings::default()
        );
    }

    #[test]
    fn unknown_keys_comments_and_bad_values_fall_back_to_defaults() {
        let text = "\
# a comment
colour = blue
volume = loud
muted = maybe
fullscreen = true   # trailing comment
fast_forward = 3x
window_width = 100
window_height = 50
";
        let settings = Settings::parse(text);
        assert_eq!(settings.volume, DEFAULT_VOLUME);
        assert!(!settings.muted);
        assert!(settings.fullscreen);
        assert_eq!(settings.fast_forward, None);
        assert_eq!(settings.window_size, None, "too small to be restored");
    }

    #[test]
    fn volume_is_clamped_and_the_window_needs_both_dimensions() {
        assert_eq!(Settings::parse("volume = 7").volume, 1.0);
        assert_eq!(Settings::parse("volume = -1").volume, 0.0);
        assert_eq!(Settings::parse("volume = NaN").volume, DEFAULT_VOLUME);
        assert_eq!(Settings::parse("window_width = 960").window_size, None);
        assert_eq!(
            Settings::parse("window_width = 960\nwindow_height = 640").window_size,
            Some((960, 640))
        );
    }

    #[test]
    fn settings_live_beside_the_rom_and_a_missing_file_is_the_defaults() {
        assert_eq!(
            Settings::path_for_rom(Path::new("/games/Game.gba")),
            Path::new("/games/Game.settings")
        );
        let missing = std::env::temp_dir().join("gba-settings-test-does-not-exist.settings");
        assert_eq!(Settings::load(&missing).unwrap(), Settings::default());
    }

    #[test]
    fn saved_settings_are_loaded_back() {
        let path =
            std::env::temp_dir().join(format!("gba-settings-test-{}.settings", std::process::id()));
        let settings = Settings {
            volume: 0.5,
            window_size: Some((720, 480)),
            fast_forward: Some(8),
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        fs::remove_file(path).unwrap();
    }
}
