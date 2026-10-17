//! The 100% catch rate cheat: every thrown ball catches.
//!
//! **Mechanism: replace a value in the middle of a routine.** The emulator
//! stops at one instruction inside the ball-throw battle script and sets the
//! register holding the catch odds (`r6 = 0xFF`) (see `Emulator::step`).
//!
//! # The catch check
//!
//! A thrown ball computes `odds` from the target's catch rate, the ball and
//! the target's HP and status, then tests `if (odds > 254)`: above 254 the
//! Pokémon is caught on the spot, otherwise a shake roll decides. In Thumb the
//! test follows the attempt-counter bookkeeping and `odds` lives in `r6`:
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
//! The branch offset (`??`) is a wildcard and the hook is the `cmp r6, #0xFE`,
//! 8 bytes into the match; with `r6` set to 0xFF the `bls` is not taken.
//!
//! Every ball goes through this test (the Safari Zone's ball, the Master
//! Ball); trainers' Pokémon and the scripted tutorial catch never reach it.
//!
//! # How the place was found
//!
//! By recording the addresses executed during a real ball throw and keeping
//! the `cmp rN, #0xFE` instructions; exactly one sat in front of a shake-roll
//! branch. A new build is checked by throwing a ball at a high-level Pokémon
//! with the cheat off and on.

use super::Signature;

/// The bytes around the catch check (see the module documentation); `None` is
/// a wildcard for the branch offset, which is the same in the builds examined
/// but is not what identifies the code. The hook is the `cmp r6, #0xFE`, 8
/// bytes in.
pub(crate) const SIGNATURE: Signature = Signature::new(
    &[
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
    ],
    8,
);

/// The address of the `cmp r6, #0xFE` in `rom`, if it is a recognised FireRed
/// or LeafGreen cartridge.
pub fn find(rom: &[u8]) -> Option<u32> {
    SIGNATURE.find(rom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::gen3::test_support::{assert_single_match_rules, rom_with};
    use crate::cheats::gen3::wild_species;

    #[test]
    fn the_catch_check_follows_the_rules_for_one_place() {
        assert_single_match_rules(find, &SIGNATURE);
    }

    #[test]
    fn the_branch_offset_is_a_wildcard() {
        // Another build may differ in it.
        for branch in [0x2F, 0x31] {
            let mut rom = rom_with(b"BPRE", SIGNATURE.bytes, &[0x400]);
            rom[0x400 + 10] = branch;
            assert_eq!(find(&rom), Some(0x0800_0408));
        }
    }

    #[test]
    fn the_two_cheats_do_not_find_each_others_code() {
        let wild = rom_with(b"BPRE", wild_species::SIGNATURE.bytes, &[0x400]);
        assert_eq!(find(&wild), None);
        let catch = rom_with(b"BPRE", SIGNATURE.bytes, &[0x400]);
        assert_eq!(wild_species::find(&catch), None);
    }
}
