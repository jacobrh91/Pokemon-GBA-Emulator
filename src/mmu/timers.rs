//! Timers 0-3: prescaled counters with reload, cascading and overflow
//! interrupts (GBATEK "Timers").

use super::*;

#[derive(Default)]
pub(super) struct Timer {
    pub(super) reload: u16,
    pub(super) counter: u16,
    pub(super) control: u16,
    pub(super) prescaler_cycles: u16,
}

impl MemoryBus {
    /// Cycles until the earliest overflow among free-running timers
    /// (`u32::MAX` if none is counting).
    pub(super) fn timer_event_distance(&self) -> u32 {
        let mut distance = u32::MAX;
        for (index, timer) in self.timers.iter().enumerate() {
            let counting = timer.control & (1 << 7) != 0;
            let cascaded = index > 0 && timer.control & (1 << 2) != 0;
            if counting && !cascaded {
                let shift = [0_u32, 6, 8, 10][(timer.control & 3) as usize];
                let ticks = 0x1_0000 - u64::from(timer.counter);
                let cycles = (ticks << shift) - u64::from(timer.prescaler_cycles);
                distance = distance.min(cycles.min(u64::from(u32::MAX)) as u32);
            }
        }
        distance
    }

    /// Advances every timer by `cycles` of elapsed time and returns how many
    /// times timers 0 and 1 overflowed, for the Direct Sound FIFOs.
    pub(super) fn advance_timers(&mut self, cycles: u32) -> [u64; 2] {
        let mut cascade_ticks = 0;
        let mut audio_overflows = [0u64; 2];
        for index in 0..4 {
            let control = self.timers[index].control;
            if control & (1 << 7) == 0 {
                cascade_ticks = 0;
                continue;
            }
            if index > 0 && control & (1 << 2) != 0 {
                let source_ticks = cascade_ticks;
                cascade_ticks = self.advance_timer(index, source_ticks);
                if index < 2 {
                    audio_overflows[index] = cascade_ticks;
                }
                continue;
            }
            // Prescalers are 1, 64, 256 and 1024 cycles: shifts, not divides.
            let shift = [0_u32, 6, 8, 10][(control & 3) as usize];
            let accumulated = u64::from(self.timers[index].prescaler_cycles) + u64::from(cycles);
            let ticks = accumulated >> shift;
            self.timers[index].prescaler_cycles = (accumulated & ((1 << shift) - 1)) as u16;
            cascade_ticks = self.advance_timer(index, ticks);
            if index < 2 {
                audio_overflows[index] = cascade_ticks;
            }
        }
        audio_overflows
    }

    /// A timer's counter as the CPU would read it right now.
    pub(super) fn live_timer_counter(&self, index: usize) -> u16 {
        let timer = &self.timers[index];
        let counting = timer.control & (1 << 7) != 0;
        let cascaded = index > 0 && timer.control & (1 << 2) != 0;
        if !counting || cascaded {
            return timer.counter;
        }
        let shift = [0_u32, 6, 8, 10][(timer.control & 3) as usize];
        let ticks = (u64::from(timer.prescaler_cycles) + u64::from(self.pending_cycles)) >> shift;
        timer.counter.wrapping_add(ticks as u16)
    }

    pub(super) fn update_timer_register(&mut self, offset: usize) {
        if !(TIMER_BASE..TIMER_BASE + 16).contains(&offset) {
            return;
        }
        let timer_index = (offset - TIMER_BASE) / 4;
        let register_offset = (offset - TIMER_BASE) % 4;
        if register_offset < 2 {
            let base = TIMER_BASE + timer_index * 4;
            self.timers[timer_index].reload =
                u16::from_le_bytes([self.io_regs[base], self.io_regs[base + 1]]);
            if self.timers[timer_index].control & (1 << 7) == 0 {
                self.timers[timer_index].counter = self.timers[timer_index].reload;
            }
        } else if register_offset == 2 {
            let base = TIMER_BASE + timer_index * 4;
            let new_control = u16::from(self.io_regs[base + 2] & 0xC7);
            let was_enabled = self.timers[timer_index].control & (1 << 7) != 0;
            let enabled = new_control & (1 << 7) != 0;
            self.timers[timer_index].control = new_control;
            if enabled && !was_enabled {
                self.timers[timer_index].counter = self.timers[timer_index].reload;
                self.timers[timer_index].prescaler_cycles = 0;
            }
        }
    }

    fn advance_timer(&mut self, index: usize, ticks: u64) -> u64 {
        if ticks == 0 {
            return 0;
        }
        let (overflow_count, irq_enabled) = {
            let timer = &mut self.timers[index];
            let ticks_to_overflow = 0x1_0000 - u64::from(timer.counter);
            if ticks < ticks_to_overflow {
                timer.counter += ticks as u16;
                return 0;
            }
            let period = 0x1_0000 - u64::from(timer.reload);
            let remaining_ticks = ticks - ticks_to_overflow;
            let overflow_count = 1 + remaining_ticks / period;
            timer.counter = timer.reload.wrapping_add((remaining_ticks % period) as u16);
            (overflow_count, timer.control & (1 << 6) != 0)
        };
        if irq_enabled {
            let if_low = &mut self.io_regs[INTERRUPT_REQUEST];
            *if_low |= 1 << (3 + index);
        }
        overflow_count
    }
}
