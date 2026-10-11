use super::*;
use crate::mmu::IO_REG_START;
const MODE_SUPERVISOR: u32 = 0x13;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);
const ROM_START: u32 = 0x0800_0000;

fn rom_file(opcode: u16) -> std::path::PathBuf {
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-system-test-{}-{id}.gba",
        std::process::id()
    ));
    let mut rom = vec![0; 8];
    rom[..2].copy_from_slice(&opcode.to_le_bytes());
    rom[2..4].copy_from_slice(&0x2103u16.to_le_bytes());
    fs::write(&path, rom).unwrap();
    path
}

fn arm_rom_file(instruction: u32) -> std::path::PathBuf {
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-system-arm-test-{}-{id}.gba",
        std::process::id()
    ));
    fs::write(&path, instruction.to_le_bytes()).unwrap();
    path
}

#[test]
fn system_owner_steps_cpu_against_its_rom_bus() {
    let path = rom_file(0x2007);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;

    assert_eq!(emulator.step_thumb().unwrap(), 4);
    assert_eq!(emulator.cpu().registers[0], 7);
    assert_eq!(emulator.cpu().registers[15], ROM_START + 2);
    assert_eq!(emulator.cpu().cycles, 4);
}

#[test]
fn system_owner_dispatches_arm_mode_from_cpsr() {
    let path = arm_rom_file(0xE3A0_0007);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;

    assert_eq!(emulator.step().unwrap(), 6);
    assert_eq!(emulator.cpu().registers[0], 7);
    assert_eq!(emulator.cpu().registers[15], ROM_START + 4);
}

#[test]
fn gba_startup_initializes_cpu_mode_and_banked_stacks() {
    let path = arm_rom_file(0xE1A0_0000);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();

    emulator.initialize_gba_startup(ROM_START).unwrap();
    assert_eq!(emulator.cpu().mode(), 0x1F);
    assert_eq!(emulator.cpu().registers[13], 0x0300_7F00);
    assert_eq!(emulator.cpu().registers[15], ROM_START);

    emulator.cpu_mut().set_cpsr(0x12).unwrap();
    assert_eq!(emulator.cpu().registers[13], 0x0300_7FA0);
    emulator.cpu_mut().set_cpsr(0x13).unwrap();
    assert_eq!(emulator.cpu().registers[13], 0x0300_7FE0);
    emulator.cpu_mut().set_cpsr(0x1F).unwrap();
    assert_eq!(emulator.cpu().registers[13], 0x0300_7F00);
}

#[test]
fn soft_reset_restarts_at_the_cartridge_entry_in_system_mode() {
    let path = rom_file(0xDF00);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;

    emulator.step().unwrap();
    assert_eq!(emulator.cpu().registers[15], 0x08);
    assert_eq!(emulator.cpu().mode(), MODE_SUPERVISOR);

    assert_eq!(emulator.step().unwrap(), 0);
    assert_eq!(emulator.cpu().registers[15], ROM_START);
    assert_eq!(emulator.cpu().mode(), 0x1F);
    assert_eq!(emulator.cpu().registers[13], 0x0300_7F00);
    assert_eq!(emulator.cpu().registers[0], 0);
}

#[test]
fn system_handles_thumb_register_ram_reset_swi_and_returns_from_svc() {
    let path = rom_file(0xDF01);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;
    emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();
    emulator.cpu_mut().registers[0] = 0x03;
    emulator
        .memory_mut()
        .write_u8(crate::mmu::EWRAM_START, 0xA5)
        .unwrap();
    emulator
        .memory_mut()
        .write_u8(crate::mmu::IWRAM_START, 0x5A)
        .unwrap();

    emulator.step().unwrap();
    assert_eq!(emulator.cpu().mode(), MODE_SUPERVISOR);
    assert_eq!(emulator.cpu().registers[15], 0x08);

    assert_eq!(emulator.step().unwrap(), 0);
    assert_eq!(emulator.cpu().mode(), 0x1F);
    assert_eq!(emulator.cpu().cpsr & THUMB_STATE, THUMB_STATE);
    assert_eq!(emulator.cpu().registers[15], ROM_START + 2);
    assert_eq!(
        emulator.memory().read_u8(crate::mmu::EWRAM_START).unwrap(),
        0
    );
    assert_eq!(
        emulator.memory().read_u8(crate::mmu::IWRAM_START).unwrap(),
        0
    );
}

