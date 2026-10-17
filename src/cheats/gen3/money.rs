//! The infinite-money cheat: the balance the game sees is always 999,999.
//!
//! **Mechanism: replace a value at the end of a routine.** `GetMoney(&money)`
//! is the one function every reader of the balance goes through; the emulator
//! stops at its `bx lr` and sets `r0`, the returned balance, to 999,999 (see
//! `Emulator::step`). The shop's money box, `IsEnoughMoney` and the quantity
//! prompt (capped by the game's own limit of 99) all see it.
//!
//! Purchases, `AddMoney` and whiteouts start from the shown balance and write
//! the result back, so the real balance becomes 999,999 minus the price of
//! whatever was bought: nearly the most the game allows once the cheat is off.
//!
//! # Finding it
//!
//! `GetMoney` is a leaf function of nine instructions that decrypts the
//! stored balance with the save's encryption key:
//!
//! ```text
//! 04 49  ldr  r1, [pc, #0x10]     r1 = &gSaveBlock2Ptr
//! 09 68  ldr  r1, [r1]            r1 = gSaveBlock2Ptr
//! F2 22  movs r2, #0xF2           ┐
//! 12 01  lsls r2, r2, #4          │ + 0xF20: the encryption key
//! 89 18  adds r1, r1, r2          ┘
//! 00 68  ldr  r0, [r0]            r0 = the stored balance
//! 09 68  ldr  r1, [r1]            r1 = the key
//! 48 40  eors r0, r1              r0 = balance ^ key
//! 70 47  bx   lr                  <- the hook
//! ```
//!
//! These bytes also occur twice elsewhere in the ROM, so they cannot identify
//! the function. `IsEnoughMoney` can: its bytes are unique and it begins by
//! calling `GetMoney`. The scan finds `IsEnoughMoney` (see
//! [`IS_ENOUGH_SIGNATURE`]), decodes the target of its `BL`, checks that the
//! target's bytes are the ones above, and hooks the `bx lr` 16 bytes in.

use super::{ROM_BASE, Signature};

/// What is shown as the balance while the cheat is on.
pub(crate) const SHOWN_BALANCE: u32 = 999_999;

/// `IsEnoughMoney`, the locator: `push {r4, lr}; adds r4, r1, #0; bl
/// GetMoney; cmp r0, r4; bhs +1; movs r0, #0; b +0; movs r0, #1; pop {r4};
/// pop {r1}; bx r1`. `None` is the wildcard of the `BL`.
pub(crate) const IS_ENOUGH_SIGNATURE: Signature = Signature::new(
    &[
        Some(0x10),
        Some(0xB5),
        Some(0x0C),
        Some(0x1C),
        None,
        None,
        None,
        None,
        Some(0xA0),
        Some(0x42),
        Some(0x01),
        Some(0xD2),
        Some(0x00),
        Some(0x20),
        Some(0x00),
        Some(0xE0),
        Some(0x01),
        Some(0x20),
        Some(0x10),
        Some(0xBC),
        Some(0x02),
        Some(0xBC),
        Some(0x08),
        Some(0x47),
    ],
    0,
);

/// The instructions of `GetMoney` after its first one (the `ldr` of a pool
/// address, which is not compared), up to and including `bx lr`, in
/// FireRed and LeafGreen: the key is at `gSaveBlock2Ptr + 0xF20`.
pub(crate) const GET_MONEY_BODY: [u8; 16] = [
    0x09, 0x68, 0xF2, 0x22, 0x12, 0x01, 0x89, 0x18, 0x00, 0x68, 0x09, 0x68, 0x48, 0x40, 0x70, 0x47,
];

/// The same in Emerald, where the key is at `gSaveBlock2Ptr + 0xAC` (an
/// immediate add rather than a built constant):
///
/// ```text
/// 09 68  ldr  r1, [r1]
/// AC 31  adds r1, #0xAC
/// 00 68  ldr  r0, [r0]
/// 09 68  ldr  r1, [r1]
/// 48 40  eors r0, r1
/// 70 47  bx   lr
/// ```
pub(crate) const EMERALD_GET_MONEY_BODY: [u8; 12] = [
    0x09, 0x68, 0xAC, 0x31, 0x00, 0x68, 0x09, 0x68, 0x48, 0x40, 0x70, 0x47,
];

/// Offset of the `BL` to `GetMoney` within `IsEnoughMoney`.
const CALL_OFFSET: u32 = 4;

/// The address of the `bx lr` ending `GetMoney`, if `rom` is a recognised
/// game.
pub fn find(rom: &[u8]) -> Option<u32> {
    let is_enough = IS_ENOUGH_SIGNATURE.find(rom)?;
    let get_money = bl_target(rom, is_enough + CALL_OFFSET)?;
    let start = usize::try_from(get_money.checked_sub(ROM_BASE)?).ok()?;
    // Skip the first halfword: a PC-relative load, whose offset is not part
    // of the identification. The hook is the `bx lr`, the last instruction of
    // whichever body this is.
    [&GET_MONEY_BODY[..], &EMERALD_GET_MONEY_BODY[..]]
        .into_iter()
        .find_map(|body| {
            let at = start + 2;
            (rom.get(at..at + body.len())? == body).then(|| get_money + 2 + body.len() as u32 - 2)
        })
}

