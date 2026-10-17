//! DMA controller: four channels moving 16- or 32-bit units between bus
//! addresses without CPU involvement (GBATEK "DMA Transfers", mGBA `dma.c`).

use super::*;
use crate::mmu::registers::{DMA_BASE, FIFO_A, INTERRUPT_REQUEST};
use crate::state::{StateReader, StateWriter};

const CHANNEL_STRIDE: usize = 12;
/// Offset of the high byte of CNT_H, whose write may enable the channel.
const CNT_H_HIGH_BYTE: usize = 11;

const CONTROL_DEST_SHIFT: u16 = 5;
const CONTROL_SOURCE_SHIFT: u16 = 7;
const CONTROL_REPEAT: u16 = 1 << 9;
const CONTROL_WORDS: u16 = 1 << 10;
const CONTROL_TIMING_SHIFT: u16 = 12;
const CONTROL_IRQ: u16 = 1 << 14;
const CONTROL_ENABLE: u16 = 1 << 15;

const ADJUST_DECREMENT: u16 = 1;
const ADJUST_FIXED: u16 = 2;
const ADJUST_RELOAD: u16 = 3;

#[derive(Clone, Copy, Default)]
pub(super) struct DmaChannel {
    active: bool,
    source: u32,
    destination: u32,
    count: u32,
}

impl DmaChannel {
    pub(super) fn save_state(&self, writer: &mut StateWriter) {
        writer.bool(self.active);
        writer.u32(self.source);
        writer.u32(self.destination);
        writer.u32(self.count);
    }

    pub(super) fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        self.active = reader.bool()?;
        self.source = reader.u32()?;
        self.destination = reader.u32()?;
        self.count = reader.u32()?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DmaTiming {
    Immediate = 0,
    VBlank = 1,
    HBlank = 2,
    Special = 3,
}

impl MemoryBus {
    fn dma_control(&self, channel: usize) -> u16 {
        let base = DMA_BASE + channel * CHANNEL_STRIDE;
        u16::from_le_bytes([self.io_regs[base + 10], self.io_regs[base + 11]])
    }

    fn dma_word(&self, channel: usize, register: usize) -> u32 {
        let base = DMA_BASE + channel * CHANNEL_STRIDE + register;
        u32::from_le_bytes([
            self.io_regs[base],
            self.io_regs[base + 1],
            self.io_regs[base + 2],
            self.io_regs[base + 3],
        ])
    }

    fn dma_reload_count(&self, channel: usize) -> u32 {
        let base = DMA_BASE + channel * CHANNEL_STRIDE + 8;
        let raw = u32::from(u16::from_le_bytes([
            self.io_regs[base],
            self.io_regs[base + 1],
        ]));
        let (mask, zero_count) = if channel == 3 {
            (0xFFFF, 0x1_0000)
        } else {
            (0x3FFF, 0x4000)
        };
        match raw & mask {
            0 => zero_count,
            count => count,
        }
    }

    /// Called after an I/O byte in the DMA register block is written.
    pub(super) fn dma_register_written(&mut self, offset: usize) {
        if !(DMA_BASE..DMA_BASE + 4 * CHANNEL_STRIDE).contains(&offset)
            || (offset - DMA_BASE) % CHANNEL_STRIDE != CNT_H_HIGH_BYTE
        {
            return;
        }
        let channel = (offset - DMA_BASE) / CHANNEL_STRIDE;
        let control = self.dma_control(channel);
        if control & CONTROL_ENABLE == 0 {
            self.dma[channel].active = false;
            return;
        }
        if self.dma[channel].active {
            return;
        }

        let (source_mask, destination_mask) = match channel {
            0 => (0x07FF_FFFF, 0x07FF_FFFF),
            3 => (0x0FFF_FFFF, 0x0FFF_FFFF),
            _ => (0x0FFF_FFFF, 0x07FF_FFFF),
        };
        let words = control & CONTROL_WORDS != 0;
        let alignment = if words { !3 } else { !1 };
        self.dma[channel] = DmaChannel {
            active: true,
            source: self.dma_word(channel, 0) & source_mask & alignment,
            destination: self.dma_word(channel, 4) & destination_mask & alignment,
            count: self.dma_reload_count(channel),
        };
        if (control >> CONTROL_TIMING_SHIFT) & 3 == DmaTiming::Immediate as u16 {
            self.run_dma(channel);
        }
    }

    /// Runs every active channel waiting on `timing`.
    pub(super) fn trigger_dma(&mut self, timing: DmaTiming) {
        for channel in 0..4 {
            if self.dma[channel].active
                && (self.dma_control(channel) >> CONTROL_TIMING_SHIFT) & 3 == timing as u16
            {
                self.run_dma(channel);
            }
        }
    }

    /// Runs the sound DMA channel (1 for FIFO A, 2 for FIFO B) whose
    /// destination is the FIFO that just asked for data.
    pub(super) fn trigger_fifo_dma(&mut self, fifo: usize) {
        let target = IO_REG_START + (FIFO_A + fifo * 4) as u32;
        for channel in [1, 2] {
            if self.dma[channel].active
                && (self.dma_control(channel) >> CONTROL_TIMING_SHIFT) & 3
                    == DmaTiming::Special as u16
                && self.dma[channel].destination & !3 == target
            {
                self.run_dma(channel);
            }
        }
    }

