use super::*;

const TEST_PROGRAM_ADDRESS: u32 = 0x0300_0000;

#[test]
fn arm_immediate_barrel_shifts_follow_zero_and_boundary_rules() {
    let value = 0x8000_0001;
    let cases = [
        (0, 0, true, (value, true)),
        (0, 1, false, (0x0000_0002, true)),
        (0, 31, false, (0x8000_0000, false)),
        (0, 32, false, (0, true)),
        (0, 33, false, (0, false)),
        (1, 0, false, (0, true)),
        (1, 1, false, (0x4000_0000, true)),
        (1, 31, false, (1, false)),
        (1, 32, false, (0, true)),
        (1, 33, false, (0, false)),
        (2, 0, false, (u32::MAX, true)),
        (2, 1, false, (0xC000_0000, true)),
        (2, 31, false, (u32::MAX, false)),
        (2, 32, false, (u32::MAX, true)),
        (2, 33, false, (u32::MAX, true)),
        (3, 0, false, (0x4000_0000, true)),
        (3, 0, true, (0xC000_0000, true)),
        (3, 1, false, (0xC000_0000, true)),
        (3, 31, false, (0x0000_0003, false)),
    ];

    for (shift_type, amount, carry_in, expected) in cases {
        assert_eq!(
            shift_operand(value, shift_type, amount, false, carry_in),
            expected,
            "shift type {shift_type}, immediate amount {amount}, carry {carry_in}"
        );
    }
}

#[test]
fn arm_register_barrel_shifts_follow_zero_and_boundary_rules() {
    let value = 0x8000_0001;
    let cases = [
        (0, 0, true, (value, true)),
        (0, 1, false, (0x0000_0002, true)),
        (0, 31, false, (0x8000_0000, false)),
        (0, 32, false, (0, true)),
        (0, 33, false, (0, false)),
        (1, 0, true, (value, true)),
        (1, 1, false, (0x4000_0000, true)),
        (1, 31, false, (1, false)),
        (1, 32, false, (0, true)),
        (1, 33, false, (0, false)),
        (2, 0, false, (value, false)),
        (2, 1, false, (0xC000_0000, true)),
        (2, 31, false, (u32::MAX, false)),
        (2, 32, false, (u32::MAX, true)),
        (2, 255, false, (u32::MAX, true)),
        (3, 0, true, (value, true)),
        (3, 1, false, (0xC000_0000, true)),
        (3, 31, false, (3, false)),
        (3, 32, false, (value, true)),
        (3, 33, false, (0xC000_0000, true)),
        (3, 64, false, (value, true)),
    ];

    for (shift_type, amount, carry_in, expected) in cases {
        assert_eq!(
            shift_operand(value, shift_type, amount, true, carry_in),
            expected,
            "shift type {shift_type}, register amount {amount}, carry {carry_in}"
        );
    }
}

#[test]
fn set_flag_updates_only_the_requested_cpsr_bit() {
    let mut cpu = Cpu::default();
    cpu.cpsr = 0x0000_000F;

    cpu.set_flag(N_FLAG, true);
    assert_eq!(cpu.cpsr, N_FLAG | 0x0000_000F);

    cpu.set_flag(N_FLAG, false);
    assert_eq!(cpu.cpsr, 0x0000_000F);
}

fn cpu_with_program(program: &[u16]) -> (Cpu, MemoryBus) {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-cpu-test-rom-{}-{id}.bin",
        std::process::id()
    ));
    fs::write(&path, [0; 4]).unwrap();
    let mut bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    for (index, opcode) in program.iter().enumerate() {
        bus.iwram[index * 2..index * 2 + 2].copy_from_slice(&opcode.to_le_bytes());
    }

    let mut cpu = Cpu::default();
    cpu.registers[15] = TEST_PROGRAM_ADDRESS;
    (cpu, bus)
}

fn cpu_with_arm_program(program: &[u32]) -> (Cpu, MemoryBus) {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-arm-test-rom-{}-{id}.bin",
        std::process::id()
    ));
    fs::write(&path, [0; 4]).unwrap();
    let mut bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    for (index, instruction) in program.iter().enumerate() {
        bus.iwram[index * 4..index * 4 + 4].copy_from_slice(&instruction.to_le_bytes());
    }

    let mut cpu = Cpu::default();
    cpu.registers[15] = TEST_PROGRAM_ADDRESS;
    (cpu, bus)
}

fn cpu_with_rom(contents: &[u8]) -> (Cpu, MemoryBus) {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-rom-exception-test-{}-{id}.gba",
        std::process::id()
    ));
    fs::write(&path, contents).unwrap();
    let bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();

    let mut cpu = Cpu::default();
    cpu.registers[15] = crate::mmu::ROM_START;
    (cpu, bus)
}

#[test]
fn bios_region_reads_return_the_last_bios_opcode_instead_of_faulting() {
    let (_, mut bus) = cpu_with_program(&[]);
    assert_eq!(bus.read_u32(0).unwrap(), 0xE129_F000);
    assert_eq!(bus.read_u8(0x0A).unwrap(), 0x29);
    bus.set_bios_open_bus(0xE3A0_2004);
    assert_eq!(bus.read_u32(0x3FFC).unwrap(), 0xE3A0_2004);
}

#[test]
fn unmapped_reads_return_the_last_fetched_opcode_and_writes_are_ignored() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE591_0000]); // LDR r0, [r1]
    cpu.registers[1] = 0x1000_0000;
    let mode = cpu.mode();
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.mode(), mode);
    assert_eq!(cpu.registers[0], 0xE591_0000);
    bus.write_u32(0x1000_0000, 0x1234).unwrap();
}

