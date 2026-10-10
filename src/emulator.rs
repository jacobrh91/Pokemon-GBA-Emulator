use crate::cpu::Cpu;
use crate::mmu::MemoryBus;
use crate::ppu::{Ppu, SCREEN_HEIGHT};
use std::io;

const THUMB_STATE: u32 = 1 << 5;
/// Granularity at which a halted CPU lets the rest of the system advance.
const HALT_STEP_CYCLES: u32 = 32;

pub struct Emulator {
    cpu: Cpu,
    memory: MemoryBus,
    ppu: Ppu,
    frame_ready: bool,
}

impl Emulator {
    /// Creates an emulator with a mapped ROM and default CPU/PPU state.
    ///
    /// Initializes the bus and devices without configuring CPU boot state.
    /// Call `initialize_gba_startup` or set the CPU PC and execution mode
    /// before stepping.
    pub fn new(rom_path: &str) -> io::Result<Self> {
        Ok(Self {
            cpu: Cpu::default(),
            memory: MemoryBus::new(rom_path)?,
            ppu: Ppu::default(),
            frame_ready: false,
        })
    }

    pub fn step_thumb(&mut self) -> io::Result<u32> {
        self.cpu.step_thumb(&mut self.memory)
    }

    pub fn initialize_gba_startup(&mut self, entry_point: u32) -> io::Result<()> {
        self.cpu.initialize_gba_startup(entry_point)
    }

    pub fn step(&mut self) -> io::Result<u32> {
        if self.cpu.halted {
            if !self.cpu.should_wake(&self.memory) {
                self.cpu.cycles += u64::from(HALT_STEP_CYCLES);
                self.advance_bus(HALT_STEP_CYCLES);
                return Ok(HALT_STEP_CYCLES);
            }
            self.cpu.halted = false;
            if self.memory.irq_pending() && self.cpu.request_irq(&mut self.memory)? {
                self.advance_bus(3);
                return Ok(3);
            }
        }

        if let Some(cycles) = self.cpu.step_bios_hle(&mut self.memory)? {
            return Ok(cycles);
        }

        let mut cycles = if self.cpu.cpsr & THUMB_STATE != 0 {
            self.step_thumb()
        } else {
            self.cpu.step_arm(&mut self.memory)
        }?;
        self.advance_bus(cycles);
        // DMA stalls the CPU for as long as it owns the bus.
        let dma_cycles = self.memory.take_dma_cycles();
        if dma_cycles > 0 {
            self.cpu.cycles += u64::from(dma_cycles);
            self.advance_bus(dma_cycles);
            cycles += dma_cycles;
        }
        if self.memory.irq_pending() && self.cpu.request_irq(&mut self.memory)? {
            self.advance_bus(3);
            cycles += 3;
        }
        Ok(cycles)
    }

    /// Advances the bus and draws every scanline that finished meanwhile.
    fn advance_bus(&mut self, cycles: u32) {
        self.memory.advance(cycles);
        for snapshot in self.memory.take_scanlines() {
            self.ppu
                .render_scanline(snapshot.line, &snapshot.registers, &self.memory);
            if snapshot.line == SCREEN_HEIGHT - 1 {
                self.frame_ready = true;
            }
        }
    }

    /// Cartridge backup memory type detected from the ROM.
    pub fn save_kind(&self) -> crate::mmu::SaveKind {
        self.memory.save_kind()
    }

    /// Restores a persisted backup memory image (call before running).
    pub fn load_save(&mut self, image: &[u8]) {
        self.memory.load_save_data(image);
    }

    /// The backup image if it changed since the last call.
    pub fn take_save_if_changed(&mut self) -> Option<Vec<u8>> {
        self.memory
            .take_save_dirty()
            .then(|| self.memory.save_data().to_vec())
    }

    /// Sets the held buttons (see [`crate::input`]) for subsequent steps.
    pub fn set_buttons(&mut self, buttons: crate::input::Buttons) {
        self.memory
            .set_pressed_buttons(crate::input::without_opposing_directions(buttons));
    }

    /// Stereo audio samples (32768 Hz, `[left, right]`) mixed since the last call.
    pub fn take_audio_samples(&mut self) -> Vec<[i16; 2]> {
        self.memory.take_audio_samples()
    }

    /// True once per frame, after the last visible scanline was drawn.
    pub fn take_frame_ready(&mut self) -> bool {
        std::mem::take(&mut self.frame_ready)
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    pub fn memory(&self) -> &MemoryBus {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut MemoryBus {
        &mut self.memory
    }

    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    pub fn render_frame(&mut self) {
        self.ppu.render_frame(&self.memory);
    }
}

#[cfg(test)]
mod tests {
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
}
