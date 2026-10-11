//! Display timing: scanline and HBlank/VBlank events, display interrupts and
//! the per-scanline register snapshots the PPU draws from.

use super::*;

pub(super) const CYCLES_PER_SCANLINE: u32 = 1232;
pub(super) const VBLANK_START_SCANLINE: u32 = 160;
pub(super) const TOTAL_SCANLINES: u32 = 228;
pub(super) const HBLANK_START_CYCLES: u32 = 240 * 4;
#[cfg(test)]
pub(super) const VBLANK_START_CYCLES: u32 = CYCLES_PER_SCANLINE * VBLANK_START_SCANLINE;
pub(super) const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * TOTAL_SCANLINES;

/// Display registers (DISPCNT..BLDY) as they were when a visible scanline
/// finished drawing. HBlank DMA runs after the snapshot, so per-line scroll
/// and window effects land on the following line, as on hardware.
#[derive(Clone)]
pub struct ScanlineSnapshot {
    pub line: usize,
    /// BG2/BG3 reference point (X/Y) registers written since the previous
    /// snapshot; the PPU reloads its internal reference from them.
    pub affine_reload: [bool; 2],
    pub registers: [u8; DISPLAY_REGISTER_BYTES],
}

pub const DISPLAY_REGISTER_BYTES: usize = 0x60;
/// Snapshots queued without a consumer are dropped past this many.
pub(super) const MAX_QUEUED_SCANLINES: usize = 512;

impl MemoryBus {
    /// Raises display interrupts and starts VBlank/HBlank DMA for every
    /// scanline boundary crossed while `cycles` elapse.
    pub(super) fn display_events(&mut self, cycles: u32) {
        let start = u64::from(self.display_cycles);
        let end = start + u64::from(cycles);
        let dispstat = u16::from_le_bytes([self.io_regs[DISPSTAT], self.io_regs[DISPSTAT + 1]]);
        let line_cycles = u64::from(CYCLES_PER_SCANLINE);
        let hblank_offset = u64::from(HBLANK_START_CYCLES);

        for scanline in start / line_cycles..=end / line_cycles {
            let line = (scanline % u64::from(TOTAL_SCANLINES)) as u32;
            let line_start = scanline * line_cycles;
            if line_start > start && line_start <= end {
                if line == VBLANK_START_SCANLINE {
                    if dispstat & (1 << 3) != 0 {
                        self.io_regs[INTERRUPT_REQUEST] |= 1;
                    }
                    self.trigger_dma(DmaTiming::VBlank);
                }
                if dispstat & (1 << 5) != 0 && line == u32::from(dispstat >> 8) {
                    self.io_regs[INTERRUPT_REQUEST] |= 1 << 2;
                }
            }
            let hblank = line_start + hblank_offset;
            if hblank > start && hblank <= end {
                if dispstat & (1 << 4) != 0 {
                    self.io_regs[INTERRUPT_REQUEST] |= 1 << 1;
                }
                if line < VBLANK_START_SCANLINE {
                    if self.scanlines.len() < MAX_QUEUED_SCANLINES {
                        let mut registers = [0; DISPLAY_REGISTER_BYTES];
                        registers.copy_from_slice(&self.io_regs[..DISPLAY_REGISTER_BYTES]);
                        self.scanlines.push(ScanlineSnapshot {
                            line: line as usize,
                            affine_reload: std::mem::take(&mut self.affine_reload),
                            registers,
                        });
                    }
                    self.trigger_dma(DmaTiming::HBlank);
                }
            }
        }
    }

    /// The display position including cycles not yet flushed.
    pub(super) fn live_display_cycles(&self) -> u32 {
        self.display_cycles + self.pending_cycles
    }

    /// True if [`Self::take_scanlines`] would return something.
    #[inline]
    pub fn has_scanlines(&self) -> bool {
        !self.scanlines.is_empty()
    }

    /// Scanlines that finished drawing since the last call, oldest first.
    pub fn take_scanlines(&mut self) -> Vec<ScanlineSnapshot> {
        std::mem::take(&mut self.scanlines)
    }
}
