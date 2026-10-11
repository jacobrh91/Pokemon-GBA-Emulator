//! The wild-encounter override: every wild Pokémon is the chosen species at
//! the chosen level.
//!
//! **Mechanism: replace a function's arguments at its entry.** The emulator
//! stops when the CPU is about to execute the first instruction of
//! `CreateWildMon` and rewrites the argument registers `r0` (species) and `r1`
//! (level); the game carries on as if it had chosen them itself (see
//! `Emulator::step`). Compare [`super::catch_rate`], which changes a value in
//! the middle of a routine.
//!
//! # What is being found: `CreateWildMon`
//!
//! Every wild encounter in Pokémon FireRed (grass, surfing, fishing, rock
//! smash) builds the wild Pokémon through one function,
//! `CreateWildMon(species, level, …)`. Its *address* is different in every
//! build of the game, so instead of keeping a table of addresses the emulator
//! finds the function by what it looks like (see [`super`] for how a scan
//! works).
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
//! These bytes contain nothing that legitimately changes between builds: only
//! register moves and shifts, no `BL` or PC-relative `LDR`. The hook address
//! is the match itself, the function's entry point.
//!
//! # Checking a build
//!
//! Run the emulator on the ROM with `wild_species` set (or the cheat menu's
//! wild row on) and walk in grass; look at the enemy species. If a build has a
//! rewritten prologue, find the function by tracing an encounter (the call
//! chain ends in `CreateMon` with the new Pokémon's species in `r1`) and add
//! its own signature.

use super::find_in_firered;

/// The function's opening bytes; see the module documentation.
pub(crate) const SIGNATURE: [u8; 16] = [
    0x70, 0xB5, 0x84, 0xB0, 0x00, 0x04, 0x04, 0x0C, 0x09, 0x06, 0x0E, 0x0E, 0x12, 0x06, 0x15, 0x0E,
];

/// The bus address of `CreateWildMon` in `rom`, if `rom` is a FireRed
/// cartridge and the function can be identified unambiguously.
pub fn find(rom: &[u8]) -> Option<u32> {
    let pattern: Vec<Option<u8>> = SIGNATURE.iter().map(|byte| Some(*byte)).collect();
    find_in_firered(rom, &pattern, 0)
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
        assert_eq!(find(&rom(b"BPRE", &[0x400])), Some(0x0800_0400));
        assert_eq!(find(&rom(b"BPRJ", &[0x7F0])), Some(0x0800_07F0));
    }

    #[test]
    fn no_match_or_several_matches_are_declined() {
        assert_eq!(find(&rom(b"BPRE", &[])), None);
        assert_eq!(find(&rom(b"BPRE", &[0x400, 0x800])), None);
    }

    #[test]
    fn only_halfword_aligned_matches_count() {
        // An odd offset is not a Thumb instruction boundary.
        assert_eq!(find(&rom(b"BPRE", &[0x401])), None);
        // ...and does not make an aligned match ambiguous.
        assert_eq!(find(&rom(b"BPRE", &[0x400, 0x801])), Some(0x0800_0400));
    }

    #[test]
    fn other_games_are_left_alone_even_if_the_bytes_appear() {
        assert_eq!(find(&rom(b"AXVE", &[0x400])), None);
        // LeafGreen shares the engine but is not enabled.
        assert_eq!(find(&rom(b"BPGE", &[0x400])), None);
    }

    #[test]
    fn tiny_or_headerless_roms_do_not_panic() {
        assert_eq!(find(&[]), None);
        assert_eq!(find(&[0; 8]), None);
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
}
