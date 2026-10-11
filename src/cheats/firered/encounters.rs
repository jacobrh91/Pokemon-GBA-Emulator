//! The no-random-encounters cheat: walking in grass, caves and on water never
//! starts a wild battle.
//!
//! **Mechanism: replace a value at the end of a routine**, like
//! [`super::money`]. Every step on an encounter tile asks the game whether an
//! encounter happens, and the answer comes from one small function: it
//! compares a random number with the encounter rate (already scaled by the
//! tile, the Repel, the lead Pokémon's ability and so on) and returns 1 for
//! "yes". The emulator stops at the instruction where that answer is final and
//! sets it to 0 (see `Emulator::step`), so the game goes on exactly as if the
//! dice had said no. Nothing else changes: steps are counted, a Repel still
//! wears off, the encounter tables are intact, and fishing and scripted or
//! static encounters (which do not ask) still work.
//!
//! # The function
//!
//! `Random() % 1600 < rate`, in Thumb:
//!
//! ```text
//! 10 B5        push {r4, lr}
//! 04 1C        adds r4, r0, #0        r4 = rate
//! 24 04  24 0C lsls/lsrs r4, r4, #16  r4 = (u16) rate
//! ?? ?? ?? ??  bl Random
//! 00 04  00 0C lsls/lsrs r0, r0, #16  r0 = (u16) random
//! C8 21  C9 00 movs r1, #0xC8 ; lsls r1, r1, #3     r1 = 1600
//! ?? ?? ?? ??  bl __umodsi3
//! 00 04  00 0C lsls/lsrs r0, r0, #16
//! A0 42        cmp  r0, r4
//! 01 D3        blo  +1
//! 00 20        movs r0, #0            no encounter
//! 00 E0        b    +0
//! 01 20        movs r0, #1            encounter
//! 10 BC        pop  {r4}              <- the hook: r0 is the answer
//! 02 BC  08 47 pop  {r1} ; bx r1
//! ```
//!
//! The two calls are wildcards (relative distances); the hook is the `pop
//! {r4}` 38 bytes in. As for the other scans, a ROM is used only when there
//! is exactly one aligned match in a `BPR?` cartridge (see [`super`]).

use super::find_in_firered;

/// The bytes of the function; `None` is the wildcard of a `BL`.
pub(crate) const SIGNATURE: [Option<u8>; 44] = [
    Some(0x10),
    Some(0xB5),
    Some(0x04),
    Some(0x1C),
    Some(0x24),
    Some(0x04),
    Some(0x24),
    Some(0x0C),
    None,
    None,
    None,
    None,
    Some(0x00),
    Some(0x04),
    Some(0x00),
    Some(0x0C),
    Some(0xC8),
    Some(0x21),
    Some(0xC9),
    Some(0x00),
    None,
    None,
    None,
    None,
    Some(0x00),
    Some(0x04),
    Some(0x00),
    Some(0x0C),
    Some(0xA0),
    Some(0x42),
    Some(0x01),
    Some(0xD3),
    Some(0x00),
    Some(0x20),
    Some(0x00),
    Some(0xE0),
    Some(0x01),
    Some(0x20),
    Some(0x10),
    Some(0xBC),
    Some(0x02),
    Some(0xBC),
    Some(0x08),
    Some(0x47),
];

/// Offset of the `pop {r4}` within the signature.
const HOOK_OFFSET: usize = 38;

/// The bus address of the `pop {r4}` that ends the encounter test in `rom`,
/// if `rom` is a FireRed cartridge and the function can be identified
/// unambiguously.
pub fn find(rom: &[u8]) -> Option<u32> {
    find_in_firered(rom, &SIGNATURE, HOOK_OFFSET)
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
    fn the_hook_is_the_instruction_that_ends_the_function() {
        assert_eq!(find(&rom(b"BPRE", &[0x400])), Some(0x0800_0426));
        assert_eq!(find(&rom(b"BPRJ", &[0x7F0])), Some(0x0800_0816));
    }

    #[test]
    fn the_place_is_declined_unless_unique_aligned_and_firered() {
        assert_eq!(find(&rom(b"BPRE", &[])), None);
        assert_eq!(find(&rom(b"BPRE", &[0x400, 0x800])), None);
        assert_eq!(find(&rom(b"BPRE", &[0x401])), None);
        assert_eq!(find(&rom(b"AXVE", &[0x400])), None);
        assert_eq!(find(&rom(b"BPGE", &[0x400])), None);
        assert_eq!(find(&[]), None);
    }

    #[test]
    fn the_signature_is_the_documented_function() {
        let halfword = |index: usize| {
            u16::from_le_bytes([SIGNATURE[index].unwrap(), SIGNATURE[index + 1].unwrap()])
        };
        assert_eq!(halfword(0), 0xB510, "push {{r4, lr}}");
        assert_eq!(halfword(16), 0x21C8, "movs r1, #0xC8");
        assert_eq!(halfword(18), 0x00C9, "lsls r1, r1, #3");
        assert_eq!(0xC8u32 << 3, 1600);
        assert_eq!(halfword(28), 0x42A0, "cmp r0, r4");
        assert_eq!(halfword(38), 0xBC10, "pop {{r4}}");
        assert_eq!(halfword(42), 0x4708, "bx r1");
    }
}