#[test]
fn unsupported_thumb_opcode_enters_undefined_exception() {
    let (mut cpu, mut bus) = cpu_with_program(&[0xDE00]);
    cpu.set_cpsr(MODE_SYSTEM | T_FLAG).unwrap();

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_UNDEFINED);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 2);
    assert_eq!(cpu.registers[15], VECTOR_UNDEFINED);
    assert_eq!(cpu.saved_status().unwrap() & MODE_MASK, MODE_SYSTEM);
    assert_ne!(cpu.saved_status().unwrap() & T_FLAG, 0);
}

#[test]
fn unaligned_thumb_fetch_enters_prefetch_abort() {
    let (mut cpu, mut bus) = cpu_with_program(&[0x2007]);
    cpu.registers[15] += 1;
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_ABORT);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 5);
    assert_eq!(cpu.registers[15], VECTOR_PREFETCH_ABORT);

    cpu.registers[15] = TEST_PROGRAM_ADDRESS;
    cpu.set_cpsr(MODE_SYSTEM | T_FLAG).unwrap();
    cpu.cycles = u64::MAX;
    assert_eq!(
        cpu.step_thumb(&mut bus).unwrap_err().kind(),
        io::ErrorKind::Other
    );
    assert_eq!(cpu.registers[0], 0);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS);
    assert_eq!(cpu.cycles, u64::MAX);
}

#[test]
fn thumb_word_and_byte_load_store_use_bus_memory() {
    let (mut cpu, mut bus) = cpu_with_program(&[0x6048, 0x684A, 0x7048, 0x784A]);
    cpu.registers[0] = 0x1234_56AB;
    cpu.registers[1] = TEST_PROGRAM_ADDRESS + 0x100;

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 2);
    assert_eq!(
        bus.read_u32(TEST_PROGRAM_ADDRESS + 0x104).unwrap(),
        0x1234_56AB
    );

    // Loads are 1S+1N+1I = 3 cycles; stores are 2N = 2 cycles.
    cpu.registers[1] = TEST_PROGRAM_ADDRESS + 0x100;
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[2], 0x1234_56AB);

    cpu.registers[1] = TEST_PROGRAM_ADDRESS + 0x100;
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 2);
    assert_eq!(bus.read_u8(TEST_PROGRAM_ADDRESS + 0x101).unwrap(), 0xAB);

    cpu.registers[1] = TEST_PROGRAM_ADDRESS + 0x100;
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[2], 0xAB);
    assert_eq!(cpu.cycles, 10);
}

#[test]
fn thumb_load_from_the_bios_region_reads_open_bus_without_aborting() {
    let (mut cpu, mut bus) = cpu_with_program(&[0x6808]); // LDR r0, [r1]
    cpu.registers[1] = 0;
    let mode = cpu.mode();
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), mode);
    assert_eq!(cpu.registers[0], 0xE129_F000);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 2);
}

#[test]
fn arm_rotated_immediate_and_adc_use_carry_correctly() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE3A0_1102, 0xE2B0_2000]);
    cpu.registers[0] = u32::MAX;
    cpu.cpsr |= C_FLAG;

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], 0x8000_0000);
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[2], 0);
    assert_ne!(cpu.cpsr & Z_FLAG, 0);
    assert_ne!(cpu.cpsr & C_FLAG, 0);
    assert_eq!(cpu.cpsr & N_FLAG, 0);
}

#[test]
fn arm_branch_with_link_saves_return_address() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xEB00_0001]);

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 12);
    assert_eq!(cpu.cycles, 3);
}

#[test]
fn arm_not_taken_condition_advances_without_side_effects() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0x13A0_0001]);
    cpu.cpsr |= Z_FLAG;

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 0);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.cycles, 1);
}

#[test]
fn arm_unsupported_instruction_enters_undefined_exception() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xEE00_0010]);

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_UNDEFINED);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.registers[15], VECTOR_UNDEFINED);
    assert_eq!(
        cpu.saved_status().unwrap(),
        MODE_SUPERVISOR | I_FLAG | F_FLAG
    );
}

#[test]
fn exception_cycles_include_faulting_fetch_wait_states() {
    let (mut arm_cpu, mut arm_bus) = cpu_with_rom(&0xEE00_0010u32.to_le_bytes());
    assert_eq!(arm_cpu.step_arm(&mut arm_bus).unwrap(), 8);
    assert_eq!(arm_cpu.mode(), MODE_UNDEFINED);
    assert_eq!(arm_cpu.cycles, 8);

    let (mut thumb_cpu, mut thumb_bus) = cpu_with_rom(&0xDE00u16.to_le_bytes());
    thumb_cpu.set_cpsr(MODE_SYSTEM | T_FLAG).unwrap();
    assert_eq!(thumb_cpu.step_thumb(&mut thumb_bus).unwrap(), 6);
    assert_eq!(thumb_cpu.mode(), MODE_UNDEFINED);
    assert_eq!(thumb_cpu.cycles, 6);
}

#[test]
fn rom_fetch_cycles_switch_from_nonsequential_to_sequential() {
    let instructions = [0xE3A0_0001u32, 0xE3A0_1002u32];
    let contents = instructions
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let (mut cpu, mut bus) = cpu_with_rom(&contents);

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 6);
    assert_eq!(
        cpu.sequential_fetch,
        Some((crate::mmu::ROM_START + 4, false))
    );
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 4);
}

#[test]
fn rom_fetch_after_branch_uses_nonsequential_wait_states() {
    let instructions = [
        0xEA00_0001u32,
        0xE3A0_0001u32,
        0xE3A0_1002u32,
        0xE3A0_2003u32,
    ];
    let contents = instructions
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let (mut cpu, mut bus) = cpu_with_rom(&contents);

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 8);
    assert_eq!(cpu.registers[15], crate::mmu::ROM_START + 12);
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 6);
}

