use super::*;

#[derive(Debug, PartialEq, Eq)]
pub enum ArmOp {
    DataProcessingImmediate {
        condition: u8,
        opcode: u8,
        set_flags: bool,
        rn: u8,
        rd: u8,
        immediate: u8,
        rotate: u8,
    },
    DataProcessingRegister {
        condition: u8,
        opcode: u8,
        set_flags: bool,
        rn: u8,
        rd: u8,
        rm: u8,
        shift_type: u8,
        shift_amount: u8,
        shift_by_register: bool,
        rs: u8,
    },
    SingleDataTransfer {
        condition: u8,
        pre_index: bool,
        add_offset: bool,
        byte: bool,
        writeback: bool,
        load: bool,
        rn: u8,
        rd: u8,
        offset_immediate: bool,
        offset: u16,
        rm: u8,
        shift_type: u8,
        shift_amount: u8,
    },
    HalfwordTransfer {
        condition: u8,
        pre_index: bool,
        add_offset: bool,
        writeback: bool,
        load: bool,
        transfer_type: u8,
        rn: u8,
        rd: u8,
        immediate_offset: bool,
        offset: u16,
        rm: u8,
    },
    BlockTransfer {
        condition: u8,
        pre_index: bool,
        add_offset: bool,
        user_registers: bool,
        writeback: bool,
        load: bool,
        rn: u8,
        register_list: u16,
    },
    Multiply {
        condition: u8,
        accumulate: bool,
        set_flags: bool,
        rd: u8,
        rn: u8,
        rs: u8,
        rm: u8,
    },
    MultiplyLong {
        condition: u8,
        signed: bool,
        accumulate: bool,
        set_flags: bool,
        rd_hi: u8,
        rd_lo: u8,
        rs: u8,
        rm: u8,
    },
    ReadStatus {
        condition: u8,
        saved: bool,
        rd: u8,
    },
    WriteStatusRegister {
        condition: u8,
        saved: bool,
        field_mask: u8,
        rm: u8,
    },
    WriteStatusImmediate {
        condition: u8,
        saved: bool,
        field_mask: u8,
        immediate: u8,
        rotate: u8,
    },
    Swap {
        condition: u8,
        byte: bool,
        rn: u8,
        rd: u8,
        rm: u8,
    },
    BranchExchange {
        condition: u8,
        rm: u8,
    },
    SoftwareInterrupt {
        condition: u8,
        comment: u32,
    },
    Branch {
        condition: u8,
        link: bool,
        offset: i32,
    },
    Unknown(u32),
}

