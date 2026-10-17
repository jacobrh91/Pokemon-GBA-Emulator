//! Infinitely usable TMs: teaching a move from a TM never uses it up. This
//! cheat is **always on**, with no row to switch.
//!
//! **Mechanism: replace a function's argument just before the call.** Once a
//! Pokémon has learned the move, the game removes the item from the bag unless
//! it is an HM:
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
//! At the `bl` the emulator sets `r1` to 0, so the game asks to remove none
//! and the TM keeps its count.
//!
//! # The places
//!
//! The same code is compiled in more than one way, and a game has one or more
//! such places:
//!
//! * **Variant A** (above): `cmp r1, r0` / `adds r0, r1, #0`, followed by `mov
//!   r1, r8`. FireRed and LeafGreen have two of these, one for each way the
//!   lesson finishes.
//! * **Variant B**: the item is in `r4`: `cmp r4, r0` / `adds r0, r4, #0`,
//!   followed by a PC-relative `ldr r1, [pc, #..]`. FireRed and LeafGreen have
//!   one more of these (a third way the lesson finishes, found by scanning
//!   Emerald, which has only this one), and so does Emerald.
//!
//! Every one found is hooked, one to three in all. Selling or tossing a TM
//! goes through other code and is not affected.

use super::Signature;

/// Variant A: the item is in `r1`. `None` is the wildcard of the `BL`.
pub(crate) const SIGNATURE: Signature = Signature::new(
    &[
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
    ],
    12,
);

/// Variant B: the item is in `r4`; the instruction after the call is a
/// PC-relative load (its offset is a wildcard).
pub(crate) const VARIANT_B: Signature = Signature::new(
    &[
        Some(0xA9),
        Some(0x20),
        Some(0x40),
        Some(0x00),
        Some(0x84),
        Some(0x42),
        Some(0x03),
        Some(0xD8),
        Some(0x20),
        Some(0x1C),
        Some(0x01),
        Some(0x21),
        None,
        None,
        None,
        None,
        None,
        Some(0x49),
    ],
    12,
);

/// The most places a game has; more matches than this are not believed.
const MAX_SITES: usize = 3;

/// The addresses of every `bl RemoveBagItem` of the TM lesson, ascending, if
/// `rom` is a recognised game and between one and three are found. Empty
/// otherwise.
pub fn find(rom: &[u8]) -> Vec<u32> {
    let mut found: Vec<u32> = [&SIGNATURE, &VARIANT_B]
        .into_iter()
        .flat_map(|signature| signature.find_all(rom))
        .collect();
    found.sort_unstable();
    found.dedup();
    if found.len() > MAX_SITES {
        found.clear();
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::gen3::test_support::rom_with;

    #[test]
    fn variant_a_sites_are_found_in_order() {
        let rom = rom_with(b"BPRE", SIGNATURE.bytes, &[0x400, 0x300]);
        assert_eq!(find(&rom), vec![0x0800_030C, 0x0800_040C]);
    }

    #[test]
    fn variant_b_and_a_together_are_all_found() {
        // FireRed and LeafGreen: two of A and one of B.
        let mut rom = rom_with(b"BPGE", SIGNATURE.bytes, &[0x300, 0x500]);
        let b = rom_with(b"BPGE", VARIANT_B.bytes, &[0x400]);
        rom[0x400..0x400 + VARIANT_B.bytes.len()]
            .copy_from_slice(&b[0x400..0x400 + VARIANT_B.bytes.len()]);
        assert_eq!(find(&rom), vec![0x0800_030C, 0x0800_040C, 0x0800_050C]);
        // Emerald: one of B.
        assert_eq!(
            find(&rom_with(b"BPEE", VARIANT_B.bytes, &[0x7F0])),
            vec![0x0800_07FC]
        );
    }

    #[test]
    fn none_too_many_other_games_unaligned_or_empty_are_declined() {
        assert!(find(&rom_with(b"BPRE", SIGNATURE.bytes, &[])).is_empty());
        let four = rom_with(b"BPRE", SIGNATURE.bytes, &[0x200, 0x300, 0x400, 0x500]);
        assert!(find(&four).is_empty(), "more than a game has");
        assert!(find(&rom_with(b"BPRE", SIGNATURE.bytes, &[0x301])).is_empty());
        for code in [b"AXVE", b"BPXE"] {
            assert!(find(&rom_with(code, SIGNATURE.bytes, &[0x300])).is_empty());
        }
        assert!(find(&[]).is_empty());
    }

    #[test]
    fn the_signatures_are_the_documented_guard_on_hms() {
        let halfword = |signature: &Signature, index: usize| {
            u16::from_le_bytes([
                signature.bytes[index].unwrap(),
                signature.bytes[index + 1].unwrap(),
            ])
        };
        for signature in [&SIGNATURE, &VARIANT_B] {
            assert_eq!(halfword(signature, 0), 0x20A9, "movs r0, #0xA9");
            assert_eq!(halfword(signature, 2), 0x0040, "lsls r0, r0, #1");
            assert_eq!(halfword(signature, 6), 0xD803, "bhi");
            assert_eq!(halfword(signature, 10), 0x2101, "movs r1, #1");
        }
        assert_eq!(0xA9u32 << 1, 338, "TM50");
        assert_eq!(halfword(&SIGNATURE, 4), 0x4281, "cmp r1, r0");
        assert_eq!(halfword(&VARIANT_B, 4), 0x4284, "cmp r4, r0");
    }
}
