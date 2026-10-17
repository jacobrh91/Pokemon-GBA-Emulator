//! The built-in cheats, run in the emulator against small cartridge images
//! that hold the real bytes of the game code each one hooks (see
//! `cheats::gen3`); the images are built by [`Image`].

use super::*;

use crate::cheats::gen3::Signature;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A cartridge image for the cheat tests: the game code in the header, a
/// branch over the header, and code and data placed at ROM offsets.
struct Image {
    bytes: Vec<u8>,
}

impl Image {
    fn new(game_code: &[u8; 4]) -> Self {
        let mut image = Self {
            bytes: vec![0; 0x8_3000],
        };
        image.word(0, 0xEA00_002E); // b 0xC0 (over the header)
        image.bytes[0xAC..0xB0].copy_from_slice(game_code);
        image
    }

    fn word(&mut self, offset: usize, word: u32) -> &mut Self {
        self.bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        self
    }

    fn half(&mut self, offset: usize, half: u16) -> &mut Self {
        self.bytes[offset..offset + 2].copy_from_slice(&half.to_le_bytes());
        self
    }

    /// The bytes of `signature` at `offset`; its wildcards are zero.
    fn signature(&mut self, offset: usize, signature: &Signature) -> &mut Self {
        for (index, byte) in signature.bytes.iter().enumerate() {
            self.bytes[offset + index] = byte.unwrap_or(0);
        }
        self
    }

    /// The bus address of the Thumb code at ROM offset `offset`.
    fn thumb(offset: usize) -> u32 {
        ROM_START + offset as u32 + 1
    }

    /// ARM startup code at the cartridge entry: it loads each `(register,
    /// value)` and then jumps to the Thumb code at ROM offset `entry`.
    fn start(&mut self, registers: &[(u32, u32)], entry: usize) -> &mut Self {
        const BASE: usize = 0xC0;
        // One `ldr` per register, then `ldr r3` and `bx r3`; the constants
        // follow the code, in the same order.
        let code_len = registers.len() + 2;
        let values: Vec<u32> = registers
            .iter()
            .map(|(_, value)| *value)
            .chain([Self::thumb(entry)])
            .collect();
        let targets: Vec<u32> = registers
            .iter()
            .map(|(register, _)| *register)
            .chain([3])
            .collect();
        // `ldr rN, [pc, #imm]`: pc is the instruction's address plus 8, and
        // each constant is as far past its `ldr` as the code is long, less the
        // two instructions of that offset.
        let imm = 4 * (code_len - 2) as u32;
        for (index, register) in targets.iter().enumerate() {
            self.word(BASE + 4 * index, 0xE59F_0000 | (register << 12) | imm);
        }
        self.word(BASE + 4 * (code_len - 1), 0xE12F_FF13); // bx r3
        for (index, value) in values.into_iter().enumerate() {
            self.word(BASE + 4 * code_len + 4 * index, value);
        }
        self
    }

    /// An emulator started on this image. The emulator maps a ROM from a
    /// path, so the image goes through a temporary file.
    fn emulator(&self) -> Emulator {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("gba-cheat-test-{}-{id}.gba", std::process::id()));
        std::fs::write(&path, &self.bytes).unwrap();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        emulator
    }
}

fn run(emulator: &mut Emulator, steps: usize) {
    for _ in 0..steps {
        emulator.step().unwrap();
    }
}

/// An image shaped like a game around `CreateWildMon`: the code loads species
/// 0x13 and level 4 and calls a function at ROM offset `function` that begins
/// with the real function's prologue (the signature the scan looks for) and
/// spins. Once the prologue has run, `r4` holds the species the function
/// received.
fn wild_mon_image(game_code: &[u8; 4], function: usize) -> Image {
    use crate::cheats::gen3::wild_species::SIGNATURE;
    let mut image = Image::new(game_code);
    image
        .start(&[(0, 0x13), (1, 4)], function)
        .signature(function, &SIGNATURE)
        .half(function + 16, 0xE7FE); // b .
    image
}