pub fn decode(instruction: u32) -> ArmOp {
    let condition = (instruction >> 28) as u8;
    if condition == 0xF {
        return ArmOp::Unknown(instruction);
    }

    if instruction & 0x0FFF_FFF0 == 0x012F_FF10 {
        return ArmOp::BranchExchange {
            condition,
            rm: (instruction & 0xF) as u8,
        };
    }
    if instruction & 0x0F00_0000 == 0x0F00_0000 {
        return ArmOp::SoftwareInterrupt {
            condition,
            comment: instruction & 0x00FF_FFFF,
        };
    }
    if instruction & 0x0FBF_0FFF == 0x010F_0000 {
        return ArmOp::ReadStatus {
            condition,
            saved: instruction & (1 << 22) != 0,
            rd: ((instruction >> 12) & 0xF) as u8,
        };
    }
    if instruction & 0x0FB0_FFF0 == 0x0120_F000 {
        return ArmOp::WriteStatusRegister {
            condition,
            saved: instruction & (1 << 22) != 0,
            field_mask: ((instruction >> 16) & 0xF) as u8,
            rm: (instruction & 0xF) as u8,
        };
    }
    if instruction & 0x0FB0_F000 == 0x0320_F000 {
        return ArmOp::WriteStatusImmediate {
            condition,
            saved: instruction & (1 << 22) != 0,
            field_mask: ((instruction >> 16) & 0xF) as u8,
            immediate: (instruction & 0xFF) as u8,
            rotate: ((instruction >> 8) & 0xF) as u8,
        };
    }
    if instruction & 0x0FB0_0FF0 == 0x0100_0090 {
        return ArmOp::Swap {
            condition,
            byte: instruction & (1 << 22) != 0,
            rn: ((instruction >> 16) & 0xF) as u8,
            rd: ((instruction >> 12) & 0xF) as u8,
            rm: (instruction & 0xF) as u8,
        };
    }
    if instruction & 0x0F80_00F0 == 0x0080_0090 {
        return ArmOp::MultiplyLong {
            condition,
            signed: instruction & (1 << 22) != 0,
            accumulate: instruction & (1 << 21) != 0,
            set_flags: instruction & (1 << 20) != 0,
            rd_hi: ((instruction >> 16) & 0xF) as u8,
            rd_lo: ((instruction >> 12) & 0xF) as u8,
            rs: ((instruction >> 8) & 0xF) as u8,
            rm: (instruction & 0xF) as u8,
        };
    }
    if instruction & 0x0FC0_00F0 == 0x0000_0090 {
        return ArmOp::Multiply {
            condition,
            accumulate: instruction & (1 << 21) != 0,
            set_flags: instruction & (1 << 20) != 0,
            rd: ((instruction >> 16) & 0xF) as u8,
            rn: ((instruction >> 12) & 0xF) as u8,
            rs: ((instruction >> 8) & 0xF) as u8,
            rm: (instruction & 0xF) as u8,
        };
    }

    match (instruction >> 25) & 0b111 {
        0b001 => ArmOp::DataProcessingImmediate {
            condition,
            opcode: ((instruction >> 21) & 0xF) as u8,
            set_flags: instruction & (1 << 20) != 0,
            rn: ((instruction >> 16) & 0xF) as u8,
            rd: ((instruction >> 12) & 0xF) as u8,
            immediate: (instruction & 0xFF) as u8,
            rotate: ((instruction >> 8) & 0xF) as u8,
        },
        0b101 => {
            let raw_offset = instruction & 0x00FF_FFFF;
            let signed_offset = ((raw_offset << 8) as i32) >> 6;
            ArmOp::Branch {
                condition,
                link: instruction & (1 << 24) != 0,
                offset: signed_offset,
            }
        }
        0b100 => ArmOp::BlockTransfer {
            condition,
            pre_index: instruction & (1 << 24) != 0,
            add_offset: instruction & (1 << 23) != 0,
            user_registers: instruction & (1 << 22) != 0,
            writeback: instruction & (1 << 21) != 0,
            load: instruction & (1 << 20) != 0,
            rn: ((instruction >> 16) & 0xF) as u8,
            register_list: instruction as u16,
        },
        0b000
            if instruction & 0x90 == 0x90
                && instruction & (1 << 25) == 0
                && instruction & 0x60 != 0 =>
        {
            ArmOp::HalfwordTransfer {
                condition,
                pre_index: instruction & (1 << 24) != 0,
                add_offset: instruction & (1 << 23) != 0,
                writeback: instruction & (1 << 21) != 0,
                load: instruction & (1 << 20) != 0,
                transfer_type: ((instruction >> 5) & 0x3) as u8,
                rn: ((instruction >> 16) & 0xF) as u8,
                rd: ((instruction >> 12) & 0xF) as u8,
                immediate_offset: instruction & (1 << 22) != 0,
                offset: (((instruction >> 4) & 0xF0) | (instruction & 0xF)) as u16,
                rm: (instruction & 0xF) as u8,
            }
        }
        0b010 | 0b011 => ArmOp::SingleDataTransfer {
            condition,
            pre_index: instruction & (1 << 24) != 0,
            add_offset: instruction & (1 << 23) != 0,
            byte: instruction & (1 << 22) != 0,
            writeback: instruction & (1 << 21) != 0,
            load: instruction & (1 << 20) != 0,
            rn: ((instruction >> 16) & 0xF) as u8,
            rd: ((instruction >> 12) & 0xF) as u8,
            offset_immediate: instruction & (1 << 25) == 0,
            offset: (instruction & 0xFFF) as u16,
            rm: (instruction & 0xF) as u8,
            shift_type: ((instruction >> 5) & 0x3) as u8,
            shift_amount: ((instruction >> 7) & 0x1F) as u8,
        },
        0b000 if instruction & (1 << 25) == 0 && instruction & (1 << 4) == 0 => {
            ArmOp::DataProcessingRegister {
                condition,
                opcode: ((instruction >> 21) & 0xF) as u8,
                set_flags: instruction & (1 << 20) != 0,
                rn: ((instruction >> 16) & 0xF) as u8,
                rd: ((instruction >> 12) & 0xF) as u8,
                rm: (instruction & 0xF) as u8,
                shift_type: ((instruction >> 5) & 0x3) as u8,
                shift_amount: ((instruction >> 7) & 0x1F) as u8,
                shift_by_register: false,
                rs: 0,
            }
        }
        0b000
            if instruction & (1 << 25) == 0
                && instruction & (1 << 4) != 0
                && instruction & (1 << 7) == 0 =>
        {
            ArmOp::DataProcessingRegister {
                condition,
                opcode: ((instruction >> 21) & 0xF) as u8,
                set_flags: instruction & (1 << 20) != 0,
                rn: ((instruction >> 16) & 0xF) as u8,
                rd: ((instruction >> 12) & 0xF) as u8,
                rm: (instruction & 0xF) as u8,
                shift_type: ((instruction >> 5) & 0x3) as u8,
                shift_amount: 0,
                shift_by_register: true,
                rs: ((instruction >> 8) & 0xF) as u8,
            }
        }
        _ => ArmOp::Unknown(instruction),
    }
}

