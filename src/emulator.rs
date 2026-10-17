use crate::cheats::Cheats;
use crate::cheats::gen3::{
    experience, field_moves, field_poison, money, party_menu, perfect_stats, shiny,
};
use crate::cheats::options::CheatOptions;
use crate::cheats::scan::CheatScan;
use crate::cpu::Cpu;
use crate::mmu::{MemoryBus, ROM_START};
use crate::ppu::{Ppu, SCREEN_HEIGHT};
use std::io;

mod idle;
mod state;
use idle::IdleLoopDetector;

/// Size of the hook filter, in bits (a power of two).
const HOOK_FILTER_BITS: usize = 1 << 15;

const THUMB_STATE: u32 = 1 << 5;
/// CPSR bit that masks IRQs.
const IRQ_DISABLED: u32 = 1 << 7;
/// A hook address that is not set; the program counter is never this value.
const NO_HOOK: u32 = u32::MAX;
/// A register value that is never written: "leave this argument alone".
const NO_VALUE: u32 = u32::MAX;
/// Catch `odds` that always pass the game's `odds > 254` test.
const CATCH_ALWAYS_ODDS: u32 = 0xFF;

/// `address` if the cheat that hooks there is on and its place was found,
/// else `NO_HOOK`.
fn armed(address: Option<u32>, on: bool) -> u32 {
    address.filter(|_| on).unwrap_or(NO_HOOK)
}

/// Cycles a halted CPU advances per step.
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
    /// ROM address the cheats file's codes run at while enabled, or `NO_HOOK`.
    cheat_hook: u32,
    cheat_options: CheatOptions,
    /// Where each built-in cheat hooks in this ROM, found by signature scans
    /// (see `cheats::scan`); `None` where the ROM is not recognised. The
    /// always-on cheats (TMs, HMs, poison) run at these addresses; the others
    /// arm one of the hooks below while their row is on.
    scan: CheatScan,
    catch_hook: u32,
    build_hook: u32,
    money_hook: u32,
    exp_hook: u32,
    encounter_hook: u32,
    /// The address of the party menu's list of actions and of its count, while
    /// the menu for the first Pokémon is being built (see
    /// `cheats::gen3::party_menu`).
    party_actions: Option<(u32, u32)>,
    make_shiny: bool,
    /// With `make_shiny`: one Pokémon in this many is made shiny.
    shiny_one_in: u32,
    perfect_wild: bool,
    perfect_opponents: bool,
    /// One bit per (folded) halfword address that may carry a hook.
    hook_filter: Box<[u64; HOOK_FILTER_BITS / 64]>,
    /// `CreateWildMon`'s entry while perfect stats must tell wild Pokémon
    /// apart, and its return address while it runs.
    wild_scope_hook: u32,
    wild_return: u32,
    /// Where the wild-encounter override acts, and the species and level it
    /// writes there (`NO_VALUE` keeps the game's).
    wild_species_hook: u32,
    wild_species: u32,
    wild_level: u32,
}

impl Emulator {
    /// Creates an emulator with a mapped ROM. Call `initialize_gba_startup`
    /// before stepping.
    pub fn new(rom_path: &str) -> io::Result<Self> {
        Self::with_cheat_scan(rom_path, None)
    }

    /// Like [`Emulator::new`], reusing the hook addresses of an earlier scan
    /// of this ROM instead of scanning again (a scan of another ROM is ignored).
    pub fn with_cheat_scan(rom_path: &str, previous: Option<&CheatScan>) -> io::Result<Self> {
        let memory = MemoryBus::new(rom_path)?;
        let scan = CheatScan::for_rom(&memory.rom, previous);
        let mut emulator = Self {
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
            scan,
            catch_hook: NO_HOOK,
            build_hook: NO_HOOK,
            money_hook: NO_HOOK,
            exp_hook: NO_HOOK,
            encounter_hook: NO_HOOK,
            party_actions: None,
            make_shiny: false,
            hook_filter: Box::new([0; HOOK_FILTER_BITS / 64]),
            shiny_one_in: 1,
            perfect_wild: false,
            perfect_opponents: false,
            wild_scope_hook: NO_HOOK,
            wild_return: NO_HOOK,
            cheat_options: CheatOptions::default(),
            wild_species_hook: NO_HOOK,
            wild_species: NO_VALUE,
            wild_level: NO_VALUE,
        };
        emulator.refresh_hook_filter();
        Ok(emulator)
    }