/// Runs the image with `options` and returns `(species, level)` as the
/// function received them (`r4`, `r6` once its prologue has masked its
/// arguments), whether the build was recognised, and the hook address.
fn wild_run(
    game_code: &[u8; 4],
    function: usize,
    options: CheatOptions,
    file: &str,
) -> (u32, u32, bool, Option<u32>) {
    use crate::cheats::Cheats;
    let mut emulator = wild_mon_image(game_code, function).emulator();
    emulator.set_cheats(Cheats::parse(file));
    emulator.set_cheat_options(options);
    run(&mut emulator, 24);
    (
        emulator.cpu().registers[4],
        emulator.cpu().registers[6],
        emulator.wild_species_supported(),
        emulator.wild_species_hook_address(),
    )
}

/// The wild-encounter row switched on with the given species and level.
fn wild(species: Option<u16>, level: Option<u8>) -> CheatOptions {
    CheatOptions {
        wild_enabled: true,
        wild_species: species,
        wild_level: level,
        ..CheatOptions::default()
    }
}

#[test]
fn the_wild_row_replaces_the_species_and_level_arguments() {
    // The synthetic caller asks for species 0x13 at level 4.
    let (species, level, supported, hook) =
        wild_run(b"BPRE", 0x8_29FC, CheatOptions::default(), "");
    assert_eq!(
        (species, level),
        (0x13, 4),
        "nothing set: the game's choice"
    );
    assert!(supported);
    assert_eq!(hook, Some(0x0808_29FC));

    assert_eq!(
        wild_run(b"BPRE", 0x8_29FC, wild(Some(147), None), "").0,
        147
    );
    assert_eq!(wild_run(b"BPRE", 0x8_29FC, wild(Some(25), None), "").0, 25);
    // Species and level are independent.
    let (species, level, ..) = wild_run(b"BPRE", 0x8_29FC, wild(Some(147), None), "");
    assert_eq!((species, level), (147, 4));
    let (species, level, ..) = wild_run(b"BPRE", 0x8_29FC, wild(None, Some(50)), "");
    assert_eq!((species, level), (0x13, 50));
    let (species, level, ..) = wild_run(b"BPRE", 0x8_29FC, wild(Some(25), Some(7)), "");
    assert_eq!((species, level), (25, 7));
}

#[test]
fn a_wild_row_that_is_switched_off_keeps_its_choices_but_does_nothing() {
    let off = CheatOptions {
        wild_enabled: false,
        wild_species: Some(147),
        wild_level: Some(50),
        ..CheatOptions::default()
    };
    let (species, level, ..) = wild_run(b"BPRE", 0x8_29FC, off, "");
    assert_eq!((species, level), (0x13, 4));
    let on = CheatOptions {
        wild_enabled: true,
        ..off
    };
    let (species, level, ..) = wild_run(b"BPRE", 0x8_29FC, on, "");
    assert_eq!((species, level), (147, 50));
}

#[test]
fn the_cheats_files_wild_species_line_is_not_read_by_the_emulator() {
    // The frontend turns it into the menu's species; on its own it does
    // nothing, so there is one switch for the effect: the row's.
    let (species, ..) = wild_run(
        b"BPRE",
        0x8_29FC,
        CheatOptions::default(),
        "wild_species = Dratini",
    );
    assert_eq!(species, 0x13);
    let (species, ..) = wild_run(
        b"BPRE",
        0x8_29FC,
        CheatOptions {
            file_codes: true,
            ..CheatOptions::default()
        },
        "wild_species = Dratini",
    );
    assert_eq!(species, 0x13, "not even with the file's codes switched on");
}

