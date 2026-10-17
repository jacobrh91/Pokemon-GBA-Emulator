use super::display::VBLANK_START_CYCLES;
use super::interrupts::KEYPAD_IRQ_BIT;
use super::*;
use crate::mmu::registers::{
    DISPSTAT, INTERRUPT_ENABLE, INTERRUPT_MASTER_ENABLE, INTERRUPT_REQUEST, KEYCNT, KEYINPUT,
    VCOUNT,
};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);

fn bus_with_rom(contents: &[u8]) -> MemoryBus {
    let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gba-emulator-test-rom-{}-{id}.bin",
        std::process::id()
    ));
    fs::write(&path, contents).unwrap();
    let bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    bus
}

#[test]
fn reads_and_writes_ram_and_io_bytes_and_words() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u8(EWRAM_START, 0x12).unwrap();
    bus.write_u32(IWRAM_START, 0x7856_3412).unwrap();
    bus.write_u32(IO_REG_START, 0xDEAD_BEEF).unwrap();

    assert_eq!(bus.read_u8(EWRAM_START).unwrap(), 0x12);
    assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0x3412);
    assert_eq!(bus.read_u32(IWRAM_START).unwrap(), 0x7856_3412);
    assert_eq!(bus.read_u32(IO_REG_START).unwrap(), 0xDEAD_BEEF);
}

#[test]
fn save_ram_is_writable_and_mirrors_across_the_backup_memory_window() {
    let mut bus = bus_with_rom(&[0; 4]);
    let address = 0x0E00_5555;

    assert_eq!(bus.read_u8(address).unwrap(), 0xFF);
    bus.write_u8(address, 0xA5).unwrap();

    assert_eq!(bus.read_u8(0x0E01_5555).unwrap(), 0xA5);
    assert_eq!(bus.transfer_cycles(address, 1, false).unwrap(), 4);
}

#[test]
fn ram_words_wrap_at_mirrored_bank_boundaries() {
    let mut bus = bus_with_rom(&[0; 4]);
    let address = EWRAM_START + EWRAM_MASK - 1;

    bus.write_u32(address, 0x7856_3412).unwrap();

    assert_eq!(bus.read_u32(address).unwrap(), 0x7856_3412);
    assert_eq!(bus.ewram[0], 0x56);
    assert_eq!(bus.ewram[1], 0x78);
}

#[test]
fn ram_palette_oam_and_vram_mirror_aliases_share_storage() {
    let mut bus = bus_with_rom(&[0; 4]);

    bus.write_u8(EWRAM_START, 0x11).unwrap();
    bus.write_u8(IWRAM_START, 0x22).unwrap();
    bus.write_u8(PALETTE_RAM_START, 0x33).unwrap();
    bus.write_u8(VRAM_START + 0x1_0000, 0x44).unwrap();
    bus.write_u8(OAM_START, 0x55).unwrap();

    assert_eq!(bus.read_u8(EWRAM_START + 0x0040_0000).unwrap(), 0x11);
    assert_eq!(bus.read_u8(IWRAM_START + 0x0000_8000).unwrap(), 0x22);
    assert_eq!(bus.read_u8(PALETTE_RAM_START + 0x400).unwrap(), 0x33);
    assert_eq!(bus.read_u8(VRAM_START + 0x1_8000).unwrap(), 0x44);
    assert_eq!(bus.read_u8(OAM_START + 0x400).unwrap(), 0x55);
}

#[test]
fn graphics_memory_regions_are_mapped_and_vram_mirrors_correctly() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(PALETTE_RAM_START, 0x1234).unwrap();
    bus.write_u16(VRAM_START + 0x1_8000, 0x5678).unwrap();
    bus.write_u16(OAM_START, 0x9ABC).unwrap();

    assert_eq!(bus.read_u16(PALETTE_RAM_START).unwrap(), 0x1234);
    assert_eq!(bus.read_u16(VRAM_START + 0x1_0000,).unwrap(), 0x5678);
    assert_eq!(bus.read_u16(OAM_START).unwrap(), 0x9ABC);
}

