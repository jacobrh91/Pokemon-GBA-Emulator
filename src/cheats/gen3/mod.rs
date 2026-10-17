//! Built-in cheats for Pokémon FireRed, LeafGreen and Emerald. Unlike the raw
//! codes of a `.cheats` file (see [`super::codebreaker`]), which write
//! constants into RAM, these act on the game's own code: each finds a place in
//! the ROM by its instruction bytes (a *signature scan*) and the emulator
//! changes a register or a value when the CPU reaches that address.
//!
//! The cheats, by what they change (the ones marked *always on* have no row in
//! the menu):
//!
//! * [`wild_species`]: a function's arguments at its entry.
//! * [`catch_rate`], [`shiny`], [`experience`], [`field_poison`]: a value in
//!   the middle of a routine (the catch odds, a Pokémon's personality value, the
//!   OT ID compared with the player's, the ailment tested by the poison check;
//!   the last is *always on*). [`perfect_stats`] uses the same place as
//!   [`shiny`], for the IVs.
//! * [`encounters`], [`money`]: the value a function returns.
//! * [`tm`]: an argument just before a call (*always on*).
//! * [`field_moves`]: the answer of `MonKnowsMove` for the first party slot and
//!   an HM move (*always on*); [`party_menu`] adds field moves to that
//!   Pokémon's START-menu list of actions.
//!
//! This module holds what they share: recognising one of the cartridges, the
//! scan, and the table of builds that were checked.
//!
//! # How a scan works
//!
//! 1. The ROM must be a FireRed, LeafGreen or Emerald cartridge: game code
//!    `BPR?` (FireRed: `BPRE` English, `BPRJ` Japanese, …), `BPG?`
//!    (LeafGreen: `BPGE`, …) or `BPE?` (Emerald: `BPEE`, …) in the header at
//!    `0xAC`. FireRed and LeafGreen are the same game with a few data
//!    differences, and Emerald's code is almost the same: most hooks are the
//!    same bytes in all three. Where a game compiled the code differently, a
//!    cheat lists one signature for each variant (see each module), so a
//!    signature can only match the game it was written for.
//! 2. Every 2-byte-aligned offset is compared with the pattern; bytes that
//!    differ between builds (`BL` targets, pool offsets) are wildcards.
//! 3. There must be **exactly one** match (a cheat that has several places, such
//!    as [`tm`], states how many it accepts). With none, or the wrong number,
//!    nothing is hooked and the cheat reports itself unsupported rather than
//!    guessing.
//! 4. The hook address is `0x0800_0000` (where the ROM appears on the bus)
//!    plus the match offset plus the offset of the instruction the cheat
//!    cares about.
//!
//! The scan runs once, when the ROM is opened.
//!
//! # Known builds
//!
//! | Build | Game code | Version | CRC32 | `CreateWildMon` at | Catch check at | Checked |
//! |---|---|---|---|---|---|---|
//! | FireRed v1.0 (US) | `BPRE` | 0 | `7845FBDD` | `0x080829FC` | `0x0802D6BC` | encounter: Rattata → Dratini; catch: Lv100 Mewtwo caught first throw |
//! | FireRed v1.1 (US) | `BPRE` | 1 | `84EE4776` | `0x08082A10` | `0x0802D6D0` | encounter: Pidgey → Dratini; catch: Lv100 Mewtwo caught first throw |
//! | FireRed (Japan)   | `BPRJ` | 0 | `3B2056E9` | `0x080825E8` | `0x0802CE94` | encounter: Rattata → Dratini; catch: Lv100 Mewtwo caught first throw |
//! | LeafGreen v1.1 (US) | `BPGE` | 1 | | `0x080829E4` | `0x0802D6D0` | wild species, shiny, no encounters live; all hooks found |
//! | Emerald (USA, Europe) | `BPEE` | 0 | | `0x080B4E68` | `0x08056564` | all hooks found by disassembly; boots with every cheat on; **not yet checked in the game** |
//!
//! The shiny cheat's hook (inside `CreateBoxMon`) is at `0x0803DB92` (v1.0),
//! `0x0803DBA6` (v1.1) and `0x0803D2FE` (Japan); each was checked live from a
//! save state in the grass.
//!
//! "Checked" says how far each build was verified live: a unique match shows
//! the bytes are there, not that the code behaves the same.
//!
//! # Adding a build
//!
//! Run the emulator on the ROM with the cheat on; the terminal reports the
//! address hooked, or why none was. A build whose code was compiled
//! differently has no match and is declined; for one with rewritten code,
//! trace the game (see each cheat's notes) and add its own signature.