#[test]
fn thumb_rom_fetches_use_sequential_wait_states_after_fallthrough() {
    let instructions = [0x2001u16, 0x2102u16];
    let contents = instructions
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let (mut cpu, mut bus) = cpu_with_rom(&contents);
    cpu.set_cpsr(MODE_SYSTEM | T_FLAG).unwrap();

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 4);
    assert_eq!(
        cpu.sequential_fetch,
        Some((crate::mmu::ROM_START + 2, true))
    );
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 2);
}

#[test]
fn condition_failed_unsupported_arm_instruction_does_not_enter_undefined() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0x0E00_0010]);

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_eq!(cpu.mode(), MODE_SUPERVISOR);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);

    cpu.registers[15] = TEST_PROGRAM_ADDRESS;
    cpu.cpsr |= Z_FLAG;
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_UNDEFINED);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.registers[15], VECTOR_UNDEFINED);
}

#[test]
fn arm_register_shifted_data_processing_executes() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE1A0_0101]);
    cpu.registers[1] = 3;

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 12);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
}

#[test]
fn arm_pc_as_register_shifted_operand_uses_plus_twelve() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE1B0_011F]);
    cpu.registers[1] = 0;

    cpu.step_arm(&mut bus).unwrap();

    assert_eq!(cpu.registers[0], TEST_PROGRAM_ADDRESS + 12);
}

#[test]
fn arm_movs_updates_carry_for_immediate_and_register_shift_edges() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[
        0xE1B0_0001, // MOVS r0, r1, LSL #0
        0xE1B0_0021, // MOVS r0, r1, LSR #0
        0xE1B0_0041, // MOVS r0, r1, ASR #0
        0xE1B0_0061, // MOVS r0, r1, ROR #0 (RRX)
        0xE1B0_0211, // MOVS r0, r1, LSL r2
    ]);
    cpu.registers[1] = 0x8000_0001;
    cpu.registers[2] = 32;
    cpu.cpsr |= C_FLAG;

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0x8000_0001);
    assert_ne!(cpu.cpsr & C_FLAG, 0);

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0);
    assert_ne!(cpu.cpsr & C_FLAG, 0);

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], u32::MAX);
    assert_ne!(cpu.cpsr & C_FLAG, 0);

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0xC000_0000);
    assert_ne!(cpu.cpsr & C_FLAG, 0);

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0);
    assert_ne!(cpu.cpsr & C_FLAG, 0);
}

#[test]
fn arm_data_processing_opcodes_write_expected_results_and_flags() {
    struct Case {
        opcode: u8,
        lhs: u32,
        carry_in: bool,
        expected_result: u32,
        writes_result: bool,
        expected_flags: u32,
    }

    let cases = [
        Case {
            opcode: 0x0,
            lhs: 0xF000_0001,
            carry_in: true,
            expected_result: 1,
            writes_result: true,
            expected_flags: C_FLAG | V_FLAG,
        },
        Case {
            opcode: 0x1,
            lhs: 0x8000_0000,
            carry_in: false,
            expected_result: 0x8000_0001,
            writes_result: true,
            expected_flags: N_FLAG | V_FLAG,
        },
        Case {
            opcode: 0x2,
            lhs: 0x8000_0000,
            carry_in: true,
            expected_result: 0x7FFF_FFFF,
            writes_result: true,
            expected_flags: C_FLAG | V_FLAG,
        },
        Case {
            opcode: 0x3,
            lhs: 0x8000_0000,
            carry_in: true,
            expected_result: 0x8000_0001,
            writes_result: true,
            expected_flags: N_FLAG | V_FLAG,
        },
        Case {
            opcode: 0x4,
            lhs: 0x7FFF_FFFF,
            carry_in: false,
            expected_result: 0x8000_0000,
            writes_result: true,
            expected_flags: N_FLAG | V_FLAG,
        },
        Case {
            opcode: 0x5,
            lhs: u32::MAX,
            carry_in: true,
            expected_result: 1,
            writes_result: true,
            expected_flags: C_FLAG,
        },
        Case {
            opcode: 0x6,
            lhs: 0,
            carry_in: false,
            expected_result: u32::MAX - 1,
            writes_result: true,
            expected_flags: N_FLAG,
        },
        Case {
            opcode: 0x7,
            lhs: 0x8000_0000,
            carry_in: true,
            expected_result: 0x8000_0001,
            writes_result: true,
            expected_flags: N_FLAG | V_FLAG,
        },
        Case {
            opcode: 0x8,
            lhs: 0,
            carry_in: true,
            expected_result: 0,
            writes_result: false,
            expected_flags: Z_FLAG | C_FLAG | V_FLAG,
        },
        Case {
            opcode: 0x9,
            lhs: u32::MAX,
            carry_in: false,
            expected_result: u32::MAX - 1,
            writes_result: false,
            expected_flags: N_FLAG | V_FLAG,
        },
        Case {
            opcode: 0xA,
            lhs: 1,
            carry_in: true,
            expected_result: 0,
            writes_result: false,
            expected_flags: Z_FLAG | C_FLAG,
        },
        Case {
            opcode: 0xB,
            lhs: u32::MAX,
            carry_in: false,
            expected_result: 0,
            writes_result: false,
            expected_flags: Z_FLAG | C_FLAG,
        },
        Case {
            opcode: 0xC,
            lhs: 0x8000_0000,
            carry_in: false,
            expected_result: 0x8000_0001,
            writes_result: true,
            expected_flags: N_FLAG | V_FLAG,
        },
        Case {
            opcode: 0xD,
            lhs: 0,
            carry_in: true,
            expected_result: 1,
            writes_result: true,
            expected_flags: C_FLAG | V_FLAG,
        },
        Case {
            opcode: 0xE,
            lhs: u32::MAX,
            carry_in: false,
            expected_result: u32::MAX - 1,
            writes_result: true,
            expected_flags: N_FLAG | V_FLAG,
        },
        Case {
            opcode: 0xF,
            lhs: 0,
            carry_in: true,
            expected_result: u32::MAX - 1,
            writes_result: true,
            expected_flags: N_FLAG | C_FLAG | V_FLAG,
        },
    ];

    for case in cases {
        let instruction =
            0xE200_0001 | (u32::from(case.opcode) << 21) | (1 << 20) | (1 << 16) | (2 << 12);
        let (mut cpu, mut bus) = cpu_with_arm_program(&[instruction]);
        cpu.registers[1] = case.lhs;
        cpu.registers[2] = 0xA5A5_A5A5;
        cpu.cpsr = MODE_SUPERVISOR | V_FLAG | if case.carry_in { C_FLAG } else { 0 };

        cpu.step_arm(&mut bus).unwrap();

        assert_eq!(
            cpu.registers[2],
            if case.writes_result {
                case.expected_result
            } else {
                0xA5A5_A5A5
            },
            "opcode {:#X} result",
            case.opcode
        );
        assert_eq!(
            cpu.cpsr & (N_FLAG | Z_FLAG | C_FLAG | V_FLAG),
            case.expected_flags,
            "opcode {:#X} flags",
            case.opcode
        );
        assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
    }
}

