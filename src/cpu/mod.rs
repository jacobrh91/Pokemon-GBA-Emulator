mod arm;
mod arm_exec;
#[cfg(test)]
#[path = "../../tests/unit/cpu.rs"]
mod tests;
mod thumb;
mod thumb_exec;
use crate::cpu::arm::ArmOp;
use crate::cpu::thumb::ThumbOp;
use crate::mmu::{MemoryBus, ROM_START};
use std::io;

pub struct Cpu {
    pub registers: [u32; 16],
    pub cpsr: u32,
    pub cycles: u64,
    banked_user: [u32; 7],
    banked_fiq: [u32; 7],
    banked_modes: [[u32; 2]; 5],
    saved_status: [u32; 5],
    sequential_fetch: Option<(u32, bool)>,
}

const N_FLAG: u32 = 1 << 31;
const Z_FLAG: u32 = 1 << 30;
const C_FLAG: u32 = 1 << 29;
const V_FLAG: u32 = 1 << 28;
const T_FLAG: u32 = 1 << 5;
const I_FLAG: u32 = 1 << 7;
const F_FLAG: u32 = 1 << 6;
const MODE_MASK: u32 = 0x1F;
const MODE_USER: u32 = 0x10;
const MODE_FIQ: u32 = 0x11;
const MODE_IRQ: u32 = 0x12;
const MODE_SUPERVISOR: u32 = 0x13;
const MODE_ABORT: u32 = 0x17;
const MODE_UNDEFINED: u32 = 0x1B;
const MODE_SYSTEM: u32 = 0x1F;
const VECTOR_UNDEFINED: u32 = 0x04;
const VECTOR_SWI: u32 = 0x08;
const VECTOR_PREFETCH_ABORT: u32 = 0x0C;
const VECTOR_DATA_ABORT: u32 = 0x10;
const BIOS_IRQ_HANDLER_POINTER: u32 = 0x0300_7FFC;
const VECTOR_IRQ: u32 = 0x18;
const VECTOR_FIQ: u32 = 0x1C;
const GBA_SYSTEM_STACK: u32 = 0x0300_7F00;
const GBA_IRQ_STACK: u32 = 0x0300_7FA0;
const GBA_SUPERVISOR_STACK: u32 = 0x0300_7FE0;

impl Default for Cpu {
    fn default() -> Self {
        Self {
            registers: [0; 16],
            cpsr: MODE_SUPERVISOR | I_FLAG | F_FLAG,
            cycles: 0,
            banked_user: [0; 7],
            banked_fiq: [0; 7],
            banked_modes: [[0; 2]; 5],
            saved_status: [0; 5],
            sequential_fetch: None,
        }
    }
}

impl Cpu {
    pub fn initialize_gba_startup(&mut self, entry_point: u32) -> io::Result<()> {
        self.set_cpsr(MODE_SYSTEM)?;
        self.registers[13] = GBA_SYSTEM_STACK;

        self.set_cpsr(MODE_IRQ)?;
        self.registers[13] = GBA_IRQ_STACK;

        self.set_cpsr(MODE_SUPERVISOR)?;
        self.registers[13] = GBA_SUPERVISOR_STACK;

        self.set_cpsr(MODE_SYSTEM)?;
        self.registers[15] = entry_point;
        self.cycles = 0;
        self.sequential_fetch = None;
        Ok(())
    }