#[test]
fn vblank_intr_wait_halts_until_the_irq_handler_flags_vblank() {
    let path = rom_file(0xDF05);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;
    emulator
        .cpu_mut()
        .set_cpsr(0x1F | THUMB_STATE | (1 << 7))
        .unwrap();

    emulator.step().unwrap();
    assert_eq!(emulator.cpu().registers[15], 0x08);

    // HLE rewinds to the SWI and halts while no flag has been raised.
    assert_eq!(emulator.step().unwrap(), 0);
    assert!(emulator.cpu().halted);
    assert_eq!(emulator.cpu().registers[15], ROM_START);
    assert_eq!(emulator.cpu().mode(), 0x1F);
    let idle = emulator.step().unwrap();
    assert!(idle > 0 && emulator.cpu().halted);

    // A VBlank interrupt wakes the CPU; the game handler would set BIOS_IF.
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x200, 1)
        .unwrap();
    emulator.memory_mut().io_regs[0x202] = 1;
    emulator.memory_mut().write_u16(0x0300_7FF8, 1).unwrap();
    emulator.step().unwrap(); // wakes and re-executes the SWI
    assert!(!emulator.cpu().halted);
    assert_eq!(emulator.cpu().registers[15], 0x08);
    assert_eq!(emulator.step().unwrap(), 0);
    assert_eq!(emulator.cpu().registers[15], ROM_START + 2);
    assert_eq!(emulator.memory().read_u16(0x0300_7FF8).unwrap(), 0);
}

#[test]
fn bios_irq_stub_saves_registers_and_returns_to_the_interrupted_code() {
    let path = arm_rom_file(0xE1A0_0000);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    let handler = 0x0300_3580;
    emulator
        .memory_mut()
        .write_u32(0x0300_7FFC, handler)
        .unwrap();
    // Handler is a single `bx lr`.
    emulator
        .memory_mut()
        .write_u32(handler, 0xE12F_FF1E)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x200, 1)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x208, 1)
        .unwrap();
    emulator.memory_mut().io_regs[0x202] = 1;
    let cpsr = emulator.cpu().cpsr & !(1 << 7);
    emulator.cpu_mut().set_cpsr(cpsr).unwrap();
    emulator.cpu_mut().registers[0] = 0xAAAA;
    emulator.cpu_mut().registers[12] = 0xBBBB;

    emulator.step().unwrap(); // nop at ROM_START, then IRQ entry
    assert_eq!(emulator.cpu().mode(), 0x12);
    assert_eq!(emulator.cpu().registers[15], handler);
    assert_eq!(emulator.cpu().registers[14], crate::cpu::BIOS_IRQ_RETURN);
    // Handler acknowledges the interrupt, clobbers registers, returns.
    emulator.memory_mut().io_regs[0x202] = 0;
    emulator.cpu_mut().registers[0] = 0;
    emulator.cpu_mut().registers[12] = 0;
    emulator.step().unwrap(); // bx lr
    assert_eq!(emulator.cpu().registers[15], crate::cpu::BIOS_IRQ_RETURN);
    emulator.step().unwrap(); // HLE ldmfd + subs pc, lr, #4

    assert_eq!(emulator.cpu().mode(), 0x1F);
    assert_eq!(emulator.cpu().registers[15], ROM_START + 4);
    assert_eq!(emulator.cpu().registers[0], 0xAAAA);
    assert_eq!(emulator.cpu().registers[12], 0xBBBB);
    assert_eq!(emulator.cpu().registers[13], 0x0300_7F00);
}

