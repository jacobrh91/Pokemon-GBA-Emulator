//! The result of the first pass through a ROM: where each built-in cheat
//! hooks (see [`super::gen3`]), remembered in the per-game settings file
//! so later launches skip the scan.
//!
//! The result is only trusted for the exact ROM it was found in, told apart
//! by a fingerprint (length and a 64-bit FNV-1a hash). "Not found" is
//! remembered too. [`CheatScan::to_text`] and [`CheatScan::from_pairs`] write
//! and read it as `cheat_scan_*` lines of the settings file.

use super::gen3::field_moves::{self, FieldMoves};
use super::gen3::party_menu::{self, Layout, PartyMenu};
use super::gen3::{
    catch_rate, encounters, experience, field_poison, money, shiny, tm, wild_species,
};

/// The version of the rules a scan follows. Results cached in a settings file
/// are only trusted when they were made under the same version: bump this
/// whenever what the scans accept changes (a new game recognised, a signature
/// changed), so that "not found" answers from the old rules are not believed.
pub const SCAN_VERSION: u32 = 6;

/// Identifies a ROM image: `<length>-<hash>`, both in hex.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomFingerprint(String);

impl RomFingerprint {
    pub fn of(rom: &[u8]) -> Self {
        let mut hash = 0xCBF2_9CE4_8422_2325_u64;
        for &byte in rom {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3);
        }
        Self(format!("{:x}-{hash:016x}", rom.len()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A fingerprint read back from a settings file; anything not shaped like
    /// one is rejected (and so never matches a ROM).
    pub fn parse(text: &str) -> Option<Self> {
        let (length, hash) = text.split_once('-')?;
        let hex = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_hexdigit());
        (hex(length) && hex(hash)).then(|| Self(text.to_ascii_lowercase()))
    }
}

/// Where the built-in cheats hook in one ROM; `None` is "looked, not found".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheatScan {
    pub rom: RomFingerprint,
    /// Entry of the game's `CreateWildMon` (see [`wild_species`]).
    pub create_wild_mon: Option<u32>,
    /// The catch check's compare instruction (see [`catch_rate`]).
    pub catch_check: Option<u32>,
    /// Where `CreateBoxMon` has the personality and OT ID (see [`shiny`]).
    pub shiny_point: Option<u32>,
    /// The return of `GetMoney` (see [`money`]).
    pub money_balance: Option<u32>,
    /// Where `IsTradedMon` has fetched the OT ID (see [`experience`]).
    pub exp_traded: Option<u32>,
    /// The end of the wild-encounter dice roll (see [`encounters`]).
    pub encounter_roll: Option<u32>,
    /// The calls that take a TM out of the bag (see [`tm`]); empty when none
    /// was found.
    pub tm_uses: Vec<u32>,
    /// Where the first party Pokémon is made to know the HMs (see
    /// [`field_moves`]).
    pub field_moves: Option<FieldMoves>,
    /// Where the party menu's action list is built (see [`party_menu`]).
    pub party_menu: Option<PartyMenu>,
    /// The poison check of the overworld (see [`field_poison`]).
    pub field_poison: Option<u32>,
}

impl CheatScan {
    /// A scan that found nothing in the ROM with this fingerprint.
    pub fn none(rom: RomFingerprint) -> Self {
        Self {
            rom,
            create_wild_mon: None,
            catch_check: None,
            shiny_point: None,
            money_balance: None,
            exp_traded: None,
            encounter_roll: None,
            tm_uses: Vec::new(),
            field_moves: None,
            party_menu: None,
            field_poison: None,
        }
    }

    /// Scans `rom` (the slow path: one pass per signature).
    pub fn scan(rom: &[u8]) -> Self {
        Self {
            create_wild_mon: wild_species::find(rom),
            catch_check: catch_rate::find(rom),
            shiny_point: shiny::find(rom),
            money_balance: money::find(rom),
            exp_traded: experience::find(rom),
            encounter_roll: encounters::find(rom),
            tm_uses: tm::find(rom),
            field_moves: field_moves::find(rom),
            party_menu: party_menu::find(rom),
            field_poison: field_poison::find(rom),
            ..Self::none(RomFingerprint::of(rom))
        }
    }

    /// `previous` if it was recorded for exactly this ROM, else a fresh scan.
    pub fn for_rom(rom: &[u8], previous: Option<&CheatScan>) -> Self {
        let fingerprint = RomFingerprint::of(rom);
        match previous {
            Some(scan) if scan.rom == fingerprint => scan.clone(),
            _ => Self::scan(rom),
        }
    }