#[test]
fn rom_writes_are_ignored_and_out_of_range_accesses_are_reported() {
    let mut bus = bus_with_rom(&[0x12, 0x34, 0x56, 0x78]);

    assert_eq!(bus.read_u32(ROM_START).unwrap(), 0x7856_3412);
    bus.write_u16(ROM_START + 0x01FE_2FFE, 0x1234).unwrap();
    bus.write_u8(ROM_START, 0).unwrap();
    bus.write_u16(ROM_START + 1, 0).unwrap();
    bus.write_u32(ROM_START, 0).unwrap();
    bus.write_u16(ROM_START + 0x01FE_2FFE, 0x1234).unwrap();
    assert!(
        bus.transfer_cycles(ROM_START + 0x01FE_2FFE, 2, false)
            .is_ok()
    );
    assert_eq!(bus.read_u32(ROM_START).unwrap(), 0x7856_3412);
    assert_eq!(bus.read_u8(ROM_START + 4).unwrap(), 0x12);
    // Past the cartridge the bus echoes the halfword address.
    let odd_sized = bus_with_rom(&[0; 6]);
    assert_eq!(odd_sized.read_u16(ROM_START + 0x100).unwrap(), 0x0080);
    // BIOS and unmapped regions return open bus rather than faulting.
    assert_eq!(bus.read_u32(0).unwrap(), BIOS_OPEN_BUS_RESET);
    bus.set_open_bus(0xCAFE_F00D);
    assert_eq!(bus.read_u32(IO_REG_END + 1).unwrap(), 0xCAFE_F00D);
    bus.write_u32(IO_REG_END + 1, 0).unwrap();
    assert_eq!(bus.read_u8(ROM_END).unwrap(), 0x78);
}

#[test]
fn dispstat_vblank_flag_tracks_frame_cycles() {
    let mut bus = bus_with_rom(&[0; 4]);

    bus.advance(CYCLES_PER_SCANLINE * (VBLANK_START_SCANLINE - 1));
    assert_eq!(bus.read_u16(IO_REG_START + DISPSTAT as u32).unwrap() & 1, 0);

    bus.advance(CYCLES_PER_SCANLINE);
    assert_eq!(bus.read_u16(IO_REG_START + DISPSTAT as u32).unwrap() & 1, 1);

    bus.advance(CYCLES_PER_FRAME - CYCLES_PER_SCANLINE * VBLANK_START_SCANLINE);
    assert_eq!(bus.read_u16(IO_REG_START + DISPSTAT as u32).unwrap() & 1, 0);
}

#[test]
fn vblank_irq_is_requested_when_enabled_and_acknowledged_write_one_to_clear() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(IO_REG_START + DISPSTAT as u32, 1 << 3)
        .unwrap();
    bus.write_u16(IO_REG_START + INTERRUPT_ENABLE as u32, 1)
        .unwrap();
    bus.write_u16(IO_REG_START + INTERRUPT_MASTER_ENABLE as u32, 1)
        .unwrap();

    bus.advance(VBLANK_START_CYCLES - 1);
    assert!(!bus.irq_pending());
    bus.advance(1);
    assert!(bus.irq_pending());

    bus.write_u16(IO_REG_START + INTERRUPT_REQUEST as u32, 1)
        .unwrap();
    assert!(!bus.irq_pending());
}

#[test]
fn vcount_tracks_scanlines_and_wraps_at_frame_boundary() {
    let mut bus = bus_with_rom(&[0; 4]);

    bus.advance(CYCLES_PER_SCANLINE * 224);
    assert_eq!(bus.read_u16(IO_REG_START + VCOUNT as u32).unwrap(), 224);

    bus.advance(CYCLES_PER_SCANLINE * 4);
    assert_eq!(bus.read_u16(IO_REG_START + VCOUNT as u32).unwrap(), 0);
}

#[test]
fn keyinput_defaults_to_released_and_is_read_only() {
    let mut bus = bus_with_rom(&[0; 4]);
    assert_eq!(
        bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
        0x03FF
    );

    bus.set_pressed_buttons(crate::input::A);
    assert_eq!(
        bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
        0x03FE
    );
    bus.write_u16(IO_REG_START + KEYINPUT as u32, 0).unwrap();
    assert_eq!(
        bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
        0x03FE
    );
}

#[test]
fn timer_counts_reloads_and_requests_irq() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(IO_REG_START + 0x100, 0xFFFE).unwrap();
    bus.write_u16(IO_REG_START + 0x102, 0x00C0).unwrap();
    bus.advance(2);

    assert_eq!(bus.read_u16(IO_REG_START + 0x100).unwrap(), 0xFFFE);
    assert_ne!(
        u16::from_le_bytes([
            bus.io_regs[INTERRUPT_REQUEST],
            bus.io_regs[INTERRUPT_REQUEST + 1]
        ]) & (1 << 3),
        0
    );
}