#[test]
fn system_handles_thumb_cpuset_copy_and_word_fill() {
    let path = rom_file(0xDF0B);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;
    emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();
    emulator
        .memory_mut()
        .write_u16(crate::mmu::EWRAM_START, 0x1234)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(crate::mmu::EWRAM_START + 2, 0x5678)
        .unwrap();
    emulator.cpu_mut().registers[0] = crate::mmu::EWRAM_START;
    emulator.cpu_mut().registers[1] = crate::mmu::IWRAM_START;
    emulator.cpu_mut().registers[2] = 2;

    emulator.step().unwrap();
    emulator.step().unwrap();
    assert_eq!(
        emulator.memory().read_u32(crate::mmu::IWRAM_START).unwrap(),
        0x5678_1234
    );

    emulator.cpu_mut().registers[15] = ROM_START;
    emulator.cpu_mut().registers[0] = crate::mmu::EWRAM_START;
    emulator.cpu_mut().registers[1] = crate::mmu::IWRAM_START + 8;
    emulator.cpu_mut().registers[2] = (1 << 26) | (1 << 24) | 2;
    emulator.step().unwrap();
    emulator.step().unwrap();
    assert_eq!(
        emulator
            .memory()
            .read_u32(crate::mmu::IWRAM_START + 8)
            .unwrap(),
        0x5678_1234
    );
    assert_eq!(
        emulator
            .memory()
            .read_u32(crate::mmu::IWRAM_START + 12)
            .unwrap(),
        0x5678_1234
    );
}

#[test]
fn system_decodes_arm_swi_service_from_low_byte() {
    let path = arm_rom_file(0xEF00_0001);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    emulator.cpu_mut().registers[0] = 1;
    emulator
        .memory_mut()
        .write_u8(crate::mmu::EWRAM_START, 0xA5)
        .unwrap();

    emulator.step().unwrap();
    assert_eq!(emulator.cpu().registers[15], 0x08);
    emulator.step().unwrap();

    assert_eq!(emulator.cpu().mode(), 0x1F);
    assert_eq!(emulator.cpu().registers[15], ROM_START + 4);
    assert_eq!(
        emulator.memory().read_u8(crate::mmu::EWRAM_START).unwrap(),
        0
    );
}

#[test]
fn system_advances_timers_and_enters_irq_when_enabled() {
    let path = arm_rom_file(0xE1A0_0000);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;
    let cpsr = emulator.cpu().cpsr & !(1 << 7);
    emulator.cpu_mut().set_cpsr(cpsr).unwrap();

    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x100, 0xFFFE)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x102, 0x00C0)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x200, 1 << 3)
        .unwrap();
    emulator
        .memory_mut()
        .write_u8(IO_REG_START + 0x208, 1)
        .unwrap();

    assert_eq!(emulator.step().unwrap(), 9);
    assert_eq!(emulator.cpu().mode(), 0x12);
    assert_eq!(emulator.cpu().registers[15], 0x18);
    assert_eq!(emulator.cpu().cycles, 9);
}

#[test]
fn vblank_irq_dispatches_to_the_bios_handler_pointer() {
    let path = arm_rom_file(0xE1A0_0000);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    let handler = 0x0300_3580;
    emulator
        .memory_mut()
        .write_u32(0x0300_7FFC, handler)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x004, 1 << 3)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x200, 1)
        .unwrap();
    emulator
        .memory_mut()
        .write_u16(IO_REG_START + 0x208, 1)
        .unwrap();
    emulator.memory_mut().advance(1232 * 160 - 1);

    emulator.step().unwrap();

    assert_eq!(emulator.cpu().mode(), 0x12);
    assert_eq!(emulator.cpu().registers[15], handler);
    assert_eq!(emulator.cpu().saved_status().unwrap() & 0x1F, 0x1F);
    assert_eq!(
        emulator.memory().read_u16(IO_REG_START + 0x202).unwrap() & 1,
        1
    );
}

#[test]
fn obj_affine_set_writes_rotation_matrix_with_output_stride() {
    let path = rom_file(0xDF0F);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.cpu_mut().registers[15] = ROM_START;
    emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();
    let source = crate::mmu::EWRAM_START;
    let dest = crate::mmu::IWRAM_START;
    let bus = emulator.memory_mut();
    bus.write_u16(source, 0x0200).unwrap(); // sx = 2.0
    bus.write_u16(source + 2, 0x0100).unwrap(); // sy = 1.0
    bus.write_u16(source + 4, 0x4000).unwrap(); // 90 degrees
    emulator.cpu_mut().registers[0] = source;
    emulator.cpu_mut().registers[1] = dest;
    emulator.cpu_mut().registers[2] = 1;
    emulator.cpu_mut().registers[3] = 8;

    emulator.step().unwrap();
    emulator.step().unwrap();

    let read = |offset: u32| emulator.memory().read_u16(dest + offset).unwrap() as i16;
    assert_eq!(read(0), 0); // pa = sx*cos
    assert_eq!(read(8), -0x200); // pb = -sx*sin
    assert_eq!(read(16), 0x100); // pc = sy*sin
    assert_eq!(read(24), 0); // pd = sy*cos
}

