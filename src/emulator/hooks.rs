//! The cheats that act when the CPU reaches an address: finding which
//! addresses to watch, and what to change when one is reached.

use super::Emulator;
use crate::cheats::gen3::{
    experience, field_moves, field_poison, money, party_menu, perfect_stats, shiny,
};
use crate::cheats::options::CheatOptions;

/// Size of the hook filter, in bits (a power of two).
pub(super) const HOOK_FILTER_BITS: usize = 1 << 15;

/// A hook address that is not set; the program counter is never this value.
pub(super) const NO_HOOK: u32 = u32::MAX;
/// A register value that is never written: "leave this argument alone".
pub(super) const NO_VALUE: u32 = u32::MAX;
/// Catch `odds` that always pass the game's `odds > 254` test.
const CATCH_ALWAYS_ODDS: u32 = 0xFF;

/// `address` if the cheat that hooks there is on and its place was found,
/// else `NO_HOOK`.
fn armed(address: Option<u32>, on: bool) -> u32 {
    address.filter(|_| on).unwrap_or(NO_HOOK)
}

impl Emulator {
    /// The filter bit for `pc`.
    #[inline(always)]
    pub(super) fn hook_slot(pc: u32) -> usize {
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
    pub(super) fn refresh_hook_filter(&mut self) {
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
    pub(super) fn run_hooks(&mut self, pc_before: u32) {
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
        // No poison outside battle (always on): the status tested is "none".
        if Some(pc_before) == self.scan.field_poison {
            self.cpu.registers[0] = field_poison::NO_AILMENT;
        }
        // No random encounters: the dice roll always says no.
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

    /// Arms the hooks of the cheats that are on, from the options and the scan.
    pub(super) fn update_cheat_hook(&mut self) {
        // The file's codes run only while their row is on: some crash the game
        // if written before it has loaded the routine they patch.
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
}
