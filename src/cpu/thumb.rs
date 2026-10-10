use super::*;

#[derive(Debug, PartialEq, Eq)]
pub enum ThumbOp {
    ShiftImmediate {
        rd: u8,
        rm: u8,
        shift_type: u8,
        amount: u8,
    },
    AddSubtract {
        rd: u8,
        rn: u8,
        operand: u8,
        immediate: bool,
        subtract: bool,
    },
    Alu {
        opcode: u8,
        rd: u8,
        rs: u8,
    },
    HighRegister {
        operation: u8,
        rd: u8,
        rs: u8,
    },
    BranchExchange {
        rm: u8,
    },
    LoadStoreRegister {
        operation: u8,
        rd: u8,
        rb: u8,
        ro: u8,
    },
    LoadPcRelative {
        rd: u8,
        offset: u8,
    },
    MoveImmediate {
        rd: u8,
        imm: u8,
    },
    CompareImmediate {
        rn: u8,
        imm: u8,
    },
    AddImmediate {
        rd: u8,
        imm: u8,
    },
    SubtractImmediate {
        rd: u8,
        imm: u8,
    },
    LoadStoreImmediate {
        load: bool,
        byte: bool,
        rd: u8,
        rb: u8,
        offset: u8,
    },
    LoadStoreHalfwordImmediate {
        load: bool,
        rd: u8,
        rb: u8,
        offset: u8,
    },
    SpRelativeLoadStore {
        load: bool,
        rd: u8,
        offset: u8,
    },
    LoadAddress {
        use_sp: bool,
        rd: u8,
        offset: u8,
    },
    AdjustSp {
        subtract: bool,
        offset: u8,
    },
    PushPop {
        pop: bool,
        extra_register: bool,
        register_list: u8,
    },
    MultipleTransfer {
        load: bool,
        rb: u8,
        register_list: u8,
    },
    BranchConditional {
        condition: u8,
        offset: i8,
    },
    SoftwareInterrupt {
        comment: u8,
    },
    LongBranchPrefix {
        offset: i16,
    },
    LongBranchSuffix {
        offset: u16,
    },
    BranchUnconditional {
        offset: i16,
    },
    Unknown(u16),
}

