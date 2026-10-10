use gba_emulator::emulator::Emulator;
use gba_emulator::mmu::{IO_REG_START, IWRAM_START, ROM_START};
use std::path::PathBuf;

const TESTNUM: u32 = IWRAM_START + 8;
const CURSEL: u32 = IWRAM_START + 16;
const MAX_INSTRUCTIONS: usize = 1_000_000;

fn rom_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join(name)
}

fn result_colors(emulator: &Emulator, y: usize) -> (usize, usize) {
    let vram = &emulator.memory().vram;
    let mut passed_pixels = 0;
    let mut failed_pixels = 0;

    for row in y..y + 8 {
        for x in 72..112 {
            let color = vram[row * 240 + x];
            passed_pixels += usize::from(color == 1);
            failed_pixels += usize::from(color == 2);
        }
    }

    (passed_pixels, failed_pixels)
}

fn result_rows(emulator: &Emulator) -> Vec<(usize, usize, usize)> {
    (16..160)
        .step_by(8)
        .map(|y| {
            let (passed_pixels, failed_pixels) = result_colors(emulator, y);
            (y, passed_pixels, failed_pixels)
        })
        .filter(|(_, passed_pixels, failed_pixels)| *passed_pixels + *failed_pixels > 0)
        .collect()
}

fn run_armwrestler_test(rom: &str, menu_selection: u8, expected_test: u8, minimum_results: usize) {
    let path = rom_path(rom);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();

    // Keep only Start pressed; holding every direction can change the menu selection.
    emulator.set_buttons(gba_emulator::input::START);
    let mut menu_selection_set = false;

    for instruction in 0..MAX_INSTRUCTIONS {
        if !menu_selection_set && emulator.memory().read_u32(TESTNUM).unwrap() == 10 {
            emulator
                .memory_mut()
                .write_u8(CURSEL, menu_selection)
                .unwrap();
            menu_selection_set = true;
        }

        let previous_pc = emulator.cpu().registers[15];
        emulator.step().unwrap_or_else(|error| {
            panic!(
                "{rom} stopped at PC {:#010X} while running suite {expected_test}: {error}",
                emulator.cpu().registers[15]
            )
        });
        assert!(
            emulator.cpu().mode() != 0x17,
            "{rom} entered Abort mode while running suite {expected_test}: TESTNUM={}, CURSEL={}, prior PC={previous_pc:#010X}, opcode={:#010X}, vector PC={:#010X}, LR={:#010X}, SPSR={:#010X}, regs={:?}",
            emulator.memory().read_u32(TESTNUM).unwrap(),
            emulator.memory().read_u8(CURSEL).unwrap(),
            emulator.memory().read_u32(previous_pc).unwrap_or_default(),
            emulator.cpu().registers[15],
            emulator.cpu().registers[14],
            emulator.cpu().saved_status().unwrap_or_default(),
            emulator.cpu().registers
        );
        assert!(
            emulator.cpu().mode() != 0x1B,
            "{rom} entered Undefined mode while running suite {expected_test}: vector PC={:#010X}, LR={:#010X}, fault PC={:#010X}, opcode={:#010X}, SPSR={:#010X}",
            emulator.cpu().registers[15],
            emulator.cpu().registers[14],
            emulator.cpu().registers[14].wrapping_sub(4),
            emulator
                .memory()
                .read_u32(emulator.cpu().registers[14].wrapping_sub(4))
                .unwrap_or_default(),
            emulator.cpu().saved_status().unwrap_or_default()
        );

        if instruction % 128 == 0 && emulator.memory().read_u8(TESTNUM).unwrap() == expected_test {
            let rows = result_rows(&emulator);
            if rows.len() >= minimum_results {
                for (y, passed_pixels, failed_pixels) in &rows {
                    assert!(
                        *passed_pixels > 0 && *failed_pixels == 0,
                        "{rom} result row {y} has {passed_pixels} OK pixels and {failed_pixels} BAD pixels; CPSR={:#010X}, r0-r7={:?}",
                        emulator.cpu().cpsr,
                        &emulator.cpu().registers[..8]
                    );
                }
                return;
            }
        }
    }

    panic!(
        "{rom} did not report a result for suite {expected_test} within {MAX_INSTRUCTIONS} instructions; PC={:#010X}, mode={:#04X}, LR={:#010X}, SPSR={:#010X}, TESTNUM={}, CURSEL={}, DISPSTAT={:#06X}, cycles={}, rows={:?}",
        emulator.cpu().registers[15],
        emulator.cpu().mode(),
        emulator.cpu().registers[14],
        emulator.cpu().saved_status().unwrap_or_default(),
        emulator.memory().read_u32(TESTNUM).unwrap(),
        emulator.memory().read_u8(CURSEL).unwrap(),
        emulator.memory().read_u16(IO_REG_START + 4).unwrap(),
        emulator.cpu().cycles,
        result_rows(&emulator),
    );
}

#[test]
fn armwrestler_reports_arm_test_results_in_vram() {
    run_armwrestler_test("armwrestler-gba-fixed.gba", 0, 0, 15);
    run_armwrestler_test("armwrestler-gba-fixed.gba", 1, 2, 12);
    run_armwrestler_test("armwrestler-gba-fixed.gba", 2, 4, 4);
    run_armwrestler_test("armwrestler.gba", 0, 0, 15);
}

#[test]
fn armwrestler_reports_thumb_load_store_results_in_vram() {
    run_armwrestler_test("armwrestler-gba-fixed.gba", 4, 1, 7);
}

#[test]
fn armwrestler_reports_thumb_multiple_transfer_results_in_vram() {
    run_armwrestler_test("armwrestler-gba-fixed.gba", 5, 2, 2);
}

#[test]
fn armwrestler_reports_thumb_alu_results_in_vram() {
    run_armwrestler_test("armwrestler-gba-fixed.gba", 3, 0, 11);
}