    /// Fetches and executes one supported ARM instruction.
    ///
    /// Returns instruction cycles including estimated memory wait states.
    pub fn step_arm(&mut self, bus: &mut MemoryBus) -> io::Result<u32> {
        let pc = self.registers[15];
        if pc & 3 != 0 {
            return self.take_exception(
                MODE_ABORT,
                VECTOR_PREFETCH_ABORT,
                pc.wrapping_add(4),
                self.cpsr,
                3,
            );
        }

        let instruction = match bus.read_u32(pc) {
            Ok(instruction) => instruction,
            Err(_) => {
                return self.take_exception(
                    MODE_ABORT,
                    VECTOR_PREFETCH_ABORT,
                    pc.wrapping_add(4),
                    self.cpsr,
                    3,
                );
            }
        };
        let fetch_sequential = self.sequential_fetch == Some((pc, false));
        let fetch_wait_cycles = bus
            .transfer_cycles(pc, 4, fetch_sequential)?
            .saturating_sub(1);
        let op = arm::decode(instruction);
        let condition = match &op {
            ArmOp::DataProcessingImmediate { condition, .. }
            | ArmOp::DataProcessingRegister { condition, .. }
            | ArmOp::SingleDataTransfer { condition, .. }
            | ArmOp::HalfwordTransfer { condition, .. }
            | ArmOp::BlockTransfer { condition, .. }
            | ArmOp::Multiply { condition, .. }
            | ArmOp::MultiplyLong { condition, .. }
            | ArmOp::ReadStatus { condition, .. }
            | ArmOp::WriteStatusRegister { condition, .. }
            | ArmOp::WriteStatusImmediate { condition, .. }
            | ArmOp::Swap { condition, .. }
            | ArmOp::BranchExchange { condition, .. }
            | ArmOp::Branch { condition, .. }
            | ArmOp::SoftwareInterrupt { condition, .. } => *condition,
            ArmOp::Unknown(instruction) if instruction >> 28 == 0xF => {
                return self.take_exception(
                    MODE_UNDEFINED,
                    VECTOR_UNDEFINED,
                    pc.wrapping_add(4),
                    self.cpsr,
                    exception_entry_cycles(3, fetch_wait_cycles)?,
                );
            }
            ArmOp::Unknown(instruction) => (instruction >> 28) as u8,
        };
        let condition_passed = self.condition_passed(condition);
        let refills_pipeline = match &op {
            ArmOp::Branch { .. }
            | ArmOp::BranchExchange { .. }
            | ArmOp::SoftwareInterrupt { .. }
            | ArmOp::DataProcessingImmediate { rd: 15, .. }
            | ArmOp::DataProcessingRegister { rd: 15, .. }
            | ArmOp::SingleDataTransfer {
                load: true, rd: 15, ..
            }
            | ArmOp::HalfwordTransfer {
                load: true, rd: 15, ..
            } => true,
            ArmOp::BlockTransfer {
                load: true,
                register_list,
                ..
            } => *register_list == 0 || register_list & (1 << 15) != 0,
            _ => false,
        };
        let base_cycles = if !condition_passed {
            1
        } else {
            match &op {
                ArmOp::Branch { .. }
                | ArmOp::BranchExchange { .. }
                | ArmOp::SoftwareInterrupt { .. }
                | ArmOp::DataProcessingImmediate { rd: 15, .. }
                | ArmOp::DataProcessingRegister { rd: 15, .. } => 3,
                ArmOp::SingleDataTransfer { load: true, .. }
                | ArmOp::HalfwordTransfer { load: true, .. } => 3,
                ArmOp::SingleDataTransfer { .. } | ArmOp::HalfwordTransfer { .. } => 2,
                ArmOp::BlockTransfer {
                    load,
                    register_list,
                    ..
                } => {
                    let has_pc = *register_list == 0 || register_list & (1 << 15) != 0;
                    let count = if *register_list == 0 {
                        1
                    } else {
                        register_list.count_ones()
                    };
                    count + u32::from(*load) + u32::from(*load && has_pc)
                }
                ArmOp::Multiply { accumulate, .. } => 2 + u32::from(*accumulate),
                ArmOp::MultiplyLong { accumulate, .. } => 3 + u32::from(*accumulate),
                ArmOp::Swap { .. } => 1,
                ArmOp::DataProcessingRegister {
                    shift_by_register: true,
                    ..
                } => 2,
                _ => 1,
            }
        };
        let wait_cycles = if condition_passed {
            match arm_wait_cycles(
                &op,
                bus,
                pc,
                fetch_sequential,
                &self.registers,
                self.cpsr & C_FLAG != 0,
            ) {
                Ok(cycles) => cycles,
                Err(error) if is_memory_fault(&error) => {
                    return self.take_exception(
                        MODE_ABORT,
                        VECTOR_DATA_ABORT,
                        pc.wrapping_add(8),
                        self.cpsr,
                        exception_entry_cycles(4, fetch_wait_cycles)?,
                    );
                }
                Err(error) => return Err(error),
            }
        } else {
            fetch_wait_cycles
        };
        let instruction_cycles = base_cycles
            .checked_add(wait_cycles)
            .ok_or_else(|| io::Error::other("instruction cycle count overflow"))?;
        let total_cycles = self
            .cycles
            .checked_add(u64::from(instruction_cycles))
            .ok_or_else(|| io::Error::other("CPU cycle counter overflow"))?;

        let instruction_status = self.cpsr;
        if condition_passed {
            if let Err(error) = self.execute_arm(op, pc, bus) {
                let (mode, vector, return_address, cycles) = if is_memory_fault(&error) {
                    (
                        MODE_ABORT,
                        VECTOR_DATA_ABORT,
                        pc.wrapping_add(8),
                        exception_entry_cycles(4, fetch_wait_cycles)?,
                    )
                } else if matches!(
                    error.kind(),
                    io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput
                ) {
                    (
                        MODE_UNDEFINED,
                        VECTOR_UNDEFINED,
                        pc.wrapping_add(4),
                        exception_entry_cycles(3, fetch_wait_cycles)?,
                    )
                } else {
                    return Err(error);
                };
                return self.take_exception(
                    mode,
                    vector,
                    return_address,
                    instruction_status,
                    cycles,
                );
            }
        } else {
            self.registers[15] = pc.wrapping_add(4);
        }
        self.cycles = total_cycles;
        self.sequential_fetch = if refills_pipeline && condition_passed {
            None
        } else {
            Some((pc.wrapping_add(4), false))
        };
        Ok(instruction_cycles)
    }

