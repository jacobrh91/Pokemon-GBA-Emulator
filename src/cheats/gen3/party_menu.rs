//! Lists field moves the first Pokémon does not know in its party-menu actions,
//! so Flash, Fly, Dig, Teleport and the HMs can be chosen from the START menu.
//! Part of the always-on "first Pokémon knows every HM" cheat (see
//! [`super::field_moves`], which covers the game's own checks).
//!
//! **Mechanism: change a list the game has just built.** When a Pokémon is
//! selected, `SetPartyMonFieldSelectionActions` builds the menu: SUMMARY, one
//! entry per field move among the Pokémon's four moves, then SWITCH, ITEM (or
//! MAIL) and CANCEL. The entries are bytes in a list of eight plus a count, in a
//! structure the game allocates. Two hooks:
//!
//! * **Capture**, at the first `bl AppendToList` (the one that adds SUMMARY):
//!   `r0` and `r1` are the addresses of the list and count, `r10` the selected
//!   slot. They are kept for slot 0.
//! * **Append**, after the loops over the moves, before SWITCH is added: the
//!   emulator adds entries for the HMs not already listed ([`extra_actions`]).
//!
//! The game adds up to three more entries after the second hook, so at most five
//! may be in the list then; entries are added while there is room, in the order
//! Flash, Fly, Dig, Teleport, Dive (Emerald), Cut, Surf, Strength, Rock Smash,
//! Waterfall. Those that only work from this menu come first; the rest also work
//! from the overworld through [`super::field_moves`]. Choosing an entry runs the
//! game's own code for the move, which does not check that the Pokémon knows it
//! (HMs still need their badge; Dig and Teleport need none).
//!
//! # The signatures
//!
//! FireRed, LeafGreen and Emerald differ only in the count of field moves in the
//! loop (12 against 14) and the number of the first field-move entry (0x12
//! against 0x13), which tells the games apart:
//!
//! ```text
//! capture:  ldr r2, [pc, #..] ; ldr r1, [r2] ; movs r0, #0 ; strb r0, [r1, #0x17]
//!           ldr r1, [r2] ; adds r0, r1, #0 ; adds r0, #0xF ; adds r1, #0x17
//!           movs r2, #0 ; bl AppendToList                <- the hook
//! append:   (end of the loop over the field moves)
//!           adds r0, r4, #1 ; lsls/lsrs r4 ; lsls r0, r4, #1 ; adds r0, r0, r5
//!           ldrh r0, [r0] ; cmp r0, #12|14 ; bne loop
//!           mov r1, r8 ; lsls/lsrs r7 ; cmp r7, #3 ; bls loop
//!           ...                                            <- the hook: next instruction
//! ```

use super::Signature;

/// Which game family's party menu this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    FireRedLeafGreen,
    Emerald,
}

impl Layout {
    /// The word used for it in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Self::FireRedLeafGreen => "frlg",
            Self::Emerald => "emerald",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "frlg" => Some(Self::FireRedLeafGreen),
            "emerald" => Some(Self::Emerald),
            _ => None,
        }
    }

    /// The action number of the first field move (`MENU_FIELD_MOVES`); the
    /// entry for field move `j` of the game's table is this plus `j`.
    pub fn first_field_move_action(self) -> u8 {
        match self {
            Self::FireRedLeafGreen => 0x12,
            Self::Emerald => 0x13,
        }
    }

    /// The indexes (in the game's table of field moves) of the moves offered,
    /// in the order offered. The tables are: FireRed and LeafGreen: Flash 0,
    /// Cut 1, Fly 2, Strength 3, Surf 4, Rock Smash 5, Waterfall 6, Teleport 7,
    /// Dig 8; Emerald: Cut 0, Flash 1, Rock Smash 2, Strength 3, Surf 4, Fly 5,
    /// Dive 6, Waterfall 7, Teleport 8, Dig 9.
    fn offered_indexes(self) -> &'static [u8] {
        match self {
            // Flash, Fly, Dig, Teleport, Cut, Surf, Strength, Rock Smash,
            // Waterfall.
            Self::FireRedLeafGreen => &[0, 2, 8, 7, 1, 4, 3, 5, 6],
            // Flash, Fly, Dig, Teleport, Dive, Cut, Surf, Strength, Rock Smash,
            // Waterfall.
            Self::Emerald => &[1, 5, 9, 8, 6, 0, 4, 3, 2, 7],
        }
    }
}

