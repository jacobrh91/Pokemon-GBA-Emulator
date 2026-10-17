//! Per-ROM user settings, kept in a small `key = value` text file beside the
//! ROM so they are restored the next time the game is played.
//!
//! The format is forgiving: unknown keys, malformed lines and out-of-range
//! values fall back to the default, so a bad file never blocks a game.

use crate::cheats::options::{CheatOptions, MAX_LEVEL, PerfectTarget, ShinyOdds};
use crate::cheats::scan::{self, CheatScan};
use crate::cheats::species;
use crate::game_library;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_VOLUME: f32 = 0.8;
/// Extra frames run, at most, after loading a save state so the game's random
/// number generator is not replayed identically (see the frontend).
pub const DEFAULT_STATE_JITTER_FRAMES: u32 = 30;
pub const MAX_STATE_JITTER_FRAMES: u32 = 600;
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
    /// Upper bound of the random number of extra frames run after loading a
    /// save state; 0 replays states exactly.
    pub state_jitter_frames: u32,
    /// The cheat menu's options, restored the next time the game is played.
    pub cheat_options: CheatOptions,
    /// Where the built-in cheats hook in this game's ROM, found by the first
    /// scan of it; `None` until that has happened.
    pub cheat_scan: Option<CheatScan>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: DEFAULT_VOLUME,
            muted: false,
            fullscreen: false,
            window_size: None,
            fast_forward: None,
            state_jitter_frames: DEFAULT_STATE_JITTER_FRAMES,
            cheat_options: CheatOptions::default(),
            cheat_scan: None,
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
        // Settings from before each row had its own switch: a saved species
        // or level was active, and so were saved perfect-stats targets.
        let (mut wild_switch_seen, mut perfect_switch_seen) = (false, false);
        // The perfect-stats target was two flags before it became one choice
        // of wild / opponents / both; old files are read into that choice.
        let (mut legacy_wild, mut legacy_opponents, mut target_seen) = (false, false, false);
        // The `cheat_scan_*` lines, handed to `CheatScan::from_pairs`.
        let mut scan_pairs: Vec<(&str, &str)> = Vec::new();
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
                "state_jitter_frames" => {
                    if let Ok(frames) = value.parse::<u32>() {
                        settings.state_jitter_frames = frames.min(MAX_STATE_JITTER_FRAMES);
                    }
                }
                "cheat_shiny" => set_flag(value, &mut settings.cheat_options.shiny),
                "cheat_shiny_odds" => {
                    // "10" or "1/10": one in ten.
                    let number = value.strip_prefix("1/").unwrap_or(value);
                    if let Some(odds) = number.parse::<u32>().ok().and_then(ShinyOdds::from_one_in)
                    {
                        settings.cheat_options.shiny_odds = odds;
                    }
                }
                "cheat_perfect_target" => {
                    if let Some(target) = PerfectTarget::from_key(value) {
                        settings.cheat_options.perfect_target = target;
                        target_seen = true;
                    }
                }
                "cheat_perfect_wild" => set_flag(value, &mut legacy_wild),
                "cheat_perfect_opponent" => set_flag(value, &mut legacy_opponents),
                "cheat_infinite_money" => {
                    set_flag(value, &mut settings.cheat_options.infinite_money)
                }
                "cheat_catch_always" => set_flag(value, &mut settings.cheat_options.catch_always),
                "cheat_no_encounters" => set_flag(value, &mut settings.cheat_options.no_encounters),
                "cheat_exp_boost" => set_flag(value, &mut settings.cheat_options.exp_boost),
                "cheat_wild_enabled" => {
                    wild_switch_seen = true;
                    set_flag(value, &mut settings.cheat_options.wild_enabled);
                }
                "cheat_perfect_enabled" => {
                    perfect_switch_seen = true;
                    set_flag(value, &mut settings.cheat_options.perfect_enabled);
                }
                "cheat_wild_species" => {
                    // `any` (or empty) is the default.
                    if let Some(number) = species::parse(value) {
                        settings.cheat_options.wild_species = Some(number);
                    }
                }
                "cheat_wild_level" => {
                    if let Ok(level) = value.parse::<u8>()
                        && (1..=MAX_LEVEL).contains(&level)
                    {
                        settings.cheat_options.wild_level = Some(level);
                    }
                }
                key if key.starts_with(scan::PREFIX) => scan_pairs.push((key, value)),
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
        settings.cheat_scan = CheatScan::from_pairs(&scan_pairs);
        let options = &mut settings.cheat_options;
        if !wild_switch_seen {
            options.wild_enabled = options.wild_species.is_some() || options.wild_level.is_some();
        }
        if !target_seen {
            options.perfect_target = match (legacy_wild, legacy_opponents) {
                (true, true) => PerfectTarget::Both,
                (false, true) => PerfectTarget::Opponents,
                _ => PerfectTarget::Wild,
            };
        }
        if !perfect_switch_seen {
            options.perfect_enabled = legacy_wild || legacy_opponents || target_seen;
        }
        if let (Some(width), Some(height)) = (width, height)
            && (MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH).contains(&width)
            && height > 0
        {
            settings.window_size = Some((width, height));
        }
        settings
    }

    /// The cheat menu's options as settings lines. Only options that differ
    /// from the default are written, so a file without cheats stays short.
    fn cheat_options_text(&self) -> String {
        let options = &self.cheat_options;
        let mut text = String::new();
        let mut flag = |key: &str, on: bool| {
            if on {
                text.push_str(&format!("{key} = true\n"));
            }
        };
        flag("cheat_shiny", options.shiny);

        flag("cheat_infinite_money", options.infinite_money);
        flag("cheat_catch_always", options.catch_always);
        flag("cheat_no_encounters", options.no_encounters);
        flag("cheat_exp_boost", options.exp_boost);
        if options.shiny_odds != ShinyOdds::default() {
            text.push_str(&format!(
                "cheat_shiny_odds = {}\n",
                options.shiny_odds.one_in()
            ));
        }
        // A row's switch is written whenever the row holds choices, even when
        // it is off: a missing switch means "a file from before the switches",
        // which reads saved choices as on.
        if options.wild_enabled || options.wild_species.is_some() || options.wild_level.is_some() {
            text.push_str(&format!("cheat_wild_enabled = {}\n", options.wild_enabled));
        }
        if options.perfect_enabled || options.perfect_target != PerfectTarget::default() {
            text.push_str(&format!(
                "cheat_perfect_enabled = {}\n",
                options.perfect_enabled
            ));
            text.push_str(&format!(
                "cheat_perfect_target = {}\n",
                options.perfect_target.key()
            ));
        }
        if let Some(number) = options.wild_species {
            text.push_str(&format!("cheat_wild_species = {number}\n"));
        }
        if let Some(level) = options.wild_level {
            text.push_str(&format!("cheat_wild_level = {level}\n"));
        }
        text
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
        text.push_str("# Random extra frames (at most) run after loading a save state, so the\n");
        text.push_str("# game's random numbers differ each time; 0 replays states exactly.\n");
        text.push_str(&format!(
            "state_jitter_frames = {}\n",
            self.state_jitter_frames
        ));
        text.push_str(&self.cheat_options_text());
        if let Some(scan) = &self.cheat_scan {
            text.push_str(&scan.to_text());
        }
        text
    }
}

