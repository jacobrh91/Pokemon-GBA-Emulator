//! Built-in cheats for Pokémon FireRed. Unlike the raw codes of a `.cheats`
//! file (see [`super::codebreaker`]), which write constants into RAM, these
//! act on the game's own code: each finds a place in the ROM by its
//! instruction bytes (a *signature scan*) and the emulator changes a register
//! when the CPU reaches that address.
//!
//! The mechanisms are kept apart:
//!
//! * [`wild_species`]: replaces the arguments of a function *at its entry*
//!   (the species and level of every wild encounter).
//! * [`catch_rate`]: replaces a value *in the middle of a routine* (the ball's
//!   catch odds, just before the game tests them).
//! * [`shiny`]: replaces a value *in the middle of a routine* of the code that
//!   builds every Pokémon (its personality value, so that it is shiny).
//! * [`experience`]: replaces a value *in the middle of a routine* (the OT ID
//!   the game compares with the player's), so that every Pokémon counts as
//!   traded and earns the traded-Pokémon experience bonus.
//! * [`money`]: replaces the value returned by the function that reads the
//!   balance, so that the game sees 999,999.
//! * [`perfect_stats`]: the same place, to set the IVs (a local that the
//!   routine tests a little later).
//!
//! This module holds what they share: recognising a FireRed cartridge, the
//! scan itself, and the table of builds that were checked.
//!
//! # How a scan works
//!
//! 1. The ROM must be a FireRed cartridge: game code `BPR?` in the header at
//!    `0xAC` (`BPRE` English, `BPRJ` Japanese, …). A short pattern is unlikely
//!    to occur by chance elsewhere, but this keeps the hooks away from other
//!    games altogether. (LeafGreen, `BPG?`, shares the engine but has not been
//!    checked, so it is not enabled.)
//! 2. Every 2-byte-aligned offset of the ROM is compared with the pattern
//!    (Thumb instructions are halfwords, so a match elsewhere is meaningless).
//!    Bytes that legitimately differ between builds are wildcards.
//! 3. The result must be **exactly one** match. Zero means this build is not
//!    recognised; more than one means the signature is ambiguous. In both
//!    cases nothing is hooked and the cheat reports itself unsupported, rather
//!    than guessing.
//! 4. The hook address is the match offset (plus the instruction the cheat
//!    cares about) plus `0x0800_0000`, where the ROM appears on the bus.
//!
//! A signature contains only register moves, shifts and compares: a `BL`
//! call encodes a *relative* distance and a PC-relative `LDR` the distance to
//! a data pool, both of which change when code or data around them moves, so
//! they are never part of a pattern. The scan runs once, when the ROM is
//! opened.
//!
//! # Known builds
//!
//! | Build | Game code | Version | CRC32 | `CreateWildMon` at | Catch check at | Checked |
//! |---|---|---|---|---|---|---|
//! | FireRed v1.0 (US) | `BPRE` | 0 | `7845FBDD` | `0x080829FC` | `0x0802D6BC` | encounter: Rattata → Dratini; catch: Lv100 Mewtwo caught first throw |
//! | FireRed v1.1 (US) | `BPRE` | 1 | `84EE4776` | `0x08082A10` | `0x0802D6D0` | encounter: Pidgey → Dratini; catch: Lv100 Mewtwo caught first throw |
//! | FireRed (Japan)   | `BPRJ` | 0 | `3B2056E9` | `0x080825E8` | `0x0802CE94` | encounter: Rattata → Dratini; catch: Lv100 Mewtwo caught first throw |
//!
//! The shiny cheat's hook (inside `CreateBoxMon`) is at `0x0803DB92` (v1.0),
//! `0x0803DBA6` (v1.1) and `0x0803D2FE` (Japan); each was checked live from a
//! save state in the grass.
//!
//! The enemy party is not at the same RAM address in every build (it is
//! `0x0202402C` in the English builds and `0x02023F8C` in the Japanese one),
//! which matters for anything that reads the species back out of RAM, but not
//! for these cheats: they replace a register and never touch RAM.
//!
//! "Checked" says how far each build has been verified. A unique signature
//! match shows the bytes are there, not that the code behaves the same; each
//! new build deserves a live encounter and a live ball throw before it is
//! relied on.
//!
//! # Adding or checking a build
//!
//! Run the emulator on the ROM with the cheat on; the terminal reports the
//! address it hooked, or why it did not. A build whose code was compiled
//! differently simply has no match and is declined. For a build with rewritten
//! code, find the place by tracing the game (see each cheat's notes) and add
//! its own signature.