    /// Fetches and executes one Thumb instruction at the current PC.
    ///
    /// The PC register points to the instruction being fetched. Sequential
    /// instructions advance it by two bytes. Cycles count base instruction
    /// cycles, including estimated memory wait states.
    pub fn step_thumb(&mut self, bus: &mut MemoryBus) -> io::Result<u32> {
        let pc = self.registers[15];
        if pc & 1 != 0 {
            return self.take_exception(
                MODE_ABORT,
                VECTOR_PREFETCH_ABORT,
                pc.wrapping_add(4),
                self.cpsr,
                3,
            );
        }

        let opcode = match bus.read_u16(pc) {
            Ok(opcode) => opcode,
            Err(_) => {
                return self.take_exception(
                    MODE_ABORT,
                    VECTOR_PREFETCH_ABORT,
                    pc.wrapping_add(4),
                    self.cpsr,
                    3,
                );
            }
        };
        let fetch_sequential = self.sequential_fetch == Some((pc, true));
        let fetch_wait_cycles = bus
            .transfer_cycles(pc, 2, fetch_sequential)?
            .saturating_sub(1);
        let op = thumb::decode(opcode);
        let base_cycles: u32 = match &op {
            ThumbOp::ShiftImmediate { .. }
            | ThumbOp::MoveImmediate { .. }
            | ThumbOp::CompareImmediate { .. }
            | ThumbOp::AddImmediate { .. }
            | ThumbOp::SubtractImmediate { .. } => 1,
            ThumbOp::AddSubtract { .. } | ThumbOp::Alu { .. } => 1,
            ThumbOp::HighRegister {
                operation: 0 | 2,
                rd: 15,
                ..
            }
            | ThumbOp::BranchExchange { .. } => 3,
            ThumbOp::HighRegister { .. } => 1,
            ThumbOp::LoadPcRelative { .. } => 3,
            ThumbOp::LoadStoreRegister { operation, .. } => {
                if *operation <= 2 {
                    2
                } else {
                    3
                }
            }
            ThumbOp::LoadStoreImmediate { .. } => 2,
            ThumbOp::LoadStoreHalfwordImmediate { load, .. }
            | ThumbOp::SpRelativeLoadStore { load, .. } => {
                if *load {
                    3
                } else {
                    2
                }
            }
            ThumbOp::LoadAddress { .. } | ThumbOp::AdjustSp { .. } => 1,
            ThumbOp::PushPop {
                pop,
                register_list,
                extra_register,
            } => {
                let count = register_list.count_ones() + u32::from(*extra_register);
                count + if *pop { 2 } else { 1 }
            }
            ThumbOp::MultipleTransfer {
                load,
                register_list,
                ..
            } => register_list.count_ones() + u32::from(*load),
            ThumbOp::BranchConditional { condition, .. } => {
                if self.condition_passed(*condition) {
                    3
                } else {
                    1
                }
            }
            ThumbOp::SoftwareInterrupt { .. } => 3,
            ThumbOp::LongBranchPrefix { .. } => 1,
            ThumbOp::LongBranchSuffix { .. } => 3,
            ThumbOp::BranchUnconditional { .. } => 3,
            ThumbOp::Unknown(_) => {
                return self.take_exception(
                    MODE_UNDEFINED,
                    VECTOR_UNDEFINED,
                    pc.wrapping_add(2),
                    self.cpsr,
                    exception_entry_cycles(3, fetch_wait_cycles)?,
                );
            }
        };
        let wait_cycles = match thumb_wait_cycles(&op, bus, pc, fetch_sequential, &self.registers) {
            Ok(cycles) => cycles,
            Err(error) if is_memory_fault(&error) => {
                return self.take_exception(
                    MODE_ABORT,
                    VECTOR_DATA_ABORT,
                    pc.wrapping_add(8),
                    self.cpsr,
                    exception_entry_cycles(4, fetch_wait_cycles)?,
                );
            }
            Err(error) => return Err(error),
        };
        let instruction_cycles = base_cycles
            .checked_add(wait_cycles)
            .ok_or_else(|| io::Error::other("instruction cycle count overflow"))?;
        let total_cycles = self
            .cycles
            .checked_add(u64::from(instruction_cycles))
            .ok_or_else(|| io::Error::other("CPU cycle counter overflow"))?;

        let branched = match &op {
            ThumbOp::BranchUnconditional { .. }
            | ThumbOp::BranchExchange { .. }
            | ThumbOp::LongBranchSuffix { .. }
            | ThumbOp::SoftwareInterrupt { .. }
            | ThumbOp::HighRegister {
                operation: 0 | 2,
                rd: 15,
                ..
            }
            | ThumbOp::PushPop {
                pop: true,
                extra_register: true,
                ..
            } => true,
            ThumbOp::BranchConditional { condition, .. } => self.condition_passed(*condition),
            _ => false,
        };
        let instruction_status = self.cpsr;
        if let Err(error) = self.execute_thumb(op, pc, bus) {
            let (mode, vector, return_address, cycles) = if is_memory_fault(&error) {
                (
                    MODE_ABORT,
                    VECTOR_DATA_ABORT,
                    pc.wrapping_add(8),
                    exception_entry_cycles(4, fetch_wait_cycles)?,
                )
            } else if matches!(
                error.kind(),
                io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput
            ) {
                (
                    MODE_UNDEFINED,
                    VECTOR_UNDEFINED,
                    pc.wrapping_add(2),
                    exception_entry_cycles(3, fetch_wait_cycles)?,
                )
            } else {
                return Err(error);
            };
            return self.take_exception(mode, vector, return_address, instruction_status, cycles);
        }
        if !branched {
            self.registers[15] = pc.wrapping_add(2);
        }
        self.cycles = total_cycles;
        self.sequential_fetch = if branched {
            None
        } else {
            Some((pc.wrapping_add(2), true))
        };

        Ok(instruction_cycles)
    }