fn set_flag(value: &str, target: &mut bool) {
    if let Ok(flag) = value.parse() {
        *target = flag;
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
            state_jitter_frames: 12,
            cheat_options: CheatOptions {
                shiny: true,
                shiny_odds: ShinyOdds::from_one_in(100).unwrap(),
                wild_species: Some(147),
                wild_level: Some(50),
                perfect_target: PerfectTarget::Both,
                perfect_enabled: true,
                infinite_money: true,
                no_encounters: true,
                ..CheatOptions::default()
            },
            // (How a scan is written and read is tested with `CheatScan`.)
            cheat_scan: Some(CheatScan {
                create_wild_mon: Some(0x0808_29FC),
                tm_uses: vec![0x0812_4F24],
                ..CheatScan::none(scan::RomFingerprint::of(&[1, 2, 3]))
            }),
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
    fn state_jitter_defaults_on_and_is_bounded() {
        assert_eq!(
            Settings::default().state_jitter_frames,
            DEFAULT_STATE_JITTER_FRAMES
        );
        assert_eq!(
            Settings::parse("state_jitter_frames = 0").state_jitter_frames,
            0
        );
        assert_eq!(
            Settings::parse("state_jitter_frames = 99999").state_jitter_frames,
            MAX_STATE_JITTER_FRAMES
        );
        assert_eq!(
            Settings::parse("state_jitter_frames = lots").state_jitter_frames,
            DEFAULT_STATE_JITTER_FRAMES
        );
    }

    #[test]
    fn cheat_menu_options_are_persisted_and_default_to_off() {
        assert!(Settings::default().cheat_options.is_default());
        let text = Settings {
            cheat_options: CheatOptions {
                wild_species: Some(25),
                perfect_target: PerfectTarget::Opponents,
                perfect_enabled: true,
                catch_always: true,
                exp_boost: true,
                ..CheatOptions::default()
            },
            ..Settings::default()
        }
        .to_text();
        assert!(text.contains("cheat_wild_species = 25"));
        assert!(text.contains("cheat_catch_always = true"));
        assert!(text.contains("cheat_exp_boost = true"));
        assert!(!text.contains("cheat_shiny"), "defaults are not written");
        assert!(!text.contains("cheat_wild_level"));
        let back = Settings::parse(&text).cheat_options;
        assert_eq!(back.wild_species, Some(25));
        assert_eq!(back.wild_level, None);
        assert_eq!(back.perfect_target, PerfectTarget::Opponents);
        assert!(back.perfect_enabled && back.catch_always && back.exp_boost);
        assert!(!back.shiny && !back.infinite_money);
    }

    #[test]
    fn the_row_switches_are_saved_and_older_files_keep_their_choices_active() {
        // A row can be off while its choices are remembered.
        let off = Settings {
            cheat_options: CheatOptions {
                wild_species: Some(147),
                wild_enabled: false,
                perfect_target: PerfectTarget::Both,
                perfect_enabled: false,
                ..CheatOptions::default()
            },
            ..Settings::default()
        };
        let back = Settings::parse(&off.to_text()).cheat_options;
        assert_eq!(back.wild_species, Some(147));
        assert!(!back.wild_enabled);
        assert_eq!(back.perfect_target, PerfectTarget::Both);
        assert!(!back.perfect_enabled);
        // ...and on.
        let on = Settings {
            cheat_options: CheatOptions {
                wild_species: Some(147),
                wild_enabled: true,
                perfect_target: PerfectTarget::Opponents,
                perfect_enabled: true,
                ..CheatOptions::default()
            },
            ..Settings::default()
        };
        let back = Settings::parse(&on.to_text()).cheat_options;
        assert!(back.wild_enabled && back.perfect_enabled);
        // A file written before the switches existed: saved choices were
        // active, so they stay active.
        let legacy = Settings::parse("cheat_wild_species = 25\ncheat_perfect_wild = true\n");
        assert!(legacy.cheat_options.wild_enabled);
        assert!(legacy.cheat_options.perfect_enabled);
        // With nothing saved, nothing is on.
        let none = Settings::parse("volume = 0.5\n");
        assert!(!none.cheat_options.wild_enabled && !none.cheat_options.perfect_enabled);
    }

    #[test]
    fn the_cheats_file_switch_is_never_saved() {
        let settings = Settings {
            cheat_options: CheatOptions {
                file_codes: true,
                ..CheatOptions::default()
            },
            ..Settings::default()
        };
        assert!(!settings.to_text().contains("file"));
        assert!(
            !Settings::parse(&settings.to_text())
                .cheat_options
                .file_codes
        );
        assert!(
            !Settings::parse("cheat_file_codes = true")
                .cheat_options
                .file_codes
        );
    }

    #[test]
    fn a_settings_file_from_before_my_pokemon_was_removed_still_loads() {
        let old = "volume = 0.5\ncheat_perfect_mine = true\ncheat_perfect_wild = true\n";
        let settings = Settings::parse(old);
        assert_eq!(settings.volume, 0.5);
        assert_eq!(settings.cheat_options.perfect_target, PerfectTarget::Wild);
        assert!(settings.cheat_options.perfect_enabled);
        assert!(!settings.to_text().contains("perfect_mine"));
    }

    #[test]
    fn the_two_old_target_flags_read_into_the_single_choice() {
        let target = |text: &str| Settings::parse(text).cheat_options;
        let both = target("cheat_perfect_wild = true\ncheat_perfect_opponent = true\n");
        assert_eq!(both.perfect_target, PerfectTarget::Both);
        assert!(both.perfect_enabled, "saved targets were active");
        let opponents = target("cheat_perfect_opponent = true\n");
        assert_eq!(opponents.perfect_target, PerfectTarget::Opponents);
        let wild = target("cheat_perfect_wild = true\n");
        assert_eq!(wild.perfect_target, PerfectTarget::Wild);
        // The new key wins over old flags left in the same file.
        let new = target("cheat_perfect_wild = true\ncheat_perfect_target = opponents\n");
        assert_eq!(new.perfect_target, PerfectTarget::Opponents);
        // The choice is written as a word and read back.
        let written = Settings {
            cheat_options: CheatOptions {
                perfect_target: PerfectTarget::Both,
                perfect_enabled: true,
                ..CheatOptions::default()
            },
            ..Settings::default()
        }
        .to_text();
        assert!(written.contains("cheat_perfect_target = both"));
        assert!(!written.contains("cheat_perfect_wild"));
        // A bad word is ignored.
        let bad = target("cheat_perfect_target = everyone\n");
        assert_eq!(bad.perfect_target, PerfectTarget::Wild);
        assert!(!bad.perfect_enabled);
    }

    #[test]
    fn bad_cheat_menu_values_fall_back_to_off() {
        let s = Settings::parse(
            "cheat_wild_species = Missingno\ncheat_wild_level = 500\ncheat_shiny = maybe\ncheat_wild_level = 0\n",
        );
        assert!(s.cheat_options.is_default());
        // A name is as good as a number for the species.
        assert_eq!(
            Settings::parse("cheat_wild_species = Dratini")
                .cheat_options
                .wild_species,
            Some(147)
        );
        assert_eq!(
            Settings::parse("cheat_wild_level = 100")
                .cheat_options
                .wild_level,
            Some(100)
        );
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
