use crate::cpu::Cpu;
use crate::mmu::{MemoryBus, ROM_START};
use crate::ppu::{Ppu, SCREEN_HEIGHT};
use std::io;

mod idle;
use idle::IdleLoopDetector;

const THUMB_STATE: u32 = 1 << 5;
/// Granularity at which a halted CPU lets the rest of the system advance.
const HALT_STEP_CYCLES: u32 = 32;

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
        self.idle.skipped_to(self.cpu.cycles);
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
mod tests;
