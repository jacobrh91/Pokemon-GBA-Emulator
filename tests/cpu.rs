use gba_emulator::cpu::Cpu;
use gba_emulator::mmu::{IWRAM_START, MemoryBus};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

const TEST_PROGRAM_ADDRESS: u32 = IWRAM_START;

fn empty_bus() -> MemoryBus {
    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-cpu-integration-{}-{id}.gba",
        std::process::id()
    ));
    fs::write(&path, [0; 4]).unwrap();
    let bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    bus
}

fn cpu_with_thumb_program(program: &[u16]) -> (Cpu, MemoryBus) {
    let mut bus = empty_bus();
    for (index, opcode) in program.iter().enumerate() {
        bus.iwram[index * 2..index * 2 + 2].copy_from_slice(&opcode.to_le_bytes());
    }

    let mut cpu = Cpu::default();
    cpu.registers[15] = TEST_PROGRAM_ADDRESS;
    (cpu, bus)
}

fn cpu_with_arm_program(program: &[u32]) -> (Cpu, MemoryBus) {
    let mut bus = empty_bus();
    for (index, instruction) in program.iter().enumerate() {
        bus.iwram[index * 4..index * 4 + 4].copy_from_slice(&instruction.to_le_bytes());
    }

    let mut cpu = Cpu::default();
    cpu.registers[15] = TEST_PROGRAM_ADDRESS;
    (cpu, bus)
}

#[test]
fn thumb_instructions_execute_and_accumulate_cycles() {
    let (mut cpu, mut bus) = cpu_with_thumb_program(&[0x2007, 0x3001]);

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 7);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 2);
    assert_eq!(cpu.cycles, 1);

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 8);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 4);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn thumb_branch_updates_pc_and_counts_refill_cycles() {
    let (mut cpu, mut bus) = cpu_with_thumb_program(&[0xE001]);

    assert_eq!(cpu.step_thumb(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 6);
    assert_eq!(cpu.cycles, 3);
}

#[test]
fn arm_data_processing_updates_registers_and_flags() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE3A0_0007, 0xE280_0001, 0xE350_0008]);

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 7);
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_eq!(cpu.registers[0], 8);
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 1);
    assert_ne!(cpu.cpsr & (1 << 30), 0);
    assert_eq!(cpu.registers[0], 8);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 12);
    assert_eq!(cpu.cycles, 3);
}

#[test]
fn arm_condition_and_branch_pipeline_are_executed() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[
        0xE350_0000, // CMP r0, #0
        0x0A00_0001, // BEQ +1
        0xE3A0_1001, // MOV r1, #1 (skipped)
    ]);

    cpu.step_arm(&mut bus).unwrap();
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[15], TEST_PROGRAM_ADDRESS + 16);
    assert_eq!(cpu.registers[1], 0);
    assert_eq!(cpu.cycles, 4);
}

#[test]
fn arm_single_data_transfers_read_and_write_memory() {
    let (mut cpu, mut bus) = cpu_with_arm_program(&[0xE581_0004, 0xE591_2004]);
    cpu.registers[0] = 0x1234_5678;
    cpu.registers[1] = TEST_PROGRAM_ADDRESS + 0x100;

    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 2);
    assert_eq!(
        bus.read_u32(TEST_PROGRAM_ADDRESS + 0x104).unwrap(),
        0x1234_5678
    );
    assert_eq!(cpu.step_arm(&mut bus).unwrap(), 3);
    assert_eq!(cpu.registers[2], 0x1234_5678);
    assert_eq!(cpu.cycles, 5);
}