#[test]
fn arm_load_with_writeback_and_matching_base_keeps_loaded_value() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE5B0_0004, 0xE490_0004]);
    let base = crate::mmu::EWRAM_START;
    cpu.registers[0] = base;
    bus.write_u32(base + 4, 0xAABB_CCDD).unwrap();
    bus.write_u32(base, 0x1122_3344).unwrap();

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0xAABB_CCDD);

    cpu.registers[15] = TEST_PROGRAM_ADDRESS + 4;
    cpu.registers[0] = base;
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0x1122_3344);
}

#[test]
fn arm_register_shifted_transfer_offset_executes_and_rejects_register_shift() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE791_0102]);
    let base = crate::mmu::EWRAM_START;
    cpu.registers[1] = base;
    cpu.registers[2] = 1;
    bus.write_u32(base + 4, 0xAABB_CCDD).unwrap();

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0xAABB_CCDD);

    let (mut invalid_cpu, mut invalid_bus) = cpu_with_arm_program(&[0xE791_0112]);
    invalid_cpu.registers[1] = base;
    invalid_cpu.registers[2] = 1;
    assert_eq!(invalid_cpu.step_arm(&mut invalid_bus).unwrap(), 3);
    assert_eq!(invalid_cpu.mode(), MODE_UNDEFINED);
    assert_eq!(invalid_cpu.registers[15], VECTOR_UNDEFINED);
}

#[test]
fn arm_branch_exchange_changes_instruction_set() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE12F_FF11]);
    cpu.registers[1] = TEST_PROGRAM_ADDRESS + 0x21;

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_ne!(cpu.cpsr & (1 << 5), 0);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 0x20);
}

#[test]
fn mode_switches_preserve_fiq_and_exception_register_banks() {
    let mut cpu = Cpu::default();
    cpu.registers[8] = 0x88;
    cpu.registers[13] = 0x13;
    cpu.registers[14] = 0x14;

    cpu.set_cpsr((cpu.cpsr & !MODE_MASK) | MODE_FIQ).unwrap();
    assert_eq!(cpu.registers[8], 0);
    cpu.registers[8] = 0xF8;
    cpu.registers[13] = 0xF13;
    cpu.registers[14] = 0xF14;

    cpu.set_cpsr((cpu.cpsr & !MODE_MASK) | MODE_USER).unwrap();
    assert_eq!(cpu.registers[8], 0x88);
    assert_eq!(cpu.registers[13], 0);
    cpu.registers[13] = 0xA13;
    cpu.registers[14] = 0xA14;

    cpu.set_cpsr((cpu.cpsr & !MODE_MASK) | MODE_IRQ).unwrap();
    assert_eq!(cpu.registers[13], 0);
    cpu.registers[13] = 0x1D;
    cpu.set_cpsr((cpu.cpsr & !MODE_MASK) | MODE_USER).unwrap();
    assert_eq!(cpu.registers[13], 0xA13);
    cpu.set_cpsr((cpu.cpsr & !MODE_MASK) | MODE_IRQ).unwrap();
    assert_eq!(cpu.registers[13], 0x1D);
    cpu.set_cpsr((cpu.cpsr & !MODE_MASK) | MODE_SUPERVISOR)
        .unwrap();
    assert_eq!(cpu.registers[13], 0x13);
    cpu.set_cpsr((cpu.cpsr & !MODE_MASK) | MODE_FIQ).unwrap();
    assert_eq!(cpu.registers[8], 0xF8);
}

#[test]
fn software_interrupt_saves_status_and_exception_return_restores_it() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xEF00_0001]);
    cpu.cpsr = MODE_SYSTEM | T_FLAG | C_FLAG;
    let old_cpsr = cpu.cpsr;

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_SUPERVISOR);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.registers[15], 0x08);
    assert_eq!(cpu.saved_status().unwrap(), old_cpsr);

    cpu.registers[14] = TEST_PROGRAM_ADDRESS + 5;
    let return_address = cpu.registers[14];
    cpu.execute_arm_data_processing(0xD, true, 0, 15, return_address, false, 0x08)
        .unwrap();
    assert_eq!(cpu.mode(), MODE_SYSTEM);
    assert_ne!(cpu.cpsr & T_FLAG, 0);
    assert_ne!(cpu.cpsr & C_FLAG, 0);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
}

