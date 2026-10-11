//! Whole-console save states.
//!
//! Layout: an 8-byte magic, a format version, the ROM's length and header
//! hash (so a state is only loaded into the game that made it), the payload
//! length and an FNV-1a checksum of the payload, then the payload itself. The
//! header and checksum are verified before anything is modified, so a stale or
//! damaged file is rejected without disturbing the running game.

use super::{Emulator, NO_HOOK};
use crate::state::{StateReader, StateWriter, fnv1a};
use std::io;

const MAGIC: &[u8; 8] = b"GBAEMUST";
/// Bump when the layout of any component's state changes.
const VERSION: u32 = 1;
/// The cartridge header, whose hash identifies the game.
const ROM_HEADER_BYTES: usize = 0xC0;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

impl Emulator {
    fn rom_identity(&self) -> (u64, u64) {
        let rom = &self.memory.rom;
        let header = &rom[..rom.len().min(ROM_HEADER_BYTES)];
        (rom.len() as u64, fnv1a(header))
    }

    /// Serializes the whole console. Call between frames.
    pub fn save_state(&self) -> Vec<u8> {
        let mut payload = StateWriter::new();
        self.cpu.save_state(&mut payload);
        self.memory.save_state(&mut payload);
        self.ppu.save_state(&mut payload);
        payload.bool(self.frame_ready);
        let payload = payload.into_bytes();

        let (rom_length, rom_hash) = self.rom_identity();
        let mut out = StateWriter::new();
        out.raw(MAGIC);
        out.u32(VERSION);
        out.u64(rom_length);
        out.u64(rom_hash);
        out.u64(payload.len() as u64);
        out.u64(fnv1a(&payload));
        out.raw(&payload);
        out.into_bytes()
    }

    /// Restores a state made by [`Emulator::save_state`] for the same ROM.
    ///
    /// Nothing is modified unless the header, ROM identity and checksum all
    /// check out. The backup memory (battery save) is restored with the rest
    /// and marked changed, so it is flushed to the save file.
    pub fn load_state(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut header = StateReader::new(bytes);
        let mut magic = [0; 8];
        header.raw_into(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid("not a save state"));
        }
        if header.u32()? != VERSION {
            return Err(invalid("save state was made by an incompatible version"));
        }
        let (rom_length, rom_hash) = self.rom_identity();
        if header.u64()? != rom_length || header.u64()? != rom_hash {
            return Err(invalid("save state belongs to a different game"));
        }
        let payload_length = header.u64()?;
        let checksum = header.u64()?;
        if payload_length != header.remaining() as u64 {
            return Err(invalid("save state is truncated or has trailing data"));
        }
        let payload = &bytes[bytes.len() - header.remaining()..];
        if fnv1a(payload) != checksum {
            return Err(invalid("save state is corrupt (checksum mismatch)"));
        }

        let mut reader = StateReader::new(payload);
        self.cpu.load_state(&mut reader)?;
        self.memory.load_state(&mut reader)?;
        self.ppu.load_state(&mut reader)?;
        self.frame_ready = reader.bool()?;
        if reader.remaining() != 0 {
            return Err(invalid("save state has unread data"));
        }
        // Idle-loop history belongs to the timeline being replaced.
        self.idle.reset();
        // A state may be loaded in the middle of `CreateWildMon`'s run.
        self.wild_return = NO_HOOK;
        Ok(())
    }
}