    /// The settings-file lines that record this scan.
    pub fn to_text(&self) -> String {
        let address = |address: Option<u32>| address.map_or("none".to_owned(), hex);
        let list = |addresses: &[u32]| {
            if addresses.is_empty() {
                "none".to_owned()
            } else {
                addresses
                    .iter()
                    .copied()
                    .map(hex)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        let mut text = String::from(
            "# Where the built-in cheats hook in this ROM (found by the first scan;\n\
             # ignored, and scanned again, if the ROM changes).\n",
        );
        let mut line = |key: &str, value: String| {
            text.push_str(&format!("{PREFIX}{key} = {value}\n"));
        };
        line("version", SCAN_VERSION.to_string());
        line("rom", self.rom.as_str().to_owned());
        line("create_wild_mon", address(self.create_wild_mon));
        line("catch_check", address(self.catch_check));
        line("shiny", address(self.shiny_point));
        line("money_balance", address(self.money_balance));
        line("exp_traded", address(self.exp_traded));
        line("encounter_roll", address(self.encounter_roll));
        line("field_poison", address(self.field_poison));
        line("tm_uses", list(&self.tm_uses));
        line(
            "field_moves",
            self.field_moves.map_or("none".to_owned(), |moves| {
                list(&[moves.knows_move_return, moves.party_base])
            }),
        );
        line(
            "party_menu",
            self.party_menu.map_or("none".to_owned(), |menu| {
                format!(
                    "{}, {}, {}",
                    hex(menu.capture),
                    hex(menu.append),
                    menu.layout.key()
                )
            }),
        );
        text
    }

    /// Rebuilds a scan from the `key = value` pairs of a settings file (the
    /// last of a repeated key wins). `None` unless it was written under the
    /// current [`SCAN_VERSION`] and every part is present and well formed:
    /// then the ROM is scanned afresh.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Option<Self> {
        let get = |key: &str| {
            pairs
                .iter()
                .rev()
                .find(|(name, _)| name.strip_prefix(PREFIX) == Some(key))
                .map(|(_, value)| *value)
        };
        if get("version")?.parse::<u32>().ok()? != SCAN_VERSION {
            return None;
        }
        let address = |key: &str| parse_address(get(key)?);
        let list = |key: &str| parse_list(get(key)?);
        Some(Self {
            rom: RomFingerprint::parse(get("rom")?)?,
            create_wild_mon: address("create_wild_mon")?,
            catch_check: address("catch_check")?,
            shiny_point: address("shiny")?,
            money_balance: address("money_balance")?,
            exp_traded: address("exp_traded")?,
            encounter_roll: address("encounter_roll")?,
            field_poison: address("field_poison")?,
            tm_uses: list("tm_uses")?,
            field_moves: match list("field_moves")?.as_slice() {
                [] => None,
                &[knows_move_return, party_base] => Some(FieldMoves {
                    knows_move_return,
                    party_base,
                }),
                _ => return None,
            },
            party_menu: parse_party_menu(get("party_menu")?)?,
        })
    }
}

/// The start of every key of a scan in the settings file.
pub const PREFIX: &str = "cheat_scan_";

fn hex(address: u32) -> String {
    format!("{address:#010X}")
}

/// `Some(None)` for `none`, `Some(Some(address))` for a hex address, `None`
/// for anything else.
fn parse_address(value: &str) -> Option<Option<u32>> {
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))?;
    u32::from_str_radix(digits, 16).ok().map(Some)
}

/// A comma-separated list of hex addresses, or `none` for the empty list;
/// `None` for anything else.
fn parse_list(value: &str) -> Option<Vec<u32>> {
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    value
        .split(',')
        .map(|part| parse_address(part.trim()).flatten())
        .collect()
}