#[test]
fn conditional_arm_swi_enters_exception_only_when_condition_passes() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0x0F00_0001]);
    cpu.set_cpsr(MODE_SYSTEM).unwrap();

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_eq!(cpu.mode(), MODE_SYSTEM);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);

    cpu.registers[15] = TEST_PROGRAM_ADDRESS;
    cpu.cpsr |= Z_FLAG;
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_SUPERVISOR);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.registers[15], VECTOR_SWI);
    assert_eq!(cpu.saved_status().unwrap(), MODE_SYSTEM | Z_FLAG);
}

#[test]
fn irq_request_respects_mask_and_enters_banked_irq_mode() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[]);
    cpu.registers[13] = 0x1357;
    cpu.registers[15] = 0x0800_0100;
    assert!(!cpu.request_irq(&mut bus).unwrap());

    cpu.cpsr &= !I_FLAG;
    assert!(cpu.request_irq(&mut bus).unwrap());
    assert_eq!(cpu.mode(), MODE_IRQ);
    assert_eq!(cpu.registers[13], 0);
    assert_eq!(cpu.registers[14], 0x0800_0104);
    assert_eq!(cpu.registers[15], 0x18);
    assert_eq!(cpu.saved_status().unwrap() & I_FLAG, 0);

    cpu.set_cpsr((cpu.saved_status().unwrap() & !MODE_MASK) | MODE_SUPERVISOR)
        .unwrap();
    assert_eq!(cpu.registers[13], 0x1357);
}

#[test]
fn fiq_request_obeys_mask_saves_status_and_sets_both_interrupt_masks() {
    let mut cpu = Cpu::default();
    cpu.registers[15] = 0x0800_0100;
    let old_status = cpu.cpsr & !I_FLAG & !F_FLAG;
    cpu.cpsr = old_status | I_FLAG | F_FLAG;
    assert!(!cpu.request_fiq().unwrap());

    cpu.cpsr &= !F_FLAG;
    assert!(cpu.request_fiq().unwrap());
    assert_eq!(cpu.mode(), MODE_FIQ);
    assert_eq!(cpu.registers[14], 0x0800_0104);
    assert_eq!(cpu.registers[15], VECTOR_FIQ);
    assert_ne!(cpu.cpsr & (I_FLAG | F_FLAG), 0);
    assert_eq!(cpu.saved_status().unwrap(), old_status | I_FLAG);
}

#[test]
fn bios_lz77_services_decompress_to_ram_and_vram() {
    let compressed = [0x10, 4, 0, 0, 0x40, b'A', 0, 0];
    for (service, destination) in [
        (0x11, crate::mmu::EWRAM_START),
        (0x12, crate::mmu::VRAM_START),
    ] {
        let (mut cpu, mut bus) = cpu_with_program(&[]);
        let source = TEST_PROGRAM_ADDRESS + 0x100;
        bus.iwram[0x100..0x100 + compressed.len()].copy_from_slice(&compressed);
        cpu.set_cpsr(MODE_SYSTEM).unwrap();
        cpu.enter_exception(MODE_SUPERVISOR, VECTOR_SWI, TEST_PROGRAM_ADDRESS + 2)
            .unwrap();
        cpu.registers[0] = source;
        cpu.registers[1] = destination;

        cpu.handle_bios_swi(service, &mut bus).unwrap();

        assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 2);
        assert_eq!(
            (0..4)
                .map(|offset| bus.read_u8(destination + offset).unwrap())
                .collect::<Vec<_>>(),
            b"AAAA"
        );
    }
}

#[test]
fn bios_lz77_rejects_backreferences_before_output() {
    let (mut cpu, mut bus) = cpu_with_program(&[]);
    let source = TEST_PROGRAM_ADDRESS + 0x100;
    bus.iwram[0x100..0x108].copy_from_slice(&[0x10, 3, 0, 0, 0x80, 0, 0, 0]);
    cpu.set_cpsr(MODE_SYSTEM).unwrap();
    cpu.enter_exception(MODE_SUPERVISOR, VECTOR_SWI, TEST_PROGRAM_ADDRESS + 2)
        .unwrap();
    cpu.registers[0] = source;
    cpu.registers[1] = crate::mmu::EWRAM_START;

    let error = cpu.handle_bios_swi(0x11, &mut bus).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        error.to_string(),
        "BIOS LZ77 back-reference precedes the decompressed output"
    );
}

#[test]
fn arm_block_transfer_s_bit_reads_and_writes_user_banked_registers() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE8E0_2102, 0xE8F0_2102]);
    cpu.set_cpsr(MODE_USER).unwrap();
    cpu.registers[1] = 0x11;
    cpu.registers[8] = 0x88;
    cpu.registers[13] = 0xD13;
    cpu.set_cpsr(MODE_SUPERVISOR).unwrap();
    cpu.registers[0] = TEST_PROGRAM_ADDRESS + 0x100;

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(bus.read_u32(TEST_PROGRAM_ADDRESS + 0x100).unwrap(), 0x11);
    assert_eq!(bus.read_u32(TEST_PROGRAM_ADDRESS + 0x104).unwrap(), 0x88);
    assert_eq!(bus.read_u32(TEST_PROGRAM_ADDRESS + 0x108).unwrap(), 0xD13);

    bus.write_u32(TEST_PROGRAM_ADDRESS + 0x100, 0x22).unwrap();
    bus.write_u32(TEST_PROGRAM_ADDRESS + 0x104, 0x99).unwrap();
    bus.write_u32(TEST_PROGRAM_ADDRESS + 0x108, 0xD99).unwrap();
    cpu.registers[0] = TEST_PROGRAM_ADDRESS + 0x100;
    cpu.step_arm(&mut bus).unwrap();

    cpu.set_cpsr(MODE_USER).unwrap();
    assert_eq!(cpu.registers[1], 0x22);
    assert_eq!(cpu.registers[8], 0x99);
    assert_eq!(cpu.registers[13], 0xD99);
}

