//! The boosted-experience cheat: the player's Pokémon are treated as if
//! received in a trade, earning the traded-Pokémon bonus (×1.5).
//!
//! **Mechanism: change a value in the middle of a routine.** The game does not
//! store "traded": it compares the Pokémon's original-trainer ID with the
//! player's. The cheat gives the Pokémon an ID that is not the player's at the
//! moment the game asks.
//!
//! # Where
//!
//! The battle command that awards experience calls `IsTradedMon(mon)` once
//! for each Pokémon, and the bonus applies if it answers yes:
//!
//! ```text
//! 10 B5        push {r4, lr}
//! 82 B0        sub  sp, #8
//! 04 1C        adds r4, r0, #0        r4 = mon
//! 07 21        movs r1, #7           ┐ GetMonData(mon, MON_DATA_OT_NAME, &name)
//! 6A 46        mov  r2, sp            │
//! ?? ?? ?? ??  bl GetMonData         ┘
//! 20 1C        adds r0, r4, #0       ┐ r0 = GetMonData(mon, MON_DATA_OT_ID)
//! 01 21        movs r1, #1            │
//! 00 22        movs r2, #0            │
//! ?? ?? ?? ??  bl GetMonData         ┘
//! 69 46        mov  r1, sp            <- the hook: r0 is the OT ID
//! ?? ?? ?? ??  bl IsOtherTrainer     (r0 = OT ID, r1 = OT name)
//! 00 06 00 0E  lsls/lsrs r0, r0, #24  the answer
//! 02 B0 10 BC 02 BC 08 47             add sp, #8; pop {r4}; pop {r1}; bx r1
//! ```
//!
//! At the hook the emulator inverts `r0`, which makes the ID differ from the
//! player's (inverting cannot give back the same value), so `IsOtherTrainer`
//! answers yes without looking at the name.
//!
//! This is only asked when experience is awarded; obedience reads the ID
//! itself, so the player's Pokémon do not start disobeying. Nothing is changed
//! in the Pokémon, so those already in the party are covered too.

use super::Signature;

/// The bytes of `IsTradedMon` up to its end; `None` is the wildcard of a
/// `BL`.
pub(crate) const SIGNATURE: Signature = Signature::new(
    &[
        Some(0x10),
        Some(0xB5),
        Some(0x82),
        Some(0xB0),
        Some(0x04),
        Some(0x1C),
        Some(0x07),
        Some(0x21),
        Some(0x6A),
        Some(0x46),
        None,
        None,
        None,
        None,
        Some(0x20),
        Some(0x1C),
        Some(0x01),
        Some(0x21),
        Some(0x00),
        Some(0x22),
        None,
        None,
        None,
        None,
        Some(0x69),
        Some(0x46),
        None,
        None,
        None,
        None,
        Some(0x00),
        Some(0x06),
        Some(0x00),
        Some(0x0E),
        Some(0x02),
        Some(0xB0),
        Some(0x10),
        Some(0xBC),
        Some(0x02),
        Some(0xBC),
        Some(0x08),
        Some(0x47),
    ],
    24,
);

/// The hook address in `IsTradedMon`, if `rom` is a recognised FireRed
/// or LeafGreen cartridge.
pub fn find(rom: &[u8]) -> Option<u32> {
    SIGNATURE.find(rom)
}

/// An OT ID that is not `ot_id`, hence not the player's if it was.
pub fn other_ot_id(ot_id: u32) -> u32 {
    !ot_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_place_follows_the_rules_for_one_place() {
        crate::cheats::gen3::test_support::assert_single_match_rules(find, &SIGNATURE);
    }

    #[test]
    fn the_signature_decodes_to_the_documented_instructions() {
        let halfword = |index: usize| {
            u16::from_le_bytes([
                SIGNATURE.bytes[index].unwrap(),
                SIGNATURE.bytes[index + 1].unwrap(),
            ])
        };
        assert_eq!(halfword(0), 0xB510, "push {{r4, lr}}");
        assert_eq!(halfword(2), 0xB082, "sub sp, #8");
        assert_eq!(halfword(8), 0x466A, "mov r2, sp");
        assert_eq!(halfword(24), 0x4669, "mov r1, sp");
        assert_eq!(halfword(40), 0x4708, "bx r1");
    }

    #[test]
    fn the_replacement_id_never_equals_the_original() {
        for id in [0, 1, 0xDEAD_BEEF, u32::MAX, 0x8000_0000] {
            assert_ne!(other_ot_id(id), id);
            assert_eq!(other_ot_id(other_ot_id(id)), id);
        }
    }
}
