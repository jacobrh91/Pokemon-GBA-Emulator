//! Locating the game's wild-encounter function, for the `wild_species`
//! override, by scanning the ROM for its opening bytes (a *signature scan*).
//!
//! # What is being found
//!
//! Every wild encounter in Pokémon FireRed (grass, surfing, fishing, rock
//! smash) builds the wild Pokémon through one function,
//! `CreateWildMon(species, level, …)`. Its entry point is where the override
//! acts: when the CPU is about to execute that first instruction, the species
//! argument register (`r0`) is replaced, and the game carries on as if it had
//! chosen that species itself (see `Emulator::step`).
//!
//! The function's *address* is different in every build of the game. Instead
//! of keeping a table of addresses, the emulator finds the function by what
//! it looks like.
//!
//! # The signature
//!
//! The function begins with these 16 bytes (eight little-endian Thumb
//! instructions), identical in every FireRed build examined:
//!
//! ```text
//! 70 B5   push {r4, r5, r6, lr}      save registers
//! 84 B0   sub  sp, #0x10             room for locals
//! 00 04   lsls r0, r0, #16  ┐
//! 04 0C   lsrs r4, r0, #16  ┘        r4 = (u16) argument 0   (the species)
//! 09 06   lsls r1, r1, #24  ┐
//! 0E 0E   lsrs r6, r1, #24  ┘        r6 = (u8)  argument 1   (the level)
//! 12 06   lsls r2, r2, #24  ┐
//! 15 0E   lsrs r5, r2, #24  ┘        r5 = (u8)  argument 2
//! ```
//!
//! These bytes were chosen because they contain nothing that legitimately
//! changes between builds. Two kinds of instruction do: a `BL` call encodes a
//! *relative* distance to its target, and a PC-relative `LDR` encodes the
//! distance to a data pool. Both change when code or data around them moves.
//! The prologue above has only register moves and shifts, so a rebuild of the
//! same source with the same compiler produces the same bytes, wherever the
//! function ends up.
//!
//! # How the scan works
//!
//! 1. The ROM must be a FireRed cartridge: game code `BPR?` in the header at
//!    `0xAC` (`BPRE` English, `BPRJ` Japanese, …). A 16-byte pattern is
//!    unlikely to occur by chance elsewhere, but this keeps the hook away from
//!    other games altogether. (LeafGreen, `BPG?`, shares the engine but has
//!    not been checked, so it is not enabled.)
//! 2. Every 2-byte-aligned offset of the ROM is compared with the signature
//!    (Thumb instructions are halfwords, so a match elsewhere is meaningless).
//! 3. The result must be **exactly one** match. Zero means this build is not
//!    recognised; more than one means the signature is ambiguous. In both
//!    cases nothing is hooked and the override reports itself unsupported,
//!    rather than guessing.
//! 4. The hook address is the match offset plus `0x0800_0000`, where the ROM
//!    appears on the bus.
//!
//! The scan runs once, when the ROM is opened.
//!
//! # Known builds
//!
//! | Build | Game code | Version | CRC32 | Function at | Checked |
//! |---|---|---|---|---|---|
//! | FireRed v1.0 (US) | `BPRE` | 0 | `7845FBDD` | `0x080829FC` | live encounter: Rattata → Dratini |
//! | FireRed v1.1 (US) | `BPRE` | 1 | `84EE4776` | `0x08082A10` | live encounter: Pidgey → Dratini |
//! | FireRed (Japan)   | `BPRJ` | 0 | `3B2056E9` | `0x080825E8` | live encounter: Rattata → Dratini (the function body also matches v1.0 apart from branch and pool operands) |
//!
//! The enemy party is not at the same RAM address in every build (it is
//! `0x0202402C` in the English builds and `0x02023F8C` in the Japanese one),
//! which matters for anything that reads the species back out of RAM, but not
//! for the override: it replaces a register at the function entry and never
//! touches RAM.
//!
//! "Checked" says how far each build has been verified. A unique signature
//! match shows the bytes are there, not that the function behaves the same;
//! each new build deserves a live encounter before it is relied on (walk in
//! grass with `wild_species` set and look at the enemy species).
//!
//! # Adding or checking a build
//!
//! Run the emulator on the ROM with `wild_species` set; the terminal reports
//! the address it hooked, or why it did not. A build whose function was
//! compiled differently simply has no match and is declined. If a build has a
//! rewritten prologue, find the function by tracing an encounter (the call
//! chain ends in `CreateMon` with the new Pokémon's species in `r1`) and add
//! its own signature.

/// The function's opening bytes; see the module documentation.
pub(super) const SIGNATURE: [u8; 16] = [
    0x70, 0xB5, 0x84, 0xB0, 0x00, 0x04, 0x04, 0x0C, 0x09, 0x06, 0x0E, 0x0E, 0x12, 0x06, 0x15, 0x0E,
];