#[test]
fn arm_ldm_with_s_and_pc_restores_spsr_and_thumb_state() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE8F0_8002]);
    let base = TEST_PROGRAM_ADDRESS + 0x100;
    cpu.registers[0] = base;
    bus.write_u32(base, 0x1234_5678).unwrap();
    bus.write_u32(base + 4, TEST_PROGRAM_ADDRESS + 0x203)
        .unwrap();
    cpu.saved_status[2] = MODE_SYSTEM | T_FLAG | C_FLAG;

    cpu.step_arm(&mut bus).unwrap();

    assert_eq!(cpu.mode(), MODE_SYSTEM);
    assert_ne!(cpu.cpsr & T_FLAG, 0);
    assert_ne!(cpu.cpsr & C_FLAG, 0);
    assert_eq!(cpu.registers[1], 0x1234_5678);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 0x202);
    assert_eq!(cpu.registers[0], base + 8);
}

#[test]
fn arm_load_from_the_bios_region_reads_open_bus_without_aborting() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE5D1_000A]); // LDRB r0, [r1, #10]
    cpu.registers[1] = 0;
    let mode = cpu.mode();
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.mode(), mode);
    assert_eq!(cpu.registers[0], 0x29);
}

#[test]
fn arm_multiply_and_long_multiply_execute() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE000_0291, 0xE081_0392, 0xE0A1_0392]);
    cpu.registers[1] = 7;
    cpu.registers[2] = 6;
    cpu.registers[3] = 5;
    cpu.registers[4] = 0xFFFF_FFFF;

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 2);
    assert_eq!(cpu.registers[0], 42);
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!((cpu.registers[1], cpu.registers[0]), (0, 30));
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 4);
    assert_eq!(cpu.registers[0], 60);
}

#[test]
fn arm_status_register_transfers_read_and_update_selected_fields() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[
        0xE10F_0000,
        0xE128_F000,
        0xE10F_1000,
        0xE328_F480,
        0xE10F_2000,
    ]);
    cpu.cpsr = MODE_SUPERVISOR | I_FLAG;

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], MODE_SUPERVISOR | I_FLAG);

    cpu.registers[0] = N_FLAG | C_FLAG;
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.cpsr, MODE_SUPERVISOR | I_FLAG | N_FLAG | C_FLAG);

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], MODE_SUPERVISOR | I_FLAG | N_FLAG | C_FLAG);

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.cpsr, MODE_SUPERVISOR | I_FLAG | N_FLAG);
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[2], MODE_SUPERVISOR | I_FLAG | N_FLAG);
}

#[test]
fn arm_user_mode_msr_cannot_change_control_or_interrupt_mask_bits() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE121_F000]);
    cpu.set_cpsr(MODE_USER | C_FLAG).unwrap();
    cpu.registers[0] = MODE_IRQ | I_FLAG | N_FLAG;

    cpu.step_arm(&mut bus).unwrap();

    assert_eq!(cpu.mode(), MODE_USER);
    assert_eq!(cpu.cpsr & (I_FLAG | F_FLAG), 0);
    assert_eq!(cpu.cpsr & (N_FLAG | C_FLAG), C_FLAG);
}

#[test]
fn arm_status_transfers_access_the_spsr_and_preserve_unselected_fields() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE14F_1000, 0xE168_F000, 0xE14F_2000]);
    let old_status = MODE_USER | T_FLAG | I_FLAG;
    cpu.saved_status[2] = old_status;
    cpu.registers[0] = N_FLAG;

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], old_status);
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.saved_status[2], old_status | N_FLAG);
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[2], old_status | N_FLAG);
}

#[test]
fn arm_swap_and_swap_byte_exchange_memory_atomically() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE102_1093, 0xE142_1093]);
    let address = TEST_PROGRAM_ADDRESS + 0x100;
    cpu.registers[2] = address;
    cpu.registers[3] = 0xAABB_CCDD;
    bus.write_u32(address, 0x1122_3344).unwrap();

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], 0x1122_3344);
    assert_eq!(bus.read_u32(address).unwrap(), 0xAABB_CCDD);

    cpu.registers[3] = 0x55;
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], 0xDD);
    assert_eq!(bus.read_u8(address).unwrap(), 0x55);
}

#[test]
fn arm_unaligned_word_swap_enters_undefined_exception() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE102_1093]);
    cpu.registers[2] = TEST_PROGRAM_ADDRESS + 0x101;
    cpu.registers[3] = 0xAABB_CCDD;

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_UNDEFINED);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.registers[15], VECTOR_UNDEFINED);
}

#[test]
fn arm_halfword_and_signed_byte_transfers_work() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE1C1_00B2, 0xE1D1_20B2, 0xE1D1_30D2]);
    cpu.registers[0] = 0xABCD;
    cpu.registers[1] = TEST_PROGRAM_ADDRESS + 0x100;
    bus.write_u8(TEST_PROGRAM_ADDRESS + 0x102, 0x80).unwrap();

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 2);
    assert_eq!(bus.read_u16(TEST_PROGRAM_ADDRESS + 0x102).unwrap(), 0xABCD);
    bus.write_u8(TEST_PROGRAM_ADDRESS + 0x102, 0x80).unwrap();
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[2], 0xAB80);
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[3], 0xFFFF_FF80);
}