/// Where the two hooks are and which layout the game has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyMenu {
    /// The first `bl AppendToList`, where the list and count are captured.
    pub capture: u32,
    /// The instruction after the loops over the moves.
    pub append: u32,
    pub layout: Layout,
}

/// How many entries the game's list holds, and how many it adds after the
/// append hook (SWITCH, ITEM or MAIL, CANCEL).
const LIST_CAPACITY: usize = 8;
const ADDED_AFTER: usize = 3;

/// The capture hook: its `bl AppendToList`. `None` is a wildcard.
pub(crate) const CAPTURE: Signature = Signature::new(
    &pattern![
        _, 0x4A, 0x11, 0x68, 0x00, 0x20, 0xC8, 0x75, 0x11, 0x68, 0x08, 0x1C, 0x0F, 0x30, 0x17,
        0x31, 0x00, 0x22, _, _, _, _,
    ],
    18,
);

/// The end of the loop over the field moves; `count` is the number of them in
/// the game's table (the `cmp r0, #count`).
const fn append_signature(count: u8) -> [Option<u8>; 26] {
    pattern![
        0x60, 0x1C, 0x00, 0x06, 0x04, 0x0E, 0x60, 0x00, 0x40, 0x19, 0x00, 0x88, count, 0x28, _,
        0xD1, 0x41, 0x46, 0x08, 0x06, 0x07, 0x0E, 0x03, 0x2F, _, 0xD9,
    ]
}

/// The hook is the instruction after the `bls`, 26 bytes in.
const APPEND_HOOK: usize = 26;

pub(crate) const APPEND_FRLG: Signature = Signature::new(&append_signature(12), APPEND_HOOK);
pub(crate) const APPEND_EMERALD: Signature = Signature::new(&append_signature(14), APPEND_HOOK);

/// The party menu's hooks in `rom`, if it is a recognised game and both are
/// found once.
pub fn find(rom: &[u8]) -> Option<PartyMenu> {
    let capture = CAPTURE.find(rom)?;
    let frlg = APPEND_FRLG.find_all(rom);
    let emerald = APPEND_EMERALD.find_all(rom);
    let (append, layout) = match (frlg.as_slice(), emerald.as_slice()) {
        ([append], []) => (*append, Layout::FireRedLeafGreen),
        ([], [append]) => (*append, Layout::Emerald),
        _ => return None,
    };
    Some(PartyMenu {
        capture,
        append,
        layout,
    })
}