/// Where the ROM appears on the bus.
const ROM_BASE: u32 = 0x0800_0000;

/// Game-code prefix of the FireRed cartridges (`BPRE`, `BPRJ`, ...).
const FIRERED_GAME_CODE_PREFIX: &[u8; 3] = b"BPR";

/// The bus address of `CreateWildMon` in `rom`, if `rom` is a FireRed
/// cartridge and the function can be identified unambiguously.
pub(super) fn find_create_wild_mon(rom: &[u8]) -> Option<u32> {
    // The game code lives at 0xAC..0xB0 in the cartridge header.
    let game_code = rom.get(0xAC..0xB0)?;
    if !game_code.starts_with(FIRERED_GAME_CODE_PREFIX) {
        return None;
    }
    find_unique(rom, &SIGNATURE).map(|offset| ROM_BASE + offset as u32)
}

/// The offset of `needle` in `haystack` if it occurs exactly once at a
/// 2-byte-aligned position, and nowhere else at such a position.
fn find_unique(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let mut found = None;
    for offset in (0..=haystack.len() - needle.len()).step_by(2) {
        // Cheap first-byte test keeps the 16 MiB scan quick.
        if haystack[offset] == needle[0] && haystack[offset..offset + needle.len()] == *needle {
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

    /// A header-sized prefix followed by padding, with `SIGNATURE` placed at
    /// each of `offsets`.
    fn rom(game_code: &[u8; 4], offsets: &[usize]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x1000];
        rom[0xAC..0xB0].copy_from_slice(game_code);
        for &offset in offsets {
            rom[offset..offset + SIGNATURE.len()].copy_from_slice(&SIGNATURE);
        }
        rom
    }

    #[test]
    fn a_unique_aligned_match_in_a_firered_rom_is_found() {
        assert_eq!(
            find_create_wild_mon(&rom(b"BPRE", &[0x400])),
            Some(0x0800_0400)
        );
        assert_eq!(
            find_create_wild_mon(&rom(b"BPRJ", &[0x7F0])),
            Some(0x0800_07F0)
        );
    }

    #[test]
    fn no_match_or_several_matches_are_declined() {
        assert_eq!(find_create_wild_mon(&rom(b"BPRE", &[])), None);
        assert_eq!(find_create_wild_mon(&rom(b"BPRE", &[0x400, 0x800])), None);
    }

    #[test]
    fn only_halfword_aligned_matches_count() {
        // An odd offset is not a Thumb instruction boundary.
        assert_eq!(find_create_wild_mon(&rom(b"BPRE", &[0x401])), None);
        // ...and does not make an aligned match ambiguous.
        assert_eq!(
            find_create_wild_mon(&rom(b"BPRE", &[0x400, 0x801])),
            Some(0x0800_0400)
        );
    }

    #[test]
    fn other_games_are_left_alone_even_if_the_bytes_appear() {
        assert_eq!(find_create_wild_mon(&rom(b"AXVE", &[0x400])), None);
        // LeafGreen shares the engine but is not enabled.
        assert_eq!(find_create_wild_mon(&rom(b"BPGE", &[0x400])), None);
    }

    #[test]
    fn tiny_or_headerless_roms_do_not_panic() {
        assert_eq!(find_create_wild_mon(&[]), None);
        assert_eq!(find_create_wild_mon(&[0; 8]), None);
        assert_eq!(find_unique(&[1, 2], &SIGNATURE), None);
        assert_eq!(find_unique(&[0; 32], &[]), None);
    }

    #[test]
    fn the_signature_decodes_to_the_documented_prologue() {
        let halfwords: Vec<u16> = SIGNATURE
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            halfwords,
            [
                0xB570, 0xB084, 0x0400, 0x0C04, 0x0609, 0x0E0E, 0x0612, 0x0E15
            ]
        );
    }

    /// The builds in the table above, when their ROMs are present under
    /// `rom/`; each absent ROM is skipped (the ROMs are not in the repository).
    #[test]
    fn known_builds_are_found_where_the_table_says() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("rom");
        let builds = [
            ("Pokemon Fire Red/Pokemon Fire Red (v1.0).gba", 0x0808_29FC),
            ("Pokemon Fire Red/Pokemon Fire Red (v1.1).gba", 0x0808_2A10),
            ("Pokemon Fire Red/Pokemon Fire Red (J).gba", 0x0808_25E8),
        ];
        for (file, expected) in builds {
            let Ok(rom) = std::fs::read(root.join(file)) else {
                continue;
            };
            // A differently dumped copy is not an error here, only unlisted.
            if rom.len() != 16 * 1024 * 1024 {
                continue;
            }
            assert_eq!(find_create_wild_mon(&rom), Some(expected), "{file}");
        }
    }
}
