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
