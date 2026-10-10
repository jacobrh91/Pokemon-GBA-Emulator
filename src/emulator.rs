use crate::cpu::Cpu;
use crate::mmu::{MemoryBus, ROM_START};
use crate::ppu::{Ppu, SCREEN_HEIGHT};
use std::io;

const THUMB_STATE: u32 = 1 << 5;
/// Granularity at which a halted CPU lets the rest of the system advance.
const HALT_STEP_CYCLES: u32 = 32;

/// Recognises a loop that leaves the whole machine state unchanged.
///
/// At every backward jump the detector compares the CPU state and the bus's
/// write/volatile-read counters with those at the previous jump to the same
/// address. If nothing changed, one more iteration would change nothing
/// either, until an interrupt, DMA transfer or timer event intervenes, so the
/// emulator may advance time by whole iterations up to the next event.
#[derive(Default)]
struct IdleLoopDetector {
    armed: bool,
    head: u32,
    registers: [u32; 16],
    cpsr: u32,
    writes: u64,
    flushes: u64,
    volatile_reads: u32,
    cycles: u64,
}

impl IdleLoopDetector {
    /// Records a backward jump; returns the loop's cycle length if the
    /// iteration just finished was provably a no-op.
    fn observe(&mut self, cpu: &Cpu, memory: &MemoryBus) -> Option<u64> {
        let head = cpu.registers[15];
        let writes = memory.write_count();
        let flushes = memory.flush_count();
        let volatile_reads = memory.volatile_read_count();
        let idle = self.armed
            && self.head == head
            && self.cpsr == cpu.cpsr
            && self.registers == cpu.registers
            && self.writes == writes
            // No event may have fired inside the iteration: one that did
            // changes what the next iteration sees (a scanline counter, a
            // flag set by an interrupt) even though this one looked idle.
            && self.flushes == flushes
            && self.volatile_reads == volatile_reads;
        let iteration = if idle {
            cpu.cycles.checked_sub(self.cycles)
        } else {
            None
        };

        self.armed = true;
        self.head = head;
        self.cpsr = cpu.cpsr;
        self.registers = cpu.registers;
        self.writes = writes;
        self.flushes = flushes;
        self.volatile_reads = volatile_reads;
        self.cycles = cpu.cycles;
        iteration
    }

    fn reset(&mut self) {
        self.armed = false;
    }
}

