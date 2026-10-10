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
