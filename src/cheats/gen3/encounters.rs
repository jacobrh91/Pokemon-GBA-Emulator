//! The no-random-encounters cheat: walking in grass, caves and on water never
//! starts a wild battle.
//!
//! **Mechanism: replace a value at the end of a routine.** Every step on an
//! encounter tile asks one small function whether an encounter happens: it
//! compares a random number with the (already scaled) encounter rate and
//! returns 1 for yes. The emulator stops where the answer is final and sets
//! it to 0 (see `Emulator::step`). Steps are still counted and fishing and
//! scripted encounters, which do not ask, still work.
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
//! The two calls are wildcards; the hook is the `pop {r4}` 38 bytes in.
//!
//! The divisor is the one thing that differs between games: FireRed and
//! LeafGreen use 1600 (`movs r1, #0xC8 ; lsls r1, r1, #3`) and Emerald 2880
//! (`movs r1, #0xB4 ; lsls r1, r1, #4`). The four bytes of that constant are
//! wildcards (with the `21` of `movs r1, #imm` kept), and the rest still
//! matches exactly once in each game.

use super::Signature;

/// The bytes of the function; `None` is the wildcard of a `BL`.
pub(crate) const SIGNATURE: Signature = Signature::new(
    &[
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
        None,
        Some(0x21),
        None,
        None,
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
    ],
    38,
);

/// The address of the `pop {r4}` that ends the encounter test, if `rom` is a
/// recognised FireRed or LeafGreen cartridge.
pub fn find(rom: &[u8]) -> Option<u32> {
    SIGNATURE.find(rom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_place_follows_the_rules_for_one_place() {
        crate::cheats::gen3::test_support::assert_single_match_rules(find, &SIGNATURE);
    }

    #[test]
    fn the_signature_is_the_documented_function() {
        let halfword = |index: usize| {
            u16::from_le_bytes([
                SIGNATURE.bytes[index].unwrap(),
                SIGNATURE.bytes[index + 1].unwrap(),
            ])
        };
        assert_eq!(halfword(0), 0xB510, "push {{r4, lr}}");
        // The divisor (1600 or 2880) is a wildcard; only `movs r1, #imm` is
        // fixed.
        assert_eq!(SIGNATURE.bytes[16], None);
        assert_eq!(SIGNATURE.bytes[17], Some(0x21), "movs r1, #imm");
        assert_eq!(0xC8u32 << 3, 1600);
        assert_eq!(0xB4u32 << 4, 2880);
        assert_eq!(halfword(28), 0x42A0, "cmp r0, r4");
        assert_eq!(halfword(38), 0xBC10, "pop {{r4}}");
        assert_eq!(halfword(42), 0x4708, "bx r1");
    }
}