/// The entries to add to a list that holds `present` (the game's own, SUMMARY
/// and the field moves of the Pokémon): the moves it does not already list, in
/// the order offered, as many as fit.
pub fn extra_actions(layout: Layout, present: &[u8]) -> Vec<u8> {
    let first = layout.first_field_move_action();
    let room = (LIST_CAPACITY - ADDED_AFTER).saturating_sub(present.len());
    layout
        .offered_indexes()
        .iter()
        .map(|index| first + index)
        .filter(|action| !present.contains(action))
        .take(room)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cheats::gen3::test_support::rom_with;

    /// An image with both parts of the menu code: the capture at 0x400, the
    /// append at 0x800 with the count of this layout.
    fn rom(game_code: &[u8; 4], count: u8) -> Vec<u8> {
        let mut rom = rom_with(game_code, CAPTURE.bytes, &[0x400]);
        let signature = append_signature(count);
        let second = rom_with(game_code, &signature, &[0x800]);
        rom[0x800..0x800 + signature.len()]
            .copy_from_slice(&second[0x800..0x800 + signature.len()]);
        rom
    }

    #[test]
    fn the_hooks_and_the_layout_are_found_for_each_game() {
        for (code, count, layout) in [
            (b"BPRE", 12, Layout::FireRedLeafGreen),
            (b"BPGE", 12, Layout::FireRedLeafGreen),
            (b"BPRJ", 12, Layout::FireRedLeafGreen),
            (b"BPEE", 14, Layout::Emerald),
        ] {
            assert_eq!(
                find(&rom(code, count)),
                Some(PartyMenu {
                    capture: 0x0800_0412,
                    append: 0x0800_081A,
                    layout,
                }),
                "{code:?}"
            );
        }
    }

    #[test]
    fn it_is_declined_if_anything_is_missing_ambiguous_or_another_game() {
        assert_eq!(find(&[]), None);
        assert_eq!(find(&rom(b"AXVE", 12)), None, "another game");
        assert_eq!(find(&rom(b"BPRE", 13)), None, "an unknown layout");
        assert_eq!(
            find(&rom_with(b"BPRE", CAPTURE.bytes, &[0x400])),
            None,
            "only the capture"
        );
        // Both layouts at once is ambiguous.
        let mut both = rom(b"BPRE", 12);
        let other = append_signature(14);
        both[0xC00..0xC00 + other.len()]
            .copy_from_slice(&rom_with(b"BPRE", &other, &[0xC00])[0xC00..0xC00 + other.len()]);
        assert_eq!(find(&both), None);
    }

    #[test]
    fn the_layouts_round_trip_through_their_keys() {
        for layout in [Layout::FireRedLeafGreen, Layout::Emerald] {
            assert_eq!(Layout::from_key(layout.key()), Some(layout));
        }
        assert_eq!(Layout::from_key("ruby"), None);
    }

    #[test]
    fn a_pokemon_with_no_field_moves_is_offered_flash_first() {
        // FireRed / LeafGreen: SUMMARY (0) only. Room for four: Flash (0x12),
        // Fly (0x14), Dig (0x12 + 8) and Teleport (0x12 + 7).
        assert_eq!(
            extra_actions(Layout::FireRedLeafGreen, &[0]),
            vec![0x12, 0x14, 0x1A, 0x19]
        );
        // Emerald: Flash (0x13 + 1), Fly (0x13 + 5), Dig (0x13 + 9) and
        // Teleport (0x13 + 8).
        assert_eq!(
            extra_actions(Layout::Emerald, &[0]),
            vec![0x14, 0x18, 0x1C, 0x1B]
        );
    }

    #[test]
    fn what_the_pokemon_already_lists_is_not_repeated_and_room_is_respected() {
        // It knows Cut: SUMMARY and Cut (0x13) are there; Flash, Fly and Dig
        // fill the three places left.
        assert_eq!(
            extra_actions(Layout::FireRedLeafGreen, &[0, 0x13]),
            vec![0x12, 0x14, 0x1A]
        );
        // It knows Dig itself: it is not offered twice, Teleport takes its place.
        assert_eq!(
            extra_actions(Layout::FireRedLeafGreen, &[0, 0x1A]),
            vec![0x12, 0x14, 0x19]
        );
        // Four field moves of its own leave no room at all.
        assert!(extra_actions(Layout::FireRedLeafGreen, &[0, 0x13, 0x15, 0x17, 0x18]).is_empty());
        // Entries that are not field moves (Cancel...) never count.
        assert_eq!(extra_actions(Layout::Emerald, &[0, 2]).len(), 3);
    }

    #[test]
    fn the_list_never_exceeds_its_capacity_with_what_the_game_adds_after() {
        for layout in [Layout::FireRedLeafGreen, Layout::Emerald] {
            for own in 0..=4 {
                let mut present = vec![0u8];
                present.extend((0..own).map(|i| layout.first_field_move_action() + 7 + i));
                let extra = extra_actions(layout, &present);
                assert!(present.len() + extra.len() + ADDED_AFTER <= LIST_CAPACITY);
            }
        }
    }

    #[test]
    fn the_signatures_are_the_documented_code() {
        let halfword = |signature: &Signature, index: usize| {
            u16::from_le_bytes([
                signature.bytes[index].unwrap(),
                signature.bytes[index + 1].unwrap(),
            ])
        };
        assert_eq!(CAPTURE.bytes[1], Some(0x4A), "ldr r2, [pc, #..]");
        assert_eq!(halfword(&CAPTURE, 16), 0x2200, "movs r2, #0");
        assert_eq!(APPEND_FRLG.bytes[12], Some(12));
        assert_eq!(APPEND_EMERALD.bytes[12], Some(14));
        assert_eq!(halfword(&APPEND_FRLG, 22), 0x2F03, "cmp r7, #3");
    }
}
