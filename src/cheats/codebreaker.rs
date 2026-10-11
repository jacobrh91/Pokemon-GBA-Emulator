//! Cheat codes in the CodeBreaker format: one `XXXXXXXX YYYY` pair (8 + 4 hex
//! digits) per line, kept in a text file beside the ROM.
//!
//! A cheat is applied by writing a constant into RAM once per frame, so it
//! keeps winning against the game rewriting that location. The code types
//! understood are:
//!
//! | Code | Meaning |
//! |---|---|
//! | `3aaaaaaa 00vv` | write byte `vv` to `aaaaaaa` |
//! | `8aaaaaaa vvvv` | write halfword `vvvv` to `aaaaaaa` |
//! | `0000xxxx 000y` | master code 1: game identity (ignored) |
//! | `1aaaaaaa yyyy` | master code 2: hook at ROM offset `aaaaaaa` |
//!
//! On real hardware a CodeBreaker cartridge has to get its code handler to run
//! regularly, so its *master codes* name the game and a ROM address, executed
//! every frame, where the cartridge hooks itself in. The cheat writes then
//! happen *from that point in the program*.
//!
//! That matters: many codes poke addresses in the middle of the stack, which
//! only holds something harmless at that one point. So when a code of the form
//! `1aaaaaaa yyyy` names a hook, the writes are performed each time execution
//! reaches that ROM address, exactly like the cartridge. Without one (plain
//! RAM-poke codes), they are performed once per finished frame instead.
//!
//! Only RAM (EWRAM `0x02......` and IWRAM `0x03......`) can be written. Any
//! other line is reported as a warning and skipped rather than guessed at.
//!
//! The file may also hold `key = value` directives, which are not memory
//! writes but built-in, game-specific behaviours:
//!
//! | Directive | Meaning |
//! |---|---|
//! | `wild_species = Dratini` | every wild encounter is that species (a name, a dex number or `0x..`), FireRed only; the function is found by a signature scan |

use super::species;
use crate::mmu::MemoryBus;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const EWRAM_PREFIX: u32 = 0x02;
const IWRAM_PREFIX: u32 = 0x03;
/// Extension of the cheats file, replacing the ROM's own.
const EXTENSION: &str = "cheats";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Write {
    Byte { address: u32, value: u8 },
    Halfword { address: u32, value: u16 },
}

/// A parsed set of cheats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cheats {
    writes: Vec<Write>,
    master_lines: usize,
    /// ROM address named by a master code 2; the writes are performed when
    /// execution reaches it.
    hook: Option<u32>,
    /// Species forced on wild encounters (`wild_species = ...`).
    wild_species: Option<u16>,
    warnings: Vec<String>,
}

impl Cheats {
    /// Where the cheats for `rom` live: beside it, with a `.cheats` extension.
    pub fn path_for_rom(rom: &Path) -> PathBuf {
        rom.with_extension(EXTENSION)
    }

