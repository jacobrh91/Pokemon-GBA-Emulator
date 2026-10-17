//! Exact idle-loop detection: lets the emulator skip whole iterations of a
//! loop that provably changes nothing.

use crate::cpu::Cpu;
use crate::mmu::MemoryBus;

/// Recognises a loop that leaves the whole machine state unchanged.
///
/// At every backward jump the CPU state and the bus's write, volatile-read and
/// visible-event counters are compared with those at the previous jump to the
/// same address. If nothing changed, nothing will until an event the CPU can
/// see intervenes, so time may advance by whole iterations up to that event.
#[derive(Default)]
pub(super) struct IdleLoopDetector {
    armed: bool,
    head: u32,
    registers: [u32; 16],
    cpsr: u32,
    writes: u64,
    flushes: u64,
    visible_events: u64,
    volatile_reads: u32,
    display_reads: u32,
    cycles: u64,
}

impl IdleLoopDetector {
    /// Records a backward jump; returns the loop's cycle length if the
    /// iteration just finished was provably a no-op, and whether it polled the
    /// display (so that it only stays idle until the display changes).
    pub(super) fn observe(&mut self, cpu: &Cpu, memory: &MemoryBus) -> Option<(u64, bool)> {
        let head = cpu.registers[15];
        let writes = memory.write_count();
        let flushes = memory.flush_count();
        let visible_events = memory.visible_event_count();
        let volatile_reads = memory.volatile_read_count();
        let display_reads = memory.display_read_count();
        let polls_display = self.display_reads != display_reads;
        let idle = self.armed
            && self.head == head
            && self.cpsr == cpu.cpsr
            && self.registers == cpu.registers
            && self.writes == writes
            // No event the CPU can see may have fired inside the iteration:
            // one that did changes what the next iteration sees (a flag set
            // by an interrupt) even though this one looked idle.
            && self.visible_events == visible_events
            // What a poll of the display reads changes at every scanline
            // event, so such a loop is only idle if none fell in the iteration.
            && (!polls_display || self.flushes == flushes)
            && self.volatile_reads == volatile_reads;
        let iteration = if idle {
            cpu.cycles
                .checked_sub(self.cycles)
                .map(|cycles| (cycles, polls_display))
        } else {
            None
        };

        self.armed = true;
        self.head = head;
        self.cpsr = cpu.cpsr;
        self.registers = cpu.registers;
        self.writes = writes;
        self.flushes = flushes;
        self.visible_events = visible_events;
        self.volatile_reads = volatile_reads;
        self.display_reads = display_reads;
        self.cycles = cpu.cycles;
        iteration
    }

    /// The skipped iterations leave the snapshot valid but its cycle stamp
    /// stale; re-arm from `cycles`.
    pub(super) fn skipped_to(&mut self, cycles: u64) {
        self.cycles = cycles;
    }

    pub(super) fn reset(&mut self) {
        self.armed = false;
    }
}
