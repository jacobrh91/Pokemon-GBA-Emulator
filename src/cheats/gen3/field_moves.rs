//! The first Pokémon of the party acts as if it knew every HM (Cut, Fly, Surf,
//! Strength, Flash, Rock Smash, Waterfall and Dive), without actually knowing
//! them. This cheat is **always on**, with no row to switch.
//!
//! **Mechanism: replace a value at the end of a routine.** Whenever the game
//! asks whether a Pokémon knows a move (the tree in front of the player, a
//! boulder, a rock to smash, the water at the edge of a shore, ...), it calls
//! one small function, `MonKnowsMove(mon, move)`, which looks through the four
//! move slots. The emulator stops at its return and, when the question is
//! about the first party slot and the move is an HM, makes the answer yes (see
//! `Emulator::step`). The scripts then go on exactly as if that Pokémon had
//! the move, including the "X used CUT!" that names it. The Pokémon's real
//! moves are never touched.
//!
//! This is what the game's own checks see. Choosing a field move from the
//! Pokémon's menu (the START menu's party list) reads the moves directly, so
//! it is not covered.
//!
//! # Finding it
//!
//! `MonKnowsMove(mon, move)` is a leaf loop over `GetMonData(mon,
//! MON_DATA_MOVE1 + i)`; the hook is its `pop {r4, r5, r6}`, where `r6` is the
//! Pokémon, `r5` the move and `r0` the answer:
//!
//! ```text
//! 70 B5        push {r4, r5, r6, lr}
//! 06 1C        adds r6, r0, #0        r6 = mon
//! 09 04  0D 0C lsls/lsrs r5, r1, #16  r5 = (u16) move
//! 00 24        movs r4, #0
//! 21 1C  0D 31 adds r1, r4, #0 ; adds r1, #0xD     MON_DATA_MOVE1 + i
//! 30 1C        adds r0, r6, #0
//! ?? ?? ?? ??  bl GetMonData
//! A8 42        cmp  r0, r5
//! ?? D1        bne  next
//! 01 20        movs r0, #1            knows it
//! ?? E0        b    done
//! 60 1C  00 06  04 0E   adds r0, r4, #1 ; lsls/lsrs r4, r0, #24   i++
//! 03 2C  ?? D9 cmp  r4, #3 ; bls loop
//! 00 20        movs r0, #0            does not
//! 70 BC        pop  {r4, r5, r6}      <- the hook
//! 02 BC  08 47 pop  {r1} ; bx r1
//! ```
//!
//! "The first slot" needs the address of the party array. Rather than keep a
//! table of RAM addresses (they differ between the English, Japanese and
//! Emerald builds), it is read from the game: the script command that checks
//! the party, `checkpartymove`, computes `&gPlayerParty[i]` from a constant it
//! loads from the ROM's literal pool, and that load is found by its
//! surrounding bytes ([`PARTY_LOAD`]) and decoded.

use super::{ROM_BASE, Signature};

/// `MonKnowsMove`; the hook is its `pop {r4, r5, r6}`. `None` is a wildcard
/// (the call, and the branch distances).
pub(crate) const MON_KNOWS_MOVE: Signature = Signature::new(
    &[
        Some(0x70),
        Some(0xB5),
        Some(0x06),
        Some(0x1C),
        Some(0x09),
        Some(0x04),
        Some(0x0D),
        Some(0x0C),
        Some(0x00),
        Some(0x24),
        Some(0x21),
        Some(0x1C),
        Some(0x0D),
        Some(0x31),
        Some(0x30),
        Some(0x1C),
        None,
        None,
        None,
        None,
        Some(0xA8),
        Some(0x42),
        None,
        Some(0xD1),
        Some(0x01),
        Some(0x20),
        None,
        Some(0xE0),
        Some(0x60),
        Some(0x1C),
        Some(0x00),
        Some(0x06),
        Some(0x04),
        Some(0x0E),
        Some(0x03),
        Some(0x2C),
        None,
        Some(0xD9),
        Some(0x00),
        Some(0x20),
        Some(0x70),
        Some(0xBC),
        Some(0x02),
        Some(0xBC),
        Some(0x08),
        Some(0x47),
    ],
    40,
);

