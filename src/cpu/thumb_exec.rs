use super::thumb::ThumbOp;
use super::*;

impl Cpu {
    pub(super) fn execute_thumb(
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
                    1 => bus.write_u16(address, self.registers[rd as usize] as u16)?,
                    2 => bus.write_u8(address, self.registers[rd as usize] as u8)?,
                    3 => self.registers[rd as usize] = (bus.read_u8(address)? as i8 as i32) as u32,
                    4 => {
                        self.registers[rd as usize] =
                            bus.read_u32(address & !3)?.rotate_right((address & 3) * 8)
                    }
                    5 => self.registers[rd as usize] = u32::from(bus.read_u16(address)?),
                    6 => self.registers[rd as usize] = u32::from(bus.read_u8(address)?),
                    7 => {
                        self.registers[rd as usize] = (bus.read_u16(address)? as i16 as i32) as u32
                    }
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
                    self.registers[rd as usize] = u32::from(bus.read_u16(address)?);
                } else {
                    bus.write_u16(address, self.registers[rd as usize] as u16)?;
                }
            }
            ThumbOp::SpRelativeLoadStore { load, rd, offset } => {
                let address = self.registers[13].wrapping_add(u32::from(offset) * 4);
                if load {
                    self.registers[rd as usize] = bus.read_u32(address)?;
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
                let mut address = self.registers[rb as usize];
                for register in 0..8 {
                    if register_list & (1 << register) == 0 {
                        continue;
                    }
                    if load {
                        self.registers[register] = bus.read_u32(address & !3)?;
                    } else {
                        bus.write_u32(address & !3, self.registers[register])?;
                    }
                    address = address.wrapping_add(4);
                }
                if !load || register_list & (1 << rb) == 0 {
                    self.registers[rb as usize] = address;
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