#[test]
fn cascaded_timers_count_each_source_overflow_once() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(IO_REG_START + 0x100, 0xFFFF).unwrap();
    bus.write_u16(IO_REG_START + 0x102, 0x0080).unwrap();
    bus.write_u16(IO_REG_START + 0x104, 0xFFFE).unwrap();
    bus.write_u16(IO_REG_START + 0x106, 0x00C4).unwrap();
    bus.write_u16(IO_REG_START + 0x108, 0xFFFE).unwrap();
    bus.write_u16(IO_REG_START + 0x10A, 0x0084).unwrap();

    bus.advance(2);

    assert_eq!(bus.read_u16(IO_REG_START + 0x104).unwrap(), 0xFFFE);
    assert_eq!(bus.read_u16(IO_REG_START + 0x108).unwrap(), 0xFFFF);
    assert_ne!(bus.io_regs[INTERRUPT_REQUEST] & (1 << 4), 0);
    assert_eq!(bus.io_regs[INTERRUPT_REQUEST] & (1 << 5), 0);
}

#[test]
fn timer_advancement_handles_large_cycle_batches_without_per_tick_loops() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(IO_REG_START + 0x100, 0xFFFE).unwrap();
    bus.write_u16(IO_REG_START + 0x102, 0x00C0).unwrap();

    bus.advance(1_000_002);

    assert_eq!(bus.read_u16(IO_REG_START + 0x100).unwrap(), 0xFFFE);
    assert_ne!(bus.io_regs[INTERRUPT_REQUEST] & (1 << 3), 0);
}

#[test]
fn interrupt_flags_are_write_one_to_clear_and_irq_requires_enable() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.io_regs[INTERRUPT_REQUEST] = 0b1010;
    bus.write_u16(IO_REG_START + INTERRUPT_REQUEST as u32, 0b0010)
        .unwrap();
    assert_eq!(bus.io_regs[INTERRUPT_REQUEST], 0b1000);
    assert!(!bus.irq_pending());

    bus.write_u16(IO_REG_START + INTERRUPT_ENABLE as u32, 0b1000)
        .unwrap();
    bus.write_u8(IO_REG_START + INTERRUPT_MASTER_ENABLE as u32, 1)
        .unwrap();
    assert!(bus.irq_pending());
}

#[test]
fn transfer_cycle_cost_depends_on_memory_region_and_width() {
    let mut bus = bus_with_rom(&[0; 4]);
    assert_eq!(bus.transfer_cycles(EWRAM_START, 2, false).unwrap(), 3);
    assert_eq!(bus.transfer_cycles(IWRAM_START, 4, false).unwrap(), 1);
    // 32-bit buses: BIOS, I/O and OAM words are single accesses; 16-bit
    // palette RAM and VRAM take two.
    assert_eq!(bus.transfer_cycles(0, 4, false).unwrap(), 1);
    assert_eq!(bus.transfer_cycles(IO_REG_START, 4, false).unwrap(), 1);
    assert_eq!(bus.transfer_cycles(OAM_START, 4, false).unwrap(), 1);
    assert_eq!(bus.transfer_cycles(PALETTE_RAM_START, 4, false).unwrap(), 2);
    assert_eq!(bus.transfer_cycles(VRAM_START, 4, false).unwrap(), 2);
    assert_eq!(bus.transfer_cycles(ROM_START, 2, false).unwrap(), 4);
    assert_eq!(bus.transfer_cycles(ROM_START, 2, true).unwrap(), 2);

    assert_eq!(bus.transfer_cycles(0x0A00_0000, 2, true).unwrap(), 4);
    assert_eq!(bus.transfer_cycles(0x0C00_0000, 2, true).unwrap(), 8);

    bus.write_u16(IO_REG_START + 0x204, 0x049C).unwrap();
    assert_eq!(bus.transfer_cycles(ROM_START, 2, false).unwrap(), 8);
    assert_eq!(bus.transfer_cycles(ROM_START, 2, true).unwrap(), 1);
    assert_eq!(bus.transfer_cycles(0x0A00_0000, 2, true).unwrap(), 1);
    assert_eq!(bus.transfer_cycles(0x0C00_0000, 2, true).unwrap(), 1);
}

#[test]
fn all_gamepak_waitstate_regions_map_the_rom_window() {
    let bus = bus_with_rom(&[0x12, 0x34, 0x56, 0x78]);
    for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
        assert_eq!(bus.read_u32(base).unwrap(), 0x7856_3412);
    }
}

fn program_dma(
    bus: &mut MemoryBus,
    channel: u32,
    source: u32,
    dest: u32,
    count: u16,
    control: u16,
) {
    let base = IO_REG_START + 0xB0 + channel * 12;
    bus.write_u32(base, source).unwrap();
    bus.write_u32(base + 4, dest).unwrap();
    bus.write_u16(base + 8, count).unwrap();
    bus.write_u16(base + 10, control).unwrap();
}

