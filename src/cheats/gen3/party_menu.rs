//! The party menu's list of actions for the first Pokémon includes the HMs it
//! does not know, so they can be chosen from the START menu (Flash in a dark
//! cave, Fly to a town, Dive). Part of the always-on "first Pokémon knows every
//! HM" cheat (see [`super::field_moves`], which covers the game's own checks).
//!
//! **Mechanism: change a list the game has just built.** When a Pokémon is
//! selected in the party menu, `SetPartyMonFieldSelectionActions` builds the
//! menu: SUMMARY, then one entry for each field move among the Pokémon's four
//! moves, then SWITCH, ITEM (or MAIL) and CANCEL. Each entry is a byte in a list
//! of eight (`actions`) and a count (`numActions`), both inside a structure the
//! game allocates. Two hooks:
//!
//! * **Capture**, at the first `bl AppendToList` (the one that adds SUMMARY):
//!   `r0` and `r1` are the addresses of the list and of the count, and `r10`
//!   holds which party slot was selected. They are kept if it is slot 0.
//! * **Append**, right after the loops over the Pokémon's moves, before SWITCH
//!   is added: the emulator adds the field-move entries for the HMs that are
//!   not already there. [`extra_actions`] decides which.
//!
//! The list holds eight entries and the game adds up to three more after the
//! second hook, so at most five (SUMMARY and the field moves) may be in it at
//! that point; HMs are added only while there is room, in the order Flash, Fly,
//! Dive, Cut, Surf, Strength, Rock Smash, Waterfall (the ones that cannot be
//! used any other way first: the others work from the overworld through
//! [`super::field_moves`]).
//!
//! Choosing the entry runs the game's own code for the move, which does not
//! check that the Pokémon knows it (it needs the badge, as ever).
//!
//! # The signatures
//!
//! The code is the same in FireRed, LeafGreen and Emerald except for the count
//! of field moves in the loop (12 against 14) and the number of the first
//! field-move entry (0x12 against 0x13), which is what tells the games apart:
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

    /// The indexes (in the game's table of field moves) of the HMs, in the
    /// order they are offered.
    fn hm_indexes(self) -> &'static [u8] {
        match self {
            // Flash, Cut, Fly, Strength, Surf, Rock Smash, Waterfall.
            Self::FireRedLeafGreen => &[0, 2, 1, 4, 3, 5, 6],
            // Cut, Flash, Rock Smash, Strength, Surf, Fly, Dive, Waterfall.
            Self::Emerald => &[1, 5, 6, 0, 4, 3, 2, 7],
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
    &[
        None,
        Some(0x4A),
        Some(0x11),
        Some(0x68),
        Some(0x00),
        Some(0x20),
        Some(0xC8),
        Some(0x75),
        Some(0x11),
        Some(0x68),
        Some(0x08),
        Some(0x1C),
        Some(0x0F),
        Some(0x30),
        Some(0x17),
        Some(0x31),
        Some(0x00),
        Some(0x22),
        None,
        None,
        None,
        None,
    ],
    18,
);

/// The end of the loop over the field moves; `count` is the number of them in
/// the game's table (the `cmp r0, #count`).
const fn append_signature(count: u8) -> [Option<u8>; 26] {
    [
        Some(0x60),
        Some(0x1C),
        Some(0x00),
        Some(0x06),
        Some(0x04),
        Some(0x0E),
        Some(0x60),
        Some(0x00),
        Some(0x40),
        Some(0x19),
        Some(0x00),
        Some(0x88),
        Some(count),
        Some(0x28),
        None,
        Some(0xD1),
        Some(0x41),
        Some(0x46),
        Some(0x08),
        Some(0x06),
        Some(0x07),
        Some(0x0E),
        Some(0x03),
        Some(0x2F),
        None,
        Some(0xD9),
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
/// and the field moves of the Pokémon): the HMs it does not already list, in
/// the order offered, as many as fit.
pub fn extra_actions(layout: Layout, present: &[u8]) -> Vec<u8> {
    let first = layout.first_field_move_action();
    let room = (LIST_CAPACITY - ADDED_AFTER).saturating_sub(present.len());
    layout
        .hm_indexes()
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
        // Fly (0x14), Cut (0x13), Surf (0x16).
        assert_eq!(
            extra_actions(Layout::FireRedLeafGreen, &[0]),
            vec![0x12, 0x14, 0x13, 0x16]
        );
        // Emerald: Flash (0x14), Fly (0x18), Dive (0x19), Cut (0x13).
        assert_eq!(
            extra_actions(Layout::Emerald, &[0]),
            vec![0x14, 0x18, 0x19, 0x13]
        );
    }

    #[test]
    fn what_the_pokemon_already_lists_is_not_repeated_and_room_is_respected() {
        // It knows Cut: SUMMARY and Cut (0x13) are there; Flash, Fly and Surf
        // fill the three places left.
        assert_eq!(
            extra_actions(Layout::FireRedLeafGreen, &[0, 0x13]),
            vec![0x12, 0x14, 0x16]
        );
        // Four field moves of its own leave no room at all.
        assert!(extra_actions(Layout::FireRedLeafGreen, &[0, 0x13, 0x15, 0x17, 0x18]).is_empty());
        // Entries that are not field moves (Cancel...) never count as HMs.
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
