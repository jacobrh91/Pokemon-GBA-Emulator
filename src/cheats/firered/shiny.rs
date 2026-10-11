//! The all-Pokémon-are-shiny cheat: every Pokémon the game creates is shiny.
//!
//! **Mechanism: change values in the middle of a routine.** Like
//! [`super::catch_rate`], but what is changed is the Pokémon being built: the
//! emulator stops when the CPU is about to run one instruction inside
//! `CreateBoxMon`, where both the *personality value* and the *OT ID* are
//! known, and rewrites the personality (see `Emulator::step`).
//!
//! # What makes a Pokémon shiny
//!
//! In generation III a Pokémon is shiny when
//!
//! ```text
//! (OT_ID_high ^ OT_ID_low ^ PID_high ^ PID_low) < 8
//! ```
//!
//! where the OT ID is the 32-bit trainer ID (visible ID in the low half,
//! secret ID in the high half) of its original trainer and the PID is its
//! 32-bit personality value. Nothing else is stored: shininess is not a flag,
//! so a Pokémon is made shiny by choosing its PID (or its OT ID) to satisfy
//! the test. The game itself does the opposite for opponents' Pokémon, which
//! redraw their OT ID until the sum is 8 or more.
//!
//! # Where
//!
//! Every Pokémon, wild or not, party or box, goes through `CreateBoxMon`. It
//! draws a PID (or takes a fixed one), then picks the OT ID (the player's, a
//! preset one, or a random non-shiny one), and then stores both. The three OT
//! branches join at one instruction, which is the hook:
//!
//! ```text
//! 10 7B   ldrb r0, [r2, #0xC]   ┐
//! 00 04   lsls r0, r0, #16      │
//! 01 43   orrs r1, r0           │  the player's trainer ID is being
//! 50 7B   ldrb r0, [r2, #0xD]   │  assembled into r1
//! 00 06   lsls r0, r0, #24      │
//! 01 43   orrs r1, r0           ┘
//! 06 91   str  r1, [sp, #0x18]     OT ID is a local at sp+0x18
//! 38 1C   adds r0, r7, #0          <- the hook: r7 = the BoxPokemon, the PID
//! 01 21   movs r1, #1                 is a local at sp+0x14 and was stored in
//! 4A 46   mov  r2, sb                 the BoxPokemon already; the OT ID is
//!                                     stored next
//! ```
//!
//! The 20 bytes contain no `BL` and no PC-relative load, so they are the same
//! in every build examined; as for the other scans, the ROM is used only when
//! there is exactly one aligned match in a `BPR?` cartridge (see [`super`]).
//!
//! # What is changed
//!
//! The PID is rewritten in both places it lives (the local, which the rest of
//! `CreateBoxMon` reads, and the first word of the BoxPokemon). Nothing in
//! the BoxPokemon is encrypted yet: that starts with the next field written.
//! The new PID keeps everything the game derives from it that a player can
//! see, where possible:
//!
//! * the **low byte** (gender, and with bit 0 the ability slot),
//! * the **nature** (`PID % 25`),
//!
//! and only bits 8..16 and the upper half change (see [`shiny_personality`]).
//! A Pokémon that is already shiny is left alone. **Unown is skipped**: its
//! letter is made of the low two bits of each PID byte and the game chooses
//! the letter first, so no shiny PID can keep it.
//!
//! Because the OT ID is not touched, the player's own Pokémon stay the
//! player's (they would otherwise count as traded and disobey).

use super::find_in_firered;

/// The bytes around the join of the OT-ID branches; see the module
/// documentation.
pub(crate) const SIGNATURE: [u8; 20] = [
    0x10, 0x7B, 0x00, 0x04, 0x01, 0x43, 0x50, 0x7B, 0x00, 0x06, 0x01, 0x43, 0x06, 0x91, 0x38, 0x1C,
    0x01, 0x21, 0x4A, 0x46,
];

/// Offset of the `adds r0, r7, #0` within the signature.
const HOOK_OFFSET: usize = 14;

/// Stack offsets, relative to `sp` at the hook, of `CreateBoxMon`'s locals.
pub(crate) const SPECIES_OFFSET: u32 = 0x0E;
pub(crate) const PERSONALITY_OFFSET: u32 = 0x14;
pub(crate) const OT_ID_OFFSET: u32 = 0x18;

/// Register holding the `BoxPokemon` pointer at the hook.
pub(crate) const BOX_MON_REGISTER: usize = 7;

/// Species number of Unown, which is left alone (see the module docs).
pub(crate) const UNOWN: u16 = 201;

/// The shiny test's bound: a sum below this is shiny.
const SHINY_ODDS: u32 = 8;

/// The bus address of the instruction after which the OT ID is stored, if
/// `rom` is a FireRed cartridge and the place can be identified
/// unambiguously.
pub fn find(rom: &[u8]) -> Option<u32> {
    let pattern: Vec<Option<u8>> = SIGNATURE.iter().map(|byte| Some(*byte)).collect();
    find_in_firered(rom, &pattern, HOOK_OFFSET)
}

/// Whether the Pokémon with this personality value and OT ID is among the
/// one in `one_in` that the cheat makes shiny. The decision is a hash of the
/// two values, which the game drew at random, so it is evenly spread, needs
/// no random numbers of the emulator's own (and none of the game's, which
/// would change what the game does next), and gives the same answer for the
/// same Pokémon. `one_in` of 1 (or 0) is every Pokémon.
pub fn chosen(personality: u32, ot_id: u32, one_in: u32) -> bool {
    if one_in <= 1 {
        return true;
    }
    let mut x = personality ^ ot_id.rotate_left(16);
    x = x.wrapping_add(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85EB_CA6B);
    x = (x ^ (x >> 13)).wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    x.is_multiple_of(one_in)
}