    fn set_flag(&mut self, flag: u32, value: bool) {
        if value {
            self.cpsr |= flag;
        } else {
            self.cpsr &= !flag;
        }
    }

    fn set_nz(&mut self, result: u32) {
        self.set_flag(N_FLAG, result & 0x8000_0000 != 0);
        self.set_flag(Z_FLAG, result == 0);
    }

    fn set_arithmetic_flags(&mut self, result: u32, carry: bool, overflow: bool) {
        self.set_nz(result);
        self.set_flag(C_FLAG, carry);
        self.set_flag(V_FLAG, overflow);
    }

    fn execute_thumb_arithmetic(&mut self, rd: u8, lhs: u32, rhs: u32, subtract: bool) {
        if subtract {
            let (result, carry, overflow) = subtract_with_borrow(lhs, rhs, false);
            self.registers[rd as usize] = result;
            self.set_arithmetic_flags(result, carry, overflow);
        } else {
            let (result, carry, overflow) = add_with_carry(lhs, rhs, false);
            self.registers[rd as usize] = result;
            self.set_arithmetic_flags(result, carry, overflow);
        }
    }

    pub fn mode(&self) -> u32 {
        self.cpsr & MODE_MASK
    }

    fn read_user_register(&self, register: u8) -> u32 {
        match register {
            0..=7 => self.registers[register as usize],
            8..=12 if self.mode() == MODE_FIQ => self.banked_user[usize::from(register - 8)],
            8..=12 => self.registers[register as usize],
            13..=14 if matches!(self.mode(), MODE_USER | MODE_SYSTEM) => {
                self.registers[register as usize]
            }
            13..=14 => self.banked_user[usize::from(register - 8)],
            _ => self.registers[register as usize],
        }
    }

    fn write_user_register(&mut self, register: u8, value: u32) {
        match register {
            0..=7 => self.registers[register as usize] = value,
            8..=12 if self.mode() == MODE_FIQ => {
                self.banked_user[usize::from(register - 8)] = value
            }
            8..=12 => self.registers[register as usize] = value,
            13..=14 if matches!(self.mode(), MODE_USER | MODE_SYSTEM) => {
                self.registers[register as usize] = value
            }
            13..=14 => self.banked_user[usize::from(register - 8)] = value,
            _ => self.registers[register as usize] = value,
        }
    }