#[test]
fn disabling_rendering_skips_drawing_but_not_frame_timing() {
    // ARM `b .`; the display starts in forced blank (white) so drawing
    // would change the framebuffer from its initial all-zero state.
    let path = arm_rom_file(0xEAFF_FFFE);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();

    emulator.set_rendering(false);
    let mut steps = 0;
    while !emulator.take_frame_ready() {
        emulator.step().unwrap();
        steps += 1;
        assert!(steps < 1_000_000, "frame never completed");
    }
    assert!(emulator.ppu().framebuffer().iter().all(|p| *p == [0; 4]));

    emulator.set_rendering(true);
    while !emulator.take_frame_ready() {
        emulator.step().unwrap();
    }
    assert!(emulator.ppu().framebuffer().iter().any(|p| *p != [0; 4]));
}

// ---- idle-loop skipping ------------------------------------------------

fn arm_program_file(words: &[u32]) -> std::path::PathBuf {
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-idle-test-{}-{id}.gba",
        std::process::id()
    ));
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    fs::write(&path, bytes).unwrap();
    path
}

fn emulator_for(words: &[u32], skipping: bool) -> Emulator {
    let path = arm_program_file(words);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    emulator.set_rendering(false);
    emulator.set_idle_loop_skipping(skipping);
    emulator
}

/// Steps until the PC reaches `target`; returns `(cpu cycles, steps)`.
fn run_to_pc(emulator: &mut Emulator, target: u32) -> (u64, u64) {
    let mut steps = 0;
    while emulator.cpu().registers[15] != target {
        emulator.step().unwrap();
        steps += 1;
        assert!(steps < 5_000_000, "never reached {target:#X}");
    }
    (emulator.cpu().cycles, steps)
}

/// `ldr r1,=0x03000000; loop: ldr r0,[r1]; cmp r0,#0; beq loop; b .`
const FLAG_POLL: [u32; 6] = [
    0xE59F_100C,
    0xE591_0000,
    0xE350_0000,
    0x0AFF_FFFC,
    0xEAFF_FFFE,
    0x0300_0000,
];

fn arm_hblank_dma_sets_flag(emulator: &mut Emulator) {
    let io = IO_REG_START;
    let memory = emulator.memory_mut();
    memory.write_u16(crate::mmu::EWRAM_START, 1).unwrap();
    memory
        .write_u32(io + 0xB0, crate::mmu::EWRAM_START)
        .unwrap();
    memory
        .write_u32(io + 0xB4, crate::mmu::IWRAM_START)
        .unwrap();
    memory.write_u16(io + 0xB8, 1).unwrap();
    // Enable, HBlank timing, 16-bit units, one transfer.
    memory.write_u16(io + 0xBA, 0x8000 | (2 << 12)).unwrap();
}

#[test]
fn a_flag_polling_loop_exits_at_the_same_cycle_when_skipped() {
    let mut plain = emulator_for(&FLAG_POLL, false);
    let mut fast = emulator_for(&FLAG_POLL, true);
    arm_hblank_dma_sets_flag(&mut plain);
    arm_hblank_dma_sets_flag(&mut fast);

    let (plain_cycles, plain_steps) = run_to_pc(&mut plain, ROM_START + 0x10);
    let (fast_cycles, fast_steps) = run_to_pc(&mut fast, ROM_START + 0x10);

    assert!(plain_cycles > 900, "waited for the first HBlank");
    assert_eq!(fast_cycles, plain_cycles, "exit lands on the same cycle");
    assert_eq!(fast.cpu().registers, plain.cpu().registers);
    assert!(
        fast_steps * 10 < plain_steps,
        "skipping should remove most steps: {fast_steps} vs {plain_steps}"
    );
}

