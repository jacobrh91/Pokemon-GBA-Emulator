//! Game Pak wait states (WAITCNT) and per-region bus access costs.

use super::*;

/// Game Pak access cycles from WAITCNT: `([[non-sequential, sequential];
/// 3 wait states], SRAM cycles)`.
pub(super) fn decode_waitcnt(waitcnt: u16) -> ([[u32; 2]; 3], u32) {
    const FIRST_ACCESS: [u32; 4] = [4, 3, 2, 8];
    // Sequential cycles: [WS0, WS1, WS2] each pick between two settings.
    const SEQUENTIAL: [[u32; 2]; 3] = [[2, 1], [4, 1], [8, 1]];
    let mut rom = [[0; 2]; 3];
    for wait_state in 0..3 {
        let first = FIRST_ACCESS[usize::from((waitcnt >> [2, 5, 8][wait_state]) & 3)];
        let sequential_bit = usize::from((waitcnt >> [4, 7, 10][wait_state]) & 1);
        rom[wait_state] = [first, SEQUENTIAL[wait_state][sequential_bit]];
    }
    (rom, FIRST_ACCESS[usize::from(waitcnt & 3)])
}

impl MemoryBus {
    /// Cycles for a 16-bit (or 8-bit) access at `address`; 32-bit accesses
    /// are two of these. Mirrors the region map in [`Self::resolve`].
    #[inline]
    fn halfword_cycles(&self, address: u32, sequential: bool) -> u32 {
        match address >> 24 {
            // EWRAM sits on a 16-bit bus with 2 wait states.
            0x02 => 3,
            // Game Pak ROM wait-state windows.
            0x08..=0x0D => {
                let wait_state = match address >> 24 {
                    0x08 | 0x09 => 0,
                    0x0A | 0x0B => 1,
                    _ => 2,
                };
                self.rom_cycles[wait_state][usize::from(sequential)]
            }
            0x0E => self.sram_cycles,
            // BIOS, IWRAM, I/O, palette, VRAM, OAM and open bus.
            _ => 1,
        }
    }

    /// Wait cycles (cycles beyond the first) of a 16-bit instruction fetch.
    #[inline]
    pub fn fetch_wait_cycles_16(&self, address: u32, sequential: bool) -> u32 {
        self.halfword_cycles(address, sequential).saturating_sub(1)
    }

    /// Wait cycles of a 32-bit instruction fetch.
    #[inline]
    pub fn fetch_wait_cycles_32(&self, address: u32, sequential: bool) -> u32 {
        if matches!(address >> 24, 0x00 | 0x03 | 0x04 | 0x07) {
            return 0;
        }
        (self.halfword_cycles(address, sequential)
            + self.halfword_cycles(address.wrapping_add(2), true))
        .saturating_sub(1)
    }

    /// Cycles of an 8-, 16- or 32-bit access (`width` 1, 2 or 4 bytes).
    #[inline]
    pub fn access_cycles(&self, address: u32, width: u8, sequential: bool) -> u32 {
        if width <= 2 {
            self.halfword_cycles(address, sequential)
        } else if matches!(address >> 24, 0x00 | 0x03 | 0x04 | 0x07) {
            1
        } else {
            self.halfword_cycles(address, sequential)
                + self.halfword_cycles(address.wrapping_add(2), true)
        }
    }

    #[inline]
    pub fn transfer_cycles(&self, address: u32, width: u8, sequential: bool) -> io::Result<u32> {
        match width {
            1 | 2 => Ok(self.halfword_cycles(address, sequential)),
            // BIOS, IWRAM, I/O and OAM are on 32-bit buses: a word is one access.
            4 if matches!(address >> 24, 0x00 | 0x03 | 0x04 | 0x07) => Ok(1),
            4 => Ok(self.halfword_cycles(address, sequential)
                + self.halfword_cycles(address.wrapping_add(2), true)),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported bus access width: {width}"),
            )),
        }
    }

    /// Re-derives the cached Game Pak timings after WAITCNT changed.
    pub(super) fn refresh_waitcnt(&mut self) {
        let waitcnt = u16::from_le_bytes([self.io_regs[0x204], self.io_regs[0x205]]);
        (self.rom_cycles, self.sram_cycles) = decode_waitcnt(waitcnt);
    }
}
