use super::arm::ArmOp;
use super::*;

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
    pub(super) fn execute_arm(
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
        if transfer.load && should_writeback && transfer.rn == transfer.rd {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ARM halfword load writeback with matching base/destination is unpredictable",
            ));
        }
        if transfer.load {
            let value = match transfer.transfer_type {
                0b01 => u32::from(bus.read_u16(address)?),
                0b10 => (bus.read_u8(address)? as i8 as i32) as u32,
                0b11 => (bus.read_u16(address)? as i16 as i32) as u32,
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
            bus.write_u16(address, self.registers[transfer.rd as usize] as u16)?;
            self.registers[15] = instruction_pc.wrapping_add(4);
        }
        if should_writeback {
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
        if writes_flags {
            if restored_cpsr.is_none() {
                self.set_flag(N_FLAG, result & 0x8000_0000 != 0);
                self.set_flag(Z_FLAG, result == 0);
                if let Some((carry, overflow)) = arithmetic_flags {
                    self.set_flag(C_FLAG, carry);
                    self.set_flag(V_FLAG, overflow);
                } else {
                    self.set_flag(C_FLAG, shifter_carry);
                }
            }
        }
        Ok(())
    }
}
