//! Infinitely usable TMs: teaching a move from a TM never uses the TM up.
//! (HMs were never consumed.) This one is **always on**, with no row to switch.
//!
//! **Mechanism: replace a function's argument just before the call**, like
//! [`super::wild_species`] but at a call site. When a Pokémon has learned the
//! move, the game takes the item out of the bag unless it is an HM:
//!
//! ```text
//! 3988      ldrh r1, [r7]          r1 = the item used
//! A9 20     movs r0, #0xA9
//! 40 00     lsls r0, r0, #1        r0 = 338 (TM50)
//! 81 42     cmp  r1, r0
//! 03 D8     bhi  +3                above TM50: an HM, keep it
//! 08 1C     adds r0, r1, #0        r0 = item
//! 01 21     movs r1, #1            r1 = 1: how many
//! ?? ?? ?? ??  bl RemoveBagItem    <- the hook: r1 is set to 0
//! 41 46  08 68                     mov r1, r8 ; ldr r0, [r1]
//! ```
//!
//! At the `bl` the emulator sets `r1` to 0, so the game asks to remove none of
//! them: the TM stays at its count (`RemoveBagItem` with a count of 0 finds
//! the slot, subtracts nothing and writes the same number back).
//!
//! The code appears twice in every build examined, once for each way of
//! finishing the lesson (the move went into a free slot, or replaced one);
//! both are hooked. The 20 bytes have the call as a wildcard and nothing
//! PC-relative, so they are the same in each build. As for the other scans, a
//! ROM is used only for a `BPR?` cartridge, and only if there are exactly two
//! aligned matches (see [`super`]).
//!
//! Selling or tossing a TM is not affected: those go through `RemoveBagItem`
//! from other code, which is not hooked.

use super::find_all_in_firered;

/// The bytes before and after the call; `None` is the wildcard of the `BL`.
pub(crate) const SIGNATURE: [Option<u8>; 20] = [
    Some(0xA9),
    Some(0x20),
    Some(0x40),
    Some(0x00),
    Some(0x81),
    Some(0x42),
    Some(0x03),
    Some(0xD8),
    Some(0x08),
    Some(0x1C),
    Some(0x01),
    Some(0x21),
    None,
    None,
    None,
    None,
    Some(0x41),
    Some(0x46),
    Some(0x08),
    Some(0x68),
];

/// Offset of the `bl` within the signature.
const CALL_OFFSET: usize = 12;

/// How many places the lesson ends in.
const SITES: usize = 2;

/// The bus addresses of the two `bl RemoveBagItem` of the TM lesson in `rom`,
/// in ascending order, if `rom` is a FireRed cartridge with exactly that many
/// matches.
pub fn find(rom: &[u8]) -> Option<[u32; SITES]> {
    let found = find_all_in_firered(rom, &SIGNATURE, CALL_OFFSET);
    <[u32; SITES]>::try_from(found).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom(game_code: &[u8; 4], offsets: &[usize]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x1000];
        rom[0xAC..0xB0].copy_from_slice(game_code);
        for &offset in offsets {
            for (index, byte) in SIGNATURE.iter().enumerate() {
                rom[offset + index] = byte.unwrap_or(0xAA);
            }
        }
        rom
    }

    #[test]
    fn both_call_sites_are_found_in_order() {
        assert_eq!(
            find(&rom(b"BPRE", &[0x300, 0x400])),
            Some([0x0800_030C, 0x0800_040C])
        );
        assert_eq!(
            find(&rom(b"BPRJ", &[0x7F0, 0x200])),
            Some([0x0800_020C, 0x0800_07FC])
        );
    }

    #[test]
    fn anything_but_exactly_two_matches_in_a_firered_rom_is_declined() {
        assert_eq!(find(&rom(b"BPRE", &[])), None);
        assert_eq!(find(&rom(b"BPRE", &[0x400])), None);
        assert_eq!(find(&rom(b"BPRE", &[0x300, 0x400, 0x500])), None);
        assert_eq!(find(&rom(b"BPRE", &[0x301, 0x400])), None, "unaligned");
        assert_eq!(find(&rom(b"AXVE", &[0x300, 0x400])), None);
        assert_eq!(find(&rom(b"BPGE", &[0x300, 0x400])), None);
        assert_eq!(find(&[]), None);
    }

    #[test]
    fn the_signature_is_the_documented_guard_on_hms() {
        let halfword = |index: usize| {
            u16::from_le_bytes([SIGNATURE[index].unwrap(), SIGNATURE[index + 1].unwrap()])
        };
        assert_eq!(halfword(0), 0x20A9, "movs r0, #0xA9");
        assert_eq!(halfword(2), 0x0040, "lsls r0, r0, #1");
        assert_eq!(halfword(4), 0x4281, "cmp r1, r0");
        assert_eq!(halfword(6), 0xD803, "bhi");
        assert_eq!(0xA9u32 << 1, 338, "TM50");
        assert_eq!(halfword(10), 0x2101, "movs r1, #1");
    }
}
