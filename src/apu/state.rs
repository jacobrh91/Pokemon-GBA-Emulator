//! Save-state support for the APU.

use super::{Apu, DirectSound, FifoStats, Noise, Square, Wave};
use crate::state::{StateReader, StateWriter};
use std::io;

const MAX_FIFO_BYTES: usize = 64;
/// The wave channel plays 32 samples per bank, or 64 across both banks.
const MAX_WAVE_POSITION: u32 = 64;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

impl Square {
    fn save_state(&self, writer: &mut StateWriter) {
        writer.bool(self.enabled);
        writer.u16(self.length);
        writer.u8(self.volume);
        writer.u8(self.envelope_timer);
        writer.u16(self.frequency);
        writer.u32(self.phase_cycles);
        writer.u8(self.duty_position as u8);
        writer.u16(self.sweep_shadow);
        writer.u8(self.sweep_timer);
        writer.bool(self.sweep_enabled);
    }

    fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        self.enabled = reader.bool()?;
        self.length = reader.u16()?;
        self.volume = reader.u8()?;
        self.envelope_timer = reader.u8()?;
        self.frequency = reader.u16()?;
        self.phase_cycles = reader.u32()?;
        let duty_position = reader.u8()?;
        if duty_position >= 8 {
            return Err(invalid("save state holds an invalid duty position"));
        }
        self.duty_position = usize::from(duty_position);
        self.sweep_shadow = reader.u16()?;
        self.sweep_timer = reader.u8()?;
        self.sweep_enabled = reader.bool()?;
        Ok(())
    }
}

impl Wave {
    fn save_state(&self, writer: &mut StateWriter) {
        writer.bool(self.enabled);
        writer.u16(self.length);
        writer.u32(self.position as u32);
        writer.u32(self.phase_cycles);
        writer.u16(self.frequency);
    }

    fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        self.enabled = reader.bool()?;
        self.length = reader.u16()?;
        let position = reader.u32()?;
        if position >= MAX_WAVE_POSITION {
            return Err(invalid("save state holds an invalid wave position"));
        }
        self.position = position as usize;
        self.phase_cycles = reader.u32()?;
        self.frequency = reader.u16()?;
        Ok(())
    }
}

impl Noise {
    fn save_state(&self, writer: &mut StateWriter) {
        writer.bool(self.enabled);
        writer.u16(self.length);
        writer.u8(self.volume);
        writer.u8(self.envelope_timer);
        writer.u16(self.lfsr);
        writer.u32(self.phase_cycles);
    }

    fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        self.enabled = reader.bool()?;
        self.length = reader.u16()?;
        self.volume = reader.u8()?;
        self.envelope_timer = reader.u8()?;
        self.lfsr = reader.u16()?;
        self.phase_cycles = reader.u32()?;
        Ok(())
    }
}

impl DirectSound {
    fn save_state(&self, writer: &mut StateWriter) {
        writer.u32(self.fifo.len() as u32);
        for sample in &self.fifo {
            writer.i8(*sample);
        }
        writer.i8(self.latch);
    }

    fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        let length = reader.u32()? as usize;
        if length > MAX_FIFO_BYTES {
            return Err(invalid("save state holds an oversized sound FIFO"));
        }
        self.fifo.clear();
        for _ in 0..length {
            self.fifo.push_back(reader.i8()?);
        }
        self.latch = reader.i8()?;
        Ok(())
    }
}

impl Apu {
    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        for square in &self.square {
            square.save_state(writer);
        }
        self.wave.save_state(writer);
        self.noise.save_state(writer);
        for direct in &self.direct {
            direct.save_state(writer);
        }
        for bank in &self.wave_ram {
            writer.raw(bank);
        }
        writer.u32(self.cycle_accumulator);
        writer.u32(self.sequencer_samples);
        writer.u8(self.sequencer_step);
        writer.u64(self.stats.samples_played);
        writer.u64(self.stats.underruns);
        writer.u64(self.stats.overflow_bytes);
    }

    /// Mixed samples not yet taken are dropped: they belong to the timeline
    /// being replaced.
    pub(crate) fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        for square in &mut self.square {
            square.load_state(reader)?;
        }
        self.wave.load_state(reader)?;
        self.noise.load_state(reader)?;
        for direct in &mut self.direct {
            direct.load_state(reader)?;
        }
        for bank in &mut self.wave_ram {
            reader.raw_into(bank)?;
        }
        self.cycle_accumulator = reader.u32()?;
        self.sequencer_samples = reader.u32()?;
        self.sequencer_step = reader.u8()?;
        self.stats = FifoStats {
            samples_played: reader.u64()?,
            underruns: reader.u64()?,
            overflow_bytes: reader.u64()?,
        };
        self.output.clear();
        Ok(())
    }
}
