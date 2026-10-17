//! No poison damage outside battle: a poisoned Pokémon keeps its status but
//! loses no HP while walking, and never faints from it.
//!
//! **Mechanism: replace a value in the middle of a routine.** Every few steps
//! the game calls `DoPoisonFieldEffect`, which goes through the party and, for
//! each Pokémon whose status is poison, takes one HP (counting it as fainted
//! when that leaves 0), then flashes the screen and, if one fainted, ends in a
//! white-out. The status is tested with `GetAilmentFromStatus(status) ==
//! AILMENT_PSN`; the emulator stops at that comparison and makes the ailment
//! "none" (see `Emulator::step`), so the Pokémon is skipped. Nothing else
//! changes: the poison is still there in the party menu and in battle (battle
//! damage is a different routine), and the step counter still counts.
//!
//! # The signature
//!
//! The start of the per-Pokémon loop (the pool address of the party in `r4` is
//! loaded before it and the calls are wildcards):
//!
//! ```text
//! 00 27        movs r7, #0              r7 = number poisoned
//! 00 26        movs r6, #0              r6 = number fainted
//! 05 25        movs r5, #5              r5 = slots left
//! 20 1C  05 21 adds r0, r4, #0 ; movs r1, #5          GetMonData(mon, SPECIES2)
//! ?? ?? ?? ??  bl GetMonData
//! 00 28  ?? D0 cmp r0, #0 ; beq next                  empty slot
//! 20 1C  37 21 adds r0, r4, #0 ; movs r1, #0x37       GetMonData(mon, STATUS)
//! ?? ?? ?? ??  bl GetMonData
//! ?? ?? ?? ??  bl GetAilmentFromStatus
//! 00 06  00 0E lsls r0, r0, #24 ; lsrs r0, r0, #24
//! 01 28        cmp  r0, #1              <- the hook: AILMENT_PSN
//! ?? D1        bne  next
//! ```
//!
//! The 38 bytes are the same in FireRed, LeafGreen and Emerald (the games
//! share this code). As for the other scans, a ROM is used only if the pattern
//! matches exactly once (see [`super`]).

use super::Signature;

/// `AILMENT_NONE`: what the status is made to be.
pub(crate) const NO_AILMENT: u32 = 0;

/// The loop of `DoPoisonFieldEffect`; the hook is the `cmp r0, #1`.
pub(crate) const SIGNATURE: Signature = Signature::new(
    &[
        Some(0x00),
        Some(0x27),
        Some(0x00),
        Some(0x26),
        Some(0x05),
        Some(0x25),
        Some(0x20),
        Some(0x1C),
        Some(0x05),
        Some(0x21),
        None,
        None,
        None,
        None,
        Some(0x00),
        Some(0x28),
        None,
        Some(0xD0),
        Some(0x20),
        Some(0x1C),
        Some(0x37),
        Some(0x21),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(0x00),
        Some(0x06),
        Some(0x00),
        Some(0x0E),
        Some(0x01),
        Some(0x28),
        None,
        Some(0xD1),
    ],
    34,
);

/// The address of the `cmp r0, #1` in `rom`, if it is a recognised game.
pub fn find(rom: &[u8]) -> Option<u32> {
    SIGNATURE.find(rom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::gen3::test_support::assert_single_match_rules;

    #[test]
    fn the_poison_check_follows_the_rules_for_one_place() {
        assert_single_match_rules(find, &SIGNATURE);
    }

    #[test]
    fn the_signature_is_the_documented_loop() {
        let halfword = |index: usize| {
            u16::from_le_bytes([
                SIGNATURE.bytes[index].unwrap(),
                SIGNATURE.bytes[index + 1].unwrap(),
            ])
        };
        assert_eq!(halfword(0), 0x2700, "movs r7, #0");
        assert_eq!(halfword(4), 0x2505, "movs r5, #5");
        assert_eq!(halfword(20), 0x2137, "movs r1, #0x37 (the status)");
        assert_eq!(halfword(34), 0x2801, "cmp r0, #1 (poison)");
        assert_eq!(SIGNATURE.bytes[37], Some(0xD1), "bne");
        assert_eq!(SIGNATURE.hook_offset, 34);
    }
}