#[test]
fn immediate_dma_copies_words_and_clears_enable() {
    let mut bus = bus_with_rom(&[0; 4]);
    for index in 0..4 {
        bus.write_u32(EWRAM_START + index * 4, 0x1111_0000 + index)
            .unwrap();
    }
    program_dma(&mut bus, 3, EWRAM_START, IWRAM_START, 4, 0x8000 | (1 << 10));

    for index in 0..4 {
        assert_eq!(
            bus.read_u32(IWRAM_START + index * 4).unwrap(),
            0x1111_0000 + index
        );
    }
    assert_eq!(bus.read_u16(IO_REG_START + 0xDE).unwrap() & 0x8000, 0);
    assert!(bus.take_dma_cycles() > 8);
    assert_eq!(bus.take_dma_cycles(), 0);
}

#[test]
fn dma_zero_count_means_maximum_and_fixed_source_fills() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(EWRAM_START, 0xABCD).unwrap();
    // DMA0, count 0 => 0x4000 halfwords, fixed source.
    program_dma(&mut bus, 0, EWRAM_START, IWRAM_START, 0, 0x8000 | (2 << 7));
    assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0xABCD);
    assert_eq!(bus.read_u16(IWRAM_START + 0x7FFE).unwrap(), 0xABCD);
}

#[test]
fn vblank_dma_waits_for_vblank_and_repeats_with_irq() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(EWRAM_START, 0x1234).unwrap();
    // 1 halfword, VBlank timing, repeat, IRQ, fixed destination.
    let control = 0x8000 | (1 << 14) | (1 << 12) | (1 << 9) | (2 << 7) | (2 << 5);
    program_dma(&mut bus, 1, EWRAM_START, IWRAM_START, 1, control);
    assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0);

    bus.advance(VBLANK_START_CYCLES + 1);
    assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0x1234);
    assert_ne!(bus.read_u16(IO_REG_START + 0x202).unwrap() & (1 << 9), 0);
    assert_ne!(bus.read_u16(IO_REG_START + 0xC6).unwrap() & 0x8000, 0);

    bus.write_u16(EWRAM_START, 0x5678).unwrap();
    bus.advance(CYCLES_PER_FRAME);
    assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0x5678);
}

#[test]
fn hblank_dma_runs_once_per_visible_scanline() {
    let mut bus = bus_with_rom(&[0; 4]);
    for line in 0..3 {
        bus.write_u16(EWRAM_START + line * 2, 0x100 + line as u16)
            .unwrap();
    }
    // 1 halfword per HBlank, repeat, incrementing source and destination.
    program_dma(
        &mut bus,
        2,
        EWRAM_START,
        IWRAM_START,
        1,
        0x8000 | (2 << 12) | (1 << 9),
    );
    bus.advance(CYCLES_PER_SCANLINE * 3);
    for line in 0..3 {
        assert_eq!(
            bus.read_u16(IWRAM_START + line * 2).unwrap(),
            0x100 + line as u16
        );
    }
    assert_eq!(bus.read_u16(IWRAM_START + 6).unwrap(), 0);
}

#[test]
fn dma_address_registers_are_write_only() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u32(IO_REG_START + 0xD4, 0x0200_0000).unwrap();
    assert_eq!(bus.read_u32(IO_REG_START + 0xD4).unwrap(), 0);
}

#[test]
fn register_ram_reset_preserves_the_top_of_iwram() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u32(IWRAM_START + 0x7FFC, 0xDEAD_BEEF).unwrap();
    bus.write_u32(IWRAM_START, 0x1234).unwrap();
    bus.register_ram_reset(2);
    assert_eq!(bus.read_u32(IWRAM_START).unwrap(), 0);
    assert_eq!(bus.read_u32(IWRAM_START + 0x7FFC).unwrap(), 0xDEAD_BEEF);
}

#[test]
fn scanline_snapshots_precede_hblank_dma_register_updates() {
    let mut bus = bus_with_rom(&[0; 4]);
    // HBlank DMA: one halfword per line from EWRAM into BG0VOFS (fixed dest).
    bus.write_u16(EWRAM_START, 0x0005).unwrap();
    bus.write_u16(EWRAM_START + 2, 0x0007).unwrap();
    program_dma(
        &mut bus,
        0,
        EWRAM_START,
        IO_REG_START + 0x12,
        1,
        0x8000 | (2 << 12) | (1 << 9) | (2 << 5),
    );
    bus.advance(CYCLES_PER_SCANLINE * 2);

    let lines = bus.take_scanlines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].line, 0);
    // Line 0 was latched before its HBlank DMA wrote VOFS = 5.
    assert_eq!(lines[0].registers[0x12], 0);
    assert_eq!(lines[1].registers[0x12], 5);
    assert_eq!(bus.io_regs[0x12], 7);
    assert!(bus.take_scanlines().is_empty());
}