#[test]
fn arm_block_transfers_load_store_multiple_registers_with_writeback() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE8A0_0006, 0xE8B0_0018]);
    cpu.registers[0] = TEST_PROGRAM_ADDRESS + 0x100;
    cpu.registers[1] = 0x1122_3344;
    cpu.registers[2] = 0x5566_7788;

    // STM: (n-1)S+2N, LDM: nS+1N+1I.
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[0], TEST_PROGRAM_ADDRESS + 0x108);
    cpu.registers[0] = TEST_PROGRAM_ADDRESS + 0x100;
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 4);
    assert_eq!(cpu.registers[3], 0x1122_3344);
    assert_eq!(cpu.registers[4], 0x5566_7788);
    assert_eq!(cpu.registers[0], TEST_PROGRAM_ADDRESS + 0x108);
}

#[test]
fn arm_block_load_suppresses_writeback_when_base_is_in_register_list() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE8B3_000C]);
    let base = TEST_PROGRAM_ADDRESS + 0x100;
    cpu.registers[3] = base;
    bus.write_u32(base, 0x1122_3344).unwrap();
    bus.write_u32(base + 4, 0x5566_7788).unwrap();

    cpu.step_arm(&mut bus).unwrap();

    assert_eq!(cpu.registers[2], 0x1122_3344);
    assert_eq!(cpu.registers[3], 0x5566_7788);
}

#[test]
fn arm_empty_block_transfer_uses_pc_and_sixteen_register_writeback_span() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE8A0_0000, 0xE8B0_0000]);
    let base = TEST_PROGRAM_ADDRESS + 0x100;
    cpu.registers[0] = base;

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(bus.read_u32(base).unwrap(), TEST_PROGRAM_ADDRESS + 12);
    assert_eq!(cpu.registers[0], base + 0x40);

    cpu.registers[0] = base;
    bus.write_u32(base, TEST_PROGRAM_ADDRESS + 0x200).unwrap();
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], base + 0x40);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 0x200);
}

#[test]
fn thumb_shift_alu_and_conditional_branch_execute() {
    let (mut cpu, mut bus) = cpu_with_program(&[0x0088, 0x4308, 0xD001, 0x2109, 0x2107]);
    cpu.registers[1] = 3;
    cpu.cpsr |= Z_FLAG;

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 12);
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 15);
    cpu.cpsr |= Z_FLAG;
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 10);
    assert_eq!(cpu.registers[1], 3);
}

#[test]
fn thumb_high_register_pc_writes_branch_without_sequential_pc_overwrite() {
    let (mut cpu, mut bus) = cpu_with_program(&[0x4687]);
    cpu.cpsr |= T_FLAG;
    cpu.registers[0] = TEST_PROGRAM_ADDRESS + 0x21;

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 0x20);
    assert_ne!(cpu.cpsr & T_FLAG, 0);

    let (mut cpu, mut bus) = cpu_with_program(&[0x4487]);
    cpu.cpsr |= T_FLAG;
    cpu.registers[0] = 0x20;

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 0x24);
    assert_ne!(cpu.cpsr & T_FLAG, 0);
}

#[test]
fn thumb_branch_exchange_switches_state_and_uses_refill_cycles() {
    let (mut cpu, mut bus) = cpu_with_program(&[0x4700]);
    cpu.cpsr |= T_FLAG;
    cpu.registers[0] = TEST_PROGRAM_ADDRESS + 0x24;

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 0x24);
    assert_eq!(cpu.cpsr & T_FLAG, 0);

    let (mut cpu, mut bus) = cpu_with_program(&[0x4700]);
    cpu.cpsr |= T_FLAG;
    cpu.registers[0] = TEST_PROGRAM_ADDRESS + 0x25;

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 0x24);
    assert_ne!(cpu.cpsr & T_FLAG, 0);
}

#[test]
fn thumb_alu_and_register_shift_opcodes_use_correct_operations() {
    let cases = [
        (0x4008, 0xF0, 0x3C, 0x30, None),
        (0x4048, 0xF0, 0x3C, 0xCC, None),
        (0x4088, 0x8000_0001, 1, 0x0000_0002, Some(true)),
        (0x40C8, 0x8000_0001, 1, 0x4000_0000, Some(true)),
        (0x4108, 0x8000_0000, 1, 0xC000_0000, Some(false)),
        (0x41C8, 0x8000_0001, 1, 0xC000_0000, Some(true)),
    ];

    for (opcode, lhs, rhs, expected, expected_carry) in cases {
        let (mut cpu, mut bus) = cpu_with_program(&[opcode]);
        cpu.registers[0] = lhs;
        cpu.registers[1] = rhs;

        cpu.step_thumb(&mut bus).unwrap();

        assert_eq!(cpu.registers[0], expected, "opcode {opcode:#06X}");
        if let Some(carry) = expected_carry {
            assert_eq!(cpu.cpsr & C_FLAG != 0, carry, "opcode {opcode:#06X}");
        }
    }
}

#[test]
fn thumb_neg_cmp_and_cmn_update_all_arithmetic_flags() {
    let cases = [
        (0x4248, 0, 1, u32::MAX, N_FLAG),
        (0x4288, 0, 1, 0, N_FLAG),
        (0x42C8, 0x7FFF_FFFF, 1, 0x7FFF_FFFF, N_FLAG | V_FLAG),
    ];

    for (opcode, lhs, rhs, expected_r0, expected_flags) in cases {
        let (mut cpu, mut bus) = cpu_with_program(&[opcode]);
        cpu.registers[0] = lhs;
        cpu.registers[1] = rhs;

        cpu.step_thumb(&mut bus).unwrap();

        assert_eq!(cpu.registers[0], expected_r0, "opcode {opcode:#06X}");
        assert_eq!(
            cpu.cpsr & (N_FLAG | Z_FLAG | C_FLAG | V_FLAG),
            expected_flags,
            "opcode {opcode:#06X}"
        );
    }
}

