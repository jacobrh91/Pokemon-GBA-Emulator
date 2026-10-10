//! Arithmetic and barrel-shifter primitives shared by the ARM and Thumb
//! instruction sets.

/// Booth early-termination: the multiplier array finishes after 1-4 cycles
/// depending on how many high bytes of the multiplier are all zeros (or, for
/// signed operations, all ones).
pub(super) fn multiply_internal_cycles(multiplier: u32, signed: bool) -> u32 {
    let leading = |mask: u32| multiplier & mask == 0 || (signed && multiplier & mask == mask);
    if leading(0xFFFF_FF00) {
        1
    } else if leading(0xFFFF_0000) {
        2
    } else if leading(0xFF00_0000) {
        3
    } else {
        4
    }
}

pub(super) fn add_with_carry(lhs: u32, rhs: u32, carry_in: bool) -> (u32, bool, bool) {
    let carry_value = u64::from(carry_in);
    let unsigned_result = u64::from(lhs) + u64::from(rhs) + carry_value;
    let signed_result = i64::from(lhs as i32) + i64::from(rhs as i32) + i64::from(carry_in);
    (
        unsigned_result as u32,
        unsigned_result > u64::from(u32::MAX),
        signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
    )
}

pub(super) fn subtract_with_borrow(lhs: u32, rhs: u32, borrow_in: bool) -> (u32, bool, bool) {
    let subtrahend = u64::from(rhs) + u64::from(borrow_in);
    let signed_result = i64::from(lhs as i32) - i64::from(rhs as i32) - i64::from(borrow_in);
    (
        lhs.wrapping_sub(rhs).wrapping_sub(u32::from(borrow_in)),
        u64::from(lhs) >= subtrahend,
        signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
    )
}

pub(super) fn shift_operand(
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