#[test]
fn the_function_is_found_wherever_the_build_put_it() {
    // Same code, different builds: the hook follows the function.
    for function in [0x8_25E8, 0x8_29FC, 0x8_2A10] {
        let (species, _, supported, hook) = wild_run(b"BPRJ", function, wild(Some(147), None), "");
        assert!(supported);
        assert_eq!(hook, Some(0x0800_0000 + function as u32));
        assert_eq!(species, 147, "function at {function:#X}");
    }
}

#[test]
fn the_wild_row_only_applies_to_gen3_roms_and_clamps_the_level() {
    let (species, level, supported, hook) =
        wild_run(b"AXVE", 0x8_29FC, wild(Some(25), Some(50)), "");
    assert!(!supported);
    assert_eq!(hook, None);
    assert_eq!(
        (species, level),
        (0x13, 4),
        "another game must be left alone"
    );

    let (_, level, ..) = wild_run(b"BPRE", 0x8_29FC, wild(None, Some(250)), "");
    assert_eq!(level, 100);
}

/// An image shaped like a game around the catch check: after bookkeeping,
/// Thumb code compares `r6` ("odds") with 0xFE exactly as the real command
/// does, and records the outcome in `r2` (1 = caught, 2 = the shake roll).
fn catch_image(game_code: &[u8; 4], function: usize, odds: u32) -> Image {
    let mut image = Image::new(game_code);
    image
        // r1 is scratch for the `strb`, r0 the attempts so far, r6 the odds.
        .start(&[(1, 0x0300_0000), (0, 0), (6, odds & 0xFF)], function)
        .half(function, 0x28FE) // cmp  r0, #0xFE
        .half(function + 2, 0xD801) // bhi  +1
        .half(function + 4, 0x3001) // adds r0, #1
        .half(function + 6, 0x7008) // strb r0, [r1]
        .half(function + 8, 0x2EFE) // cmp  r6, #0xFE      <- the hook
        .half(function + 10, 0xD92F) // bls  shake_roll
        .half(function + 12, 0x2201) // movs r2, #1        (caught)
        .half(function + 14, 0xE7FE); // b    .
    // The `bls` lands 4 + 2 * 0x2F bytes after itself.
    let shakes = function + 10 + 4 + 2 * 0x2F;
    image
        .half(shakes, 0x2202) // movs r2, #2               (shake roll)
        .half(shakes + 2, 0xE7FE); // b .
    image
}

/// Runs the image and returns the outcome in `r2` and the hook address.
fn catch_outcome(game_code: &[u8; 4], odds: u32, options: CheatOptions) -> (u32, Option<u32>) {
    let mut emulator = catch_image(game_code, 0x8_2A00, odds).emulator();
    emulator.set_cheat_options(options);
    run(&mut emulator, 40);
    (emulator.cpu().registers[2], emulator.catch_check_address())
}

fn catching() -> CheatOptions {
    CheatOptions {
        catch_always: true,
        ..CheatOptions::default()
    }
}

#[test]
fn the_catch_cheat_turns_a_failed_odds_test_into_a_catch() {
    // Without the cheat, low odds go to the shake roll...
    let (outcome, hook) = catch_outcome(b"BPRE", 0x03, CheatOptions::default());
    assert_eq!(outcome, 2, "the game's own result: the shake roll");
    // ...the hook is the `cmp r6, #0xFE`, 8 bytes into the signature match.
    assert_eq!(hook, Some(0x0800_0000 + 0x8_2A00 + 8));
    // With it, the same odds are a catch.
    assert_eq!(catch_outcome(b"BPRE", 0x03, catching()).0, 1);
    // Odds that already pass are a catch either way.
    assert_eq!(catch_outcome(b"BPRE", 0xFF, CheatOptions::default()).0, 1);
    assert_eq!(catch_outcome(b"BPRE", 0xFF, catching()).0, 1);
    // The boundary is the game's: 254 is not above 254.
    assert_eq!(catch_outcome(b"BPRE", 0xFE, CheatOptions::default()).0, 2);
    assert_eq!(catch_outcome(b"BPRE", 0xFE, catching()).0, 1);
}