/// Whether a Pokémon with this personality value and OT ID is shiny.
pub fn is_shiny(personality: u32, ot_id: u32) -> bool {
    shiny_sum(personality, ot_id) < SHINY_ODDS
}

fn shiny_sum(personality: u32, ot_id: u32) -> u32 {
    let folded = personality ^ ot_id;
    (folded >> 16) ^ (folded & 0xFFFF)
}

/// A personality value that is shiny for `ot_id` and agrees with `personality`
/// in its low byte and in `% 25` (so gender, ability slot and nature are
/// unchanged), or `personality` itself if it is already shiny.
///
/// With the low byte fixed, the high half is determined by bits 8..16 of the
/// low half and the 3-bit sum, which leaves 2048 candidates; about one in 25
/// has the right nature. Candidates are tried nearest first, so a change is
/// as small as possible. If none matches (it cannot happen for any value
/// tried, but the contract should not depend on that), the nature is allowed
/// to change.
pub fn shiny_personality(personality: u32, ot_id: u32) -> u32 {
    if is_shiny(personality, ot_id) {
        return personality;
    }
    let low_byte = personality & 0xFF;
    let middle = (personality >> 8) & 0xFF;
    let nature = personality % 25;
    // `ot_id` folded to 16 bits: the high half of a candidate is
    // `low ^ folded ^ sum` for a sum in 0..8.
    let folded = (ot_id >> 16) ^ (ot_id & 0xFFFF);
    let mut fallback = None;
    for step in 0..256u32 {
        let middle = (middle + step) & 0xFF;
        let low = (middle << 8) | low_byte;
        for sum in 0..SHINY_ODDS {
            let high = low ^ folded ^ sum;
            let candidate = (high << 16) | low;
            if candidate % 25 == nature {
                return candidate;
            }
            fallback.get_or_insert(candidate);
        }
    }
    fallback.unwrap_or(personality)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chance_picks_about_one_in_n_and_one_is_every_pokemon() {
        let mut seed = 0xCAFE_F00D_u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed
        };
        let samples: Vec<(u32, u32)> = (0..100_000).map(|_| (next(), next())).collect();
        for one_in in [1u32, 2, 10, 100] {
            let chosen = samples
                .iter()
                .filter(|(personality, ot_id)| chosen(*personality, *ot_id, one_in))
                .count();
            let expected = samples.len() as f64 / f64::from(one_in);
            let spread = expected.sqrt() * 5.0 + 1.0;
            assert!(
                (chosen as f64 - expected).abs() <= spread,
                "1/{one_in}: {chosen} chosen, expected about {expected}"
            );
        }
        assert!(chosen(1, 2, 0) && chosen(1, 2, 1), "0 and 1 mean all");
        // The same Pokémon always gets the same answer.
        assert_eq!(chosen(0x1234, 0x5678, 7), chosen(0x1234, 0x5678, 7));
    }

    #[test]
    fn a_shiny_personality_is_shiny_and_keeps_gender_ability_and_nature() {
        let mut seed = 0x1234_5678_u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed
        };
        for _ in 0..5000 {
            let (personality, ot_id) = (next(), next());
            let shiny = shiny_personality(personality, ot_id);
            assert!(is_shiny(shiny, ot_id), "{personality:08X} {ot_id:08X}");
            assert_eq!(shiny & 0xFF, personality & 0xFF, "gender and ability");
            assert_eq!(shiny % 25, personality % 25, "nature");
        }
    }

    #[test]
    fn an_already_shiny_personality_is_left_alone() {
        let ot_id = 0xBEEF_1234;
        // PID halves xor to the OT halves, sum 3.
        let personality = ((0xBEEF ^ 0x1234 ^ 0x00AA ^ 3) << 16) | 0x00AA;
        assert!(is_shiny(personality, ot_id));
        assert_eq!(shiny_personality(personality, ot_id), personality);
    }

    #[test]
    fn the_shiny_test_is_the_generation_iii_rule() {
        assert!(is_shiny(0x0000_0000, 0x0000_0007));
        assert!(!is_shiny(0x0000_0000, 0x0000_0008));
        assert!(is_shiny(0x1234_5678, 0x1234_5678 ^ 0x0000_0003));
    }

    /// A header-sized prefix with the signature at each offset.
    fn rom(game_code: &[u8; 4], offsets: &[usize]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x1000];
        rom[0xAC..0xB0].copy_from_slice(game_code);
        for &offset in offsets {
            rom[offset..offset + SIGNATURE.len()].copy_from_slice(&SIGNATURE);
        }
        rom
    }

    #[test]
    fn the_hook_is_the_instruction_after_the_ot_id_is_assembled() {
        assert_eq!(find(&rom(b"BPRE", &[0x400])), Some(0x0800_040E));
        assert_eq!(find(&rom(b"BPRJ", &[0x7F0])), Some(0x0800_07FE));
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
    fn the_signature_decodes_to_the_documented_instructions() {
        let halfwords: Vec<u16> = SIGNATURE
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            halfwords,
            [
                0x7B10, 0x0400, 0x4301, 0x7B50, 0x0600, 0x4301, 0x9106, 0x1C38, 0x2101, 0x464A
            ]
        );
    }
}
