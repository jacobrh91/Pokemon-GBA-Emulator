//! Whether a battle is on, read from the game's `gMain` struct. The fast-battles
//! option uses it to run unthrottled while a battle lasts.
//!
//! `gMain` holds the game's main callbacks and, near its end, the bit field
//! `inBattle` (bit 1 of the byte at offset `0x439`, set from the start of the
//! battle's set-up until it is over). The struct sits at a fixed address in
//! RAM, which depends on the game:
//!
//! | Game | `gMain` |
//! |---|---|
//! | FireRed, LeafGreen (English; v1.0 and v1.1 checked: the main callbacks there point into the ROM) | `0x030030F0` |
//! | Emerald (USA, Europe) | `0x030022C0` |
//!
//! The Japanese FireRed keeps it elsewhere, so it is not covered. The flag was
//! checked on Emerald in a save state taken in a battle; for FireRed and
//! LeafGreen it is the same struct, but no battle state was available.

/// Offset of the byte holding `inBattle` inside `gMain`.
const IN_BATTLE_BYTE: u32 = 0x439;
/// `inBattle`'s bit in that byte.
const IN_BATTLE_BIT: u8 = 1 << 1;

/// The address of the byte that holds the in-battle bit, for a cartridge whose
/// header game code (`0xAC..0xB0`) is a covered one.
pub fn in_battle_byte(game_code: &[u8]) -> Option<u32> {
    let main = match game_code {
        b"BPRE" | b"BPGE" => 0x0300_30F0,
        b"BPEE" => 0x0300_22C0,
        _ => return None,
    };
    Some(main + IN_BATTLE_BYTE)
}

/// Whether the byte read from [`in_battle_byte`] says a battle is on.
pub fn is_in_battle(byte: u8) -> bool {
    byte & IN_BATTLE_BIT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_checked_builds_are_covered() {
        assert_eq!(in_battle_byte(b"BPEE"), Some(0x0300_26F9));
        assert_eq!(in_battle_byte(b"BPRE"), Some(0x0300_3529));
        assert_eq!(in_battle_byte(b"BPGE"), Some(0x0300_3529));
        assert_eq!(in_battle_byte(b"BPRJ"), None);
        assert_eq!(in_battle_byte(b"AXVE"), None);
    }

    #[test]
    fn the_bit_is_the_second_one() {
        assert!(is_in_battle(0x02));
        assert!(is_in_battle(0x03));
        assert!(!is_in_battle(0x01));
        assert!(!is_in_battle(0));
    }
}