#[test]
fn the_catch_cheat_is_off_until_its_row_is_on_and_leaves_other_games_alone() {
    // Other options do not turn it on.
    let other = CheatOptions {
        infinite_money: true,
        wild_enabled: true,
        wild_species: Some(150),
        ..CheatOptions::default()
    };
    assert_eq!(catch_outcome(b"BPRE", 0x03, other).0, 2);
    // A different game, even with the same bytes, is not hooked.
    let (outcome, hook) = catch_outcome(b"AXVE", 0x03, catching());
    assert_eq!(outcome, 2);
    assert_eq!(hook, None);
}

/// An image shaped like a game around the encounter dice roll: the function's
/// real bytes, with the two calls replaced by `nop` and by `movs r0, #roll`
/// (standing in for `Random() % 1600`), so the function's own comparison
/// decides. It is called with `r0 = rate`; the answer is in `r0` once the
/// Thumb code reaches the `b .` it returns to.
fn encounter_image(game_code: &[u8; 4], function: usize, rate: u32, roll: u8) -> Image {
    use crate::cheats::gen3::encounters::SIGNATURE;
    let spin = function + 0x80;
    let mut image = Image::new(game_code);
    image
        .start(&[(0, rate & 0xFF), (14, Image::thumb(spin))], function)
        .signature(function, &SIGNATURE)
        .half(function + 8, 0x46C0) // nop              (bl Random)
        .half(function + 10, 0x46C0) // nop
        .half(function + 20, 0x2000 | u16::from(roll)) // movs r0, #roll (bl __umodsi3)
        .half(function + 22, 0x46C0) // nop
        .half(spin, 0xE7FE); // b .
    image
}

/// The function's answer (`r0`) for `roll < rate` or not.
fn encounter_answer(game_code: &[u8; 4], rate: u32, roll: u8, options: CheatOptions) -> u32 {
    let mut emulator = encounter_image(game_code, 0x8_2A00, rate, roll).emulator();
    emulator.set_cheat_options(options);
    run(&mut emulator, 60);
    emulator.cpu().registers[0]
}

fn no_encounters() -> CheatOptions {
    CheatOptions {
        no_encounters: true,
        ..CheatOptions::default()
    }
}

#[test]
fn the_encounter_cheat_turns_every_yes_into_no() {
    // The game's own answer: an encounter when the roll is below the rate.
    assert_eq!(
        encounter_answer(b"BPRE", 50, 10, CheatOptions::default()),
        1
    );
    assert_eq!(
        encounter_answer(b"BPRE", 50, 90, CheatOptions::default()),
        0
    );
    // With the cheat the answer is always no, including a certain encounter.
    assert_eq!(encounter_answer(b"BPRE", 50, 10, no_encounters()), 0);
    assert_eq!(encounter_answer(b"BPRE", 50, 90, no_encounters()), 0);
    assert_eq!(encounter_answer(b"BPRE", 255, 0, no_encounters()), 0);
}

#[test]
fn the_encounter_cheat_is_off_until_its_row_is_on_and_leaves_other_games_alone() {
    let other = CheatOptions {
        infinite_money: true,
        catch_always: true,
        ..CheatOptions::default()
    };
    assert_eq!(encounter_answer(b"BPRE", 50, 10, other), 1);
    assert_eq!(encounter_answer(b"AXVE", 50, 10, no_encounters()), 1);
}

/// An image with the TM-lesson code at two places (the real bytes, the call
/// replaced by `adds r2, r1, #0; nop`, which records the count the call would
/// have been given). It enters the place at `entry` with `r1 = 200` (a TM);
/// `r2` ends up as the count.
fn tm_image(game_code: &[u8; 4], function: usize, entry: usize) -> Image {
    use crate::cheats::gen3::tm::SIGNATURE;
    let mut image = Image::new(game_code);
    // `r8` holds a readable address for the code after the call.
    image.start(&[(1, 200), (8, 0x0300_0000)], function + entry);
    for place in [function, function + 0x100] {
        image
            .signature(place, &SIGNATURE)
            .half(place + 12, 0x1C0A) // adds r2, r1, #0   (the call's count)
            .half(place + 14, 0x46C0) // nop
            .half(place + 20, 0xE7FE); // b .
    }
    image
}