    /// Reads a cheats file; a missing file is simply no cheats.
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// Parses cheat lines. `#` starts a comment; blank lines are ignored.
    pub fn parse(text: &str) -> Self {
        let mut cheats = Self::default();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let number = index + 1;
            if let Some((key, value)) = line.split_once('=') {
                cheats.directive(number, key.trim(), value.trim());
                continue;
            }
            match parse_line(line) {
                Ok(Line::Write(write)) => cheats.writes.push(write),
                Ok(Line::Master { hook }) => {
                    cheats.master_lines += 1;
                    // The first hook wins, as only one can be installed.
                    cheats.hook = cheats.hook.or(hook);
                }
                Err(reason) => cheats
                    .warnings
                    .push(format!("line {number}: {reason}: {line}")),
            }
        }
        cheats
    }

    fn directive(&mut self, line: usize, key: &str, value: &str) {
        match key {
            "wild_species" => match species::parse(value) {
                Some(species) => self.wild_species = Some(species),
                None => self.warnings.push(format!(
                    "line {line}: wild_species needs a Pokemon name or a number from 1 to {}: {value}",
                    species::LAST_SPECIES
                )),
            },
            other => self
                .warnings
                .push(format!("line {line}: unknown setting {other:?}")),
        }
    }

    /// The species forced on wild encounters, if one was asked for.
    pub fn wild_species(&self) -> Option<u16> {
        self.wild_species
    }

    /// Number of RAM writes performed each frame.
    pub fn write_count(&self) -> usize {
        self.writes.len()
    }

    /// The ROM address at which the writes are performed, if a master code
    /// named one; otherwise they are performed once per frame.
    pub fn hook_address(&self) -> Option<u32> {
        self.hook
    }

    /// Number of master-code lines.
    pub fn master_line_count(&self) -> usize {
        self.master_lines
    }

    /// Lines that could not be used, with the reason.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// True when there is nothing to apply: no RAM writes and no overrides.
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.wild_species.is_none()
    }

    /// Performs every write. Call once per frame.
    pub fn apply(&self, bus: &mut MemoryBus) {
        for write in &self.writes {
            // RAM addresses cannot fault; ignore the result like hardware.
            let _ = match *write {
                Write::Byte { address, value } => bus.write_u8(address, value),
                Write::Halfword { address, value } => bus.write_u16(address, value),
            };
        }
    }
}

enum Line {
    Write(Write),
    /// A master code; `hook` is the ROM address for a type-1 code.
    Master {
        hook: Option<u32>,
    },
}

fn parse_line(line: &str) -> Result<Line, String> {
    // `XXXXXXXX YYYY`, or the same twelve digits run together.
    let digits: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != 12 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("expected 8 + 4 hex digits".to_owned());
    }
    let code = u32::from_str_radix(&digits[..8], 16).map_err(|e| e.to_string())?;
    let value = u16::from_str_radix(&digits[8..], 16).map_err(|e| e.to_string())?;
    let address = code & 0x0FFF_FFFF;
    match code >> 28 {
        0x0 => Ok(Line::Master { hook: None }),
        0x1 => Ok(Line::Master {
            // The offset into the Game Pak, as the ROM appears on the bus.
            hook: Some(0x0800_0000 | (address & 0x01FF_FFFF)),
        }),
        0x3 => {
            if value > 0xFF {
                return Err("8-bit write needs a value of 00 to FF".to_owned());
            }
            check_ram(address)?;
            Ok(Line::Write(Write::Byte {
                address,
                value: value as u8,
            }))
        }
        0x8 => {
            if address & 1 != 0 {
                return Err("16-bit write needs an even address".to_owned());
            }
            check_ram(address)?;
            Ok(Line::Write(Write::Halfword { address, value }))
        }
        other => Err(format!("unsupported code type {other:X}")),
    }
}

