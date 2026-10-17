//! The wild-encounter override: every wild Pokémon is the chosen species at
//! the chosen level.
//!
//! **Mechanism: replace a function's arguments at its entry.** The emulator
//! stops at the first instruction of `CreateWildMon`, which builds every wild
//! encounter (grass, surfing, fishing, rock smash), and rewrites `r0`
//! (species) and `r1` (level) (see `Emulator::step`). The function's address
//! differs between builds, so it is found by what it looks like (see
//! [`super`]).
//!
//! # The signature
//!
//! The function begins with these 16 bytes (eight little-endian Thumb
//! instructions), identical in every FireRed and LeafGreen build examined:
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
//! These bytes are only register moves and shifts, with no `BL` or
//! PC-relative `LDR`, so they do not change between builds. The hook address
//! is the match itself, the function's entry.
//!
//! Emerald's `CreateWildMon` takes two arguments (no slot), so it begins
//! differently; the species and level are still `r0` and `r1`:
//!
//! ```text
//! F0 B5   push {r4, r5, r6, r7, lr}
//! 83 B0   sub  sp, #0xC
//! 00 04   lsls r0, r0, #16  ┐
//! 06 0C   lsrs r6, r0, #16  ┘        r6 = (u16) species
//! 09 06   lsls r1, r1, #24  ┐
//! 0F 0E   lsrs r7, r1, #24  ┘        r7 = (u8)  level
//! ?? ?? ?? ??  bl ZeroEnemyPartyMons
//! 01 22   movs r2, #1
//! ```
//!
//! # Checking a build
//!
//! Walk in grass with the wild row on and look at the enemy species. For a
//! build with a rewritten prologue, trace an encounter (the call chain ends
//! in `CreateMon` with the species in `r1`) and add its own signature.

use super::{Signature, exact, find_variant};

/// The function's opening bytes; the hook is its entry point.
pub(crate) const SIGNATURE: Signature = Signature::new(
    &exact([
        0x70, 0xB5, 0x84, 0xB0, 0x00, 0x04, 0x04, 0x0C, 0x09, 0x06, 0x0E, 0x0E, 0x12, 0x06, 0x15,
        0x0E,
    ]),
    0,
);

/// `CreateWildMon` in Emerald, whose prologue differs; the hook is its entry.
pub(crate) const EMERALD_SIGNATURE: Signature = Signature::new(
    &[
        Some(0xF0),
        Some(0xB5),
        Some(0x83),
        Some(0xB0),
        Some(0x00),
        Some(0x04),
        Some(0x06),
        Some(0x0C),
        Some(0x09),
        Some(0x06),
        Some(0x0F),
        Some(0x0E),
        None,
        None,
        None,
        None,
        Some(0x01),
        Some(0x22),
    ],
    0,
);

/// The address of `CreateWildMon`, if `rom` is a recognised game.
pub fn find(rom: &[u8]) -> Option<u32> {
    find_variant(rom, &[&SIGNATURE, &EMERALD_SIGNATURE])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::gen3::test_support::assert_single_match_rules;

    #[test]
    fn createwildmon_follows_the_rules_for_one_place() {
        assert_single_match_rules(find, &SIGNATURE);
        assert_single_match_rules(find, &EMERALD_SIGNATURE);
    }

    #[test]
    fn the_two_variants_do_not_find_each_others_function() {
        let firered =
            crate::cheats::gen3::test_support::rom_with(b"BPRE", SIGNATURE.bytes, &[0x400]);
        let emerald =
            crate::cheats::gen3::test_support::rom_with(b"BPEE", EMERALD_SIGNATURE.bytes, &[0x400]);
        assert_eq!(EMERALD_SIGNATURE.find(&firered), None);
        assert_eq!(SIGNATURE.find(&emerald), None);
        // Both present in one image is ambiguous: no guess.
        let mut both = firered.clone();
        both[0x800..0x800 + 18].copy_from_slice(&emerald[0x400..0x400 + 18]);
        assert_eq!(find(&both), None);
    }

    #[test]
    fn the_signature_decodes_to_the_documented_prologue() {
        let bytes: Vec<u8> = SIGNATURE.bytes.iter().map(|b| b.unwrap()).collect();
        let halfwords: Vec<u16> = bytes
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