#[test]
fn thumb_push_and_pop_transfer_low_register_and_lr() {
    let (mut cpu, mut bus) = cpu_with_program(&[0xB401, 0xBC01]);
    cpu.registers[13] = TEST_PROGRAM_ADDRESS + 0x100;
    cpu.registers[0] = 0x1234_5678;

    cpu.step_thumb(&mut bus).unwrap();
    assert_eq!(cpu.registers[13], TEST_PROGRAM_ADDRESS + 0xFC);
    assert_eq!(
        bus.read_u32(TEST_PROGRAM_ADDRESS + 0xFC).unwrap(),
        0x1234_5678
    );

    cpu.registers[0] = 0;
    cpu.step_thumb(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0x1234_5678);
    assert_eq!(cpu.registers[13], TEST_PROGRAM_ADDRESS + 0x100);
}

#[test]
fn thumb_pop_pc_branches_to_loaded_address() {
    let (mut cpu, mut bus) = cpu_with_program(&[0xBD01, 0x2001, 0x2102]);
    cpu.set_cpsr(MODE_SYSTEM | T_FLAG).unwrap();
    cpu.registers[13] = crate::mmu::IWRAM_START + 0x100;
    bus.write_u32(cpu.registers[13], 0x1234_5678).unwrap();
    bus.write_u32(cpu.registers[13] + 4, (TEST_PROGRAM_ADDRESS + 4) | 1)
        .unwrap();

    cpu.step_thumb(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0x1234_5678);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.registers[13], crate::mmu::IWRAM_START + 0x108);
    assert_ne!(cpu.cpsr & T_FLAG, 0);

    cpu.step_thumb(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], 2);
}

#[test]
fn thumb_software_interrupt_preserves_exception_vector_pc() {
    let (mut cpu, mut bus) = cpu_with_program(&[0xDF01]);
    cpu.cpsr = MODE_SYSTEM | T_FLAG;

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.mode(), MODE_SUPERVISOR);
    assert_eq!(cpu.registers[14], TEST_PROGRAM_ADDRESS + 2);
    assert_eq!(cpu.registers[15], 0x08);
    assert_eq!(cpu.saved_status().unwrap(), MODE_SYSTEM | T_FLAG);
}

#[test]
fn arm_never_condition_is_skipped_instead_of_raising_undefined() {
    // MOVNV r0, #7
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xF3A0_0007]);
    cpu.registers[0] = 1;
    let mode = cpu.mode();
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 1);
    assert_eq!(cpu.mode(), mode);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
}

#[test]
fn arm_multiply_cycles_depend_on_multiplier_magnitude() {
    // MUL r0, r1, r2 repeated with different multipliers in r2.
    for (multiplier, expected) in [
        (0x0000_00FF, 2),
        (0xFFFF_FF00, 2),
        (0x0000_FFFF, 3),
        (0x00FF_FFFF, 4),
        (0x1234_5678, 5),
    ] {
        let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE000_0291]);
        cpu.registers[1] = 3;
        cpu.registers[2] = multiplier;
        assert_eq!(cpu.step_arm(&mut bus).unwrap(), expected, "{multiplier:#X}");
    }
}

#[test]
fn arm_swap_costs_one_sequential_two_nonsequential_and_one_internal_cycle() {
    // SWP r1, r3, [r2]
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE102_1093]);
    cpu.registers[2] = 0x0300_0100;
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 4);
}

#[test]
fn halfword_loads_at_odd_addresses_rotate_or_sign_extend_the_byte() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[
        0xE1D1_00B1, // LDRH  r0, [r1, #1]
        0xE1D1_20F1, // LDRSH r2, [r1, #1]
    ]);
    bus.write_u16(0x0300_0100, 0x80F0).unwrap();
    cpu.registers[1] = 0x0300_0100;
    cpu.step_arm(&mut bus).unwrap();
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[0], 0xF000_0080);
    assert_eq!(cpu.registers[2], 0xFFFF_FF80);
}

#[test]
fn halfword_load_with_matching_base_and_destination_keeps_loaded_value() {
    // LDRH r1, [r1, #2]!
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE1F1_10B2]);
    bus.write_u16(0x0300_0102, 0xBEEF).unwrap();
    cpu.registers[1] = 0x0300_0100;
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.registers[1], 0xBEEF);
}

#[test]
fn msr_cpsr_cannot_change_the_thumb_state_bit() {
    // MSR cpsr_c, r0
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE121_F000]);
    cpu.registers[0] = MODE_SYSTEM | T_FLAG;
    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.cpsr & T_FLAG, 0);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
}

#[test]
fn thumb_load_multiple_and_pop_pc_use_datasheet_cycle_counts() {
    // LDMIA r0!, {r1, r2} ; POP {r3, pc}
    let (mut cpu, mut bus) = cpu_with_program(&[0xC806, 0xBD08]);
    cpu.registers[0] = 0x0300_0100;
    cpu.registers[13] = 0x0300_0200;
    bus.write_u32(0x0300_0204, 0x0300_0000).unwrap();
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 4);
    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 5);
}

#[test]
fn bios_service_hooks_ignore_ordinary_code() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE1A0_0000]);
    assert_eq!(cpu.step_bios_hle(&mut bus).unwrap(), None);
}
