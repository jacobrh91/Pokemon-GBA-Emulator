mod alu;
mod arm;
mod bios;
mod state;
#[cfg(test)]
mod tests;
mod thumb;
use crate::cpu::alu::{
    add_with_carry, multiply_internal_cycles, shift_operand, subtract_with_borrow,
};
use crate::mmu::{BIOS_OPEN_BUS_AFTER_IRQ, BIOS_OPEN_BUS_AFTER_SWI, MemoryBus, ROM_START};
pub use bios::BIOS_IRQ_RETURN;
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
    /// Wait states accumulated by the data accesses of the instruction
    /// being executed.
    data_wait: u32,
    /// Set by BIOS Halt/IntrWait; the system owner idles until an interrupt.
    pub halted: bool,
    intr_waiting: bool,
}

/// Upper bound on the cycles one instruction can take (a 16-word block
/// transfer with maximum wait states fits well within this).
const MAX_INSTRUCTION_CYCLES: u64 = 4096;

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
            data_wait: 0,
            halted: false,
            intr_waiting: false,
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

    fn set_flag(&mut self, flag: u32, value: bool) {
        if value {
            self.cpsr |= flag;
        } else {
            self.cpsr &= !flag;
        }
    }

    /// N and Z from `result`, in one update of the status register.
    #[inline(always)]
    fn set_nz(&mut self, result: u32) {
        self.cpsr = (self.cpsr & !(N_FLAG | Z_FLAG))
            | (result & N_FLAG)
            | (u32::from(result == 0) * Z_FLAG);
    }

    /// N and Z from `result`, with C and V given.
    #[inline(always)]
    fn set_arithmetic_flags(&mut self, result: u32, carry: bool, overflow: bool) {
        self.cpsr = (self.cpsr & !(N_FLAG | Z_FLAG | C_FLAG | V_FLAG))
            | (result & N_FLAG)
            | (u32::from(result == 0) * Z_FLAG)
            | (u32::from(carry) * C_FLAG)
            | (u32::from(overflow) * V_FLAG);
    }

    /// N and Z from `result`, with C given (V is left alone).
    #[inline(always)]
    fn set_nz_and_carry(&mut self, result: u32, carry: bool) {
        self.cpsr = (self.cpsr & !(N_FLAG | Z_FLAG | C_FLAG))
            | (result & N_FLAG)
            | (u32::from(result == 0) * Z_FLAG)
            | (u32::from(carry) * C_FLAG);
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
            // The state bit only changes through BX and exception return.
            mask &= !T_FLAG;
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

    pub fn request_irq(&mut self, bus: &mut MemoryBus) -> io::Result<bool> {
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
            // BIOS trampoline: stmfd sp!, {r0-r3, r12, lr}; lr = return stub;
            // ldr pc, [BIOS_IRQ_HANDLER_POINTER]. The stub pops the frame.
            let mut sp = self.registers[13];
            for register in [14, 12, 3, 2, 1, 0] {
                sp = sp.wrapping_sub(4);
                bus.write_u32(sp, self.registers[register])?;
            }
            self.registers[13] = sp;
            self.registers[14] = BIOS_IRQ_RETURN;
            self.registers[15] = handler & !3;
        }
        self.halted = false;
        bus.set_bios_open_bus(BIOS_OPEN_BUS_AFTER_IRQ);
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

    #[inline]
    fn condition_passed(&self, condition: u8) -> bool {
        if condition == 0xE {
            return true;
        }
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

/// LDRH: an odd address rotates the aligned halfword by 8 bits.
fn load_halfword(bus: &MemoryBus, address: u32) -> io::Result<u32> {
    let value = u32::from(bus.read_u16(address & !1)?);
    Ok(value.rotate_right((address & 1) * 8))
}

/// LDRSH: an odd address behaves like LDRSB of that byte.
fn load_signed_halfword(bus: &MemoryBus, address: u32) -> io::Result<u32> {
    if address & 1 != 0 {
        Ok(bus.read_u8(address)? as i8 as i32 as u32)
    } else {
        Ok(bus.read_u16(address)? as i16 as i32 as u32)
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

/// The data-bus accesses of one instruction. Most instructions have none or
/// one, so this is an enum rather than a buffer: timing never allocates or
/// zero-fills anything.
enum DataAccesses {
    None,
    One(u32, u8),
    Two((u32, u8), (u32, u8)),
    /// `count` consecutive word accesses (a block transfer), capped at 16.
    Words {
        start: u32,
        count: u32,
    },
}

impl DataAccesses {
    const fn none() -> Self {
        Self::None
    }

    fn one(address: u32, width: u8) -> Self {
        Self::One(address, width)
    }

    fn two(first: (u32, u8), second: (u32, u8)) -> Self {
        Self::Two(first, second)
    }

    fn words(start: u32, count: u32) -> Self {
        Self::Words {
            start,
            count: count.min(16),
        }
    }
}

fn add_data_wait_cycles(
    wait_cycles: u32,
    bus: &MemoryBus,
    accesses: &DataAccesses,
) -> io::Result<u32> {
    // Wait states are the cost beyond the single cycle the base count includes.
    let extra = |address: u32, width: u8, sequential: bool| -> u32 {
        bus.access_cycles(address, width, sequential)
            .saturating_sub(1)
    };
    let total = match *accesses {
        DataAccesses::None => 0,
        DataAccesses::One(address, width) => extra(address, width, false),
        DataAccesses::Two(first, second) => {
            extra(first.0, first.1, false) + extra(second.0, second.1, true)
        }
        DataAccesses::Words { start, count } => {
            let mut total = 0;
            for index in 0..count {
                total += extra(start.wrapping_add(index * 4), 4, index != 0);
            }
            total
        }
    };
    // At most 16 words of a few cycles each.
    Ok(wait_cycles.saturating_add(total))
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