impl Cpu {
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

        let instruction = match bus.fetch_u32(pc) {
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
        bus.set_open_bus(instruction);
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
            self.arm_base_cycles(&op)
        };
        let wait_cycles = if condition_passed {
            match arm_wait_cycles(
                &op,
                bus,
                pc,
                fetch_wait_cycles,
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

    /// Internal + sequential/non-sequential cycles with zero-wait memory
    /// (ARM7TDMI datasheet "Instruction Cycle Times").
    fn arm_base_cycles(&self, op: &ArmOp) -> u32 {
        match op {
            ArmOp::Branch { .. }
            | ArmOp::BranchExchange { .. }
            | ArmOp::SoftwareInterrupt { .. } => 3,
            // 1S, +1I for a register-specified shift, +1S+1N when Rd is PC.
            ArmOp::DataProcessingImmediate { rd, .. } => 1 + 2 * u32::from(*rd == 15),
            ArmOp::DataProcessingRegister {
                rd,
                shift_by_register,
                ..
            } => 1 + u32::from(*shift_by_register) + 2 * u32::from(*rd == 15),
            // LDR: 1S+1N+1I, plus 1S+1N when the destination is PC.
            ArmOp::SingleDataTransfer { load: true, rd, .. }
            | ArmOp::HalfwordTransfer { load: true, rd, .. } => 3 + 2 * u32::from(*rd == 15),
            ArmOp::SingleDataTransfer { .. } | ArmOp::HalfwordTransfer { .. } => 2,
            ArmOp::BlockTransfer {
                load,
                register_list,
                ..
            } => {
                let loads_pc = *register_list == 0 || register_list & (1 << 15) != 0;
                let count = register_list.count_ones().max(1);
                if *load {
                    count + 2 + 2 * u32::from(loads_pc)
                } else {
                    count + 1
                }
            }
            ArmOp::Multiply { accumulate, rs, .. } => {
                1 + multiply_internal_cycles(self.registers[*rs as usize], true)
                    + u32::from(*accumulate)
            }
            ArmOp::MultiplyLong {
                signed,
                accumulate,
                rs,
                ..
            } => {
                2 + multiply_internal_cycles(self.registers[*rs as usize], *signed)
                    + u32::from(*accumulate)
            }
            // SWP: 1S+2N+1I.
            ArmOp::Swap { .. } => 4,
            _ => 1,
        }
    }
}

struct SingleTransfer {
    pre_index: bool,
    add_offset: bool,
    byte: bool,
    writeback: bool,
    load: bool,
    rn: u8,
    rd: u8,
    offset_immediate: bool,
    offset: u16,
    rm: u8,
    shift_type: u8,
    shift_amount: u8,
}

struct HalfwordTransfer {
    pre_index: bool,
    add_offset: bool,
    writeback: bool,
    load: bool,
    transfer_type: u8,
    rn: u8,
    rd: u8,
    immediate_offset: bool,
    offset: u16,
    rm: u8,
}

struct BlockTransfer {
    pre_index: bool,
    add_offset: bool,
    user_registers: bool,
    writeback: bool,
    load: bool,
    rn: u8,
    register_list: u16,
}

impl Cpu {
    fn execute_arm(
        &mut self,
        op: ArmOp,
        instruction_pc: u32,
        bus: &mut MemoryBus,
    ) -> io::Result<()> {
        match op {
            ArmOp::Branch { link, offset, .. } => {
                if link {
                    self.registers[14] = instruction_pc.wrapping_add(4);
                }
                self.registers[15] = instruction_pc.wrapping_add(8).wrapping_add(offset as u32);
            }
            ArmOp::BranchExchange { rm, .. } => {
                let target = self.registers[rm as usize];
                if target & 1 != 0 {
                    self.cpsr |= T_FLAG;
                    self.registers[15] = target & !1;
                } else {
                    self.cpsr &= !T_FLAG;
                    self.registers[15] = target & !3;
                }
            }
            ArmOp::SoftwareInterrupt { .. } => {
                self.enter_exception(MODE_SUPERVISOR, VECTOR_SWI, instruction_pc.wrapping_add(4))?;
            }
            ArmOp::ReadStatus { saved, rd, .. } => {
                self.execute_arm_read_status(saved, rd, instruction_pc)?;
            }
            ArmOp::WriteStatusRegister {
                saved,
                field_mask,
                rm,
                ..
            } => {
                self.execute_arm_write_status_register(saved, field_mask, rm, instruction_pc)?;
            }
            ArmOp::WriteStatusImmediate {
                saved,
                field_mask,
                immediate,
                rotate,
                ..
            } => {
                let value = u32::from(immediate).rotate_right(u32::from(rotate) * 2);
                self.write_status_register(saved, field_mask, value)?;
                self.registers[15] = instruction_pc.wrapping_add(4);
            }
            ArmOp::Swap {
                byte, rn, rd, rm, ..
            } => self.execute_arm_swap(byte, rn, rd, rm, instruction_pc, bus)?,
            ArmOp::Multiply {
                accumulate,
                set_flags,
                rd,
                rn,
                rs,
                rm,
                ..
            } => {
                self.execute_arm_multiply(accumulate, set_flags, rd, rn, rs, rm, instruction_pc)?
            }
            ArmOp::MultiplyLong {
                signed,
                accumulate,
                set_flags,
                rd_hi,
                rd_lo,
                rs,
                rm,
                ..
            } => self.execute_arm_multiply_long(
                signed,
                accumulate,
                set_flags,
                rd_hi,
                rd_lo,
                rs,
                rm,
                instruction_pc,
            )?,
            ArmOp::DataProcessingImmediate {
                opcode,
                set_flags,
                rn,
                rd,
                immediate,
                rotate,
                ..
            } => {
                let rotate_amount = u32::from(rotate) * 2;
                let operand2 = u32::from(immediate).rotate_right(rotate_amount);
                let shifter_carry = if rotate_amount == 0 {
                    self.cpsr & C_FLAG != 0
                } else {
                    operand2 & 0x8000_0000 != 0
                };
                self.execute_arm_data_processing(
                    opcode,
                    set_flags,
                    rn,
                    rd,
                    operand2,
                    shifter_carry,
                    instruction_pc,
                )?;
            }
            ArmOp::DataProcessingRegister {
                opcode,
                set_flags,
                rn,
                rd,
                rm,
                shift_type,
                shift_amount,
                shift_by_register,
                rs,
                ..
            } => {
                let value = if rm == 15 {
                    instruction_pc.wrapping_add(if shift_by_register { 12 } else { 8 })
                } else {
                    self.registers[rm as usize]
                };
                let amount = if shift_by_register {
                    self.registers[rs as usize] & 0xFF
                } else {
                    u32::from(shift_amount)
                };
                let (operand2, shifter_carry) = shift_operand(
                    value,
                    shift_type,
                    amount,
                    shift_by_register,
                    self.cpsr & C_FLAG != 0,
                );
                self.execute_arm_data_processing(
                    opcode,
                    set_flags,
                    rn,
                    rd,
                    operand2,
                    shifter_carry,
                    instruction_pc,
                )?;
            }
            ArmOp::SingleDataTransfer {
                pre_index,
                add_offset,
                byte,
                writeback,
                load,
                rn,
                rd,
                offset_immediate,
                offset,
                rm,
                shift_type,
                shift_amount,
                ..
            } => self.execute_arm_single_data_transfer(
                SingleTransfer {
                    pre_index,
                    add_offset,
                    byte,
                    writeback,
                    load,
                    rn,
                    rd,
                    offset_immediate,
                    offset,
                    rm,
                    shift_type,
                    shift_amount,
                },
                instruction_pc,
                bus,
            )?,
            ArmOp::HalfwordTransfer {
                pre_index,
                add_offset,
                writeback,
                load,
                transfer_type,
                rn,
                rd,
                immediate_offset,
                offset,
                rm,
                ..
            } => self.execute_arm_halfword_transfer(
                HalfwordTransfer {
                    pre_index,
                    add_offset,
                    writeback,
                    load,
                    transfer_type,
                    rn,
                    rd,
                    immediate_offset,
                    offset,
                    rm,
                },
                instruction_pc,
                bus,
            )?,
            ArmOp::BlockTransfer {
                pre_index,
                add_offset,
                user_registers,
                writeback,
                load,
                rn,
                register_list,
                ..
            } => self.execute_arm_block_transfer(
                BlockTransfer {
                    pre_index,
                    add_offset,
                    user_registers,
                    writeback,
                    load,
                    rn,
                    register_list,
                },
                instruction_pc,
                bus,
            )?,
            ArmOp::Unknown(instruction) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported ARM instruction: {instruction:#010X}"),
                ));
            }
        }
        Ok(())
    }

