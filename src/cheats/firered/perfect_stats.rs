//! The perfect-stats cheat: Pokémon the game creates get the maximum value
//! (31) in every IV, for the wild Pokémon, the opponents' Pokémon or both.
//!
//! **Mechanism: change a value in the middle of a routine**, at the same place
//! as [`super::shiny`] (the join point of `CreateBoxMon` after the OT ID is
//! known), so the scan is shared and finds nothing new.
//!
//! # How IVs are made
//!
//! `CreateBoxMon(box_mon, species, level, fixed_iv, …)` keeps its `fixed_iv`
//! argument in a local byte at `sp+0x11` and later tests it:
//!
//! ```text
//! 0803DC6C  mov  r2, sp
//! 0803DC6E  ldrb r0, [r2, #0x11]   ; fixed_iv
//! 0803DC70  cmp  r0, #0x1F
//! 0803DC72  bhi  random            ; 32 and above: draw each IV at random
//! ...                              ; otherwise every IV is fixed_iv
//! ```
//!
//! So storing 31 there before that test makes all six IVs 31, with the game's
//! own code doing the rest (the IVs are packed and encrypted with the other
//! fields; nothing is written by hand). Wild encounters pass 32 (random);
//! trainers' Pokémon pass a fixed value, which is 0 for the trainer checked
//! (a step up from the v1.1 grass state), so the cheat lifts those from zero.
//! Nature, gender and everything else keep their values.
//!
//! # Which Pokémon
//!
//! * **Wild**: those built while `CreateWildMon` is running. The emulator
//!   notes its return address (the link register) at its entry and the
//!   Pokémon is a wild one until the CPU reaches that address. This is the
//!   same set the wild-encounter override covers: grass, surfing, fishing,
//!   rock smash. (Scripted encounters, such as a static Snorlax, are built
//!   elsewhere and are not covered.)
//! * **Opponents**: those built with the OT-ID type `2`
//!   (`OT_ID_RANDOM_NO_SHINY`), which the game uses for trainers' parties
//!   only. At the hook the type is still in `r10`.

/// Stack offset, relative to `sp` at the hook, of the `fixed_iv` byte.
pub(crate) const FIXED_IV_OFFSET: u32 = 0x11;

/// The IV every stat gets.
pub(crate) const PERFECT_IV: u8 = 31;

/// Register holding the OT-ID type at the hook.
pub(crate) const OT_ID_TYPE_REGISTER: usize = 10;

/// OT-ID type used for trainers' Pokémon.
const OT_ID_RANDOM_NO_SHINY: u32 = 2;

/// Whether the Pokémon being built gets perfect IVs.
///
/// `wild_on` / `opponents_on` are the rows of the menu, `in_wild_mon` whether
/// `CreateWildMon` is running, `ot_id_type` the argument of `CreateBoxMon`.
pub fn applies(wild_on: bool, opponents_on: bool, in_wild_mon: bool, ot_id_type: u32) -> bool {
    (wild_on && in_wild_mon) || (opponents_on && ot_id_type == OT_ID_RANDOM_NO_SHINY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wild_pokemon_are_covered_only_while_a_wild_one_is_being_built() {
        assert!(applies(true, false, true, 0));
        assert!(!applies(true, false, false, 0), "a gift, an egg...");
        assert!(!applies(true, false, false, 2), "a trainer's, wild is off");
    }

    #[test]
    fn opponents_are_recognised_by_their_ot_id_type() {
        assert!(applies(false, true, false, 2));
        assert!(!applies(false, true, true, 0), "wild is not an opponent");
        assert!(!applies(false, true, false, 0));
        assert!(!applies(false, true, false, 1));
    }

    #[test]
    fn both_and_neither() {
        assert!(applies(true, true, true, 0));
        assert!(applies(true, true, false, 2));
        assert!(!applies(true, true, false, 0));
        assert!(!applies(false, false, true, 2));
    }
}