    pub fn saved_status(&self) -> io::Result<u32> {
        let index = mode_index(self.mode()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "current CPU mode has no SPSR")
        })?;
        Ok(self.saved_status[index])
    }

    fn write_status_register(&mut self, saved: bool, field_mask: u8, value: u32) -> io::Result<()> {
        let mut mask = 0;
        for field in 0..4 {
            if field_mask & (1 << field) != 0 {
                mask |= 0xFF << (field * 8);
            }
        }

        if saved {
            let index = mode_index(self.mode()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "current CPU mode has no SPSR")
            })?;
            self.saved_status[index] = (self.saved_status[index] & !mask) | (value & mask);
        } else {
            if self.mode() == MODE_USER {
                mask &= 0xFF00_0000;
            }
            let new_cpsr = (self.cpsr & !mask) | (value & mask);
            self.set_cpsr(new_cpsr)?;
        }
        Ok(())
    }

    pub fn set_cpsr(&mut self, value: u32) -> io::Result<()> {
        let new_mode = value & MODE_MASK;
        if !valid_mode(new_mode) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid ARM CPU mode: {new_mode:#04X}"),
            ));
        }
        let old_mode = self.mode();
        if old_mode != new_mode {
            self.save_active_bank(old_mode)?;
            self.load_active_bank(new_mode)?;
        }
        if (self.cpsr ^ value) & T_FLAG != 0 {
            self.sequential_fetch = None;
        }
        self.cpsr = value;
        Ok(())
    }

    fn save_active_bank(&mut self, mode: u32) -> io::Result<()> {
        if mode == MODE_FIQ {
            self.banked_fiq.copy_from_slice(&self.registers[8..15]);
        } else {
            self.banked_user[..5].copy_from_slice(&self.registers[8..13]);
            if let Some(index) = mode_index(mode) {
                self.banked_modes[index].copy_from_slice(&self.registers[13..15]);
            } else if mode == MODE_USER || mode == MODE_SYSTEM {
                self.banked_user[5..7].copy_from_slice(&self.registers[13..15]);
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot save registers for invalid CPU mode",
                ));
            }
        }
        Ok(())
    }

    fn load_active_bank(&mut self, mode: u32) -> io::Result<()> {
        if mode == MODE_FIQ {
            self.registers[8..15].copy_from_slice(&self.banked_fiq);
        } else {
            self.registers[8..13].copy_from_slice(&self.banked_user[..5]);
            if let Some(index) = mode_index(mode) {
                self.registers[13..15].copy_from_slice(&self.banked_modes[index]);
            } else if mode == MODE_USER || mode == MODE_SYSTEM {
                self.registers[13..15].copy_from_slice(&self.banked_user[5..7]);
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot load registers for invalid CPU mode",
                ));
            }
        }
        Ok(())
    }

    fn enter_exception(&mut self, mode: u32, vector: u32, return_address: u32) -> io::Result<()> {
        let old_cpsr = self.cpsr;
        self.enter_exception_with_status(mode, vector, return_address, old_cpsr)
    }

    fn enter_exception_with_status(
        &mut self,
        mode: u32,
        vector: u32,
        return_address: u32,
        saved_status: u32,
    ) -> io::Result<()> {
        let mut exception_status = (saved_status & !MODE_MASK & !T_FLAG) | mode | I_FLAG;
        if mode == MODE_FIQ {
            exception_status |= F_FLAG;
        }
        self.set_cpsr(exception_status)?;
        let index = mode_index(mode).expect("exception modes always have an SPSR");
        self.saved_status[index] = saved_status;
        self.registers[14] = return_address;
        self.registers[15] = vector;
        self.sequential_fetch = None;
        Ok(())
    }

    fn take_exception(
        &mut self,
        mode: u32,
        vector: u32,
        return_address: u32,
        saved_status: u32,
        cycles: u32,
    ) -> io::Result<u32> {
        let total_cycles = self
            .cycles
            .checked_add(u64::from(cycles))
            .ok_or_else(|| io::Error::other("CPU cycle counter overflow during exception entry"))?;
        self.enter_exception_with_status(mode, vector, return_address, saved_status)?;
        self.cycles = total_cycles;
        Ok(cycles)
    }

    pub fn request_irq(&mut self, bus: &MemoryBus) -> io::Result<bool> {
        if self.cpsr & I_FLAG != 0 {
            return Ok(false);
        }
        let total_cycles = self
            .cycles
            .checked_add(3)
            .ok_or_else(|| io::Error::other("CPU cycle counter overflow during IRQ entry"))?;
        let return_address = self.registers[15].wrapping_add(4);
        self.enter_exception(MODE_IRQ, VECTOR_IRQ, return_address)?;
        let handler = bus.read_u32(BIOS_IRQ_HANDLER_POINTER)?;
        if handler != 0 {
            self.registers[15] = handler & !1;
        }
        self.cycles = total_cycles;
        Ok(true)
    }

    pub fn request_fiq(&mut self) -> io::Result<bool> {
        if self.cpsr & F_FLAG != 0 {
            return Ok(false);
        }
        let total_cycles = self
            .cycles
            .checked_add(3)
            .ok_or_else(|| io::Error::other("CPU cycle counter overflow during FIQ entry"))?;
        let return_address = self.registers[15].wrapping_add(4);
        self.enter_exception(MODE_FIQ, VECTOR_FIQ, return_address)?;
        self.cycles = total_cycles;
        Ok(true)
    }

    pub fn handle_bios_swi(&mut self, service: u8, bus: &mut MemoryBus) -> io::Result<()> {
        if self.mode() != MODE_SUPERVISOR || self.registers[15] != 0x08 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "BIOS SWI handler invoked outside the SVC vector",
            ));
        }

        match service {
            0x00 => {
                *self = Cpu::default();
                self.initialize_gba_startup(ROM_START)?;
                return Ok(());
            }
            0x01 => bus.register_ram_reset(self.registers[0]),
            // The placeholder PPU does not generate VBlank interrupts.
            0x05 => {}
            0x0B => {
                let mut source = self.registers[0];
                let mut destination = self.registers[1];
                let control = self.registers[2];
                let word_transfer = control & (1 << 26) != 0;
                let fill = control & (1 << 24) != 0;
                let count = control & 0x001F_FFFF;

                for _ in 0..count {
                    if word_transfer {
                        let value = bus.read_u32(source & !3)?;
                        bus.write_u32(destination & !3, value)?;
                        if !fill {
                            source = source.wrapping_add(4);
                        }
                        destination = destination.wrapping_add(4);
                    } else {
                        let value = bus.read_u16(source & !1)?;
                        bus.write_u16(destination & !1, value)?;
                        if !fill {
                            source = source.wrapping_add(2);
                        }
                        destination = destination.wrapping_add(2);
                    }
                }
            }
            0x11 | 0x12 => bios_lz77_decompress(bus, self.registers[0], self.registers[1])?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("unsupported BIOS software interrupt: {service:#04X}"),
                ));
            }
        }

        let return_address = self.registers[14];
        let saved_status = self.saved_status()?;
        self.set_cpsr(saved_status)?;
        self.registers[15] = return_address;
        Ok(())
    }

    fn condition_passed(&self, condition: u8) -> bool {
        let n = self.cpsr & N_FLAG != 0;
        let z = self.cpsr & Z_FLAG != 0;
        let c = self.cpsr & C_FLAG != 0;
        let v = self.cpsr & V_FLAG != 0;

        match condition {
            0x0 => z,
            0x1 => !z,
            0x2 => c,
            0x3 => !c,
            0x4 => n,
            0x5 => !n,
            0x6 => v,
            0x7 => !v,
            0x8 => c && !z,
            0x9 => !c || z,
            0xA => n == v,
            0xB => n != v,
            0xC => !z && n == v,
            0xD => z || n != v,
            0xE => true,
            _ => false,
        }
    }
}