    fn execute_arm_read_status(
        &mut self,
        saved: bool,
        rd: u8,
        instruction_pc: u32,
    ) -> io::Result<()> {
        if rd == 15 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PC is not permitted as the destination of MRS",
            ));
        }
        self.registers[rd as usize] = if saved {
            self.saved_status()?
        } else {
            self.cpsr
        };
        self.registers[15] = instruction_pc.wrapping_add(4);
        Ok(())
    }

    fn execute_arm_write_status_register(
        &mut self,
        saved: bool,
        field_mask: u8,
        rm: u8,
        instruction_pc: u32,
    ) -> io::Result<()> {
        if rm == 15 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PC is not permitted as the source of MSR",
            ));
        }
        self.write_status_register(saved, field_mask, self.registers[rm as usize])?;
        self.registers[15] = instruction_pc.wrapping_add(4);
        Ok(())
    }

    fn execute_arm_swap(
        &mut self,
        byte: bool,
        rn: u8,
        rd: u8,
        rm: u8,
        instruction_pc: u32,
        bus: &mut MemoryBus,
    ) -> io::Result<()> {
        if rn == 15 || rd == 15 || rm == 15 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PC is not permitted in an ARM swap encoding",
            ));
        }
        let address = self.registers[rn as usize];
        let source = self.registers[rm as usize];
        let previous = if byte {
            let previous = bus.read_u8(address)?;
            bus.write_u8(address, source as u8)?;
            u32::from(previous)
        } else {
            if address & 3 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unaligned ARM word swap address",
                ));
            }
            let previous = bus.read_u32(address)?;
            bus.write_u32(address, source)?;
            previous
        };
        self.registers[rd as usize] = previous;
        self.registers[15] = instruction_pc.wrapping_add(4);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_arm_multiply(
        &mut self,
        accumulate: bool,
        set_flags: bool,
        rd: u8,
        rn: u8,
        rs: u8,
        rm: u8,
        instruction_pc: u32,
    ) -> io::Result<()> {
        if rd == 15 || rn == 15 || rs == 15 || rm == 15 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PC is not permitted in this ARM multiply encoding",
            ));
        }
        let mut result = self.registers[rm as usize].wrapping_mul(self.registers[rs as usize]);
        if accumulate {
            result = result.wrapping_add(self.registers[rn as usize]);
        }
        self.registers[rd as usize] = result;
        self.registers[15] = instruction_pc.wrapping_add(4);
        if set_flags {
            self.set_flag(N_FLAG, result & 0x8000_0000 != 0);
            self.set_flag(Z_FLAG, result == 0);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_arm_multiply_long(
        &mut self,
        signed: bool,
        accumulate: bool,
        set_flags: bool,
        rd_hi: u8,
        rd_lo: u8,
        rs: u8,
        rm: u8,
        instruction_pc: u32,
    ) -> io::Result<()> {
        if [rd_hi, rd_lo, rs, rm].contains(&15) || rd_hi == rd_lo {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid ARM long-multiply register encoding",
            ));
        }
        let lhs = self.registers[rm as usize];
        let rhs = self.registers[rs as usize];
        let product = if signed {
            (lhs as i32 as i64).wrapping_mul(rhs as i32 as i64) as u64
        } else {
            u64::from(lhs) * u64::from(rhs)
        };
        let initial = (u64::from(self.registers[rd_hi as usize]) << 32)
            | u64::from(self.registers[rd_lo as usize]);
        let result = if accumulate {
            product.wrapping_add(initial)
        } else {
            product
        };
        self.registers[rd_lo as usize] = result as u32;
        self.registers[rd_hi as usize] = (result >> 32) as u32;
        self.registers[15] = instruction_pc.wrapping_add(4);
        if set_flags {
            self.set_flag(N_FLAG, result & (1 << 63) != 0);
            self.set_flag(Z_FLAG, result == 0);
        }
        Ok(())
    }

    fn execute_arm_single_data_transfer(
        &mut self,
        transfer: SingleTransfer,
        instruction_pc: u32,
        bus: &mut MemoryBus,
    ) -> io::Result<()> {
        if !transfer.offset_immediate && transfer.offset & 0x0010 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "register-controlled shifts are invalid in an ARM transfer offset",
            ));
        }
        let base = if transfer.rn == 15 {
            instruction_pc.wrapping_add(8)
        } else {
            self.registers[transfer.rn as usize]
        };
        let displacement = if transfer.offset_immediate {
            u32::from(transfer.offset)
        } else {
            let register_value = if transfer.rm == 15 {
                instruction_pc.wrapping_add(12)
            } else {
                self.registers[transfer.rm as usize]
            };
            shift_operand(
                register_value,
                transfer.shift_type,
                u32::from(transfer.shift_amount),
                false,
                self.cpsr & C_FLAG != 0,
            )
            .0
        };
        let adjusted = if transfer.add_offset {
            base.wrapping_add(displacement)
        } else {
            base.wrapping_sub(displacement)
        };
        let address = if transfer.pre_index { adjusted } else { base };
        let should_writeback = !transfer.pre_index || transfer.writeback;
        if transfer.load {
            let value = if transfer.byte {
                u32::from(bus.read_u8(address)?)
            } else {
                bus.read_u32(address & !3)?.rotate_right((address & 3) * 8)
            };
            if transfer.rd == 15 {
                self.registers[15] = value & !3;
            } else {
                self.registers[transfer.rd as usize] = value;
                self.registers[15] = instruction_pc.wrapping_add(4);
            }
        } else {
            let value = if transfer.rd == 15 {
                instruction_pc.wrapping_add(12)
            } else {
                self.registers[transfer.rd as usize]
            };
            if transfer.byte {
                bus.write_u8(address, value as u8)?;
            } else {
                bus.write_u32(address & !3, value)?;
            }
            self.registers[15] = instruction_pc.wrapping_add(4);
        }
        if should_writeback && !(transfer.load && transfer.rn == transfer.rd) {
            if transfer.rn == 15 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ARM writeback with PC base is unpredictable",
                ));
            }
            self.registers[transfer.rn as usize] = adjusted;
        }
        Ok(())
    }

    fn execute_arm_halfword_transfer(
        &mut self,
        transfer: HalfwordTransfer,
        instruction_pc: u32,
        bus: &mut MemoryBus,
    ) -> io::Result<()> {
        let base = if transfer.rn == 15 {
            instruction_pc.wrapping_add(8)
        } else {
            self.registers[transfer.rn as usize]
        };
        let displacement = if transfer.immediate_offset {
            u32::from(transfer.offset)
        } else if transfer.rm == 15 {
            instruction_pc.wrapping_add(12)
        } else {
            self.registers[transfer.rm as usize]
        };
        let adjusted = if transfer.add_offset {
            base.wrapping_add(displacement)
        } else {
            base.wrapping_sub(displacement)
        };
        let address = if transfer.pre_index { adjusted } else { base };
        let should_writeback = !transfer.pre_index || transfer.writeback;
        if transfer.load {
            let value = match transfer.transfer_type {
                0b01 => load_halfword(bus, address)?,
                0b10 => (bus.read_u8(address)? as i8 as i32) as u32,
                0b11 => load_signed_halfword(bus, address)?,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unsupported ARM halfword transfer type",
                    ));
                }
            };
            if transfer.rd == 15 {
                self.registers[15] = value & !3;
            } else {
                self.registers[transfer.rd as usize] = value;
                self.registers[15] = instruction_pc.wrapping_add(4);
            }
        } else {
            if transfer.transfer_type != 0b01 || transfer.rd == 15 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported ARM halfword store encoding",
                ));
            }
            bus.write_u16(address & !1, self.registers[transfer.rd as usize] as u16)?;
            self.registers[15] = instruction_pc.wrapping_add(4);
        }
        if should_writeback && !(transfer.load && transfer.rn == transfer.rd) {
            if transfer.rn == 15 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ARM halfword transfer writeback with PC base is unpredictable",
                ));
            }
            self.registers[transfer.rn as usize] = adjusted;
        }
        Ok(())
    }

    fn execute_arm_block_transfer(
        &mut self,
        transfer: BlockTransfer,
        instruction_pc: u32,
        bus: &mut MemoryBus,
    ) -> io::Result<()> {
        if transfer.rn == 15 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ARM block transfer with PC base is unpredictable",
            ));
        }
        let empty_list = transfer.register_list == 0;
        let transfer_list = if empty_list {
            1 << 15
        } else {
            transfer.register_list
        };
        let count = if empty_list {
            16
        } else {
            transfer_list.count_ones()
        };
        let base = self.registers[transfer.rn as usize];
        let span = count * 4;
        let mut address = match (transfer.add_offset, transfer.pre_index) {
            (true, true) => base.wrapping_add(4),
            (true, false) => base,
            (false, true) => base.wrapping_sub(span),
            (false, false) => base.wrapping_sub(span).wrapping_add(4),
        };
        let final_base = if transfer.add_offset {
            base.wrapping_add(span)
        } else {
            base.wrapping_sub(span)
        };
        let exception_return =
            transfer.load && transfer.user_registers && transfer_list & (1 << 15) != 0;
        let user_bank_transfer = transfer.user_registers && !exception_return;
        let first_register = transfer_list.trailing_zeros() as usize;
        let mut loaded_pc = None;
        for register in 0..16 {
            if transfer_list & (1 << register) == 0 {
                continue;
            }
            if transfer.load {
                let value = bus.read_u32(address)?;
                if register == 15 {
                    loaded_pc = Some(value);
                } else if user_bank_transfer {
                    self.write_user_register(register as u8, value);
                } else {
                    self.registers[register] = value;
                }
            } else {
                let mut value = if register == 15 {
                    instruction_pc.wrapping_add(12)
                } else if user_bank_transfer {
                    self.read_user_register(register as u8)
                } else {
                    self.registers[register]
                };
                if register == transfer.rn as usize
                    && transfer.writeback
                    && register != first_register
                {
                    value = final_base;
                }
                bus.write_u32(address, value)?;
            }
            address = address.wrapping_add(4);
        }
        if transfer.writeback && !(transfer.load && transfer_list & (1 << transfer.rn) != 0) {
            self.registers[transfer.rn as usize] = final_base;
        }
        if exception_return {
            let restored_status = self.saved_status()?;
            self.set_cpsr(restored_status)?;
            self.registers[15] = loaded_pc.expect("exception return transfer includes PC")
                & if self.cpsr & T_FLAG != 0 { !1 } else { !3 };
        } else {
            self.registers[15] = loaded_pc
                .map(|value| value & !3)
                .unwrap_or_else(|| instruction_pc.wrapping_add(4));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_arm_data_processing(
        &mut self,
        opcode: u8,
        set_flags: bool,
        rn: u8,
        rd: u8,
        operand2: u32,
        shifter_carry: bool,
        instruction_pc: u32,
    ) -> io::Result<()> {
        let lhs = if rn == 15 {
            instruction_pc.wrapping_add(8)
        } else {
            self.registers[rn as usize]
        };
        let (result, writes_result, arithmetic_flags) = match opcode {
            0x0 => (lhs & operand2, true, None),
            0x1 => (lhs ^ operand2, true, None),
            0x2 => {
                let (result, carry, overflow) = subtract_with_borrow(lhs, operand2, false);
                (result, true, Some((carry, overflow)))
            }
            0x3 => {
                let (result, carry, overflow) = subtract_with_borrow(operand2, lhs, false);
                (result, true, Some((carry, overflow)))
            }
            0x4 => {
                let (result, carry, overflow) = add_with_carry(lhs, operand2, false);
                (result, true, Some((carry, overflow)))
            }
            0x5 => {
                let (result, carry, overflow) =
                    add_with_carry(lhs, operand2, self.cpsr & C_FLAG != 0);
                (result, true, Some((carry, overflow)))
            }
            0x6 => {
                let (result, carry, overflow) =
                    subtract_with_borrow(lhs, operand2, self.cpsr & C_FLAG == 0);
                (result, true, Some((carry, overflow)))
            }
            0x7 => {
                let (result, carry, overflow) =
                    subtract_with_borrow(operand2, lhs, self.cpsr & C_FLAG == 0);
                (result, true, Some((carry, overflow)))
            }
            0x8 => (lhs & operand2, false, None),
            0x9 => (lhs ^ operand2, false, None),
            0xA => {
                let (result, carry, overflow) = subtract_with_borrow(lhs, operand2, false);
                (result, false, Some((carry, overflow)))
            }
            0xB => {
                let (result, carry, overflow) = add_with_carry(lhs, operand2, false);
                (result, false, Some((carry, overflow)))
            }
            0xC => (lhs | operand2, true, None),
            0xD => (operand2, true, None),
            0xE => (lhs & !operand2, true, None),
            0xF => (!operand2, true, None),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported ARM data-processing opcode: {opcode:#X}"),
                ));
            }
        };
        let writes_flags = set_flags || matches!(opcode, 0x8..=0xB);
        let restored_cpsr = if rd == 15 && set_flags {
            Some(self.saved_status()?)
        } else {
            None
        };
        if writes_result {
            if rd == 15 {
                if let Some(restored) = restored_cpsr {
                    self.set_cpsr(restored)?;
                    self.registers[15] = result & if self.cpsr & T_FLAG != 0 { !1 } else { !3 };
                } else {
                    self.registers[15] = result & !3;
                }
            } else {
                self.registers[rd as usize] = result;
                self.registers[15] = instruction_pc.wrapping_add(4);
            }
        } else {
            self.registers[15] = instruction_pc.wrapping_add(4);
        }
        if writes_flags && restored_cpsr.is_none() {
            self.set_flag(N_FLAG, result & 0x8000_0000 != 0);
            self.set_flag(Z_FLAG, result == 0);
            if let Some((carry, overflow)) = arithmetic_flags {
                self.set_flag(C_FLAG, carry);
                self.set_flag(V_FLAG, overflow);
            } else {
                self.set_flag(C_FLAG, shifter_carry);
            }
        }
        Ok(())
    }
}

