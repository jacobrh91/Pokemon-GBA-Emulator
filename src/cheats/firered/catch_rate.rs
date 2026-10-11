//! The 100% catch rate cheat: every thrown ball catches.
//!
//! **Mechanism: replace a value in the middle of a routine.** The emulator
//! stops when the CPU is about to execute one instruction inside the
//! ball-throw battle script and sets the register holding the catch odds
//! (`r6 = 0xFF`), so the very next comparison goes the way of a certain catch
//! (see `Emulator::step`). Compare [`super::wild_species`], which rewrites a
//! function's arguments at its entry.
//!
//! # The catch check
//!
//! A thrown ball is resolved by one battle-script command. It computes `odds`
//! from the target's catch rate, the ball and the target's HP and status, and
//! then tests `if (odds > 254)`: above 254 the Pokémon is caught on the spot
//! (three shakes and the click); otherwise a shake roll decides. Forcing `odds`
//! above 254 therefore makes every ball a catch.
//!
//! In the Thumb code the test sits right after the bookkeeping of the number
//! of attempts, and `odds` lives in `r6`:
//!
//! ```text
//! FE 28   cmp  r0, #0xFE      ; if (catchAttempts < 0xFF)
//! 01 D8   bhi  +1             ;
//! 01 30   adds r0, #1         ;     catchAttempts++
//! 08 70   strb r0, [r1]       ;
//! FE 2E   cmp  r6, #0xFE      ; if (odds > 254)   <- the hook is here
//! ?? D9   bls  shake_roll     ;     else: shakes
//! ```
//!
//! The 12 bytes are matched with the branch offset (`??`) as a wildcard, and
//! the hook address is the `cmp r6, #0xFE`, 8 bytes into the match. At that
//! moment `r6` is set to 0xFF, so the `bls` is not taken. As for the other
//! scans, a ROM is used only when there is exactly one 2-byte aligned match in
//! a `BPR?` cartridge (see [`super`]).
//!
//! Other ball throws go through the same test (the Safari Zone's ball, the
//! Master Ball). Throws that never reach it are unaffected: a trainer's
//! Pokémon ("it dodged the ball") and the scripted tutorial catch.
//!
//! # How the place was found
//!
//! By recording every ROM address the CPU executed during a real ball throw
//! and keeping the `cmp rN, #0xFE` instructions; exactly one sat in front of a
//! shake-roll branch. A new build is checked the same way, or simply by
//! throwing a ball at a high-level Pokémon with the cheat off and on.

use super::find_in_firered;

/// The bytes around the catch check (see the module documentation); `None` is
/// a wildcard for the branch offset, which is the same in the builds examined
/// but is not what identifies the code.
pub(crate) const SIGNATURE: [Option<u8>; 12] = [
    Some(0xFE),
    Some(0x28),
    Some(0x01),
    Some(0xD8),
    Some(0x01),
    Some(0x30),
    Some(0x08),
    Some(0x70),
    Some(0xFE),
    Some(0x2E),
    None,
    Some(0xD9),
];

/// Offset of the `cmp r6, #0xFE` within the signature.
const COMPARE_OFFSET: usize = 8;

/// The bus address of the `cmp r6, #0xFE` of the catch check in `rom`, if
/// `rom` is a FireRed cartridge and the check can be identified
/// unambiguously.
pub fn find(rom: &[u8]) -> Option<u32> {
    find_in_firered(rom, &SIGNATURE, COMPARE_OFFSET)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::firered::wild_species;

    /// A header-sized prefix with the catch-check bytes at each offset, the
    /// branch offset being the paired byte.
    fn rom(game_code: &[u8; 4], offsets: &[(usize, u8)]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x1000];
        rom[0xAC..0xB0].copy_from_slice(game_code);
        for &(offset, branch) in offsets {
            for (index, byte) in SIGNATURE.iter().enumerate() {
                rom[offset + index] = byte.unwrap_or(branch);
            }
        }
        rom
    }

    #[test]
    fn the_catch_check_is_found_and_the_hook_is_the_compare() {
        // The hook is the `cmp r6, #0xFE`, 8 bytes into the match.
        assert_eq!(find(&rom(b"BPRE", &[(0x400, 0x2F)])), Some(0x0800_0408));
        // The branch offset is a wildcard: another build may differ in it.
        assert_eq!(find(&rom(b"BPRJ", &[(0x7F0, 0x31)])), Some(0x0800_07F8));
    }

    #[test]
    fn the_catch_check_is_declined_unless_it_is_unique_aligned_and_firered() {
        assert_eq!(find(&rom(b"BPRE", &[])), None);
        assert_eq!(
            find(&rom(b"BPRE", &[(0x400, 0x2F), (0x800, 0x2F)])),
            None,
            "ambiguous"
        );
        assert_eq!(find(&rom(b"BPRE", &[(0x401, 0x2F)])), None);
        assert_eq!(find(&rom(b"AXVE", &[(0x400, 0x2F)])), None);
        assert_eq!(find(&rom(b"BPGE", &[(0x400, 0x2F)])), None);
    }

    #[test]
    fn tiny_or_headerless_roms_do_not_panic() {
        assert_eq!(find(&[]), None);
        assert_eq!(find(&[0; 8]), None);
    }

    #[test]
    fn the_two_cheats_do_not_find_each_others_code() {
        let mut wild = vec![0u8; 0x1000];
        wild[0xAC..0xB0].copy_from_slice(b"BPRE");
        wild[0x400..0x410].copy_from_slice(&wild_species::SIGNATURE);
        assert_eq!(find(&wild), None);
        assert_eq!(wild_species::find(&rom(b"BPRE", &[(0x400, 0x2F)])), None);
    }
}