fn exception_entry_cycles(base_cycles: u32, fetch_wait_cycles: u32) -> io::Result<u32> {
    base_cycles
        .checked_add(fetch_wait_cycles)
        .ok_or_else(|| io::Error::other("exception cycle count overflow"))
}

fn is_memory_fault(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::AddrNotAvailable
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::PermissionDenied
    )
}

fn thumb_wait_cycles(
    op: &ThumbOp,
    bus: &MemoryBus,
    pc: u32,
    fetch_sequential: bool,
    registers: &[u32; 16],
) -> io::Result<u32> {
    let mut wait_cycles = bus
        .transfer_cycles(pc, 2, fetch_sequential)?
        .saturating_sub(1);
    let data_accesses: Vec<(u32, u8)> = match op {
        ThumbOp::LoadStoreRegister {
            operation, rb, ro, ..
        } => {
            let address = registers[*rb as usize].wrapping_add(registers[*ro as usize]);
            let width = match operation {
                0 | 4 => 4,
                1 | 5 | 7 => 2,
                2 | 3 | 6 => 1,
                _ => unreachable!(),
            };
            vec![(address, width)]
        }
        ThumbOp::LoadPcRelative { offset, .. } => vec![(
            (pc.wrapping_add(4) & !3).wrapping_add(u32::from(*offset) * 4),
            4,
        )],
        ThumbOp::LoadStoreImmediate {
            byte, rb, offset, ..
        } => vec![(
            registers[*rb as usize].wrapping_add(u32::from(*offset) * if *byte { 1 } else { 4 }),
            if *byte { 1 } else { 4 },
        )],
        ThumbOp::LoadStoreHalfwordImmediate { rb, offset, .. } => vec![(
            registers[*rb as usize].wrapping_add(u32::from(*offset) * 2),
            2,
        )],
        ThumbOp::SpRelativeLoadStore { offset, .. } => {
            vec![(registers[13].wrapping_add(u32::from(*offset) * 4), 4)]
        }
        ThumbOp::PushPop {
            pop,
            extra_register,
            register_list,
        } => {
            let count = register_list.count_ones() + u32::from(*extra_register);
            let start = if *pop {
                registers[13]
            } else {
                registers[13].wrapping_sub(count * 4)
            };
            (0..count)
                .map(|index| (start.wrapping_add(index * 4), 4))
                .collect()
        }
        ThumbOp::MultipleTransfer {
            rb, register_list, ..
        } => (0..register_list.count_ones())
            .map(|index| (registers[*rb as usize].wrapping_add(index * 4), 4))
            .collect(),
        _ => Vec::<(u32, u8)>::new(),
    };
    wait_cycles = add_data_wait_cycles(wait_cycles, bus, &data_accesses)?;
    Ok(wait_cycles)
}