/// `r2` after running the TM-lesson code from the copy at `entry` (an offset
/// from the first), and whether the scan found both.
fn tm_count(game_code: &[u8; 4], entry: usize, options: CheatOptions) -> (u32, bool) {
    let mut emulator = tm_image(game_code, 0x8_2A00, entry).emulator();
    emulator.set_cheat_options(options);
    run(&mut emulator, 60);
    let scan = emulator.cheat_scan();
    (emulator.cpu().registers[2], scan.tm_uses.len() == 2)
}

#[test]
fn tms_are_never_used_up_whatever_the_menu_says() {
    // The game's own count is 1 (`movs r1, #1`); the hook makes it 0, at
    // both places, with every option off and with others on.
    let other = CheatOptions {
        infinite_money: true,
        no_encounters: true,
        ..CheatOptions::default()
    };
    for entry in [0, 0x100] {
        for options in [CheatOptions::default(), other] {
            let (count, found) = tm_count(b"BPRE", entry, options);
            assert!(found, "both sites are found");
            assert_eq!(count, 0, "entry {entry:#X}");
        }
    }
}

#[test]
fn other_games_still_use_up_their_items() {
    for entry in [0, 0x100] {
        let (count, found) = tm_count(b"AXVE", entry, CheatOptions::default());
        assert!(!found);
        assert_eq!(count, 1, "the game's own call is untouched");
    }
}

/// An image with `MonKnowsMove` (its real bytes, the call to `GetMonData`
/// replaced by `movs r0, #0 ; nop` so that the game's own answer is always
/// "does not know") and `checkpartymove`'s party load, whose pool holds
/// `0x02024284`. The function is called with `r0 = mon` and `r1 = move`; the
/// answer is `r0` once the Thumb code reaches the `b .` it returns to.
fn knows_move_image(game_code: &[u8; 4], mon: u32, move_id: u32) -> Image {
    use crate::cheats::gen3::field_moves::{MON_KNOWS_MOVE, PARTY_LOAD};
    let function = 0x8_2000;
    let load = 0x8_2400;
    let spin = 0x8_2800;
    let mut image = Image::new(game_code);
    image
        .start(
            &[(0, mon), (1, move_id), (14, Image::thumb(spin))],
            function,
        )
        .signature(function, &MON_KNOWS_MOVE)
        .bytes[function + 16..function + 20]
        .copy_from_slice(&[0x00, 0x20, 0xC0, 0x46]);
    // The loop's branches, which the wildcards left at zero: `bne +1`, `b
    // done`, `bls loop`, as the game has them.
    image.bytes[function + 22] = 0x01;
    image.bytes[function + 26] = 0x05;
    image.bytes[function + 36] = 0xF1;
    // `ldr r0, [pc, #imm]` at `load + 6`, whose pool entry is at `load + 0x40`.
    let at = (load + 6) as u32;
    image.signature(load, &PARTY_LOAD).bytes[load + 6] =
        ((load as u32 + 0x40 - ((at + 4) & !3)) / 4) as u8;
    image.word(load + 0x40, 0x0202_4284).half(spin, 0xE7FE); // b .
    image
}

fn knows_move(game_code: &[u8; 4], mon: u32, move_id: u32) -> u32 {
    let mut emulator = knows_move_image(game_code, mon, move_id).emulator();
    run(&mut emulator, 200);
    emulator.cpu().registers[0]
}