pub mod catch_rate;
pub mod experience;
pub mod money;
pub mod perfect_stats;
pub mod shiny;
pub mod wild_species;

/// Where the ROM appears on the bus.
const ROM_BASE: u32 = 0x0800_0000;

/// Game-code prefix of the FireRed cartridges (`BPRE`, `BPRJ`, ...).
const GAME_CODE_PREFIX: &[u8; 3] = b"BPR";

/// The bus address of the single match of `pattern` in `rom` plus `offset`,
/// if `rom` is a FireRed cartridge and the pattern occurs exactly once at a
/// halfword-aligned position. `None` entries of the pattern match any byte.
fn find_in_firered(rom: &[u8], pattern: &[Option<u8>], offset: usize) -> Option<u32> {
    // The game code lives at 0xAC..0xB0 in the cartridge header.
    let game_code = rom.get(0xAC..0xB0)?;
    if !game_code.starts_with(GAME_CODE_PREFIX) {
        return None;
    }
    find_unique(rom, pattern).map(|found| ROM_BASE + (found + offset) as u32)
}

/// The offset of `pattern` in `haystack` if it occurs exactly once at a
/// 2-byte-aligned position, and nowhere else at such a position.
fn find_unique(haystack: &[u8], pattern: &[Option<u8>]) -> Option<usize> {
    let first = pattern.first().copied().flatten();
    if pattern.is_empty() || first.is_none() || haystack.len() < pattern.len() {
        return None;
    }
    let first = first.unwrap();
    let mut found = None;
    for offset in (0..=haystack.len() - pattern.len()).step_by(2) {
        // Cheap first-byte test keeps the 16 MiB scan quick.
        if haystack[offset] != first {
            continue;
        }
        let window = &haystack[offset..offset + pattern.len()];
        if pattern
            .iter()
            .zip(window)
            .all(|(want, byte)| want.is_none_or(|want| want == *byte))
        {
            if found.is_some() {
                return None;
            }
            found = Some(offset);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_or_empty_input_does_not_panic() {
        assert_eq!(find_in_firered(&[], &[Some(1)], 0), None);
        assert_eq!(find_in_firered(&[0; 8], &[Some(1)], 0), None);
        assert_eq!(find_unique(&[1, 2], &[Some(1), Some(2), Some(3)]), None);
        assert_eq!(find_unique(&[0; 32], &[]), None);
        assert_eq!(find_unique(&[0; 32], &[None, Some(0)]), None);
    }

    #[test]
    fn wildcards_match_any_byte_and_the_offset_is_added() {
        let mut rom = vec![0u8; 0x1000];
        rom[0xAC..0xB0].copy_from_slice(b"BPRE");
        rom[0x400..0x403].copy_from_slice(&[7, 99, 9]);
        let pattern = [Some(7), None, Some(9)];
        assert_eq!(find_in_firered(&rom, &pattern, 0), Some(0x0800_0400));
        assert_eq!(find_in_firered(&rom, &pattern, 2), Some(0x0800_0402));
    }

    /// The builds in the table above, when their ROMs are present under
    /// `rom/`; each absent ROM is skipped (the ROMs are not in the repository).
    #[test]
    fn known_builds_are_found_where_the_table_says() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("rom");
        let builds = [
            (
                "Pokemon Fire Red/Pokemon Fire Red (v1.0).gba",
                0x0808_29FC,
                0x0802_D6BC,
            ),
            (
                "Pokemon Fire Red/Pokemon Fire Red (v1.1).gba",
                0x0808_2A10,
                0x0802_D6D0,
            ),
            (
                "Pokemon Fire Red/Pokemon Fire Red (J).gba",
                0x0808_25E8,
                0x0802_CE94,
            ),
        ];
        for (file, expected, expected_catch) in builds {
            let Ok(rom) = std::fs::read(root.join(file)) else {
                continue;
            };
            // A differently dumped copy is not an error here, only unlisted.
            if rom.len() != 16 * 1024 * 1024 {
                continue;
            }
            assert_eq!(wild_species::find(&rom), Some(expected), "{file}");
            assert_eq!(catch_rate::find(&rom), Some(expected_catch), "{file}");
        }
    }
}