#[test]
fn timer_overflow_pops_the_fifo_and_special_dma_refills_it() {
    let mut bus = bus_with_rom(&[0; 4]);
    // 32 bytes of audio data in EWRAM.
    for index in 0..32 {
        bus.write_u8(EWRAM_START + index, (index as u8).wrapping_add(1))
            .unwrap();
    }
    // Sound on; FIFO A: 100%, both sides, timer 0.
    bus.write_u8(IO_REG_START + 0x84, 0x80).unwrap();
    bus.write_u16(IO_REG_START + 0x82, 0x0304).unwrap();
    // DMA1: special timing, repeat, 32-bit, fixed destination FIFO A.
    program_dma(
        &mut bus,
        1,
        EWRAM_START,
        IO_REG_START + 0xA0,
        4,
        0x8000 | (3 << 12) | (1 << 9) | (1 << 10) | (2 << 5),
    );
    // Nothing transferred until the FIFO requests data.
    assert_eq!(bus.apu.fifo_len(0), 0);

    // Timer 0: overflow every 512 cycles (reload 0xFE00), no prescaler.
    bus.write_u16(IO_REG_START + 0x100, 0xFE00).unwrap();
    bus.write_u16(IO_REG_START + 0x102, 0x0080).unwrap();
    bus.advance(512);

    // The first overflow found the FIFO empty, requested DMA, and four
    // words (16 bytes) arrived.
    assert_eq!(bus.apu.fifo_len(0), 16);
    let samples = bus.take_audio_samples();
    assert_eq!(samples.len(), 1);
}

#[test]
fn audio_registers_round_trip_and_fifo_reads_zero() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(IO_REG_START + 0x80, 0xFF77).unwrap();
    assert_eq!(bus.read_u16(IO_REG_START + 0x80).unwrap(), 0xFF77);
    bus.write_u32(IO_REG_START + 0xA0, 0x7F7F_7F7F).unwrap();
    assert_eq!(bus.read_u32(IO_REG_START + 0xA0).unwrap(), 0);
    assert_eq!(bus.apu.fifo_len(0), 4);
    bus.write_u8(IO_REG_START + 0x84, 0x80).unwrap();
    assert_eq!(bus.read_u8(IO_REG_START + 0x84).unwrap() & 0x80, 0x80);
}

#[test]
fn keyinput_is_active_low_for_each_button() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.set_pressed_buttons(crate::input::START | crate::input::L);
    let value = bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap();
    assert_eq!(value, 0x03FF & !(1 << 3) & !(1 << 9));
    bus.set_pressed_buttons(0);
    assert_eq!(
        bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
        0x03FF
    );
}

fn keypad_flag(bus: &MemoryBus) -> bool {
    bus.io_regs[INTERRUPT_REQUEST + 1] & KEYPAD_IRQ_BIT != 0
}

#[test]
fn keypad_irq_or_mode_fires_once_when_any_selected_button_is_pressed() {
    let mut bus = bus_with_rom(&[0; 4]);
    let selected = crate::input::A | crate::input::B;
    bus.write_u16(IO_REG_START + KEYCNT as u32, (1 << 14) | selected)
        .unwrap();

    bus.set_pressed_buttons(crate::input::START);
    assert!(!keypad_flag(&bus));
    bus.set_pressed_buttons(crate::input::B);
    assert!(keypad_flag(&bus));

    // Acknowledge; holding the button must not retrigger.
    bus.write_u16(IO_REG_START + INTERRUPT_REQUEST as u32, 1 << 12)
        .unwrap();
    bus.set_pressed_buttons(crate::input::B | crate::input::START);
    assert!(!keypad_flag(&bus));
    // Release and press again.
    bus.set_pressed_buttons(0);
    bus.set_pressed_buttons(crate::input::A);
    assert!(keypad_flag(&bus));
}

#[test]
fn keypad_irq_and_mode_requires_every_selected_button() {
    let mut bus = bus_with_rom(&[0; 4]);
    let combo = crate::input::A | crate::input::B | crate::input::START | crate::input::SELECT;
    bus.write_u16(IO_REG_START + KEYCNT as u32, (1 << 15) | (1 << 14) | combo)
        .unwrap();
    bus.set_pressed_buttons(crate::input::A | crate::input::B | crate::input::START);
    assert!(!keypad_flag(&bus));
    bus.set_pressed_buttons(combo);
    assert!(keypad_flag(&bus));
}