#[test]
fn the_first_pokemon_is_said_to_know_every_hm_and_nothing_else_changes() {
    let first = 0x0202_4284;
    for hm in crate::cheats::gen3::field_moves::HM_MOVES {
        assert_eq!(knows_move(b"BPRE", first, u32::from(hm)), 1, "HM {hm}");
    }
    // The game's own answer ("no") stands for anything else.
    assert_eq!(knows_move(b"BPRE", first, 33), 0, "Tackle");
    assert_eq!(
        knows_move(b"BPRE", first + 100, 15),
        0,
        "the second Pokémon"
    );
    assert_eq!(knows_move(b"BPRE", first + 500, 57), 0, "the sixth");
    assert_eq!(
        knows_move(b"BPRE", 0x0203_0000, 15),
        0,
        "not a party member"
    );
}

#[test]
fn the_hm_cheat_covers_every_supported_game_and_no_other() {
    let first = 0x0202_4284;
    for code in [b"BPRJ", b"BPGE", b"BPEE"] {
        assert_eq!(knows_move(code, first, 15), 1, "{code:?}");
    }
    // Ruby and Sapphire have no such hook.
    assert_eq!(knows_move(b"AXVE", first, 15), 0);
    assert_eq!(knows_move(b"AXPE", first, 15), 0);
}

/// An image with the party menu's two hook places: the capture signature at
/// `capture` (its `bl` replaced by two `nop`s, followed by a jump to the append
/// place) and the append signature at `append` (a `b .` at the hook). It enters
/// at the capture hook with `r0` = the list, `r1` = the count and `r10` = the
/// selected slot.
fn party_menu_image(game_code: &[u8; 4], count_constant: u8, slot: u32) -> Image {
    use crate::cheats::gen3::party_menu::CAPTURE;
    let capture = 0x8_2000;
    let append = 0x8_2400;
    let hook = capture + CAPTURE.hook_offset;
    let mut image = Image::new(game_code);
    image
        .start(&[(0, 0x0300_0100), (1, 0x0300_0200), (10, slot)], hook)
        .signature(capture, &CAPTURE)
        // The hook instruction and the one after it are `nop`s; then `ldr r3,
        // [pc, #imm] ; bx r3` to the append place.
        .half(hook, 0x46C0)
        .half(hook + 2, 0x46C0);
    // The word to load must be 4-byte aligned: the instruction at `hook + 4`
    // reads relative to its own address plus 4, rounded down.
    let pool = (hook + 16) & !3;
    let imm = (pool - ((hook + 8) & !3)) / 4;
    image
        .half(hook + 4, 0x4B00 | imm as u16) // ldr r3, [pc, #imm]
        .half(hook + 6, 0x4718) // bx r3
        .word(pool, Image::thumb(append + 26));
    // The append signature, with this layout's count of field moves, and a
    // `b .` at its hook.
    let tail = [
        0x60,
        0x1C,
        0x00,
        0x06,
        0x04,
        0x0E,
        0x60,
        0x00,
        0x40,
        0x19,
        0x00,
        0x88,
        count_constant,
        0x28,
        0x00,
        0xD1,
        0x41,
        0x46,
        0x08,
        0x06,
        0x07,
        0x0E,
        0x03,
        0x2F,
        0x00,
        0xD9,
        0xFE,
        0xE7,
    ];
    image.bytes[append..append + tail.len()].copy_from_slice(&tail);
    image
}

/// The list and count after the menu for `slot` has been built from a list that
/// already holds SUMMARY and Cut.
fn built_actions(game_code: &[u8; 4], count_constant: u8, slot: u32) -> (Vec<u8>, u8) {
    let mut emulator = party_menu_image(game_code, count_constant, slot).emulator();
    // SUMMARY and Cut: entry 0x13 in both layouts (index 1 of FireRed's table
    // of field moves, index 0 of Emerald's).
    let bus = emulator.memory_mut();
    for (index, action) in [0u8, 0x13].into_iter().enumerate() {
        bus.write_u8(0x0300_0100 + index as u32, action).unwrap();
    }
    bus.write_u8(0x0300_0200, 2).unwrap();
    run(&mut emulator, 200);
    let count = emulator.memory().read_u8(0x0300_0200).unwrap();
    let list = (0..u32::from(count))
        .map(|index| emulator.memory().read_u8(0x0300_0100 + index).unwrap())
        .collect();
    (list, count)
}