fn add_data_wait_cycles(
    mut wait_cycles: u32,
    bus: &MemoryBus,
    accesses: &[(u32, u8)],
) -> io::Result<u32> {
    for (index, (address, width)) in accesses.iter().copied().enumerate() {
        wait_cycles = wait_cycles
            .checked_add(
                bus.transfer_cycles(address, width, index != 0)?
                    .saturating_sub(1),
            )
            .ok_or_else(|| io::Error::other("bus wait-state cycle count overflow"))?;
    }
    Ok(wait_cycles)
}

fn arm_wait_cycles(
    op: &ArmOp,
    bus: &MemoryBus,
    pc: u32,
    fetch_sequential: bool,
    registers: &[u32; 16],
    carry: bool,
) -> io::Result<u32> {
    let wait_cycles = bus
        .transfer_cycles(pc, 4, fetch_sequential)?
        .saturating_sub(1);
    let data_accesses: Vec<(u32, u8)> = match op {
        ArmOp::Swap { byte, rn, .. } => {
            let address = registers[*rn as usize];
            let width = if *byte { 1 } else { 4 };
            vec![(address, width), (address, width)]
        }
        ArmOp::SingleDataTransfer {
            pre_index,
            add_offset,
            byte,
            rn,
            offset_immediate,
            offset,
            rm,
            shift_type,
            shift_amount,
            ..
        } => {
            let base = if *rn == 15 {
                pc.wrapping_add(8)
            } else {
                registers[*rn as usize]
            };
            let displacement = if *offset_immediate {
                u32::from(*offset)
            } else {
                let value = if *rm == 15 {
                    pc.wrapping_add(12)
                } else {
                    registers[*rm as usize]
                };
                shift_operand(value, *shift_type, u32::from(*shift_amount), false, carry).0
            };
            let adjusted = if *add_offset {
                base.wrapping_add(displacement)
            } else {
                base.wrapping_sub(displacement)
            };
            let address = if *pre_index { adjusted } else { base };
            vec![(address, if *byte { 1 } else { 4 })]
        }
        ArmOp::HalfwordTransfer {
            pre_index,
            add_offset,
            rn,
            immediate_offset,
            offset,
            ..
        } => {
            let base = if *rn == 15 {
                pc.wrapping_add(8)
            } else {
                registers[*rn as usize]
            };
            let displacement = if *immediate_offset {
                u32::from(*offset)
            } else {
                0
            };
            let adjusted = if *add_offset {
                base.wrapping_add(displacement)
            } else {
                base.wrapping_sub(displacement)
            };
            vec![(if *pre_index { adjusted } else { base }, 2)]
        }
        ArmOp::BlockTransfer {
            pre_index,
            add_offset,
            rn,
            register_list,
            ..
        } => {
            let transfer_list = if *register_list == 0 {
                1 << 15
            } else {
                *register_list
            };
            let count = if *register_list == 0 {
                16
            } else {
                transfer_list.count_ones()
            };
            let base = if *rn == 15 {
                pc.wrapping_add(8)
            } else {
                registers[*rn as usize]
            };
            let span = count * 4;
            let start = match (add_offset, pre_index) {
                (true, true) => base.wrapping_add(4),
                (true, false) => base,
                (false, true) => base.wrapping_sub(span),
                (false, false) => base.wrapping_sub(span).wrapping_add(4),
            };
            (0..transfer_list.count_ones())
                .map(|index| (start.wrapping_add(index * 4), 4))
                .collect()
        }
        _ => Vec::new(),
    };
    add_data_wait_cycles(wait_cycles, bus, &data_accesses)
}