/// Decodes a 16-bit Thumb instruction into an executable enum variant.
pub fn decode(opcode: u16) -> ThumbOp {
    // Extract the top 5 bits to identify the structural format category
    let format_bits = (opcode >> 11) & 0x1F;

    match format_bits {
        0x00..=0x02 => ThumbOp::ShiftImmediate {
            rd: (opcode & 0x07) as u8,
            rm: ((opcode >> 3) & 0x07) as u8,
            shift_type: ((opcode >> 11) & 0x03) as u8,
            amount: ((opcode >> 6) & 0x1F) as u8,
        },
        0x03 => ThumbOp::AddSubtract {
            rd: (opcode & 0x07) as u8,
            rn: ((opcode >> 3) & 0x07) as u8,
            operand: ((opcode >> 6) & 0x07) as u8,
            immediate: opcode & (1 << 10) != 0,
            subtract: opcode & (1 << 9) != 0,
        },
        0x08 if opcode & 0xFC00 == 0x4400 && (opcode >> 8) & 3 == 3 => {
            if opcode & 0x0080 != 0 {
                ThumbOp::Unknown(opcode)
            } else {
                ThumbOp::BranchExchange {
                    rm: ((opcode >> 3) & 0x0F) as u8,
                }
            }
        }
        0x08 if opcode & 0xFC00 == 0x4400 => ThumbOp::HighRegister {
            operation: ((opcode >> 8) & 0x03) as u8,
            rd: ((opcode & 0x07) | ((opcode >> 4) & 0x08)) as u8,
            rs: ((opcode >> 3) & 0x0F) as u8,
        },
        0x08 => ThumbOp::Alu {
            opcode: ((opcode >> 6) & 0xF) as u8,
            rd: (opcode & 0x07) as u8,
            rs: ((opcode >> 3) & 0x07) as u8,
        },
        0x09 => ThumbOp::LoadPcRelative {
            rd: ((opcode >> 8) & 0x07) as u8,
            offset: (opcode & 0xFF) as u8,
        },
        0x0A | 0x0B => ThumbOp::LoadStoreRegister {
            operation: ((opcode >> 9) & 0x07) as u8,
            rd: (opcode & 0x07) as u8,
            rb: ((opcode >> 3) & 0x07) as u8,
            ro: ((opcode >> 6) & 0x07) as u8,
        },
        // Format 3: Move/Compare/Add/Subtract Immediate (00100 to 00111)
        0x04 => ThumbOp::MoveImmediate {
            rd: ((opcode >> 8) & 0x07) as u8,
            imm: (opcode & 0xFF) as u8,
        },
        0x05 => ThumbOp::CompareImmediate {
            rn: ((opcode >> 8) & 0x07) as u8,
            imm: (opcode & 0xFF) as u8,
        },
        0x06 => ThumbOp::AddImmediate {
            rd: ((opcode >> 8) & 0x07) as u8,
            imm: (opcode & 0xFF) as u8,
        },
        0x07 => ThumbOp::SubtractImmediate {
            rd: ((opcode >> 8) & 0x07) as u8,
            imm: (opcode & 0xFF) as u8,
        },

        // Format 9: Load/store with immediate offset.
        0x0C..=0x0F => ThumbOp::LoadStoreImmediate {
            load: opcode & (1 << 11) != 0,
            byte: opcode & (1 << 12) != 0,
            rd: (opcode & 0x07) as u8,
            rb: ((opcode >> 3) & 0x07) as u8,
            offset: ((opcode >> 6) & 0x1F) as u8,
        },
        0x10 | 0x11 => ThumbOp::LoadStoreHalfwordImmediate {
            load: opcode & (1 << 11) != 0,
            rd: (opcode & 0x07) as u8,
            rb: ((opcode >> 3) & 0x07) as u8,
            offset: ((opcode >> 6) & 0x1F) as u8,
        },
        0x12 | 0x13 => ThumbOp::SpRelativeLoadStore {
            load: opcode & (1 << 11) != 0,
            rd: ((opcode >> 8) & 0x07) as u8,
            offset: (opcode & 0xFF) as u8,
        },
        0x14 | 0x15 => ThumbOp::LoadAddress {
            use_sp: opcode & (1 << 11) != 0,
            rd: ((opcode >> 8) & 0x07) as u8,
            offset: (opcode & 0xFF) as u8,
        },
        0x16 if opcode & 0xFF00 == 0xB000 => ThumbOp::AdjustSp {
            subtract: opcode & (1 << 7) != 0,
            offset: (opcode & 0x7F) as u8,
        },
        0x16 | 0x17 if opcode & 0xFE00 == 0xB400 || opcode & 0xFE00 == 0xBC00 => ThumbOp::PushPop {
            pop: opcode & (1 << 11) != 0,
            extra_register: opcode & (1 << 8) != 0,
            register_list: (opcode & 0xFF) as u8,
        },
        0x18 | 0x19 => ThumbOp::MultipleTransfer {
            load: opcode & (1 << 11) != 0,
            rb: ((opcode >> 8) & 0x07) as u8,
            register_list: (opcode & 0xFF) as u8,
        },
        0x1A | 0x1B if opcode & 0xF000 == 0xD000 => {
            let condition = ((opcode >> 8) & 0x0F) as u8;
            if condition == 0xF {
                ThumbOp::SoftwareInterrupt {
                    comment: opcode as u8,
                }
            } else if condition == 0xE {
                ThumbOp::Unknown(opcode)
            } else {
                ThumbOp::BranchConditional {
                    condition,
                    offset: opcode as u8 as i8,
                }
            }
        }

        0x1E => {
            let raw_offset = opcode & 0x07FF;
            let offset = if raw_offset & 0x0400 != 0 {
                (raw_offset | 0xF800) as i16
            } else {
                raw_offset as i16
            };
            ThumbOp::LongBranchPrefix { offset }
        }
        0x1F => ThumbOp::LongBranchSuffix {
            offset: opcode & 0x07FF,
        },

        // Format 18: Unconditional Branch (11100)
        0x1C => {
            let raw_offset = opcode & 0x07FF;
            let offset = if (raw_offset & 0x0400) != 0 {
                (raw_offset | 0xF800) as i16 // Negative offset
            } else {
                raw_offset as i16
            };
            ThumbOp::BranchUnconditional { offset }
        }

        _ => ThumbOp::Unknown(opcode),
    }
}