#[test]
fn counting_loops_are_never_skipped() {
    // mov r0,#0x400; loop: subs r0,r0,#1; bne loop; b .
    let program = [0xE3A0_0B01, 0xE250_0001, 0x1AFF_FFFD, 0xEAFF_FFFE];
    let mut plain = emulator_for(&program, false);
    let mut fast = emulator_for(&program, true);

    let (plain_cycles, plain_steps) = run_to_pc(&mut plain, ROM_START + 0x0C);
    let (fast_cycles, fast_steps) = run_to_pc(&mut fast, ROM_START + 0x0C);

    assert_eq!(fast_cycles, plain_cycles);
    assert_eq!(fast_steps, plain_steps, "registers change every iteration");
    assert_eq!(fast.cpu().registers[0], 0);
}

#[test]
fn loops_waiting_on_a_timer_counter_are_not_skipped() {
    // ldr r1,=TM0CNT; loop: ldrh r0,[r1]; cmp r0,#0x800; blo loop; b .
    let program = [
        0xE59F_1010,
        0xE1D1_00B0,
        0xE350_0B02,
        0x3AFF_FFFC,
        0xEAFF_FFFE,
        0x0000_0000,
        0x0400_0100,
    ];
    let mut plain = emulator_for(&program, false);
    let mut fast = emulator_for(&program, true);
    for emulator in [&mut plain, &mut fast] {
        let memory = emulator.memory_mut();
        memory.write_u16(IO_REG_START + 0x100, 0).unwrap();
        memory.write_u16(IO_REG_START + 0x102, 0x0080).unwrap();
    }

    let (plain_cycles, plain_steps) = run_to_pc(&mut plain, ROM_START + 0x10);
    let (fast_cycles, fast_steps) = run_to_pc(&mut fast, ROM_START + 0x10);

    assert!(plain_cycles >= 0x800, "timer counted to the threshold");
    assert_eq!(fast_cycles, plain_cycles);
    assert_eq!(fast_steps, plain_steps, "the counter varies per cycle");
}

#[test]
fn loops_that_write_memory_are_not_skipped() {
    // ldr r1,=0x03000000; loop: str r0,[r1]; cmp r0,#0; beq loop
    let program = [
        0xE59F_100C,
        0xE581_0000,
        0xE350_0000,
        0x0AFF_FFFC,
        0,
        0x0300_0000,
    ];
    let mut plain = emulator_for(&program, false);
    let mut fast = emulator_for(&program, true);

    for emulator in [&mut plain, &mut fast] {
        for _ in 0..2 {
            while !emulator.take_frame_ready() {
                emulator.step().unwrap();
            }
        }
    }
    // Same cycles and, because nothing was skipped, identical work done.
    assert_eq!(fast.cpu().cycles, plain.cpu().cycles);
    assert_eq!(fast.cpu().registers, plain.cpu().registers);
    assert!(fast.memory().write_count() > 1000);
    assert_eq!(fast.memory().write_count(), plain.memory().write_count());
}

#[test]
fn skipping_does_not_change_frame_timing() {
    // A bare `b .` is the most idle program possible.
    let program = [0xEAFF_FFFE];
    let mut plain = emulator_for(&program, false);
    let mut fast = emulator_for(&program, true);
    let mut fast_steps = 0u64;
    let mut plain_steps = 0u64;
    for _ in 0..5 {
        while !fast.take_frame_ready() {
            fast.step().unwrap();
            fast_steps += 1;
        }
        while !plain.take_frame_ready() {
            plain.step().unwrap();
            plain_steps += 1;
        }
        assert_eq!(fast.cpu().cycles, plain.cpu().cycles);
    }
    // Audio samples (every 512 cycles) bound each skip: about 18x fewer.
    assert!(
        fast_steps * 10 < plain_steps,
        "{fast_steps} vs {plain_steps}"
    );
}

