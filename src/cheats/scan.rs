//! The result of the first pass through a ROM: where each built-in cheat
//! hooks (see [`super::firered`]), remembered in the per-game settings file
//! so later launches skip the scan.
//!
//! The result is only trusted for the exact ROM it was found in, which a
//! fingerprint (length and a 64-bit FNV-1a hash of every byte) tells apart.
//! "Not found" is remembered too, so a ROM that is not FireRed is not scanned
//! again either.

use super::firered::{catch_rate, money, shiny, wild_species};

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
}

impl CheatScan {
    /// Scans `rom` (the slow path: one pass per signature).
    pub fn scan(rom: &[u8]) -> Self {
        Self {
            rom: RomFingerprint::of(rom),
            create_wild_mon: wild_species::find(rom),
            catch_check: catch_rate::find(rom),
            shiny_point: shiny::find(rom),
            money_balance: money::find(rom),
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
            rom: RomFingerprint::of(&rom),
            create_wild_mon: Some(0x0800_1234),
            catch_check: None,
            shiny_point: None,
            money_balance: None,
        };
        assert_eq!(CheatScan::for_rom(&rom, Some(&recorded)), recorded);
        let mut other = rom.clone();
        other[0x10] = 1;
        let fresh = CheatScan::for_rom(&other, Some(&recorded));
        assert_eq!(fresh, CheatScan::scan(&other));
        assert_eq!(fresh.create_wild_mon, None);
    }
}