#[test]
fn the_first_pokemons_menu_lists_the_hms_it_does_not_know() {
    // FireRed / LeafGreen: it lists SUMMARY and Cut (0x13); Flash (0x12), Fly
    // (0x14) and Dig (0x1A) are added.
    assert_eq!(
        built_actions(b"BPRE", 12, 0),
        (vec![0, 0x13, 0x12, 0x14, 0x1A], 5)
    );
    assert_eq!(
        built_actions(b"BPGE", 12, 0),
        (vec![0, 0x13, 0x12, 0x14, 0x1A], 5)
    );
    // Emerald: SUMMARY and Cut (0x13); Flash (0x14), Fly (0x18), Dig (0x1C).
    assert_eq!(
        built_actions(b"BPEE", 14, 0),
        (vec![0, 0x13, 0x14, 0x18, 0x1C], 5)
    );
}

#[test]
fn other_pokemons_menus_and_other_games_are_left_as_the_game_built_them() {
    for slot in 1..6 {
        assert_eq!(
            built_actions(b"BPRE", 12, slot),
            (vec![0, 0x13], 2),
            "slot {slot}"
        );
    }
    assert_eq!(built_actions(b"AXVE", 12, 0), (vec![0, 0x13], 2));
}

/// An image with the poison check's bytes, followed by the code that matters:
/// the game's `cmp r0, #1 ; bne +1` and two markers, `movs r2, #1` (the status
/// was poison) and `movs r2, #2` (it was not). It enters at the `cmp` with `r0
/// = ailment`.
fn poison_image(game_code: &[u8; 4], ailment: u32) -> Image {
    use crate::cheats::gen3::field_poison::SIGNATURE;
    let function = 0x8_2000;
    let hook = function + SIGNATURE.hook_offset;
    let mut image = Image::new(game_code);
    image
        .start(&[(0, ailment)], hook)
        .signature(function, &SIGNATURE)
        .half(hook + 4, 0x2201) // movs r2, #1   (poisoned)
        .half(hook + 6, 0xE7FE) // b .
        .half(hook + 8, 0x2202) // movs r2, #2   (not)
        .half(hook + 10, 0xE7FE); // b .
    // `cmp r0, #1` is already there; the wildcard before `bne` is its `+1`.
    image.bytes[function + 36] = 0x01;
    image
}

/// 1 if the game goes on to poison the Pokémon, 2 if it skips it.
fn poison_outcome(game_code: &[u8; 4], ailment: u32, options: CheatOptions) -> u32 {
    let mut emulator = poison_image(game_code, ailment).emulator();
    emulator.set_cheat_options(options);
    run(&mut emulator, 100);
    emulator.cpu().registers[2]
}

#[test]
fn poisoned_pokemon_are_always_skipped_by_the_field_check_whatever_the_menu_says() {
    // Poison (1) is skipped, with every option off and with others on.
    let other = CheatOptions {
        no_encounters: true,
        infinite_money: true,
        ..CheatOptions::default()
    };
    for options in [CheatOptions::default(), other] {
        assert_eq!(poison_outcome(b"BPRE", 1, options), 2);
    }
    // Another ailment (burn is 4 in the game's numbering) is skipped anyway.
    assert_eq!(poison_outcome(b"BPRE", 4, CheatOptions::default()), 2);
    // Every game of the family.
    for code in [b"BPGE", b"BPRJ", b"BPEE"] {
        assert_eq!(
            poison_outcome(code, 1, CheatOptions::default()),
            2,
            "{code:?}"
        );
    }
    // Another game is left alone: the game's own comparison poisons.
    assert_eq!(poison_outcome(b"AXVE", 1, CheatOptions::default()), 1);
}