#[test]
fn reset_restarts_the_console_but_keeps_the_save() {
    // A flash cartridge whose program is a bare `b .`.
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-reset-test-{}-{id}.gba",
        std::process::id()
    ));
    let mut rom = vec![0u8; 0x200];
    rom[..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    rom[0x100..0x10C].copy_from_slice(b"FLASH1M_V103");
    fs::write(&path, rom).unwrap();
    // `reset` reopens the ROM, so the file must outlive the emulator.
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    emulator.set_rendering(false);

    // Run, dirty RAM and registers, and program one byte of Flash.
    for _ in 0..3 {
        while !emulator.take_frame_ready() {
            emulator.step().unwrap();
        }
    }
    let bus = emulator.memory_mut();
    bus.write_u32(crate::mmu::IWRAM_START, 0xDEAD_BEEF).unwrap();
    bus.write_u16(IO_REG_START + 0x200, 0x0001).unwrap();
    for (address, value) in [
        (0x5555, 0xAA),
        (0x2AAA, 0x55),
        (0x5555, 0xA0),
        (0x0042, 0x7E),
    ] {
        bus.write_u8(0x0E00_0000 + address, value).unwrap();
    }
    emulator.cpu_mut().registers[0] = 99;
    assert!(emulator.cpu().cycles > 0);

    emulator.reset().unwrap();

    // Back at boot: ROM entry, zeroed RAM and registers, fresh timing.
    assert_eq!(emulator.cpu().registers[15], ROM_START);
    assert_eq!(emulator.cpu().registers[0], 0);
    assert_eq!(emulator.cpu().cycles, 0);
    assert_eq!(emulator.cpu().mode(), 0x1F);
    assert_eq!(
        emulator.memory().read_u32(crate::mmu::IWRAM_START).unwrap(),
        0
    );
    assert_eq!(emulator.memory().read_u16(IO_REG_START + 0x200).unwrap(), 0);
    assert!(!emulator.take_frame_ready());
    // The save survived, and the console runs again.
    assert_eq!(emulator.memory().read_u8(0x0E00_0042).unwrap(), 0x7E);
    for _ in 0..2 {
        while !emulator.take_frame_ready() {
            emulator.step().unwrap();
        }
    }
    assert!(emulator.cpu().cycles > 0);
    fs::remove_file(path).unwrap();
}

/// Options with the cheats file's raw codes switched on.
fn codes_on() -> CheatOptions {
    CheatOptions {
        file_codes: true,
        ..CheatOptions::default()
    }
}

#[test]
fn cheats_override_a_game_that_keeps_rewriting_the_value() {
    use crate::cheats::Cheats;
    // Forever: halfword [0x03007D4E] = 0x12.
    let program = [
        0xE3A0_0403, // mov  r0, #0x03000000
        0xE280_0C7D, // add  r0, r0, #0x7D00
        0xE280_004E, // add  r0, r0, #0x4E
        0xE3A0_1012, // mov  r1, #0x12
        0xE1C0_10B0, // strh r1, [r0]
        0xEAFF_FFFD, // b    (back to the strh)
    ];
    let run_frame = |emulator: &mut Emulator| {
        while !emulator.take_frame_ready() {
            emulator.step().unwrap();
        }
    };

    let mut plain = emulator_for(&program, false);
    run_frame(&mut plain);
    assert_eq!(plain.memory().read_u16(0x0300_7D4E).unwrap(), 0x0012);

    let mut cheating = emulator_for(&program, false);
    cheating.set_cheats(Cheats::parse("83007D4E 0093"));
    cheating.set_cheat_options(codes_on());
    run_frame(&mut cheating);
    assert_eq!(
        cheating.memory().read_u16(0x0300_7D4E).unwrap(),
        0x0012,
        "the cheat is applied at the frame boundary, not during the frame"
    );
    cheating.apply_cheats();
    assert_eq!(cheating.memory().read_u16(0x0300_7D4E).unwrap(), 0x0093);

    // The game overwrites it again during the next frame; the cheat wins
    // again at the end of it.
    run_frame(&mut cheating);
    assert_eq!(cheating.memory().read_u16(0x0300_7D4E).unwrap(), 0x0012);
    cheating.apply_cheats();
    assert_eq!(cheating.memory().read_u16(0x0300_7D4E).unwrap(), 0x0093);
}