    /// Power-cycles the console: everything returns to its boot state except
    /// the cartridge's backup memory (the save).
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
        // A halted CPU and the BIOS hooks (first 0x200 bytes) are rare.
        if (self.cpu.halted || self.cpu.registers[15] < 0x200)
            && let Some(cycles) = self.step_halted_or_bios()?
        {
            return Ok(cycles);
        }

        let pc_before = self.cpu.registers[15];
        // One bit test says whether this address might be hooked.
        if self.hook_filter[Self::hook_slot(pc_before) >> 6] >> (Self::hook_slot(pc_before) & 63)
            & 1
            != 0
        {
            self.run_hooks(pc_before);
        }
        let cycles = if self.cpu.cpsr & THUMB_STATE != 0 {
            self.step_thumb()
        } else {
            self.cpu.step_arm(&mut self.memory)
        }?;
        self.advance_bus(cycles);
        if self.memory.dma_stall_pending()
            || (self.cpu.cpsr & IRQ_DISABLED == 0 && self.memory.irq_pending())
            || (self.skip_idle_loops && self.cpu.registers[15] <= pc_before)
        {
            return Ok(cycles + self.after_instruction(pc_before)?);
        }
        Ok(cycles)
    }

    /// The halted-CPU and BIOS-hook part of [`Self::step`]: `Some(cycles)` if
    /// the step is complete, `None` to execute an instruction.
    #[inline(never)]
    fn step_halted_or_bios(&mut self) -> io::Result<Option<u32>> {
        if self.cpu.halted {
            self.idle.reset();
            if !self.cpu.should_wake(&self.memory) {
                self.cpu.cycles += u64::from(HALT_STEP_CYCLES);
                self.advance_bus(HALT_STEP_CYCLES);
                return Ok(Some(HALT_STEP_CYCLES));
            }
            self.cpu.halted = false;
            if self.memory.irq_pending() && self.cpu.request_irq(&mut self.memory)? {
                self.advance_bus(3);
                return Ok(Some(3));
            }
        }
        if self.cpu.registers[15] < 0x200
            && let Some(cycles) = self.cpu.step_bios_hle(&mut self.memory)?
        {
            self.idle.reset();
            return Ok(Some(cycles));
        }
        Ok(None)
    }

    /// What follows an instruction that was stalled by DMA, took an interrupt
    /// or ended a loop iteration; returns the extra cycles.
    #[inline(never)]
    fn after_instruction(&mut self, pc_before: u32) -> io::Result<u32> {
        let mut cycles = 0;
        // Charging a DMA stall advances time, which can trigger more
        // transfers, so drain until none is left.
        while self.memory.dma_stall_pending() {
            let dma_cycles = self.memory.take_dma_cycles();
            self.cpu.cycles += u64::from(dma_cycles);
            self.advance_bus(dma_cycles);
            cycles += dma_cycles;
        }
        if self.memory.irq_pending() && self.cpu.request_irq(&mut self.memory)? {
            self.advance_bus(3);
            cycles += 3;
        }
        // Last, so loop timestamps measure only the loop itself.
        if self.skip_idle_loops && self.cpu.registers[15] <= pc_before {
            cycles += self.skip_idle_iterations();
        }
        Ok(cycles)
    }

    /// The filter bit for `pc`.
    #[inline(always)]
    fn hook_slot(pc: u32) -> usize {
        (pc >> 1) as usize & (HOOK_FILTER_BITS - 1)
    }

    /// Marks `address` for [`Self::run_hooks`].
    fn watch(&mut self, address: u32) {
        if address != NO_HOOK {
            let slot = Self::hook_slot(address);
            self.hook_filter[slot >> 6] |= 1 << (slot & 63);
        }
    }

    /// Rebuilds the filter from the hooks in use.
    fn refresh_hook_filter(&mut self) {
        self.hook_filter.fill(0);
        let fixed = [
            self.cheat_hook,
            self.catch_hook,
            self.wild_scope_hook,
            self.money_hook,
            self.exp_hook,
            self.build_hook,
            self.encounter_hook,
            self.wild_species_hook,
        ];
        for address in fixed {
            self.watch(address);
        }
        for address in self.scan.tm_uses.clone() {
            self.watch(address);
        }
        if let Some(address) = self.scan.field_poison {
            self.watch(address);
        }
        if let Some(moves) = self.scan.field_moves {
            self.watch(moves.knows_move_return);
        }
        if let Some(menu) = self.scan.party_menu {
            self.watch(menu.capture);
            self.watch(menu.append);
        }
    }

    /// Runs the cheats hooked at `pc_before` (the filter may let other
    /// addresses through).
    #[inline(never)]
    fn run_hooks(&mut self, pc_before: u32) {
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
            self.watch(self.wild_return);
        } else if pc_before == self.wild_return {
            self.wild_return = NO_HOOK;
        }
        // Infinite money: `GetMoney` returns the largest balance.
        if pc_before == self.money_hook {
            self.cpu.registers[0] = money::SHOWN_BALANCE;
        }
        // Infinite TMs (always on): ask the bag to remove none.
        if self.scan.tm_uses.contains(&pc_before) {
            self.cpu.registers[1] = 0;
        }
        // The first Pokémon knows every HM (always on): at the end of
        // `MonKnowsMove`, `r6` is the Pokémon asked about, `r5` the move.
        if let Some(moves) = self.scan.field_moves
            && pc_before == moves.knows_move_return
            && field_moves::acts_as_known(
                self.cpu.registers[6],
                self.cpu.registers[5] & 0xFFFF,
                moves.party_base,
            )
        {
            self.cpu.registers[0] = 1;
        }
        // The first Pokémon's party-menu actions list the HMs (always on).
        if let Some(menu) = self.scan.party_menu {
            if pc_before == menu.capture {
                // `r10` is the selected slot; `r0` and `r1` the list and count.
                let slot = self.cpu.registers[10] & 0xFF;
                self.party_actions =
                    (slot == 0).then_some((self.cpu.registers[0], self.cpu.registers[1]));
            } else if pc_before == menu.append {
                self.append_hm_actions(menu.layout);
            }
        }
        // No random encounters: the dice roll always says no.
        // No poison outside battle (always on): the status tested is "none".
        if Some(pc_before) == self.scan.field_poison {
            self.cpu.registers[0] = field_poison::NO_AILMENT;
        }
        if pc_before == self.encounter_hook {
            self.cpu.registers[0] = 0;
        }
        // Boosted experience: make the Pokémon look traded.
        if pc_before == self.exp_hook {
            self.cpu.registers[0] = experience::other_ot_id(self.cpu.registers[0]);
        }
        // Shiny and perfect stats: while a Pokémon is being built.
        if pc_before == self.build_hook {
            self.adjust_new_pokemon();
        }
        // Wild-encounter override: replace `CreateWildMon`'s arguments.
        if pc_before == self.wild_species_hook {
            if self.wild_species != NO_VALUE {
                self.cpu.registers[0] = self.wild_species;
            }
            if self.wild_level != NO_VALUE {
                self.cpu.registers[1] = self.wild_level;
            }
        }
    }

    /// At the party menu's append hook: adds the HMs the first Pokémon does not
    /// list to the actions the game has built (see `party_menu`). A memory
    /// fault leaves the list as it is.
    fn append_hm_actions(&mut self, layout: party_menu::Layout) {
        let Some((list, count_pointer)) = self.party_actions.take() else {
            return;
        };
        let Ok(count) = self.memory.read_u8(count_pointer) else {
            return;
        };
        let present: Vec<u8> = (0..u32::from(count))
            .filter_map(|index| self.memory.read_u8(list + index).ok())
            .collect();
        let extra = party_menu::extra_actions(layout, &present);
        for (index, action) in extra.iter().enumerate() {
            let _ = self
                .memory
                .write_u8(list + u32::from(count) + index as u32, *action);
        }
        let _ = self
            .memory
            .write_u8(count_pointer, count + extra.len() as u8);
    }

    /// At the hook inside `CreateBoxMon`: applies the cheats that are on to
    /// the Pokémon being built.
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

    /// Rewrites the personality value (in the local and in the `BoxPokemon`)
    /// so the Pokémon is shiny for its OT ID. Unown is left alone.
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
        if species == shiny::UNOWN || !shiny::chosen(personality, ot_id, self.shiny_one_in) {
            return;
        }
        let shiny = shiny::shiny_personality(personality, ot_id);
        if shiny != personality {
            let _ = memory.write_u32(sp.wrapping_add(shiny::PERSONALITY_OFFSET), shiny);
            let _ = memory.write_u32(box_mon, shiny);
        }
    }

    /// Enables or disables idle-loop skipping (on by default; it is exact).
    pub fn set_idle_loop_skipping(&mut self, enabled: bool) {
        self.skip_idle_loops = enabled;
        self.idle.reset();
    }

    /// After a backward jump: if the loop is provably idle, skips whole
    /// iterations, stopping one short of the next event. Returns the cycles
    /// skipped.
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

    /// Advances the bus and draws the scanlines that finished.
    #[inline(always)]
    fn advance_bus(&mut self, cycles: u32) {
        self.memory.advance(cycles);
        if self.memory.has_scanlines() {
            self.finish_scanlines();
        }
    }

    #[inline(never)]
    fn finish_scanlines(&mut self) {
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

    /// Enables or disables drawing, to skip frames that will not be shown;
    /// emulation is unaffected. Switch it at frame boundaries.
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

    /// Installs the cheats file's codes; they survive a reset.
    pub fn set_cheats(&mut self, cheats: Cheats) {
        self.cheats = cheats;
        self.update_cheat_hook();
    }

    fn update_cheat_hook(&mut self) {
        // The file's codes run only while their menu row is on: some crash
        // the game if written before it has loaded the routine they patch.
        self.cheat_hook = if self.cheat_options.file_codes {
            self.cheats.hook_address().unwrap_or(NO_HOOK)
        } else {
            NO_HOOK
        };
        let options = self.cheat_options;
        self.catch_hook = armed(self.scan.catch_check, options.catch_always);
        self.encounter_hook = armed(self.scan.encounter_roll, options.no_encounters);
        self.exp_hook = armed(self.scan.exp_traded, options.exp_boost);
        self.money_hook = armed(self.scan.money_balance, options.infinite_money);
        // Shiny and perfect stats hook where a Pokémon is built; perfect
        // stats for wild Pokémon also watches `CreateWildMon`.
        self.make_shiny = options.shiny;
        self.shiny_one_in = options.shiny_odds.one_in();
        self.perfect_wild = options.perfect_for_wild();
        self.perfect_opponents = options.perfect_for_opponents();
        let building = self.make_shiny || self.perfect_wild || self.perfect_opponents;
        self.build_hook = armed(self.scan.shiny_point, building);
        self.wild_scope_hook = armed(
            self.scan.create_wild_mon,
            self.perfect_wild && self.build_hook != NO_HOOK,
        );
        self.wild_return = NO_HOOK;
        self.wild_species_hook = NO_HOOK;
        self.wild_species = NO_VALUE;
        self.wild_level = NO_VALUE;
        if let Some(entry) = self.scan.create_wild_mon {
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
        self.refresh_hook_filter();
    }

    /// Installs the cheat menu's options.
    pub fn set_cheat_options(&mut self, options: CheatOptions) {
        if options != self.cheat_options {
            self.cheat_options = options;
            self.update_cheat_hook();
        }
    }

    /// The hook addresses found in this ROM, for the settings file.
    pub fn cheat_scan(&self) -> CheatScan {
        self.scan.clone()
    }

    pub fn cheat_options(&self) -> CheatOptions {
        self.cheat_options
    }

    /// Where the wild-species override hooks (`CreateWildMon`'s entry), if
    /// the ROM is recognised.
    pub fn wild_species_hook_address(&self) -> Option<u32> {
        self.scan.create_wild_mon
    }

    /// Where the 100% catch rate cheat hooks, if the ROM is recognised.
    pub fn catch_check_address(&self) -> Option<u32> {
        self.scan.catch_check
    }

    /// Where the shiny and perfect-stats cheats hook, if the ROM is recognised.
    pub fn build_hook_address(&self) -> Option<u32> {
        self.scan.shiny_point
    }

    /// Whether the wild-species override can work with this ROM.
    pub fn wild_species_supported(&self) -> bool {
        self.scan.create_wild_mon.is_some()
    }

    pub fn cheats(&self) -> &Cheats {
        &self.cheats
    }

    /// Writes the cheats file's values into RAM; call once per finished
    /// frame. Codes with a hook address run at the hook instead.
    pub fn apply_cheats(&mut self) {
        if self.cheat_options.file_codes && self.cheat_hook == NO_HOOK && !self.cheats.is_empty() {
            self.cheats.apply(&mut self.memory);
        }
    }
}

#[cfg(test)]
mod cheat_tests;
#[cfg(test)]
mod tests;