impl Cpu {
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
        bus.set_open_bus(u32::from(opcode) | u32::from(opcode) << 16);
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
            ThumbOp::AddSubtract { .. } => 1,
            ThumbOp::Alu {
                opcode: 0xD, rd, ..
            } => 1 + multiply_internal_cycles(self.registers[*rd as usize], true),
            // Register-specified shifts take an extra internal cycle.
            ThumbOp::Alu {
                opcode: 0x2 | 0x3 | 0x4 | 0x7,
                ..
            } => 2,
            ThumbOp::Alu { .. } => 1,
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
                // POP {PC} adds a pipeline refill (1S+1N) to the 1N+1I load.
                count
                    + match (*pop, *extra_register) {
                        (true, true) => 3,
                        (true, false) => 2,
                        (false, _) => 1,
                    }
            }
            ThumbOp::MultipleTransfer {
                load,
                register_list,
                ..
            } => register_list.count_ones().max(1) + 1 + u32::from(*load),
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
            }
            | ThumbOp::MultipleTransfer {
                load: true,
                register_list: 0,
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
}

impl Cpu {
    fn execute_thumb(
        &mut self,
        op: ThumbOp,
        instruction_pc: u32,
        bus: &mut MemoryBus,
    ) -> io::Result<()> {
        match op {
            ThumbOp::ShiftImmediate {
                rd,
                rm,
                shift_type,
                amount,
            } => {
                let (result, carry) = shift_operand(
                    self.registers[rm as usize],
                    shift_type,
                    u32::from(amount),
                    false,
                    self.cpsr & C_FLAG != 0,
                );
                self.registers[rd as usize] = result;
                self.set_nz(result);
                self.set_flag(C_FLAG, carry);
            }
            ThumbOp::AddSubtract {
                rd,
                rn,
                operand,
                immediate,
                subtract,
            } => {
                let rhs = if immediate {
                    u32::from(operand)
                } else {
                    self.registers[operand as usize]
                };
                self.execute_thumb_arithmetic(rd, self.registers[rn as usize], rhs, subtract);
            }
            ThumbOp::Alu { opcode, rd, rs } => {
                let lhs = self.registers[rd as usize];
                let rhs = self.registers[rs as usize];
                let result = match opcode {
                    0x0 => Some(lhs & rhs),
                    0x1 => Some(lhs ^ rhs),
                    0x2 | 0x3 | 0x4 | 0x7 => {
                        let shift_type = match opcode {
                            0x2 => 0,
                            0x3 => 1,
                            0x4 => 2,
                            0x7 => 3,
                            _ => unreachable!(),
                        };
                        let (value, carry) = shift_operand(
                            lhs,
                            shift_type,
                            rhs & 0xFF,
                            true,
                            self.cpsr & C_FLAG != 0,
                        );
                        self.set_flag(C_FLAG, carry);
                        Some(value)
                    }
                    0x5 => {
                        let (value, carry, overflow) =
                            add_with_carry(lhs, rhs, self.cpsr & C_FLAG != 0);
                        self.set_flag(C_FLAG, carry);
                        self.set_flag(V_FLAG, overflow);
                        Some(value)
                    }
                    0x6 => {
                        let (value, carry, overflow) =
                            subtract_with_borrow(lhs, rhs, self.cpsr & C_FLAG == 0);
                        self.set_flag(C_FLAG, carry);
                        self.set_flag(V_FLAG, overflow);
                        Some(value)
                    }
                    0x9 => {
                        let (result, carry, overflow) = subtract_with_borrow(0, rhs, false);
                        self.set_arithmetic_flags(result, carry, overflow);
                        Some(result)
                    }
                    0xA => {
                        let (result, carry, overflow) = subtract_with_borrow(lhs, rhs, false);
                        self.set_arithmetic_flags(result, carry, overflow);
                        Some(result)
                    }
                    0xB => {
                        let (result, carry, overflow) = add_with_carry(lhs, rhs, false);
                        self.set_arithmetic_flags(result, carry, overflow);
                        Some(result)
                    }
                    0xC => Some(lhs | rhs),
                    0xD => Some(lhs.wrapping_mul(rhs)),
                    0xE => Some(lhs & !rhs),
                    0xF => Some(!rhs),
                    0x8 => Some(lhs & rhs),
                    _ => None,
                };
                if let Some(result) = result {
                    if opcode != 0x8 && opcode != 0xA && opcode != 0xB {
                        self.registers[rd as usize] = result;
                    }
                    self.set_nz(result);
                }
            }
            ThumbOp::HighRegister { operation, rd, rs } => {
                let lhs = if rd == 15 {
                    instruction_pc.wrapping_add(4)
                } else {
                    self.registers[rd as usize]
                };
                let rhs = if rs == 15 {
                    instruction_pc.wrapping_add(4)
                } else {
                    self.registers[rs as usize]
                };
                match operation {
                    0 => {
                        let result = lhs.wrapping_add(rhs);
                        if rd == 15 {
                            self.registers[15] = result & !1;
                        } else {
                            self.registers[rd as usize] = result;
                        }
                    }
                    1 => {
                        let (result, carry, overflow) = subtract_with_borrow(lhs, rhs, false);
                        self.set_arithmetic_flags(result, carry, overflow);
                    }
                    2 => {
                        if rd == 15 {
                            self.registers[15] = rhs & !1;
                        } else {
                            self.registers[rd as usize] = rhs;
                        }
                    }
                    _ => unreachable!(),
                }
            }
            ThumbOp::BranchExchange { rm } => {
                let target = self.registers[rm as usize];
                if target & 1 == 0 {
                    self.cpsr &= !T_FLAG;
                    self.registers[15] = target & !3;
                } else {
                    self.registers[15] = target & !1;
                }
            }
            ThumbOp::LoadPcRelative { rd, offset } => {
                let address =
                    (instruction_pc.wrapping_add(4) & !3).wrapping_add(u32::from(offset) * 4);
                self.registers[rd as usize] = bus.read_u32(address)?;
            }
            ThumbOp::LoadStoreRegister {
                operation,
                rd,
                rb,
                ro,
            } => {
                let address = self.registers[rb as usize].wrapping_add(self.registers[ro as usize]);
                match operation {
                    0 => bus.write_u32(address & !3, self.registers[rd as usize])?,
                    1 => bus.write_u16(address & !1, self.registers[rd as usize] as u16)?,
                    2 => bus.write_u8(address, self.registers[rd as usize] as u8)?,
                    3 => self.registers[rd as usize] = (bus.read_u8(address)? as i8 as i32) as u32,
                    4 => {
                        self.registers[rd as usize] =
                            bus.read_u32(address & !3)?.rotate_right((address & 3) * 8)
                    }
                    5 => self.registers[rd as usize] = load_halfword(bus, address)?,
                    6 => self.registers[rd as usize] = u32::from(bus.read_u8(address)?),
                    7 => self.registers[rd as usize] = load_signed_halfword(bus, address)?,
                    _ => unreachable!(),
                }
            }
            ThumbOp::MoveImmediate { rd, imm } => {
                let val = imm as u32;
                self.registers[rd as usize] = val;

                // Update N and Z flags based on the moved value
                self.set_flag(N_FLAG, (val & (1 << 31)) != 0);
                self.set_flag(Z_FLAG, val == 0);
                // Move Immediate leaves C and V flags untouched
            }

            ThumbOp::CompareImmediate { rn, imm } => {
                let val1 = self.registers[rn as usize];
                let val2 = imm as u32;

                // CMP performs a subtraction (val1 - val2) without storing the result
                let (res, carry) = val1.overflowing_sub(val2);

                // Overflow (V) check for subtraction: signs of inputs differ, and sign of result differs from val1
                let overflow = ((val1 ^ val2) & (val1 ^ res) & 0x8000_0000) != 0;

                self.set_flag(N_FLAG, (res & 0x8000_0000) != 0);
                self.set_flag(Z_FLAG, res == 0);
                self.set_flag(C_FLAG, !carry); // ARM uses inverted borrow for Carry out of subtraction
                self.set_flag(V_FLAG, overflow);
            }

            ThumbOp::AddImmediate { rd, imm } => {
                let val1 = self.registers[rd as usize];
                let val2 = imm as u32;

                let (res, carry) = val1.overflowing_add(val2);
                let overflow = (!(val1 ^ val2) & (val1 ^ res) & 0x8000_0000) != 0;

                self.registers[rd as usize] = res;
                self.set_flag(N_FLAG, (res & 0x8000_0000) != 0);
                self.set_flag(Z_FLAG, res == 0);
                self.set_flag(C_FLAG, carry);
                self.set_flag(V_FLAG, overflow);
            }

            ThumbOp::SubtractImmediate { rd, imm } => {
                let val1 = self.registers[rd as usize];
                let val2 = imm as u32;

                let (res, carry) = val1.overflowing_sub(val2);
                let overflow = ((val1 ^ val2) & (val1 ^ res) & 0x8000_0000) != 0;

                self.registers[rd as usize] = res;
                self.set_flag(N_FLAG, (res & 0x8000_0000) != 0);
                self.set_flag(Z_FLAG, res == 0);
                self.set_flag(C_FLAG, !carry);
                self.set_flag(V_FLAG, overflow);
            }

            ThumbOp::BranchUnconditional { offset } => {
                let pipeline_pc = instruction_pc.wrapping_add(4);
                self.registers[15] =
                    pipeline_pc.wrapping_add((offset as i32).wrapping_mul(2) as u32);
            }

            ThumbOp::LoadStoreImmediate {
                load,
                byte,
                rd,
                rb,
                offset,
            } => {
                let base = self.registers[rb as usize];
                let displacement = if byte {
                    u32::from(offset)
                } else {
                    u32::from(offset) * 4
                };
                let address = base.wrapping_add(displacement);
                if load {
                    let value = if byte {
                        u32::from(bus.read_u8(address)?)
                    } else {
                        bus.read_u32(address & !3)?.rotate_right((address & 3) * 8)
                    };
                    self.registers[rd as usize] = value;
                } else if byte {
                    bus.write_u8(address, self.registers[rd as usize] as u8)?;
                } else {
                    bus.write_u32(address & !3, self.registers[rd as usize])?;
                }
            }
            ThumbOp::LoadStoreHalfwordImmediate {
                load,
                rd,
                rb,
                offset,
            } => {
                let address = self.registers[rb as usize].wrapping_add(u32::from(offset) * 2);
                if load {
                    self.registers[rd as usize] = load_halfword(bus, address)?;
                } else {
                    bus.write_u16(address & !1, self.registers[rd as usize] as u16)?;
                }
            }
            ThumbOp::SpRelativeLoadStore { load, rd, offset } => {
                let address = self.registers[13].wrapping_add(u32::from(offset) * 4);
                if load {
                    self.registers[rd as usize] =
                        bus.read_u32(address & !3)?.rotate_right((address & 3) * 8);
                } else {
                    bus.write_u32(address & !3, self.registers[rd as usize])?;
                }
            }
            ThumbOp::LoadAddress { use_sp, rd, offset } => {
                let base = if use_sp {
                    self.registers[13]
                } else {
                    instruction_pc.wrapping_add(4) & !3
                };
                self.registers[rd as usize] = base.wrapping_add(u32::from(offset) * 4);
            }
            ThumbOp::AdjustSp { subtract, offset } => {
                let delta = u32::from(offset) * 4;
                self.registers[13] = if subtract {
                    self.registers[13].wrapping_sub(delta)
                } else {
                    self.registers[13].wrapping_add(delta)
                };
            }
            ThumbOp::PushPop {
                pop,
                extra_register,
                register_list,
            } => {
                if pop {
                    let mut address = self.registers[13];
                    for register in 0..8 {
                        if register_list & (1 << register) != 0 {
                            self.registers[register] = bus.read_u32(address & !3)?;
                            address = address.wrapping_add(4);
                        }
                    }
                    if extra_register {
                        let value = bus.read_u32(address & !3)?;
                        if pop {
                            self.registers[15] = value & !1;
                            self.cpsr |= T_FLAG;
                        } else {
                            self.registers[14] = value;
                        }
                        address = address.wrapping_add(4);
                    }
                    self.registers[13] = address;
                } else {
                    let count = register_list.count_ones() + u32::from(extra_register);
                    let start = self.registers[13].wrapping_sub(count * 4);
                    let mut address = start;
                    for register in 0..8 {
                        if register_list & (1 << register) != 0 {
                            bus.write_u32(address, self.registers[register])?;
                            address = address.wrapping_add(4);
                        }
                    }
                    if extra_register {
                        bus.write_u32(address, self.registers[14])?;
                    }
                    self.registers[13] = start;
                }
            }
            ThumbOp::MultipleTransfer {
                load,
                rb,
                register_list,
            } => {
                let base = self.registers[rb as usize];
                if register_list == 0 {
                    // Empty list transfers PC alone and moves the base by 0x40.
                    if load {
                        self.registers[15] = bus.read_u32(base & !3)? & !1;
                    } else {
                        bus.write_u32(base & !3, instruction_pc.wrapping_add(6))?;
                    }
                    self.registers[rb as usize] = base.wrapping_add(0x40);
                    return Ok(());
                }
                let final_base = base.wrapping_add(register_list.count_ones() * 4);
                let first_register = register_list.trailing_zeros() as usize;
                let mut address = base;
                for register in 0..8 {
                    if register_list & (1 << register) == 0 {
                        continue;
                    }
                    if load {
                        self.registers[register] = bus.read_u32(address & !3)?;
                    } else {
                        // STMIA stores the written-back base unless Rb is first.
                        let value = if register == rb as usize && register != first_register {
                            final_base
                        } else {
                            self.registers[register]
                        };
                        bus.write_u32(address & !3, value)?;
                    }
                    address = address.wrapping_add(4);
                }
                if !load || register_list & (1 << rb) == 0 {
                    self.registers[rb as usize] = final_base;
                }
            }
            ThumbOp::BranchConditional { condition, offset } => {
                if self.condition_passed(condition) {
                    self.registers[15] = instruction_pc
                        .wrapping_add(4)
                        .wrapping_add((i32::from(offset) * 2) as u32);
                } else {
                    self.registers[15] = instruction_pc.wrapping_add(2);
                }
            }
            ThumbOp::SoftwareInterrupt { .. } => {
                self.enter_exception(MODE_SUPERVISOR, VECTOR_SWI, instruction_pc.wrapping_add(2))?;
            }
            ThumbOp::LongBranchPrefix { offset } => {
                self.registers[14] = instruction_pc
                    .wrapping_add(4)
                    .wrapping_add((i32::from(offset) * 4096) as u32);
            }
            ThumbOp::LongBranchSuffix { offset } => {
                let target = self.registers[14].wrapping_add(u32::from(offset) * 2);
                self.registers[14] = instruction_pc.wrapping_add(2) | 1;
                self.registers[15] = target & !1;
            }
            ThumbOp::Unknown(_) => unreachable!("unknown opcodes are rejected before execution"),
        }
        Ok(())
    }
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
        } => (0..register_list.count_ones().max(1))
            .map(|index| (registers[*rb as usize].wrapping_add(index * 4), 4))
            .collect(),
        _ => Vec::<(u32, u8)>::new(),
    };
    wait_cycles = add_data_wait_cycles(wait_cycles, bus, &data_accesses)?;
    Ok(wait_cycles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_move_immediate() {
        // Opcode: 00100 010 01010101 (Move value 0x55 into register r2)
        let opcode = 0b0010_0010_0101_0101;
        let expected = ThumbOp::MoveImmediate { rd: 2, imm: 0x55 };

        assert_eq!(decode(opcode), expected);
    }

    #[test]
    fn test_decode_compare_immediate() {
        // Opcode: 00101 001 11111111 (Compare register r1 with value 0xFF)
        let opcode = 0b0010_1001_1111_1111;
        let expected = ThumbOp::CompareImmediate { rn: 1, imm: 0xFF };

        assert_eq!(decode(opcode), expected);
    }

    #[test]
    fn test_decode_unconditional_branch_positive() {
        // Opcode: 11100 000 00000100 (Branch forward by 4)
        let opcode = 0b1110_0000_0000_0100;
        let expected = ThumbOp::BranchUnconditional { offset: 4 };

        assert_eq!(decode(opcode), expected);
    }

    #[test]
    fn test_decode_immediate_load_store_variants() {
        assert_eq!(
            decode(0x6048),
            ThumbOp::LoadStoreImmediate {
                load: false,
                byte: false,
                rd: 0,
                rb: 1,
                offset: 1,
            }
        );
        assert_eq!(
            decode(0x780A),
            ThumbOp::LoadStoreImmediate {
                load: true,
                byte: true,
                rd: 2,
                rb: 1,
                offset: 0,
            }
        );
    }

    #[test]
    fn decodes_thumb_shift_alu_conditional_branch_and_stack_ops() {
        assert_eq!(
            decode(0x0088),
            ThumbOp::ShiftImmediate {
                rd: 0,
                rm: 1,
                shift_type: 0,
                amount: 2,
            }
        );
        assert_eq!(
            decode(0x4308),
            ThumbOp::Alu {
                opcode: 12,
                rd: 0,
                rs: 1
            }
        );
        assert_eq!(
            decode(0xD001),
            ThumbOp::BranchConditional {
                condition: 0,
                offset: 1,
            }
        );
        assert_eq!(
            decode(0xB501),
            ThumbOp::PushPop {
                pop: false,
                extra_register: true,
                register_list: 1,
            }
        );
        assert_eq!(
            decode(0xBD01),
            ThumbOp::PushPop {
                pop: true,
                extra_register: true,
                register_list: 1,
            }
        );
        assert_eq!(decode(0xDF01), ThumbOp::SoftwareInterrupt { comment: 1 });
    }

    #[test]
    fn test_decode_unknown() {
        // An unmapped opcode pattern (e.g., all bits set)
        let opcode = 0xDE00;

        match decode(opcode) {
            ThumbOp::Unknown(val) => assert_eq!(val, 0xDE00),
            _ => panic!("Expected ThumbOp::Unknown variant"),
        }
    }

    #[test]
    fn decodes_branch_offsets_at_signed_boundaries() {
        assert_eq!(
            decode(0xE3FF),
            ThumbOp::BranchUnconditional { offset: 1023 }
        );
        assert_eq!(
            decode(0xE400),
            ThumbOp::BranchUnconditional { offset: -1024 }
        );
        assert_eq!(decode(0xE7FF), ThumbOp::BranchUnconditional { offset: -1 });

        assert_eq!(decode(0xF3FF), ThumbOp::LongBranchPrefix { offset: 1023 });
        assert_eq!(decode(0xF400), ThumbOp::LongBranchPrefix { offset: -1024 });
        assert_eq!(decode(0xF7FF), ThumbOp::LongBranchPrefix { offset: -1 });
        assert_eq!(decode(0xF800), ThumbOp::LongBranchSuffix { offset: 0 });
        assert_eq!(decode(0xFFFF), ThumbOp::LongBranchSuffix { offset: 0x07FF });
    }

    #[test]
    fn decodes_valid_bx_and_rejects_reserved_bx_encoding() {
        assert_eq!(decode(0x4700), ThumbOp::BranchExchange { rm: 0 });
        assert_eq!(decode(0x4778), ThumbOp::BranchExchange { rm: 15 });
        assert_eq!(decode(0x4780), ThumbOp::Unknown(0x4780));
    }

    #[test]
    fn decodes_conditional_branch_swi_and_reserved_condition() {
        assert_eq!(
            decode(0xD0FF),
            ThumbOp::BranchConditional {
                condition: 0,
                offset: -1,
            }
        );
        assert_eq!(decode(0xDFFF), ThumbOp::SoftwareInterrupt { comment: 0xFF });
        assert_eq!(decode(0xDEFF), ThumbOp::Unknown(0xDEFF));
    }
}