#[test]
fn keypad_irq_is_disabled_without_the_enable_bit() {
    let mut bus = bus_with_rom(&[0; 4]);
    bus.write_u16(IO_REG_START + KEYCNT as u32, crate::input::A)
        .unwrap();
    bus.set_pressed_buttons(crate::input::A);
    assert!(!keypad_flag(&bus));
}

fn flash_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x200];
    rom[0x100..0x10C].copy_from_slice(b"FLASH1M_V103");
    rom
}

fn flash_command(bus: &mut MemoryBus, address: u32, value: u8) {
    bus.write_u8(0x0E00_5555, 0xAA).unwrap();
    bus.write_u8(0x0E00_2AAA, 0x55).unwrap();
    bus.write_u8(0x0E00_0000 + address, value).unwrap();
}

#[test]
fn flash_is_detected_and_answers_the_id_command_through_the_bus() {
    let mut bus = bus_with_rom(&flash_rom());
    assert_eq!(bus.save_kind(), SaveKind::Flash128);

    flash_command(&mut bus, 0x5555, 0x90);
    assert_eq!(bus.read_u8(0x0E00_0000).unwrap(), 0xC2);
    assert_eq!(bus.read_u8(0x0E00_0001).unwrap(), 0x09);
    flash_command(&mut bus, 0x5555, 0xF0);
    assert_eq!(bus.read_u8(0x0E00_0000).unwrap(), 0xFF);
}

#[test]
fn flash_program_marks_dirty_and_persists_through_load() {
    let mut bus = bus_with_rom(&flash_rom());
    assert!(!bus.take_save_dirty());

    flash_command(&mut bus, 0x5555, 0xA0);
    bus.write_u8(0x0E00_0010, 0x42).unwrap();
    assert!(bus.take_save_dirty());
    assert_eq!(bus.save_data()[0x10], 0x42);

    let image = bus.save_data().to_vec();
    let mut restored = bus_with_rom(&flash_rom());
    restored.load_save_data(&image);
    assert_eq!(restored.read_u8(0x0E00_0010).unwrap(), 0x42);
    assert!(!restored.take_save_dirty());
}

#[test]
fn backup_memory_uses_an_8_bit_bus() {
    let mut bus = bus_with_rom(&[0; 4]); // no tag: SRAM
    assert_eq!(bus.save_kind(), SaveKind::Sram);
    bus.write_u8(0x0E00_0000, 0xAB).unwrap();
    bus.write_u8(0x0E00_0001, 0xCD).unwrap();

    // Wider reads see the addressed byte on every lane.
    assert_eq!(bus.read_u16(0x0E00_0000).unwrap(), 0xABAB);
    assert_eq!(bus.read_u32(0x0E00_0001).unwrap(), 0xCDCD_CDCD);

    // Wider writes store only the addressed lane.
    bus.write_u32(0x0E00_0010, 0x4433_2211).unwrap();
    assert_eq!(bus.read_u8(0x0E00_0010).unwrap(), 0x11);
    assert_eq!(bus.read_u8(0x0E00_0011).unwrap(), 0xFF);
    bus.write_u16(0x0E00_0021, 0x7766).unwrap();
    assert_eq!(bus.read_u8(0x0E00_0021).unwrap(), 0x77);
    assert_eq!(bus.read_u8(0x0E00_0020).unwrap(), 0xFF);
}

#[test]
fn waitcnt_writes_and_resets_refresh_the_cached_game_pak_timings() {
    let mut bus = bus_with_rom(&[0; 4]);
    assert_eq!(bus.transfer_cycles(ROM_START, 2, false).unwrap(), 4);

    // WS0 first access = 2 cycles (selection 2); sequential = 1 (bit 4).
    bus.write_u16(IO_REG_START + 0x204, (2 << 2) | (1 << 4))
        .unwrap();
    assert_eq!(bus.transfer_cycles(ROM_START, 2, false).unwrap(), 2);
    assert_eq!(bus.transfer_cycles(ROM_START, 2, true).unwrap(), 1);
    // A word is non-sequential + sequential halfwords.
    assert_eq!(bus.transfer_cycles(ROM_START, 4, false).unwrap(), 3);

    // SRAM wait = 8 cycles for selection 3.
    bus.write_u16(IO_REG_START + 0x204, 3).unwrap();
    assert_eq!(bus.transfer_cycles(0x0E00_0000, 1, false).unwrap(), 8);

    // RegisterRamReset with the I/O flag restores the defaults.
    bus.register_ram_reset(0x80);
    assert_eq!(bus.transfer_cycles(ROM_START, 2, false).unwrap(), 4);
    assert_eq!(bus.transfer_cycles(0x0E00_0000, 1, false).unwrap(), 4);
}

fn state_bytes(bus: &MemoryBus) -> Vec<u8> {
    let mut writer = crate::state::StateWriter::new();
    bus.save_state(&mut writer);
    writer.into_bytes()
}