/// A loop that stores 0x12 to [0x03007D4E]; the instruction at ROM offset
/// 0x14 is the "hook" the cheats attach to.
const HOOK_PROGRAM: [u32; 7] = [
    0xE3A0_0403, // mov  r0, #0x03000000
    0xE280_0C7D, // add  r0, r0, #0x7D00
    0xE280_004E, // add  r0, r0, #0x4E
    0xE3A0_1012, // mov  r1, #0x12
    0xE1C0_10B0, // strh r1, [r0]      (offset 0x10)
    0xE1A0_2002, // mov  r2, r2        (offset 0x14, the hook)
    0xEAFF_FFFC, // b    strh
];

fn run_until_pc(emulator: &mut Emulator, pc: u32) {
    for _ in 0..1000 {
        if emulator.cpu().registers[15] == pc {
            return;
        }
        emulator.step().unwrap();
    }
    panic!("never reached {pc:#010X}");
}

#[test]
fn cheats_tied_to_a_hook_apply_when_execution_reaches_it() {
    use crate::cheats::Cheats;
    let mut emulator = emulator_for(&HOOK_PROGRAM, false);
    emulator.set_cheats(Cheats::parse("10000014 0007\n83007D4E 0093"));
    emulator.set_cheat_options(codes_on());
    let slot = 0x0300_7D4E;

    run_until_pc(&mut emulator, 0x0800_0014);
    assert_eq!(emulator.memory().read_u16(slot).unwrap(), 0x0012);
    emulator.step().unwrap(); // the hook instruction: writes, then executes
    assert_eq!(emulator.memory().read_u16(slot).unwrap(), 0x0093);

    // Back around the loop the game stores its value again...
    run_until_pc(&mut emulator, 0x0800_0014);
    assert_eq!(emulator.memory().read_u16(slot).unwrap(), 0x0012);

    // ...and the per-frame application is not used for a hooked set.
    emulator.memory_mut().write_u16(slot, 0x0077).unwrap();
    emulator.apply_cheats();
    assert_eq!(emulator.memory().read_u16(slot).unwrap(), 0x0077);
}

#[test]
fn the_cheats_file_rows_switch_gates_its_codes_and_can_be_flipped_back() {
    use crate::cheats::Cheats;
    let mut emulator = emulator_for(&HOOK_PROGRAM, false);
    emulator.set_cheats(Cheats::parse("10000014 0007\n83007D4E 0093"));
    // Off to begin with: the default options leave the codes alone.
    assert!(!emulator.cheat_options().file_codes);
    let slot = 0x0300_7D4E;

    run_until_pc(&mut emulator, 0x0800_0014);
    emulator.step().unwrap();
    assert_eq!(emulator.memory().read_u16(slot).unwrap(), 0x0012);

    emulator.set_cheat_options(codes_on());
    run_until_pc(&mut emulator, 0x0800_0014);
    emulator.step().unwrap();
    assert_eq!(emulator.memory().read_u16(slot).unwrap(), 0x0093);

    // Switched off again, the codes stop (the game's own value returns).
    emulator.set_cheat_options(CheatOptions::default());
    run_until_pc(&mut emulator, 0x0800_0014);
    assert_eq!(emulator.memory().read_u16(slot).unwrap(), 0x0012);

    // Unhooked codes honour the switch at the frame boundary too.
    let mut plain = emulator_for(&HOOK_PROGRAM, false);
    plain.set_cheats(Cheats::parse("83007D4E 0093"));
    plain.apply_cheats();
    assert_eq!(plain.memory().read_u16(slot).unwrap(), 0);
    plain.set_cheat_options(codes_on());
    plain.apply_cheats();
    assert_eq!(plain.memory().read_u16(slot).unwrap(), 0x0093);
}

/// A ROM shaped like a FireRed build around `CreateWildMon`: the header holds
/// `game_code`; the code loads species 0x13 and level 4, then switches to
/// Thumb state and calls a function at ROM offset `function` that begins with
/// the real function's prologue (the signature the scan looks for) and spins.
/// Once the prologue has run, `r4` holds the species the function received.
fn wild_mon_rom(game_code: &[u8; 4], function: usize) -> std::path::PathBuf {
    let mut rom = vec![0u8; 0x8_3000];
    let mut put = |offset: usize, word: u32| {
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    };
    put(0x00, 0xEA00_002E); // b 0xC0 (over the header)
    put(0xC0, 0xE3A0_0013); // mov r0, #0x13
    put(0xC4, 0xE3A0_1004); // mov r1, #4
    put(0xC8, 0xE59F_2004); // ldr r2, [pc, #4]   (the word at 0xD4)
    put(0xCC, 0xE12F_FF12); // bx  r2
    put(0xD4, 0x0800_0000 + function as u32 + 1); // Thumb entry
    rom[0xAC..0xB0].copy_from_slice(game_code);
    rom[function..function + 16].copy_from_slice(&crate::cheats::firered::wild_species::SIGNATURE);
    rom[function + 16..function + 18].copy_from_slice(&0xE7FEu16.to_le_bytes()); // b .
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("gba-wild-test-{}-{id}.gba", std::process::id()));
    std::fs::write(&path, rom).unwrap();
    path
}