/// The target of the Thumb `BL` pair at bus address `at`, if it is one.
fn bl_target(rom: &[u8], at: u32) -> Option<u32> {
    let offset = usize::try_from(at.checked_sub(ROM_BASE)?).ok()?;
    let pair = rom.get(offset..offset + 4)?;
    let high = u16::from_le_bytes([pair[0], pair[1]]);
    let low = u16::from_le_bytes([pair[2], pair[3]]);
    if high & 0xF800 != 0xF000 || low & 0xF800 != 0xF800 {
        return None;
    }
    // 23 bits of signed offset: 11 from each halfword, relative to the
    // address of the pair plus 4.
    let high_part = (i32::from(high & 0x7FF) << 21) >> 9;
    let distance = high_part + (i32::from(low & 0x7FF) << 1);
    at.checked_add(4)?.checked_add_signed(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bl` encodings for `distance` bytes from the address of the pair plus 4.
    fn bl(distance: i32) -> [u8; 4] {
        let high = 0xF000 | ((distance >> 12) & 0x7FF) as u16;
        let low = 0xF800 | ((distance >> 1) & 0x7FF) as u16;
        let [h0, h1] = high.to_le_bytes();
        let [l0, l1] = low.to_le_bytes();
        [h0, h1, l0, l1]
    }

    /// A ROM with `IsEnoughMoney` at `is_enough` calling a `GetMoney` at
    /// `get_money`, laid out as the game does (the callee is earlier).
    fn rom(game_code: &[u8; 4], is_enough: usize, get_money: usize, body: &[u8]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x1000];
        rom[0xAC..0xB0].copy_from_slice(game_code);
        for (index, byte) in IS_ENOUGH_SIGNATURE.bytes.iter().enumerate() {
            rom[is_enough + index] = byte.unwrap_or(0);
        }
        let distance = get_money as i32 - (is_enough as i32 + CALL_OFFSET as i32 + 4);
        rom[is_enough + 4..is_enough + 8].copy_from_slice(&bl(distance));
        rom[get_money..get_money + 2].copy_from_slice(&[0x04, 0x49]);
        rom[get_money + 2..get_money + 2 + body.len()].copy_from_slice(body);
        rom
    }

    #[test]
    fn the_hook_is_the_return_of_the_function_that_is_called() {
        let image = rom(b"BPRE", 0x600, 0x400, &GET_MONEY_BODY);
        assert_eq!(find(&image), Some(0x0800_0000 + 0x400 + 16));
        // Emerald's shorter body, with the key at +0xAC.
        let image = rom(b"BPEE", 0x600, 0x400, &EMERALD_GET_MONEY_BODY);
        assert_eq!(find(&image), Some(0x0800_0000 + 0x400 + 12));
        let image = rom(b"BPRE", 0x600, 0x400, &GET_MONEY_BODY);
        assert_eq!(find(&image), Some(0x0800_0000 + 0x400 + 16));
        // A call forwards too.
        let image = rom(b"BPRJ", 0x200, 0x800, &GET_MONEY_BODY);
        assert_eq!(find(&image), Some(0x0800_0000 + 0x800 + 16));
    }

    #[test]
    fn the_bl_decoder_matches_a_real_call() {
        // From FireRed v1.1: `bl 0x0809FD6C` at 0x0809FDA0 encodes FFF7 E4FF.
        let mut rom = vec![0u8; 0x100];
        rom[0x10..0x14].copy_from_slice(&[0xFF, 0xF7, 0xE4, 0xFF]);
        assert_eq!(bl_target(&rom, 0x0800_0010), Some(0x0800_0014 - 0x38));
    }

    #[test]
    fn a_callee_that_is_not_getmoney_is_declined() {
        let mut other = GET_MONEY_BODY;
        other[12] = 0x49;
        assert_eq!(find(&rom(b"BPRE", 0x600, 0x400, &other)), None);
        // Not a BL at the call site.
        let mut image = rom(b"BPRE", 0x600, 0x400, &GET_MONEY_BODY);
        image[0x604..0x608].copy_from_slice(&[0, 0, 0, 0]);
        assert_eq!(find(&image), None);
    }

    #[test]
    fn it_is_declined_unless_unique_aligned_and_gen3() {
        assert_eq!(find(&[]), None);
        assert_eq!(find(&[0; 8]), None);
        let image = rom(b"AXVE", 0x600, 0x400, &GET_MONEY_BODY);
        assert_eq!(find(&image), None);
        let image = rom(b"AXPE", 0x600, 0x400, &GET_MONEY_BODY);
        assert_eq!(find(&image), None);
        let mut two = rom(b"BPRE", 0x600, 0x400, &GET_MONEY_BODY);
        let copy = two[0x600..0x618].to_vec();
        two[0x800..0x818].copy_from_slice(&copy);
        assert_eq!(find(&two), None, "ambiguous");
        let odd = rom(b"BPRE", 0x601, 0x400, &GET_MONEY_BODY);
        assert_eq!(find(&odd), None, "unaligned");
    }
}