/// The end of the loop of `checkpartymove`, which turns the slot number into
/// `&gPlayerParty[i]`: `movs r0, #100 ; adds r1, r6, #0 ; muls r1, r0, r1 ;
/// ldr r0, [pc, #..] ; adds r4, r1, r0` and the call that reads the species.
/// The hook is the PC-relative `ldr`, whose pool entry is the array's address.
pub(crate) const PARTY_LOAD: Signature = Signature::new(
    &[
        Some(0x64),
        Some(0x20),
        Some(0x31),
        Some(0x1C),
        Some(0x41),
        Some(0x43),
        None,
        Some(0x48),
        Some(0x0C),
        Some(0x18),
        Some(0x20),
        Some(0x1C),
        Some(0x0B),
        Some(0x21),
        Some(0x00),
        Some(0x22),
        None,
        None,
        None,
        None,
        Some(0x00),
        Some(0x04),
        Some(0x05),
        Some(0x0C),
        Some(0x00),
        Some(0x2D),
        None,
        Some(0xD1),
        Some(0x00),
        Some(0x20),
        Some(0xF0),
        Some(0xBC),
        Some(0x02),
        Some(0xBC),
        Some(0x08),
        Some(0x47),
    ],
    6,
);

/// What the cheat needs to know about a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMoves {
    /// The `pop {r4, r5, r6}` that ends `MonKnowsMove`.
    pub knows_move_return: u32,
    /// The address of the player's party array (`gPlayerParty`).
    pub party_base: u32,
}

/// The HMs of these games (national move numbers): Cut, Fly, Surf, Strength,
/// Waterfall, Flash, Rock Smash and Dive.
pub const HM_MOVES: [u16; 8] = [15, 19, 57, 70, 127, 148, 249, 291];

/// Size of one party member in memory.
const PARTY_MEMBER_SIZE: u32 = 100;

/// The cheat's locations in `rom`, if it is a recognised game in which both
/// are found.
pub fn find(rom: &[u8]) -> Option<FieldMoves> {
    let knows_move_return = MON_KNOWS_MOVE.find(rom)?;
    let load = PARTY_LOAD.find(rom)?;
    let party_base = pc_relative_load_value(rom, load)?;
    Some(FieldMoves {
        knows_move_return,
        party_base,
    })
}

/// The word that the Thumb `ldr rN, [pc, #imm]` at bus address `at` loads,
/// if that is one and the word is an address in the console's work RAM.
fn pc_relative_load_value(rom: &[u8], at: u32) -> Option<u32> {
    let offset = usize::try_from(at.checked_sub(ROM_BASE)?).ok()?;
    let pair = rom.get(offset..offset + 2)?;
    let instruction = u16::from_le_bytes([pair[0], pair[1]]);
    if instruction & 0xF800 != 0x4800 {
        return None;
    }
    // The base is the instruction's address plus 4, rounded down to a word.
    let pool = ((at + 4) & !3) + u32::from(instruction & 0xFF) * 4;
    let word = usize::try_from(pool.checked_sub(ROM_BASE)?).ok()?;
    let bytes = rom.get(word..word + 4)?;
    let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    // External work RAM, where the party lives.
    ((0x0200_0000..0x0204_0000).contains(&value) && value.is_multiple_of(4)).then_some(value)
}

/// Whether the game's question "does `mon` know `move`?" is to be answered yes
/// regardless: `mon` is the first slot of the party at `party_base` and `move`
/// is an HM.
pub fn acts_as_known(mon: u32, move_id: u32, party_base: u32) -> bool {
    mon == party_base && HM_MOVES.iter().any(|hm| u32::from(*hm) == move_id)
}