    /// Cycles the CPU was stalled by DMA since the last call.
    #[inline]
    pub fn take_dma_cycles(&mut self) -> u32 {
        std::mem::take(&mut self.dma_cycles)
    }

    /// Whether DMA has stalled the CPU since the last [`Self::take_dma_cycles`].
    #[inline]
    pub fn dma_stall_pending(&self) -> bool {
        self.dma_cycles != 0
    }

    /// Whether an enabled channel starts its transfers at `timing`.
    pub(super) fn dma_waits_for(&self, timing: DmaTiming) -> bool {
        (0..4).any(|channel| {
            self.dma[channel].active
                && (self.dma_control(channel) >> CONTROL_TIMING_SHIFT) & 3 == timing as u16
        })
    }

    /// Whether a sound DMA channel is set to refill `fifo` when it runs low.
    pub(super) fn dma_refills_fifo(&self, fifo: usize) -> bool {
        let target = IO_REG_START + (FIFO_A + fifo * 4) as u32;
        [1, 2].into_iter().any(|channel| {
            self.dma[channel].active
                && (self.dma_control(channel) >> CONTROL_TIMING_SHIFT) & 3
                    == DmaTiming::Special as u16
                && self.dma[channel].destination & !3 == target
        })
    }

    fn run_dma(&mut self, channel: usize) {
        self.visible_events += 1;
        let control = self.dma_control(channel);
        // Sound FIFO DMA: always four words to a fixed FIFO address.
        let fifo_transfer = (control >> CONTROL_TIMING_SHIFT) & 3 == DmaTiming::Special as u16
            && matches!(channel, 1 | 2);
        let words = fifo_transfer || control & CONTROL_WORDS != 0;
        let unit: u32 = if words { 4 } else { 2 };
        let source_adjust = (control >> CONTROL_SOURCE_SHIFT) & 3;
        let destination_adjust = if fifo_transfer {
            ADJUST_FIXED
        } else {
            (control >> CONTROL_DEST_SHIFT) & 3
        };
        let DmaChannel {
            mut source,
            mut destination,
            mut count,
            ..
        } = self.dma[channel];
        if fifo_transfer {
            count = 4;
        }

        // Two internal cycles to start, then a read and a write per unit.
        let mut cycles: u32 = 2;
        for index in 0..count {
            let sequential = index != 0;
            let value = if words {
                self.read_u32(source).unwrap_or(0)
            } else {
                u32::from(self.read_u16(source).unwrap_or(0))
            };
            let _ = if words {
                self.write_u32(destination, value)
            } else {
                self.write_u16(destination, value as u16)
            };
            cycles = cycles
                .saturating_add(
                    self.transfer_cycles(source, unit as u8, sequential)
                        .unwrap_or(1),
                )
                .saturating_add(
                    self.transfer_cycles(destination, unit as u8, sequential)
                        .unwrap_or(1),
                );
            source = adjust_address(source, source_adjust, unit);
            destination = adjust_address(destination, destination_adjust, unit);
        }
        self.dma_cycles = self.dma_cycles.saturating_add(cycles);

        let timing = (control >> CONTROL_TIMING_SHIFT) & 3;
        let repeat = control & CONTROL_REPEAT != 0 && timing != DmaTiming::Immediate as u16;
        if repeat {
            let reloaded_destination = if destination_adjust == ADJUST_RELOAD {
                let mask = if channel == 3 {
                    0x0FFF_FFFF
                } else {
                    0x07FF_FFFF
                };
                let alignment = if words { !3 } else { !1 };
                self.dma_word(channel, 4) & mask & alignment
            } else {
                destination
            };
            let count = self.dma_reload_count(channel);
            self.dma[channel] = DmaChannel {
                active: true,
                source,
                destination: reloaded_destination,
                count,
            };
        } else {
            self.dma[channel].active = false;
            let high_byte = DMA_BASE + channel * CHANNEL_STRIDE + CNT_H_HIGH_BYTE;
            self.io_regs[high_byte] &= !(CONTROL_ENABLE >> 8) as u8;
        }
        if control & CONTROL_IRQ != 0 {
            self.io_regs[INTERRUPT_REQUEST + 1] |= 1 << channel;
        }
    }
}

fn adjust_address(address: u32, adjust: u16, unit: u32) -> u32 {
    match adjust {
        ADJUST_DECREMENT => address.wrapping_sub(unit),
        ADJUST_FIXED => address,
        // Increment, and increment-with-reload both advance during a run.
        _ => address.wrapping_add(unit),
    }
}

/// Reads of the write-only address/count registers return zero.
pub(super) fn dma_register_is_write_only(offset: usize) -> bool {
    (DMA_BASE..DMA_BASE + 4 * CHANNEL_STRIDE).contains(&offset)
        && (offset - DMA_BASE) % CHANNEL_STRIDE < 10
}
