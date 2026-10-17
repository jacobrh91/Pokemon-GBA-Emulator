//! When the next event the CPU can see is due: the basis of skipping idle
//! loops and halts across the many scanline, audio and timer events that
//! change nothing a program could observe.

use super::display::{
    CYCLES_PER_FRAME, CYCLES_PER_SCANLINE, HBLANK_START_CYCLES, TOTAL_SCANLINES,
    VBLANK_START_SCANLINE,
};
use super::dma::DmaTiming;
use super::registers::{DISPSTAT, SOUNDCNT_H};
use super::*;

const DISPSTAT_VBLANK_IRQ: u16 = 1 << 3;
const DISPSTAT_HBLANK_IRQ: u16 = 1 << 4;
const DISPSTAT_VCOUNT_IRQ: u16 = 1 << 5;
const TIMER_IRQ: u16 = 1 << 6;

impl MemoryBus {
    /// Cycles the CPU can still run, with nothing it writes or reads, before
    /// an interrupt flag may go up, a DMA transfer may run or the frame ends.
    /// Everything before that (scanlines, audio samples, timer ticks nobody
    /// listens to) only has to be accounted, not acted on.
    pub fn cycles_until_visible_event(&self) -> u32 {
        let distance = self
            .display_visible_distance()
            .min(self.timer_visible_distance());
        distance
            .min(u64::from(u32::MAX))
            .saturating_sub(u64::from(self.pending_cycles)) as u32
    }

    /// Distance from the last flush to the next display event that raises an
    /// interrupt or starts a DMA transfer, or to the end of the visible part
    /// of the frame (the emulator's frame boundary).
    fn display_visible_distance(&self) -> u64 {
        let dispstat = u16::from_le_bytes([self.io_regs[DISPSTAT], self.io_regs[DISPSTAT + 1]]);
        let frame = u64::from(CYCLES_PER_FRAME);
        let line = u64::from(CYCLES_PER_SCANLINE);
        let hblank = u64::from(HBLANK_START_CYCLES);
        let position = u64::from(self.display_cycles);
        let visible_lines = u64::from(VBLANK_START_SCANLINE);
        let mut nearest = u64::MAX;
        // Time to the next `moment` of the frame, strictly in the future.
        let mut consider = |moment: u64| {
            let moment = moment % frame;
            let distance = if moment > position {
                moment - position
            } else {
                moment + frame - position
            };
            nearest = nearest.min(distance);
        };

        consider((visible_lines - 1) * line + hblank);
        if dispstat & DISPSTAT_VBLANK_IRQ != 0 || self.dma_waits_for(DmaTiming::VBlank) {
            consider(visible_lines * line);
        }
        let next_hblank_line = if position % line < hblank {
            position / line
        } else {
            position / line + 1
        };
        if dispstat & DISPSTAT_HBLANK_IRQ != 0 {
            consider(next_hblank_line * line + hblank);
        }
        if self.dma_waits_for(DmaTiming::HBlank) {
            let next_visible = if next_hblank_line < visible_lines {
                next_hblank_line
            } else {
                0
            };
            consider(next_visible * line + hblank);
        }
        let compare = u64::from(dispstat >> 8);
        if dispstat & DISPSTAT_VCOUNT_IRQ != 0 && compare < u64::from(TOTAL_SCANLINES) {
            consider(compare * line);
        }
        nearest
    }

    /// Distance from the last flush to the first timer overflow that raises
    /// an interrupt or makes a Direct Sound FIFO ask for more data.
    fn timer_visible_distance(&self) -> u64 {
        let counting = |index: usize| self.timers[index].control & (1 << 7) != 0;
        let cascaded = |index: usize| index > 0 && self.timers[index].control & (1 << 2) != 0;
        let shift = |index: usize| [0_u32, 6, 8, 10][(self.timers[index].control & 3) as usize];
        let first_overflow = |index: usize| {
            let timer = &self.timers[index];
            ((0x1_0000 - u64::from(timer.counter)) << shift(index))
                - u64::from(timer.prescaler_cycles)
        };
        // A cascaded timer overflows no sooner than the first timer that
        // counts by itself does.
        let earliest_free = (0..4)
            .filter(|&index| counting(index) && !cascaded(index))
            .map(first_overflow)
            .min()
            .unwrap_or(u64::MAX);

        let mut nearest = u64::MAX;
        for index in (0..4).filter(|&index| counting(index)) {
            if self.timers[index].control & TIMER_IRQ != 0 {
                let first = if cascaded(index) {
                    earliest_free
                } else {
                    first_overflow(index)
                };
                nearest = nearest.min(first);
            }
        }
        let routing = u16::from_le_bytes([self.io_regs[SOUNDCNT_H], self.io_regs[SOUNDCNT_H + 1]]);
        for fifo in 0..2 {
            let index = usize::from(routing >> (10 + fifo * 4) & 1);
            if !self.dma_refills_fifo(fifo) || !counting(index) {
                continue;
            }
            if cascaded(index) {
                nearest = nearest.min(earliest_free);
                continue;
            }
            // Each overflow plays one sample; the FIFO asks for a refill when
            // that leaves it at the refill level.
            let overflows = self
                .apu
                .fifo_len(fifo)
                .saturating_sub(apu::FIFO_REFILL_LEVEL)
                .max(1) as u64;
            let period = (0x1_0000 - u64::from(self.timers[index].reload)) << shift(index);
            nearest = nearest.min(first_overflow(index) + (overflows - 1) * period);
        }
        nearest
    }
}