fn bios_lz77_decompress(bus: &mut MemoryBus, source: u32, destination: u32) -> io::Result<()> {
    let header = bus.read_u8(source)?;
    if header != 0x10 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid BIOS LZ77 header: {header:#04X}"),
        ));
    }

    let length_low = u32::from(bus.read_u8(checked_lz77_address(source, 1)?)?);
    let length_middle = u32::from(bus.read_u8(checked_lz77_address(source, 2)?)?);
    let length_high = u32::from(bus.read_u8(checked_lz77_address(source, 3)?)? & 0x0F);
    let output_length = length_low | (length_middle << 8) | (length_high << 16);
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_length as usize)
        .map_err(io::Error::other)?;
    let mut input_address = checked_lz77_address(source, 4)?;

    while output.len() < output_length as usize {
        let flags = bus.read_u8(input_address)?;
        input_address = checked_lz77_address(input_address, 1)?;
        for bit in (0..8).rev() {
            if output.len() == output_length as usize {
                break;
            }
            if flags & (1 << bit) == 0 {
                output.push(bus.read_u8(input_address)?);
                input_address = checked_lz77_address(input_address, 1)?;
                continue;
            }

            let first = bus.read_u8(input_address)?;
            let second_address = checked_lz77_address(input_address, 1)?;
            let second = bus.read_u8(second_address)?;
            input_address = checked_lz77_address(input_address, 2)?;
            let copy_length = usize::from(first >> 4) + 3;
            let displacement = usize::from((u16::from(first & 0x0F) << 8) | u16::from(second)) + 1;
            if displacement > output.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "BIOS LZ77 back-reference precedes the decompressed output",
                ));
            }
            for _ in 0..copy_length {
                if output.len() == output_length as usize {
                    break;
                }
                let value = output[output.len() - displacement];
                output.push(value);
            }
        }
    }

    for (index, value) in output.into_iter().enumerate() {
        let address = destination.checked_add(index as u32).ok_or_else(|| {
            io::Error::new(io::ErrorKind::AddrNotAvailable, "LZ77 output overflow")
        })?;
        bus.write_u8(address, value)?;
    }
    Ok(())
}

fn checked_lz77_address(address: u32, offset: u32) -> io::Result<u32> {
    address.checked_add(offset).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "BIOS LZ77 input address overflow",
        )
    })
}

fn add_with_carry(lhs: u32, rhs: u32, carry_in: bool) -> (u32, bool, bool) {
    let carry_value = u64::from(carry_in);
    let unsigned_result = u64::from(lhs) + u64::from(rhs) + carry_value;
    let signed_result = i64::from(lhs as i32) + i64::from(rhs as i32) + i64::from(carry_in);
    (
        unsigned_result as u32,
        unsigned_result > u64::from(u32::MAX),
        signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
    )
}

fn subtract_with_borrow(lhs: u32, rhs: u32, borrow_in: bool) -> (u32, bool, bool) {
    let subtrahend = u64::from(rhs) + u64::from(borrow_in);
    let signed_result = i64::from(lhs as i32) - i64::from(rhs as i32) - i64::from(borrow_in);
    (
        lhs.wrapping_sub(rhs).wrapping_sub(u32::from(borrow_in)),
        u64::from(lhs) >= subtrahend,
        signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
    )
}

fn valid_mode(mode: u32) -> bool {
    matches!(
        mode,
        MODE_USER
            | MODE_FIQ
            | MODE_IRQ
            | MODE_SUPERVISOR
            | MODE_ABORT
            | MODE_UNDEFINED
            | MODE_SYSTEM
    )
}

fn mode_index(mode: u32) -> Option<usize> {
    match mode {
        MODE_FIQ => Some(0),
        MODE_IRQ => Some(1),
        MODE_SUPERVISOR => Some(2),
        MODE_ABORT => Some(3),
        MODE_UNDEFINED => Some(4),
        _ => None,
    }
}

fn shift_operand(
    value: u32,
    shift_type: u8,
    amount: u32,
    by_register: bool,
    carry_in: bool,
) -> (u32, bool) {
    match shift_type {
        0 => match amount {
            0 => (value, carry_in),
            1..=31 => (value << amount, value & (1 << (32 - amount)) != 0),
            32 => (0, value & 1 != 0),
            _ => (0, false),
        },
        1 => {
            let amount = if amount == 0 && !by_register {
                32
            } else {
                amount
            };
            match amount {
                0 => (value, carry_in),
                1..=31 => (value >> amount, value & (1 << (amount - 1)) != 0),
                32 => (0, value & 0x8000_0000 != 0),
                _ => (0, false),
            }
        }
        2 => {
            let amount = if amount == 0 && !by_register {
                32
            } else {
                amount
            };
            match amount {
                0 => (value, carry_in),
                1..=31 => (
                    ((value as i32) >> amount) as u32,
                    value & (1 << (amount - 1)) != 0,
                ),
                _ => {
                    let sign = value & 0x8000_0000 != 0;
                    (if sign { u32::MAX } else { 0 }, sign)
                }
            }
        }
        3 => {
            if amount == 0 && !by_register {
                ((u32::from(carry_in) << 31) | (value >> 1), value & 1 != 0)
            } else if amount == 0 {
                (value, carry_in)
            } else {
                let rotate = amount & 31;
                if rotate == 0 {
                    (value, value & 0x8000_0000 != 0)
                } else {
                    let result = value.rotate_right(rotate);
                    (result, result & 0x8000_0000 != 0)
                }
            }
        }
        _ => unreachable!("ARM shift type uses two bits"),
    }
}
