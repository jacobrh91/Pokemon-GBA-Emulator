//! Save-state support for the CPU.

use super::Cpu;
use crate::state::{StateReader, StateWriter};
use std::io;

impl Cpu {
    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.u32s(&self.registers);
        writer.u32(self.cpsr);
        writer.u64(self.cycles);
        writer.u32s(&self.banked_user);
        writer.u32s(&self.banked_fiq);
        for pair in &self.banked_modes {
            writer.u32s(pair);
        }
        writer.u32s(&self.saved_status);
        match self.sequential_fetch {
            Some((address, thumb)) => {
                writer.bool(true);
                writer.u32(address);
                writer.bool(thumb);
            }
            None => writer.bool(false),
        }
        writer.u32(self.data_wait);
        writer.bool(self.halted);
        writer.bool(self.intr_waiting);
    }

    pub(crate) fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        reader.u32s_into(&mut self.registers)?;
        self.cpsr = reader.u32()?;
        self.cycles = reader.u64()?;
        reader.u32s_into(&mut self.banked_user)?;
        reader.u32s_into(&mut self.banked_fiq)?;
        for pair in &mut self.banked_modes {
            reader.u32s_into(pair)?;
        }
        reader.u32s_into(&mut self.saved_status)?;
        self.sequential_fetch = if reader.bool()? {
            Some((reader.u32()?, reader.bool()?))
        } else {
            None
        };
        self.data_wait = reader.u32()?;
        self.halted = reader.bool()?;
        self.intr_waiting = reader.bool()?;
        Ok(())
    }
}