fn arm_wait_cycles(
    op: &ArmOp,
    bus: &MemoryBus,
    pc: u32,
    fetch_wait_cycles: u32,
    registers: &[u32; 16],
    carry: bool,
) -> io::Result<u32> {
    let wait_cycles = fetch_wait_cycles;
    let data_accesses = match op {
        ArmOp::Swap { byte, rn, .. } => {
            let address = registers[*rn as usize];
            let width = if *byte { 1 } else { 4 };
            DataAccesses::two((address, width), (address, width))
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
            DataAccesses::one(address, if *byte { 1 } else { 4 })
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
            DataAccesses::one(if *pre_index { adjusted } else { base }, 2)
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
            DataAccesses::words(start, transfer_list.count_ones())
        }
        _ => DataAccesses::none(),
    };
    add_data_wait_cycles(wait_cycles, bus, &data_accesses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_immediate_data_processing_and_rotation() {
        assert_eq!(
            decode(0xE3A0_0101),
            ArmOp::DataProcessingImmediate {
                condition: 0xE,
                opcode: 0xD,
                set_flags: false,
                rn: 0,
                rd: 0,
                immediate: 1,
                rotate: 1,
            }
        );
    }

    #[test]
    fn decodes_positive_and_negative_branch_offsets() {
        assert_eq!(
            decode(0xEA00_0001),
            ArmOp::Branch {
                condition: 0xE,
                link: false,
                offset: 4,
            }
        );
        assert_eq!(
            decode(0xEBFF_FFFF),
            ArmOp::Branch {
                condition: 0xE,
                link: true,
                offset: -4,
            }
        );
    }

    #[test]
    fn marks_unsupported_instruction_classes() {
        assert_eq!(decode(0xEE00_0010), ArmOp::Unknown(0xEE00_0010));
        assert_eq!(decode(0xF200_0000), ArmOp::Unknown(0xF200_0000));
    }

    #[test]
    fn decodes_software_interrupt_condition() {
        assert_eq!(
            decode(0x0F00_0001),
            ArmOp::SoftwareInterrupt {
                condition: 0,
                comment: 1,
            }
        );
    }

    #[test]
    fn decodes_register_data_processing_and_single_transfers() {
        assert_eq!(
            decode(0xE081_0302),
            ArmOp::DataProcessingRegister {
                condition: 0xE,
                opcode: 0x4,
                set_flags: false,
                rn: 1,
                rd: 0,
                rm: 2,
                shift_type: 0,
                shift_amount: 6,
                shift_by_register: false,
                rs: 0,
            }
        );
        assert!(matches!(
            decode(0xE591_0004),
            ArmOp::SingleDataTransfer {
                load: true,
                rn: 1,
                rd: 0,
                offset_immediate: true,
                offset: 4,
                ..
            }
        ));
        assert_eq!(
            decode(0xE12F_FF10),
            ArmOp::BranchExchange {
                condition: 0xE,
                rm: 0,
            }
        );
    }

    #[test]
    fn decodes_register_controlled_shifts_and_rejects_reserved_halfword_type() {
        assert_eq!(
            decode(0xE081_0312),
            ArmOp::DataProcessingRegister {
                condition: 0xE,
                opcode: 0x4,
                set_flags: false,
                rn: 1,
                rd: 0,
                rm: 2,
                shift_type: 0,
                shift_amount: 0,
                shift_by_register: true,
                rs: 3,
            }
        );
        assert!(matches!(
            decode(0xE011_00B2),
            ArmOp::HalfwordTransfer {
                pre_index: false,
                add_offset: false,
                immediate_offset: false,
                load: true,
                transfer_type: 1,
                ..
            }
        ));
        assert_eq!(decode(0xE111_0092), ArmOp::Unknown(0xE111_0092));
    }

    #[test]
    fn multiply_decode_precedes_overlapping_halfword_encodings() {
        for instruction in [0xE000_0291, 0xE030_0291] {
            assert!(matches!(decode(instruction), ArmOp::Multiply { .. }));
        }
    }

    #[test]
    fn decodes_multiply_halfword_and_block_transfer() {
        assert!(matches!(
            decode(0xE000_0291),
            ArmOp::Multiply {
                rd: 0,
                rs: 2,
                rm: 1,
                ..
            }
        ));
        assert!(matches!(
            decode(0xE1D1_00B2),
            ArmOp::HalfwordTransfer {
                load: true,
                transfer_type: 1,
                rn: 1,
                rd: 0,
                immediate_offset: true,
                offset: 2,
                ..
            }
        ));
        assert!(matches!(
            decode(0xE8B0_0006),
            ArmOp::BlockTransfer {
                load: true,
                writeback: true,
                rn: 0,
                register_list: 6,
                ..
            }
        ));
    }

    #[test]
    fn decodes_status_register_transfers() {
        assert_eq!(
            decode(0xE10F_0000),
            ArmOp::ReadStatus {
                condition: 0xE,
                saved: false,
                rd: 0,
            }
        );
        assert_eq!(
            decode(0xE14F_1000),
            ArmOp::ReadStatus {
                condition: 0xE,
                saved: true,
                rd: 1,
            }
        );
        assert_eq!(
            decode(0xE121_F000),
            ArmOp::WriteStatusRegister {
                condition: 0xE,
                saved: false,
                field_mask: 1,
                rm: 0,
            }
        );
        assert_eq!(
            decode(0xE328_F480),
            ArmOp::WriteStatusImmediate {
                condition: 0xE,
                saved: false,
                field_mask: 8,
                immediate: 0x80,
                rotate: 4,
            }
        );
    }

    #[test]
    fn decodes_word_and_byte_swaps() {
        assert_eq!(
            decode(0xE102_1093),
            ArmOp::Swap {
                condition: 0xE,
                byte: false,
                rn: 2,
                rd: 1,
                rm: 3,
            }
        );
        assert_eq!(
            decode(0xE142_1093),
            ArmOp::Swap {
                condition: 0xE,
                byte: true,
                rn: 2,
                rd: 1,
                rm: 3,
            }
        );
    }
}