pub mod catch_rate;
pub mod encounters;
pub mod experience;
pub mod field_moves;
pub mod field_poison;
pub mod money;
pub mod party_menu;
pub mod perfect_stats;
pub mod shiny;
pub mod tm;
pub mod wild_species;

/// Where the ROM appears on the bus.
const ROM_BASE: u32 = 0x0800_0000;

/// Game-code prefixes of the games the cheats cover: FireRed (`BPRE`, `BPRJ`,
/// ...), LeafGreen (`BPGE`, ...) and Emerald (`BPEE`, ...). They are built from
/// the same game code, and mostly compile to the same bytes where the cheats
/// hook; where they do not, a cheat lists one signature for each variant.
const GAME_CODE_PREFIXES: [&[u8; 3]; 3] = [b"BPR", b"BPG", b"BPE"];

/// Whether the cartridge header's game code (`0xAC..0xB0`) is one of them.
fn is_gen3(game_code: &[u8]) -> bool {
    GAME_CODE_PREFIXES
        .iter()
        .any(|prefix| game_code.starts_with(*prefix))
}

/// A pattern of bytes that identifies a place in the game's code, and which
/// instruction of it a cheat hooks.
pub(crate) struct Signature {
    /// The bytes; `None` matches any byte (the target of a `BL`, a branch
    /// distance, ... which differ between builds without the code differing).
    pub bytes: &'static [Option<u8>],
    /// Offset within `bytes` of the hooked instruction.
    pub hook_offset: usize,
}

/// A pattern with no wildcards.
pub(crate) const fn exact<const N: usize>(bytes: [u8; N]) -> [Option<u8>; N] {
    let mut out = [None; N];
    let mut index = 0;
    while index < N {
        out[index] = Some(bytes[index]);
        index += 1;
    }
    out
}

impl Signature {
    pub(crate) const fn new(bytes: &'static [Option<u8>], hook_offset: usize) -> Self {
        Self { bytes, hook_offset }
    }

    /// The bus addresses of the hooked instruction at every halfword-aligned
    /// match in `rom` (Thumb instructions are halfwords, so a match elsewhere
    /// is meaningless), in ascending order. Empty unless `rom` is one of the
    /// supported games.
    pub(crate) fn find_all(&self, rom: &[u8]) -> Vec<u32> {
        let len = self.bytes.len();
        // The game code lives at 0xAC..0xB0 in the cartridge header.
        let Some(game_code) = rom.get(0xAC..0xB0) else {
            return Vec::new();
        };
        if !is_gen3(game_code) || rom.len() < len {
            return Vec::new();
        }
        // A cheap first-byte test keeps the scan of a 16 MiB ROM quick.
        let first = self.bytes.first().copied().flatten();
        (0..=rom.len() - len)
            .step_by(2)
            .filter(|&at| first.is_none_or(|first| rom[at] == first))
            .filter(|&at| {
                self.bytes
                    .iter()
                    .zip(&rom[at..at + len])
                    .all(|(want, byte)| want.is_none_or(|want| want == *byte))
            })
            .map(|at| ROM_BASE + (at + self.hook_offset) as u32)
            .collect()
    }

    /// The bus address of the hooked instruction, if the pattern occurs
    /// **exactly once**. With none, or several, the place is not identified
    /// and the cheat reports itself unsupported rather than guessing.
    pub(crate) fn find(&self, rom: &[u8]) -> Option<u32> {
        match self.find_all(rom).as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }
}

