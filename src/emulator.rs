use crate::cheats::Cheats;
use crate::cheats::firered::{perfect_stats, shiny};
use crate::cheats::options::CheatOptions;
use crate::cheats::scan::CheatScan;
use crate::cpu::Cpu;
use crate::mmu::{MemoryBus, ROM_START};
use crate::ppu::{Ppu, SCREEN_HEIGHT};
use std::io;

mod idle;
mod state;
use idle::IdleLoopDetector;

const THUMB_STATE: u32 = 1 << 5;
/// `cheat_hook` when the cheats are not tied to a ROM address; the program
/// counter is never this value.
const NO_HOOK: u32 = u32::MAX;
/// A register value that is never written: "leave this argument alone".
const NO_VALUE: u32 = u32::MAX;
/// A value of the catch calculation's `odds` that always passes the game's
/// `odds > 254` test.
const CATCH_ALWAYS_ODDS: u32 = 0xFF;

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
    /// scan of the ROM (see [`wild_species`]); `None` if not recognised.
    create_wild_mon: Option<u32>,
    /// The cheat menu's options.
    cheat_options: CheatOptions,
    /// Bus address of the catch check's `cmp r6, #0xFE`, found by a signature
    /// scan (see [`catch_rate`]); `None` if not recognised.
    catch_check: Option<u32>,
    /// Where the 100% catch rate cheat is applied while it is on, or
    /// `NO_HOOK`.
    catch_hook: u32,
    /// Bus address in `CreateBoxMon` where the personality value and OT ID are
    /// both known, found by a signature scan (see [`shiny`]); `None` if not
    /// recognised.
    shiny_point: Option<u32>,
    /// Where the cheats that change a Pokémon as it is built (all shiny,
    /// perfect stats) are applied while one is on, or `NO_HOOK`.
    build_hook: u32,
    /// Which of those cheats are on.
    make_shiny: bool,
    perfect_wild: bool,
    perfect_opponents: bool,
    /// Entry of `CreateWildMon` while a perfect-stats row needs to know when
    /// a wild Pokémon is being built, or `NO_HOOK`.
    wild_scope_hook: u32,
    /// Where `CreateWildMon` returns to while it is running, or `NO_HOOK`.
    wild_return: u32,
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
        Self::with_cheat_scan(rom_path, None)
    }

    /// Like [`Emulator::new`], reusing the cheat hook addresses of an earlier
    /// scan of this same ROM (as kept in the settings file) instead of
    /// scanning again. A scan recorded for a different ROM is ignored.
    pub fn with_cheat_scan(rom_path: &str, previous: Option<&CheatScan>) -> io::Result<Self> {
        let memory = MemoryBus::new(rom_path)?;
        let CheatScan {
            create_wild_mon,
            catch_check,
            shiny_point,
            ..
        } = CheatScan::for_rom(&memory.rom, previous);
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
            catch_check,
            catch_hook: NO_HOOK,
            shiny_point,
            build_hook: NO_HOOK,
            make_shiny: false,
            perfect_wild: false,
            perfect_opponents: false,
            wild_scope_hook: NO_HOOK,
            wild_return: NO_HOOK,
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
        // 100% catch rate: at the `odds > 254` test, make `odds` (r6) big
        // enough that the ball always catches.
        if pc_before == self.catch_hook {
            self.cpu.registers[6] = CATCH_ALWAYS_ODDS;
        }
        // Perfect stats for wild Pokémon: know while `CreateWildMon` runs.
        if pc_before == self.wild_scope_hook {
            self.wild_return = self.cpu.registers[14] & !1;
        } else if pc_before == self.wild_return {
            self.wild_return = NO_HOOK;
        }
        // All Pokémon shiny, perfect stats: while a Pokémon is being built.
        if pc_before == self.build_hook {
            self.adjust_new_pokemon();
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

    /// At the hook inside `CreateBoxMon` (see [`shiny`] and
    /// [`perfect_stats`]): applies the cheats that are on to the Pokémon being
    /// built. A memory fault leaves everything unchanged.
    fn adjust_new_pokemon(&mut self) {
        let sp = self.cpu.registers[13];
        let ot_id_type = self.cpu.registers[perfect_stats::OT_ID_TYPE_REGISTER];
        if perfect_stats::applies(
            self.perfect_wild,
            self.perfect_opponents,
            self.wild_return != NO_HOOK,
            ot_id_type,
        ) {
            let _ = self.memory.write_u8(
                sp.wrapping_add(perfect_stats::FIXED_IV_OFFSET),
                perfect_stats::PERFECT_IV,
            );
        }
        if self.make_shiny {
            self.make_new_pokemon_shiny(sp);
        }
    }

    /// Rewrites the personality value, in the local the function goes on to
    /// read and in the `BoxPokemon` it was just stored in, so that the Pokémon
    /// is shiny for its OT ID. Unown is left alone.
    fn make_new_pokemon_shiny(&mut self, sp: u32) {
        let box_mon = self.cpu.registers[shiny::BOX_MON_REGISTER];
        let memory = &mut self.memory;
        let Ok(species) = memory.read_u16(sp.wrapping_add(shiny::SPECIES_OFFSET)) else {
            return;
        };
        let (Ok(personality), Ok(ot_id)) = (
            memory.read_u32(sp.wrapping_add(shiny::PERSONALITY_OFFSET)),
            memory.read_u32(sp.wrapping_add(shiny::OT_ID_OFFSET)),
        ) else {
            return;
        };
        if species == shiny::UNOWN {
            return;
        }
        let shiny = shiny::shiny_personality(personality, ot_id);
        if shiny != personality {
            let _ = memory.write_u32(sp.wrapping_add(shiny::PERSONALITY_OFFSET), shiny);
            let _ = memory.write_u32(box_mon, shiny);
        }
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
        // The 100% catch rate row: hook the catch check while it is on.
        self.catch_hook = match self.catch_check {
            Some(address) if self.cheat_options.catch_always => address,
            _ => NO_HOOK,
        };
        // The all-shiny and perfect-stats rows: hook where a Pokémon is being
        // built. The perfect-stats row for wild Pokémon also watches
        // `CreateWildMon`, to tell wild ones from the rest.
        self.make_shiny = self.cheat_options.shiny;
        self.perfect_wild = self.cheat_options.perfect_for_wild();
        self.perfect_opponents = self.cheat_options.perfect_for_opponents();
        let building = self.make_shiny || self.perfect_wild || self.perfect_opponents;
        self.build_hook = match self.shiny_point {
            Some(address) if building => address,
            _ => NO_HOOK,
        };
        self.wild_scope_hook = match self.create_wild_mon {
            Some(entry) if self.perfect_wild && self.build_hook != NO_HOOK => entry,
            _ => NO_HOOK,
        };
        self.wild_return = NO_HOOK;
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

    /// The hook addresses found in this ROM, for the settings file to keep.
    pub fn cheat_scan(&self) -> CheatScan {
        CheatScan {
            rom: crate::cheats::scan::RomFingerprint::of(&self.memory.rom),
            create_wild_mon: self.create_wild_mon,
            catch_check: self.catch_check,
            shiny_point: self.shiny_point,
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

    /// The bus address the 100% catch rate cheat hooks in this ROM, or `None`
    /// when it is not a FireRed build whose catch check can be found.
    pub fn catch_check_address(&self) -> Option<u32> {
        self.catch_check
    }

    /// The bus address the all-shiny cheat hooks in this ROM, or `None` when
    /// it is not a FireRed build whose `CreateBoxMon` can be found.
    pub fn build_hook_address(&self) -> Option<u32> {
        self.shiny_point
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