/// `Some(None)` for `none`, `Some(Some(..))` for the two addresses and the
/// layout (`frlg` or `emerald`), `None` for anything else.
fn parse_party_menu(value: &str) -> Option<Option<PartyMenu>> {
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let mut parts = value.split(',').map(str::trim);
    let capture = parse_address(parts.next()?)??;
    let append = parse_address(parts.next()?)??;
    let layout = Layout::from_key(parts.next()?)?;
    parts.next().is_none().then_some(Some(PartyMenu {
        capture,
        append,
        layout,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_tell_roms_apart_and_round_trip() {
        let a = RomFingerprint::of(&[1, 2, 3]);
        assert_eq!(a, RomFingerprint::of(&[1, 2, 3]));
        assert_ne!(a, RomFingerprint::of(&[1, 2, 4]));
        assert_ne!(a, RomFingerprint::of(&[1, 2, 3, 0]));
        assert_eq!(RomFingerprint::parse(a.as_str()), Some(a));
        assert_eq!(RomFingerprint::parse("nonsense"), None);
        assert_eq!(RomFingerprint::parse("-"), None);
    }

    #[test]
    fn a_recorded_scan_is_used_only_for_the_same_rom() {
        let rom = vec![0u8; 0x1000];
        let recorded = CheatScan {
            create_wild_mon: Some(0x0800_1234),
            ..CheatScan::none(RomFingerprint::of(&rom))
        };
        assert_eq!(CheatScan::for_rom(&rom, Some(&recorded)), recorded);
        let mut other = rom.clone();
        other[0x10] = 1;
        let fresh = CheatScan::for_rom(&other, Some(&recorded));
        assert_eq!(fresh, CheatScan::scan(&other));
        assert_eq!(fresh.create_wild_mon, None);
    }

    /// A scan with every part found, to write and read back.
    fn full() -> CheatScan {
        CheatScan {
            create_wild_mon: Some(0x0808_29FC),
            catch_check: None,
            shiny_point: Some(0x0803_DB92),
            money_balance: Some(0x0809_FD7C),
            exp_traded: Some(0x0804_4288),
            encounter_roll: Some(0x0808_2BDA),
            tm_uses: vec![0x0812_4F24, 0x0812_4FF0],
            field_moves: Some(FieldMoves {
                knows_move_return: 0x0812_5B60,
                party_base: 0x0202_4284,
            }),
            party_menu: Some(PartyMenu {
                capture: 0x0812_2A9A,
                append: 0x0812_2B12,
                layout: Layout::Emerald,
            }),
            field_poison: Some(0x080A_0654),
            ..CheatScan::none(RomFingerprint::of(&[9]))
        }
    }

    /// The lines of `text` as the pairs a settings file would hand over.
    fn pairs(text: &str) -> Vec<(&str, &str)> {
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim(), value.trim()))
            .collect()
    }

    #[test]
    fn a_scan_round_trips_through_its_lines_whether_or_not_it_found_everything() {
        for scan in [full(), CheatScan::none(RomFingerprint::of(&[1, 2]))] {
            let text = scan.to_text();
            assert_eq!(CheatScan::from_pairs(&pairs(&text)), Some(scan));
        }
        assert!(
            full()
                .to_text()
                .contains("cheat_scan_tm_uses = 0x08124F24, 0x08124FF0")
        );
    }

    #[test]
    fn an_incomplete_or_malformed_scan_is_dropped() {
        let text = full().to_text();
        let all = pairs(&text);
        assert!(all.len() >= 12);
        for missing in 0..all.len() {
            let partial: Vec<_> = all
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != missing)
                .map(|(_, pair)| *pair)
                .collect();
            assert_eq!(
                CheatScan::from_pairs(&partial),
                None,
                "without {:?}",
                all[missing]
            );
        }
        let bad = text.replace("0x0803DB92", "banana");
        assert_eq!(CheatScan::from_pairs(&pairs(&bad)), None);
        let odd_list = text.replace("0x08125B60, 0x02024284", "0x08125B60");
        assert_eq!(CheatScan::from_pairs(&pairs(&odd_list)), None);
        let bad_layout = text.replace("emerald", "ruby");
        assert_eq!(CheatScan::from_pairs(&pairs(&bad_layout)), None);
    }

    #[test]
    fn a_scan_written_under_other_rules_is_not_believed() {
        let text = full().to_text();
        let current = format!("{PREFIX}version = {SCAN_VERSION}");
        assert!(text.contains(&current));
        let older = text.replace(&current, &format!("{PREFIX}version = {}", SCAN_VERSION - 1));
        assert_eq!(CheatScan::from_pairs(&pairs(&older)), None);
        let unversioned: Vec<_> = pairs(&text)
            .into_iter()
            .filter(|(key, _)| *key != format!("{PREFIX}version"))
            .collect();
        assert_eq!(CheatScan::from_pairs(&unversioned), None);
    }

    #[test]
    fn the_last_of_a_repeated_key_wins() {
        let text = full().to_text();
        let mut all = pairs(&text);
        all.insert(2, ("cheat_scan_shiny", "banana"));
        assert_eq!(CheatScan::from_pairs(&all), Some(full()));
    }
}