/// A bus with timers counting, a PSG channel and the FIFO running, a DMA
/// waiting for VBlank, RAM written, and some time elapsed.
fn busy_bus() -> MemoryBus {
    let mut bus = bus_with_rom(&[0; 8]);
    bus.write_u32(EWRAM_START + 0x100, 0xCAFE_F00D).unwrap();
    bus.write_u32(IWRAM_START + 0x40, 0x0BAD_C0DE).unwrap();
    bus.write_u16(0x0500_0000 + 6, 0x7FFF).unwrap();
    // Timer 0 (prescaler 64) and a cascaded timer 1, both with IRQs.
    bus.write_u16(IO_REG_START + 0x100, 0xF000).unwrap();
    bus.write_u16(IO_REG_START + 0x102, 0x00C1).unwrap();
    bus.write_u16(IO_REG_START + 0x104, 0xFF00).unwrap();
    bus.write_u16(IO_REG_START + 0x106, 0x00C4).unwrap();
    // Sound: master enable, a square channel, and FIFO A fed with samples.
    bus.write_u16(IO_REG_START + 0x84, 0x0080).unwrap();
    bus.write_u16(IO_REG_START + 0x80, 0x7777).unwrap();
    bus.write_u16(IO_REG_START + 0x62, 0xF080).unwrap();
    bus.write_u16(IO_REG_START + 0x64, 0x8400).unwrap();
    for word in 0..4 {
        bus.write_u32(IO_REG_START + 0xA0, 0x7F40_C081 + word)
            .unwrap();
    }
    // A DMA channel armed for VBlank.
    program_dma(
        &mut bus,
        1,
        IWRAM_START,
        EWRAM_START + 0x200,
        4,
        0x9000 | 0x0400,
    );
    bus.advance(100_000);
    bus.sync();
    // States are taken between frames, when the audio has been collected.
    let _ = bus.take_audio_samples();
    bus
}

#[test]
fn bus_state_round_trips_through_a_fresh_bus() {
    let original = busy_bus();
    let bytes = state_bytes(&original);
    assert_ne!(
        bytes,
        state_bytes(&bus_with_rom(&[0; 8])),
        "the busy bus should not look like a new one"
    );

    let mut restored = bus_with_rom(&[0; 8]);
    let mut reader = crate::state::StateReader::new(&bytes);
    restored.load_state(&mut reader).unwrap();
    assert_eq!(reader.remaining(), 0);

    // Saving again reproduces every byte: nothing was dropped on the way in.
    assert_eq!(state_bytes(&restored), bytes);
    assert_eq!(restored.read_u32(EWRAM_START + 0x100).unwrap(), 0xCAFE_F00D);
    assert_eq!(restored.timers[0].reload, 0xF000);
    assert_eq!(restored.timers[1].control & 0x84, 0x84);
}

#[test]
fn a_restored_bus_keeps_running_exactly_like_the_original() {
    let mut original = busy_bus();
    let bytes = state_bytes(&original);
    let mut restored = bus_with_rom(&[0; 8]);
    restored
        .load_state(&mut crate::state::StateReader::new(&bytes))
        .unwrap();

    for _ in 0..40 {
        original.advance(7_000);
        restored.advance(7_000);
    }
    original.sync();
    restored.sync();
    assert_eq!(state_bytes(&restored), state_bytes(&original));
    assert!(
        restored.take_audio_samples() == original.take_audio_samples(),
        "audio diverged after the state was restored"
    );
}

#[test]
fn restoring_a_state_marks_backup_memory_changed() {
    let mut bus = busy_bus();
    bus.write_u8(0x0E00_0000, 0x5A).unwrap();
    let bytes = state_bytes(&bus);

    let mut restored = bus_with_rom(&[0; 8]);
    restored.take_save_dirty();
    restored
        .load_state(&mut crate::state::StateReader::new(&bytes))
        .unwrap();
    assert_eq!(restored.read_u8(0x0E00_0000).unwrap(), 0x5A);
    assert!(
        restored.take_save_dirty(),
        "the battery save file must follow the restored contents"
    );
}

// ---- the cartridge's real-time clock --------------------------------------

use super::rtc::LocalTime;

fn clock_at_noon() -> LocalTime {
    LocalTime {
        year: 2031,
        month: 1,
        day: 2,
        weekday: 3,
        hour: 12,
        minute: 34,
        second: 56,
    }
}

/// A bus for a cartridge with the given game code and the fixed clock above.
fn bus_with_clock(game_code: &[u8; 4]) -> MemoryBus {
    let mut rom = vec![0xEEu8; 0x200];
    rom[0xAC..0xB0].copy_from_slice(game_code);
    let mut bus = bus_with_rom(&rom);
    bus.set_clock(clock_at_noon);
    bus
}