fn check_ram(address: u32) -> Result<(), String> {
    match address >> 24 {
        EWRAM_PREFIX | IWRAM_PREFIX => Ok(()),
        _ => Err(format!(
            "address {address:#010X} is outside EWRAM and IWRAM"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mmu::IWRAM_START;

    const WILD_ENCOUNTER: &str = "\
# Wild Pokemon modifier
0000295F 000A
101DC9D4 0007
83007C2C 0000
83007C2E 0000
83007D4E 0093   # Dratini
";

    #[test]
    fn the_wild_encounter_codes_parse_into_master_lines_and_ram_writes() {
        let cheats = Cheats::parse(WILD_ENCOUNTER);
        assert_eq!(cheats.master_line_count(), 2);
        assert_eq!(cheats.hook_address(), Some(0x081D_C9D4));
        assert_eq!(
            cheats.writes,
            [
                Write::Halfword {
                    address: 0x0300_7C2C,
                    value: 0
                },
                Write::Halfword {
                    address: 0x0300_7C2E,
                    value: 0
                },
                Write::Halfword {
                    address: 0x0300_7D4E,
                    value: 0x0093
                },
            ]
        );
        assert!(cheats.warnings().is_empty());
    }

    #[test]
    fn the_wild_species_directive_takes_names_and_numbers() {
        let by_name = Cheats::parse("wild_species = Dratini");
        assert_eq!(by_name.wild_species(), Some(147));
        assert_eq!(by_name.write_count(), 0);
        assert!(!by_name.is_empty(), "an override is something to apply");
        assert_eq!(Cheats::parse("wild_species=147").wild_species(), Some(147));
        assert_eq!(
            Cheats::parse("  wild_species = 0x93   # dratini").wild_species(),
            Some(147)
        );
        assert_eq!(
            Cheats::parse("wild_species = Mr. Mime").wild_species(),
            Some(122)
        );
        assert_eq!(Cheats::parse("83007D4E 0093").wild_species(), None);
    }

    #[test]
    fn bad_directives_are_warnings() {
        let cheats = Cheats::parse("wild_species = Missingno\nwild_level = 5\nwild_species =");
        assert_eq!(cheats.wild_species(), None);
        assert_eq!(cheats.warnings().len(), 3);
        let joined = cheats.warnings().join("\n");
        assert!(
            joined.contains("name or a number from 1 to 251"),
            "{joined}"
        );
        assert!(joined.contains("unknown setting"), "{joined}");
    }

    #[test]
    fn codes_without_a_master_code_have_no_hook() {
        let cheats = Cheats::parse("83007D4E 0093\n0000295F 000A");
        assert_eq!(cheats.hook_address(), None);
        assert_eq!(cheats.master_line_count(), 1);
    }

    #[test]
    fn the_first_hook_is_the_one_used() {
        let cheats = Cheats::parse("10000100 0007\n10000200 0007");
        assert_eq!(cheats.hook_address(), Some(0x0800_0100));
    }

    #[test]
    fn byte_writes_spacing_case_and_comments_are_accepted() {
        let cheats = Cheats::parse("3200 0000 00AB\n  3300aabb 00ff # shout\n\n");
        // `3200 0000 00AB` is twelve digits run together with stray spaces.
        assert_eq!(
            cheats.writes,
            [
                Write::Byte {
                    address: 0x0200_0000,
                    value: 0xAB
                },
                Write::Byte {
                    address: 0x0300_AABB,
                    value: 0xFF
                },
            ]
        );
    }

    #[test]
    fn unusable_lines_become_warnings_not_guesses() {
        let cheats = Cheats::parse(
            "\
7300AABB 1234
8300AABA 0001
83007D4F 0001
88000000 0001
3300AABB 0100
nonsense
",
        );
        assert_eq!(cheats.write_count(), 1, "only the valid 16-bit write");
        let joined = cheats.warnings().join("\n");
        assert!(joined.contains("unsupported code type 7"), "{joined}");
        assert!(joined.contains("even address"), "{joined}");
        assert!(joined.contains("outside EWRAM and IWRAM"), "{joined}");
        assert!(joined.contains("00 to FF"), "{joined}");
        assert!(joined.contains("8 + 4 hex digits"), "{joined}");
        assert_eq!(cheats.warnings().len(), 5);
    }

    #[test]
    fn cheats_are_found_beside_the_rom_and_missing_means_none() {
        assert_eq!(
            Cheats::path_for_rom(Path::new("/games/Game.gba")),
            Path::new("/games/Game.cheats")
        );
        let missing = std::env::temp_dir().join("gba-cheats-test-missing.cheats");
        assert!(Cheats::load(&missing).unwrap().is_empty());
    }

    #[test]
    fn applying_writes_the_values_into_ram() {
        let path = std::env::temp_dir().join(format!("gba-cheats-test-{}.gba", std::process::id()));
        std::fs::write(&path, [0; 8]).unwrap();
        let mut bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();

        bus.write_u16(IWRAM_START + 0x7C2C, 0xBEEF).unwrap();
        Cheats::parse(WILD_ENCOUNTER).apply(&mut bus);
        assert_eq!(bus.read_u16(0x0300_7C2C).unwrap(), 0);
        assert_eq!(bus.read_u16(0x0300_7C2E).unwrap(), 0);
        assert_eq!(bus.read_u16(0x0300_7D4E).unwrap(), 0x0093);
    }
}
