//! Save-state support for the memory bus and everything it owns (RAM,
//! registers, timers, DMA, display timing, backup memory and the APU).

use super::display::DISPLAY_REGISTER_BYTES;
use super::*;
use crate::state::{StateReader, StateWriter};

/// Bounds a queued-scanline count read from a state.
const MAX_SCANLINES: usize = 512;

impl MemoryBus {
    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.raw(&self.ewram[..]);
        writer.raw(&self.iwram[..]);
        writer.raw(&self.io_regs[..]);
        writer.raw(&self.palette_ram[..]);
        writer.raw(&self.vram[..]);
        writer.raw(&self.oam[..]);
        self.savedata.save_state(writer);

        writer.u16(self.pressed_buttons);
        writer.bool(self.keypad_irq_active);
        writer.u32(self.display_cycles);
        writer.u32(self.pending_cycles);
        writer.u64(self.write_count);
        writer.u64(self.flush_count);
        writer.u32(self.volatile_reads.get());
        writer.u32(self.cycles_until_event);
        for timer in &self.timers {
            writer.u16(timer.reload);
            writer.u16(timer.counter);
            writer.u16(timer.control);
            writer.u16(timer.prescaler_cycles);
        }
        for channel in &self.dma {
            channel.save_state(writer);
        }
        writer.u32(self.dma_cycles);
        self.apu.save_state(writer);
        writer.u32(self.open_bus);
        writer.u32(self.bios_open_bus);
        writer.u32(self.scanlines.len() as u32);
        for snapshot in &self.scanlines {
            writer.u32(snapshot.line as u32);
            writer.bool(snapshot.affine_reload[0]);
            writer.bool(snapshot.affine_reload[1]);
            writer.raw(&snapshot.registers);
        }
        writer.bool(self.affine_reload[0]);
        writer.bool(self.affine_reload[1]);
    }

    pub(crate) fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        reader.raw_into(&mut self.ewram[..])?;
        reader.raw_into(&mut self.iwram[..])?;
        reader.raw_into(&mut self.io_regs[..])?;
        reader.raw_into(&mut self.palette_ram[..])?;
        reader.raw_into(&mut self.vram[..])?;
        reader.raw_into(&mut self.oam[..])?;
        self.savedata.load_state(reader)?;

        self.pressed_buttons = reader.u16()?;
        self.keypad_irq_active = reader.bool()?;
        self.display_cycles = reader.u32()?;
        self.pending_cycles = reader.u32()?;
        self.write_count = reader.u64()?;
        self.flush_count = reader.u64()?;
        self.volatile_reads.set(reader.u32()?);
        self.cycles_until_event = reader.u32()?;
        for timer in &mut self.timers {
            timer.reload = reader.u16()?;
            timer.counter = reader.u16()?;
            timer.control = reader.u16()?;
            timer.prescaler_cycles = reader.u16()?;
        }
        for channel in &mut self.dma {
            channel.load_state(reader)?;
        }
        self.dma_cycles = reader.u32()?;
        self.apu.load_state(reader)?;
        self.open_bus = reader.u32()?;
        self.bios_open_bus = reader.u32()?;
        let queued = reader.u32()? as usize;
        if queued > MAX_SCANLINES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "save state holds too many queued scanlines",
            ));
        }
        self.scanlines.clear();
        for _ in 0..queued {
            let line = reader.u32()? as usize;
            let affine_reload = [reader.bool()?, reader.bool()?];
            let mut registers = [0; DISPLAY_REGISTER_BYTES];
            reader.raw_into(&mut registers)?;
            self.scanlines.push(ScanlineSnapshot {
                line,
                affine_reload,
                registers,
            });
        }
        self.affine_reload = [reader.bool()?, reader.bool()?];
        // The Game Pak timings are derived from WAITCNT.
        self.refresh_waitcnt();
        Ok(())
    }
}