pub struct Emulator {
    cpu: Cpu,
    memory: MemoryBus,
    ppu: Ppu,
    frame_ready: bool,
    rendering: bool,
    idle: IdleLoopDetector,
    skip_idle_loops: bool,
    rom_path: String,
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
            rendering: true,
            idle: IdleLoopDetector::default(),
            skip_idle_loops: true,
            rom_path: rom_path.to_owned(),
        })
    }

    /// Restarts the game as if the console had been power-cycled: CPU, RAM,
    /// I/O, video, audio and timing return to their boot state and the CPU
    /// starts at the cartridge entry point. The cartridge's backup memory
    /// (the save) is kept, exactly as it would survive a reset on hardware.
    ///
    /// The `set_rendering` and idle-skipping switches keep their settings.
    pub fn reset(&mut self) -> io::Result<()> {
        let save_image = self.memory.save_data().to_vec();
        let mut memory = MemoryBus::new(&self.rom_path)?;
        memory.load_save_data(&save_image);
        self.memory = memory;
        self.cpu = Cpu::default();
        self.ppu = Ppu::default();
        self.frame_ready = false;
        self.idle = IdleLoopDetector::default();
        self.cpu.initialize_gba_startup(ROM_START)
    }

    pub fn step_thumb(&mut self) -> io::Result<u32> {
        self.cpu.step_thumb(&mut self.memory)
    }

    pub fn initialize_gba_startup(&mut self, entry_point: u32) -> io::Result<()> {
        self.cpu.initialize_gba_startup(entry_point)
    }

    pub fn step(&mut self) -> io::Result<u32> {
        if self.cpu.halted {
            self.idle.reset();
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

        // The BIOS hooks (SWI vector, IRQ return stub) live in the first
        // 0x200 bytes; skip the call everywhere else.
        if self.cpu.registers[15] < 0x200 {
            if let Some(cycles) = self.cpu.step_bios_hle(&mut self.memory)? {
                self.idle.reset();
                return Ok(cycles);
            }
        }

        let pc_before = self.cpu.registers[15];
        let mut cycles = if self.cpu.cpsr & THUMB_STATE != 0 {
            self.step_thumb()
        } else {
            self.cpu.step_arm(&mut self.memory)
        }?;
        self.advance_bus(cycles);
        // DMA stalls the CPU for as long as it owns the bus. Charging a stall
        // advances time, which can trigger further transfers (a refill that
        // crosses a scanline, say), so drain until none is left; otherwise
        // the follow-on stall would be billed to the next instruction.
        loop {
            let dma_cycles = self.memory.take_dma_cycles();
            if dma_cycles == 0 {
                break;
            }
            self.cpu.cycles += u64::from(dma_cycles);
            self.advance_bus(dma_cycles);
            cycles += dma_cycles;
        }
        if self.memory.irq_pending() && self.cpu.request_irq(&mut self.memory)? {
            self.advance_bus(3);
            cycles += 3;
        }
        // Looked at last, once this step's DMA stall and interrupt entry have
        // been charged, so loop timestamps measure only the loop itself.
        if self.skip_idle_loops && self.cpu.registers[15] <= pc_before {
            cycles += self.skip_idle_iterations();
        }
        Ok(cycles)
    }

    /// Enables or disables idle-loop skipping (on by default). Skipping is
    /// exact; the switch exists for debugging and for verifying that.
    pub fn set_idle_loop_skipping(&mut self, enabled: bool) {
        self.skip_idle_loops = enabled;
        self.idle.reset();
    }

    /// After a backward jump: if the loop is provably idle, advances time by
    /// whole iterations, stopping at least one iteration short of the next
    /// event so the event lands on exactly the instruction it would have.
    /// Returns the cycles skipped.
    fn skip_idle_iterations(&mut self) -> u32 {
        let Some(iteration) = self.idle.observe(&self.cpu, &self.memory) else {
            return 0;
        };
        let Ok(iteration) = u32::try_from(iteration) else {
            return 0;
        };
        if iteration == 0 {
            return 0;
        }
        let iterations = (self.memory.cycles_until_next_event() / iteration).saturating_sub(1);
        let skipped = iterations * iteration;
        if skipped == 0 {
            return 0;
        }
        self.cpu.cycles += u64::from(skipped);
        self.advance_bus(skipped);
        // The skipped iterations leave the detector's snapshot valid but its
        // cycle stamp stale; re-arm from the current point.
        self.idle.cycles = self.cpu.cycles;
        skipped
    }

    /// Advances the bus and draws every scanline that finished meanwhile.
    fn advance_bus(&mut self, cycles: u32) {
        self.memory.advance(cycles);
        if !self.memory.has_scanlines() {
            return;
        }
        for snapshot in self.memory.take_scanlines() {
            if self.rendering {
                self.ppu.render_scanline_with_events(
                    snapshot.line,
                    &snapshot.registers,
                    &self.memory,
                    snapshot.affine_reload,
                );
            }
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

    /// Enables or disables drawing. While disabled, emulation, timing and
    /// frame-ready signalling continue unchanged but the framebuffer keeps
    /// its last contents; used to skip frames that will not be shown.
    ///
    /// Switch it at frame boundaries (right after `take_frame_ready`):
    /// affine background state is per frame and must see a whole frame.
    pub fn set_rendering(&mut self, enabled: bool) {
        self.rendering = enabled;
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
}