const GPIO_DATA: u32 = ROM_START + 0xC4;
const GPIO_DIRECTION: u32 = ROM_START + 0xC6;
const GPIO_CONTROL: u32 = ROM_START + 0xC8;

/// Reads `length` bytes of the clock's register `register` the way the games do,
/// through the bus: 16-bit accesses to the GPIO port.
fn read_clock_register(bus: &mut MemoryBus, register: u8, length: usize) -> Vec<u8> {
    bus.write_u16(GPIO_CONTROL, 1).unwrap();
    bus.write_u16(GPIO_DIRECTION, 0b111).unwrap();
    bus.write_u16(GPIO_DATA, 0b001).unwrap();
    bus.write_u16(GPIO_DATA, 0b101).unwrap();
    let command = 0x60 | (register << 1) | 1;
    for index in (0..8).rev() {
        let sio = u16::from((command >> index) & 1) << 1;
        bus.write_u16(GPIO_DATA, sio | 4).unwrap();
        bus.write_u16(GPIO_DATA, sio | 5).unwrap();
    }
    bus.write_u16(GPIO_DIRECTION, 0b101).unwrap();
    let mut bytes = Vec::new();
    for _ in 0..length {
        let mut byte = 0u8;
        for index in 0..8 {
            bus.write_u16(GPIO_DATA, 4).unwrap();
            bus.write_u16(GPIO_DATA, 5).unwrap();
            byte |= (((bus.read_u16(GPIO_DATA).unwrap() >> 1) & 1) as u8) << index;
        }
        bytes.push(byte);
    }
    bus.write_u16(GPIO_DIRECTION, 0b111).unwrap();
    bus.write_u16(GPIO_DATA, 0b001).unwrap();
    bytes
}

#[test]
fn emerald_ruby_and_sapphire_read_the_clock_through_the_cartridge_port() {
    for code in [b"BPEE", b"AXVE", b"AXPE"] {
        let mut bus = bus_with_clock(code);
        assert_eq!(
            read_clock_register(&mut bus, 2, 7),
            [0x31, 0x01, 0x02, 0x03, 0x12, 0x34, 0x56],
            "{code:?}"
        );
        // The status shows 24-hour mode and no power failure.
        assert_eq!(read_clock_register(&mut bus, 1, 1)[0] & 0xC0, 0x40);
    }
}

#[test]
fn other_cartridges_have_no_clock_and_their_rom_is_untouched() {
    let mut bus = bus_with_clock(b"BPRE");
    bus.write_u16(GPIO_CONTROL, 1).unwrap();
    bus.write_u16(GPIO_DATA, 0b111).unwrap();
    assert_eq!(
        bus.read_u16(GPIO_DATA).unwrap(),
        0xEEEE,
        "the ROM's own bytes"
    );
    assert_eq!(bus.read_u8(GPIO_CONTROL).unwrap(), 0xEE);
}

#[test]
fn the_port_overlays_the_rom_only_at_its_own_bytes_in_every_width_and_mirror() {
    let mut bus = bus_with_clock(b"BPEE");
    bus.write_u16(GPIO_CONTROL, 1).unwrap();
    bus.write_u16(GPIO_DIRECTION, 0b111).unwrap();
    // A neighbour is the ROM's: reads straddling the port see both.
    assert_eq!(bus.read_u8(ROM_START + 0xC3).unwrap(), 0xEE);
    assert_eq!(bus.read_u8(ROM_START + 0xCA).unwrap(), 0xEE);
    assert_eq!(
        bus.read_u16(GPIO_DIRECTION).unwrap(),
        0b111,
        "direction, then a zero byte"
    );
    assert_eq!(bus.read_u32(GPIO_DATA).unwrap() & 0xFFFF_0000, 0b111 << 16);
    // The same offsets in the other Game Pak windows reach the same port.
    assert_eq!(bus.read_u16(0x0A00_00C6).unwrap(), 0b111);
    assert_eq!(bus.read_u16(0x0C00_00C6).unwrap(), 0b111);
    // Writing a byte or a word reaches the registers too.
    bus.write_u8(GPIO_DIRECTION, 0b101).unwrap();
    assert_eq!(bus.read_u8(GPIO_DIRECTION).unwrap(), 0b101);
    bus.write_u32(GPIO_DATA, 0x0007_0000 | 0b101).unwrap();
    assert_eq!(
        bus.read_u16(GPIO_DIRECTION).unwrap(),
        0b111,
        "the word's high half"
    );
}