/// Runs the ROM with `options` and returns `(species, level)` as the
/// function received them (`r4`, `r6` once its prologue has masked its
/// arguments), whether the build was recognised, and the hook address.
fn wild_run(
    game_code: &[u8; 4],
    function: usize,
    options: CheatOptions,
    file: &str,
) -> (u32, u32, bool, Option<u32>) {
    use crate::cheats::Cheats;
    let path = wild_mon_rom(game_code, function);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    std::fs::remove_file(path).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    emulator.set_cheats(Cheats::parse(file));
    emulator.set_cheat_options(options);
    for _ in 0..24 {
        emulator.step().unwrap();
    }
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
fn the_wild_row_only_applies_to_firered_roms_and_clamps_the_level() {
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

/// A ROM shaped like a FireRed build around the catch check: after
/// bookkeeping, Thumb code compares `r6` ("odds") with 0xFE exactly as the
/// real command does, and records the outcome in `r2` (1 = caught, 2 = the
/// shake roll). `r6` is loaded with `odds` by the ARM stub.
fn catch_rom(game_code: &[u8; 4], function: usize, odds: u32) -> std::path::PathBuf {
    let mut rom = vec![0u8; 0x8_3000];
    let mut put = |offset: usize, word: u32| {
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    };
    put(0x00, 0xEA00_002E); // b 0xC0 (over the header)
    put(0xC0, 0xE3A0_1403); // mov r1, #0x03000000  (scratch for the strb)
    put(0xC4, 0xE3A0_0000); // mov r0, #0           (catchAttempts)
    put(0xC8, 0xE3A0_6000 | (odds & 0xFF)); // mov r6, #odds
    put(0xCC, 0xE59F_3004); // ldr r3, [pc, #4]     (the word at 0xD8)
    put(0xD0, 0xE12F_FF13); // bx  r3
    put(0xD8, 0x0800_0000 + function as u32 + 1); // Thumb entry
    rom[0xAC..0xB0].copy_from_slice(game_code);
    let mut put16 = |offset: usize, half: u16| {
        rom[offset..offset + 2].copy_from_slice(&half.to_le_bytes());
    };
    put16(function, 0x28FE); // cmp  r0, #0xFE
    put16(function + 2, 0xD801); // bhi  +1
    put16(function + 4, 0x3001); // adds r0, #1
    put16(function + 6, 0x7008); // strb r0, [r1]
    put16(function + 8, 0x2EFE); // cmp  r6, #0xFE      <- the hook
    put16(function + 10, 0xD92F); // bls  shake_roll
    put16(function + 12, 0x2201); // movs r2, #1        (caught)
    put16(function + 14, 0xE7FE); // b    .
    // The `bls` lands 4 + 2 * 0x2F bytes after itself.
    let shakes = function + 10 + 4 + 2 * 0x2F;
    put16(shakes, 0x2202); // movs r2, #2               (shake roll)
    put16(shakes + 2, 0xE7FE); // b .
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("gba-catch-test-{}-{id}.gba", std::process::id()));
    std::fs::write(&path, rom).unwrap();
    path
}

/// Runs the ROM and returns the outcome in `r2` and the hook address.
fn catch_outcome(game_code: &[u8; 4], odds: u32, options: CheatOptions) -> (u32, Option<u32>) {
    let path = catch_rom(game_code, 0x8_2A00, odds);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    std::fs::remove_file(path).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    emulator.set_cheat_options(options);
    for _ in 0..40 {
        emulator.step().unwrap();
    }
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