/// The hooked address of the one place that any of `variants` (the same place
/// as compiled in different games) matches **exactly once** in total.
pub(crate) fn find_variant(rom: &[u8], variants: &[&Signature]) -> Option<u32> {
    let mut found = variants.iter().flat_map(|variant| variant.find_all(rom));
    match (found.next(), found.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// What the modules' tests share.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A small ROM image of the given game code with `bytes` (wildcards
    /// filled with `0xAA`) placed at each of `offsets`.
    pub(crate) fn rom_with(
        game_code: &[u8; 4],
        bytes: &[Option<u8>],
        offsets: &[usize],
    ) -> Vec<u8> {
        let mut rom = vec![0u8; 0x1000];
        rom[0xAC..0xB0].copy_from_slice(game_code);
        for &offset in offsets {
            for (index, byte) in bytes.iter().enumerate() {
                rom[offset + index] = byte.unwrap_or(0xAA);
            }
        }
        rom
    }

    /// The rules every single-place cheat follows, checked for `find` and its
    /// `signature` (whose hook is `hook_offset` bytes into it): found once, at
    /// that offset, in FireRed and LeafGreen images; declined when absent,
    /// ambiguous, not halfword aligned, in another game, or in an image too
    /// small to be a cartridge.
    pub(crate) fn assert_single_match_rules(find: fn(&[u8]) -> Option<u32>, signature: &Signature) {
        let hook = signature.hook_offset as u32;
        let image = |code: &[u8; 4], offsets: &[usize]| rom_with(code, signature.bytes, offsets);
        // Found, in every game of the family, wherever it is.
        for (code, at) in [
            (b"BPRE", 0x400),
            (b"BPRJ", 0x7F0),
            (b"BPGE", 0x300),
            (b"BPEE", 0x500),
        ] {
            assert_eq!(
                find(&image(code, &[at])),
                Some(ROM_BASE + at as u32 + hook),
                "{code:?}"
            );
        }
        // Declined.
        assert_eq!(find(&image(b"BPRE", &[])), None, "absent");
        assert_eq!(find(&image(b"BPRE", &[0x400, 0x800])), None, "ambiguous");
        assert_eq!(find(&image(b"BPRE", &[0x401])), None, "unaligned");
        assert_eq!(
            find(&image(b"BPRE", &[0x400, 0x801])),
            Some(ROM_BASE + 0x400 + hook),
            "an unaligned copy does not make an aligned match ambiguous"
        );
        // Ruby and Sapphire are not covered (yet), nor is anything else.
        for code in [b"AXVE", b"AXPE", b"BPXE"] {
            assert_eq!(find(&image(code, &[0x400])), None, "{code:?}");
        }
        assert_eq!(find(&[]), None, "empty");
        assert_eq!(find(&[0; 8]), None, "no header");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATTERN: Signature = Signature::new(&[Some(7), None, Some(9)], 2);

    #[test]
    fn short_or_empty_input_does_not_panic() {
        assert_eq!(PATTERN.find(&[]), None);
        assert_eq!(PATTERN.find(&[0; 8]), None);
        assert!(PATTERN.find_all(&[1, 2]).is_empty());
        let open = Signature::new(&[None, Some(0)], 0);
        let mut small = vec![0u8; 0xB0];
        small[0xAC..0xB0].copy_from_slice(b"BPRE");
        // Nothing but aligned positions are tried, and nothing runs off the end.
        let found = open.find_all(&small);
        assert!(
            found
                .iter()
                .all(|address| (address - ROM_BASE).is_multiple_of(2))
        );
        assert!(
            found
                .iter()
                .all(|address| (address - ROM_BASE) as usize + 2 <= small.len())
        );
    }

    #[test]
    fn wildcards_match_any_byte_and_the_hook_offset_is_added() {
        let rom = test_support::rom_with(b"BPRE", &[Some(7), Some(99), Some(9)], &[0x400]);
        assert_eq!(PATTERN.find(&rom), Some(0x0800_0402));
        let at_start = Signature::new(&[Some(7), None, Some(9)], 0);
        assert_eq!(at_start.find(&rom), Some(0x0800_0400));
        // A wildcard first byte is allowed too.
        let leading = Signature::new(&[None, Some(99), Some(9)], 0);
        assert_eq!(leading.find(&rom), Some(0x0800_0400));
    }

    #[test]
    fn every_cheats_rules_for_one_place_are_the_same() {
        // Exercised on a small pattern here; each module runs the same checks
        // on its own signature.
        test_support::assert_single_match_rules(|rom| PATTERN.find(rom), &PATTERN);
    }

    #[test]
    fn exact_has_no_wildcards() {
        assert_eq!(exact([1, 2, 3]), [Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn find_all_lists_every_aligned_match_in_order() {
        let rom =
            test_support::rom_with(b"BPGE", &[Some(7), None, Some(9)], &[0x800, 0x300, 0x501]);
        assert_eq!(
            PATTERN.find_all(&rom),
            vec![0x0800_0302, 0x0800_0802],
            "ascending, the unaligned copy left out"
        );
        for code in [b"AXPE", b"AXVE", b"BPXE"] {
            let other = test_support::rom_with(code, &[Some(7), None, Some(9)], &[0x400]);
            assert!(PATTERN.find_all(&other).is_empty(), "{code:?}");
        }
    }

    /// Every hook address in each build whose ROM is under `rom/` (they are
    /// not in the repository); a missing ROM is skipped. The addresses were
    /// found by disassembling each ROM, and the ones marked in the table
    /// above were also checked in the game.
    #[test]
    fn every_known_build_has_every_hook_where_the_table_says() {
        struct Build {
            file: &'static str,
            wild: u32,
            catch: u32,
            shiny: u32,
            money: u32,
            traded: u32,
            encounter: u32,
            tm: &'static [u32],
            /// `MonKnowsMove`'s return and the party array.
            field_moves: (u32, u32),
            /// The party menu's capture and append hooks.
            party_menu: (u32, u32),
            /// The overworld poison check.
            field_poison: u32,
        }
        let builds = [
            Build {
                file: "Pokemon Fire Red/Pokemon Fire Red (v1.0).gba",
                wild: 0x0808_29FC,
                catch: 0x0802_D6BC,
                shiny: 0x0803_DB92,
                money: 0x0809_FD68,
                traded: 0x0804_4274,
                encounter: 0x0808_2BC6,
                tm: &[0x0812_4EAC, 0x0812_4F78, 0x0812_5C80],
                field_moves: (0x0812_5AE8, 0x0202_4284),
                party_menu: (0x0812_2A22, 0x0812_2A9A),
                field_poison: 0x080A_0640,
            },
            Build {
                file: "Pokemon Fire Red/Pokemon Fire Red (v1.1).gba",
                wild: 0x0808_2A10,
                catch: 0x0802_D6D0,
                shiny: 0x0803_DBA6,
                money: 0x0809_FD7C,
                traded: 0x0804_4288,
                encounter: 0x0808_2BDA,
                tm: &[0x0812_4F24, 0x0812_4FF0, 0x0812_5CF8],
                field_moves: (0x0812_5B60, 0x0202_4284),
                party_menu: (0x0812_2A9A, 0x0812_2B12),
                field_poison: 0x080A_0654,
            },
            Build {
                file: "Pokemon Fire Red/Pokemon Fire Red (J).gba",
                wild: 0x0808_25E8,
                catch: 0x0802_CE94,
                shiny: 0x0803_D2FE,
                money: 0x0809_FC48,
                traded: 0x0804_38BC,
                encounter: 0x0808_27B2,
                tm: &[0x0812_5810, 0x0812_58DC, 0x0812_65CC],
                field_moves: (0x0812_6434, 0x0202_41E4),
                party_menu: (0x0812_31FE, 0x0812_3276),
                field_poison: 0x080A_1918,
            },
            Build {
                file: "Pokemon Leaf Green/Pokemon - Leaf Green (V1.1).gba",
                wild: 0x0808_29E4,
                catch: 0x0802_D6D0,
                shiny: 0x0803_DBA6,
                money: 0x0809_FD50,
                traded: 0x0804_4288,
                encounter: 0x0808_2BAE,
                tm: &[0x0812_4EFC, 0x0812_4FC8, 0x0812_5CD0],
                field_moves: (0x0812_5B38, 0x0202_4284),
                party_menu: (0x0812_2A72, 0x0812_2AEA),
                field_poison: 0x080A_0628,
            },
            Build {
                file: "Pokemon Emerald/Pokemon - Emerald (USA, Europe).gba",
                wild: 0x080B_4E68,
                catch: 0x0805_6564,
                shiny: 0x0806_7C8A,
                money: 0x080E_5120,
                traded: 0x0806_E8C0,
                encounter: 0x080B_516A,
                tm: &[0x081B_6EEC],
                field_moves: (0x081B_6D6C, 0x0202_44EC),
                party_menu: (0x081B_343A, 0x081B_34AA),
                field_poison: 0x080F_976C,
            },
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("rom");
        for build in builds {
            let Ok(rom) = std::fs::read(root.join(build.file)) else {
                continue;
            };
            if rom.len() != 16 * 1024 * 1024 {
                continue;
            }
            let file = build.file;
            assert_eq!(wild_species::find(&rom), Some(build.wild), "{file}: wild");
            assert_eq!(catch_rate::find(&rom), Some(build.catch), "{file}: catch");
            assert_eq!(shiny::find(&rom), Some(build.shiny), "{file}: shiny");
            assert_eq!(money::find(&rom), Some(build.money), "{file}: money");
            assert_eq!(experience::find(&rom), Some(build.traded), "{file}: traded");
            assert_eq!(
                encounters::find(&rom),
                Some(build.encounter),
                "{file}: roll"
            );
            assert_eq!(tm::find(&rom), build.tm, "{file}: tm");
            assert_eq!(
                field_moves::find(&rom),
                Some(field_moves::FieldMoves {
                    knows_move_return: build.field_moves.0,
                    party_base: build.field_moves.1,
                }),
                "{file}: field moves"
            );
            let menu = party_menu::find(&rom).unwrap_or_else(|| panic!("{file}: party menu"));
            assert_eq!(
                (menu.capture, menu.append),
                build.party_menu,
                "{file}: party menu"
            );
            let expected_layout = if build.file.contains("Emerald") {
                party_menu::Layout::Emerald
            } else {
                party_menu::Layout::FireRedLeafGreen
            };
            assert_eq!(menu.layout, expected_layout, "{file}: layout");
            assert_eq!(
                field_poison::find(&rom),
                Some(build.field_poison),
                "{file}: poison"
            );
        }
    }
}
