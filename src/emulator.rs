use crate::cheat_options::CheatOptions;
use crate::cheats::Cheats;
use crate::cpu::Cpu;
use crate::mmu::{MemoryBus, ROM_START};
use crate::ppu::{Ppu, SCREEN_HEIGHT};
use std::io;

mod idle;
mod state;
mod wild_species;
use idle::IdleLoopDetector;

const THUMB_STATE: u32 = 1 << 5;
/// `cheat_hook` when the cheats are not tied to a ROM address; the program
/// counter is never this value.
const NO_HOOK: u32 = u32::MAX;
/// A register value that is never written: "leave this argument alone".
const NO_VALUE: u32 = u32::MAX;

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
    cheats: Cheats,
    /// The ROM address the cheats are tied to while they are enabled, or
    /// `NO_HOOK`.
    cheat_hook: u32,
    /// Bus address of the game's `CreateWildMon`, found once by a signature
    /// scan of the ROM (see `wild_species.rs`); `None` if not recognised.
    create_wild_mon: Option<u32>,
    /// The cheat menu's options.
    cheat_options: CheatOptions,
    /// Where the wild-encounter override is applied while it is active, or
    /// `NO_HOOK`, and the species / level written there (`NO_VALUE` to keep
    /// the game's).
    wild_species_hook: u32,
    wild_species: u32,
    wild_level: u32,
}

impl Emulator {
    /// Creates an emulator with a mapped ROM and default CPU/PPU state.
    ///
    /// Initializes the bus and devices without configuring CPU boot state.
    /// Call `initialize_gba_startup` or set the CPU PC and execution mode
    /// before stepping.
    pub fn new(rom_path: &str) -> io::Result<Self> {
        let memory = MemoryBus::new(rom_path)?;
        let create_wild_mon = wild_species::find_create_wild_mon(&memory.rom);
        Ok(Self {
            cpu: Cpu::default(),
            memory,
            ppu: Ppu::default(),
            frame_ready: false,
            rendering: true,
            idle: IdleLoopDetector::default(),
            skip_idle_loops: true,
            rom_path: rom_path.to_owned(),
            cheats: Cheats::default(),
            cheat_hook: NO_HOOK,
            create_wild_mon,
            cheat_options: CheatOptions::default(),
            wild_species_hook: NO_HOOK,
            wild_species: NO_VALUE,
            wild_level: NO_VALUE,
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
        if self.cpu.registers[15] < 0x200
            && let Some(cycles) = self.cpu.step_bios_hle(&mut self.memory)?
        {
            self.idle.reset();
            return Ok(cycles);
        }

        let pc_before = self.cpu.registers[15];
        // Cheats hooked to a ROM address run when execution reaches it, as
        // the cartridge's code handler would.
        if pc_before == self.cheat_hook {
            self.cheats.apply(&mut self.memory);
        }
        // Wild-encounter override: replace the species and/or level
        // arguments of the function that builds every wild encounter.
        if pc_before == self.wild_species_hook {
            if self.wild_species != NO_VALUE {
                self.cpu.registers[0] = self.wild_species;
            }
            if self.wild_level != NO_VALUE {
                self.cpu.registers[1] = self.wild_level;
            }
        }
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

    /// Installs the cheats applied by [`Emulator::apply_cheats`]. They survive
    /// a [`reset`](Emulator::reset).
    pub fn set_cheats(&mut self, cheats: Cheats) {
        self.cheats = cheats;
        self.update_cheat_hook();
    }

    fn update_cheat_hook(&mut self) {
        // The cheats file's raw codes run only while their row of the menu is
        // switched on: some of them patch routines the game has loaded only
        // at certain moments (writing them at the title screen crashes it).
        self.cheat_hook = if self.cheat_options.file_codes {
            self.cheats.hook_address().unwrap_or(NO_HOOK)
        } else {
            NO_HOOK
        };
        // The wild-encounter override, driven by its row of the menu.
        self.wild_species_hook = NO_HOOK;
        self.wild_species = NO_VALUE;
        self.wild_level = NO_VALUE;
        if let Some(entry) = self.create_wild_mon {
            let species = self.cheat_options.active_wild_species();
            let level = self.cheat_options.active_wild_level();
            if species.is_some() || level.is_some() {
                self.wild_species_hook = entry;
                if let Some(species) = species {
                    self.wild_species = u32::from(species);
                }
                if let Some(level) = level {
                    self.wild_level = u32::from(CheatOptions::clamp_level(level));
                }
            }
        }
    }

    /// Installs the cheat menu's options. Today the wild-encounter row and the
    /// cheats file's codes take effect; the other rows are kept for when they
    /// are implemented.
    pub fn set_cheat_options(&mut self, options: CheatOptions) {
        if options != self.cheat_options {
            self.cheat_options = options;
            self.update_cheat_hook();
        }
    }

    pub fn cheat_options(&self) -> CheatOptions {
        self.cheat_options
    }

    /// The bus address the wild-species override hooks in this ROM (the entry
    /// of `CreateWildMon`), or `None` when the ROM is not a FireRed build
    /// whose function the signature scan can identify.
    pub fn wild_species_hook_address(&self) -> Option<u32> {
        self.create_wild_mon
    }

    /// Whether the wild-species override can work with this ROM.
    pub fn wild_species_supported(&self) -> bool {
        self.create_wild_mon.is_some()
    }

    pub fn cheats(&self) -> &Cheats {
        &self.cheats
    }

    /// Writes the cheats' values into RAM. Call once per finished frame, so
    /// the values keep winning against the game rewriting them. Cheats that
    /// name a hook address are instead applied when execution reaches it (see
    /// [`Emulator::step`]), so this does nothing for them.
    pub fn apply_cheats(&mut self) {
        if self.cheat_options.file_codes && self.cheat_hook == NO_HOOK && !self.cheats.is_empty() {
            self.cheats.apply(&mut self.memory);
        }
    }
}

#[cfg(test)]
mod tests;