/// The slot of the party that `mon` is, if it is one of the six.
pub fn party_slot(mon: u32, party_base: u32) -> Option<u32> {
    let distance = mon.checked_sub(party_base)?;
    (distance % PARTY_MEMBER_SIZE == 0 && distance / PARTY_MEMBER_SIZE < 6)
        .then_some(distance / PARTY_MEMBER_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::gen3::test_support::rom_with;

    /// An image with both functions: `MonKnowsMove` at 0x400 and the party
    /// load at 0x800, whose pool (at 0x840) holds `party`.
    fn rom(game_code: &[u8; 4], party: u32) -> Vec<u8> {
        let mut rom = rom_with(game_code, MON_KNOWS_MOVE.bytes, &[0x400]);
        let second = rom_with(game_code, PARTY_LOAD.bytes, &[0x800]);
        let len = PARTY_LOAD.bytes.len();
        rom[0x800..0x800 + len].copy_from_slice(&second[0x800..0x800 + len]);
        // `ldr r0, [pc, #imm]` at 0x806: the pool is 0x840.
        let at = 0x806u32;
        let imm = (0x840 - ((at + 4) & !3)) / 4;
        rom[0x806] = imm as u8;
        rom[0x807] = 0x48;
        rom[0x840..0x844].copy_from_slice(&party.to_le_bytes());
        rom
    }

    #[test]
    fn the_return_and_the_party_array_are_found_in_every_game_of_the_family() {
        for code in [b"BPRE", b"BPRJ", b"BPGE", b"BPEE"] {
            assert_eq!(
                find(&rom(code, 0x0202_4284)),
                Some(FieldMoves {
                    knows_move_return: 0x0800_0428,
                    party_base: 0x0202_4284,
                }),
                "{code:?}"
            );
        }
    }

    #[test]
    fn it_is_declined_without_either_part_or_with_a_pool_that_is_not_a_ram_address() {
        assert_eq!(find(&[]), None);
        assert_eq!(find(&rom(b"AXVE", 0x0202_4284)), None, "another game");
        // Only one of the two functions.
        let only_first = rom_with(b"BPRE", MON_KNOWS_MOVE.bytes, &[0x400]);
        assert_eq!(find(&only_first), None);
        // A pool entry that is not an address in work RAM (or not aligned).
        assert_eq!(find(&rom(b"BPRE", 0x0800_1234)), None);
        assert_eq!(find(&rom(b"BPRE", 0x0202_4285)), None);
        assert_eq!(find(&rom(b"BPRE", 0)), None);
    }

    #[test]
    fn only_the_first_slot_and_only_hms_are_answered_yes() {
        let base = 0x0202_4284;
        for hm in HM_MOVES {
            assert!(acts_as_known(base, u32::from(hm), base), "{hm}");
        }
        assert!(!acts_as_known(base, 33, base), "Tackle is not an HM");
        assert!(!acts_as_known(base, 0, base));
        assert!(!acts_as_known(base + 100, 15, base), "the second slot");
        assert!(!acts_as_known(base - 100, 15, base));
        assert!(!acts_as_known(base + 1, 15, base), "inside the first slot");
    }

    #[test]
    fn the_hms_are_the_eight_field_moves() {
        // Cut, Fly, Surf, Strength, Waterfall, Flash, Rock Smash, Dive.
        assert_eq!(HM_MOVES, [15, 19, 57, 70, 127, 148, 249, 291]);
    }

    #[test]
    fn slots_are_one_hundred_bytes_apart_and_there_are_six() {
        let base = 0x0202_4284;
        assert_eq!(party_slot(base, base), Some(0));
        assert_eq!(party_slot(base + 500, base), Some(5));
        assert_eq!(party_slot(base + 600, base), None);
        assert_eq!(party_slot(base + 50, base), None);
        assert_eq!(party_slot(base - 100, base), None);
    }

    #[test]
    fn the_signatures_are_the_documented_code() {
        let halfword = |signature: &Signature, index: usize| {
            u16::from_le_bytes([
                signature.bytes[index].unwrap(),
                signature.bytes[index + 1].unwrap(),
            ])
        };
        assert_eq!(
            halfword(&MON_KNOWS_MOVE, 0),
            0xB570,
            "push {{r4, r5, r6, lr}}"
        );
        assert_eq!(halfword(&MON_KNOWS_MOVE, 40), 0xBC70, "pop {{r4, r5, r6}}");
        assert_eq!(halfword(&MON_KNOWS_MOVE, 20), 0x42A8, "cmp r0, r5");
        assert_eq!(halfword(&PARTY_LOAD, 0), 0x2064, "movs r0, #100");
        assert_eq!(PARTY_LOAD.bytes[7], Some(0x48), "ldr r0, [pc, #..]");
    }
}
