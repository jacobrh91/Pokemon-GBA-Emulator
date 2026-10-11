//! Save-state support for the PPU's internal (non-register) state.
//!
//! The framebuffer is not saved: it is redrawn from VRAM and the registers on
//! the next frame.

use super::Ppu;
use crate::state::{StateReader, StateWriter};
use std::io;

impl Ppu {
    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        for reference in &self.affine_reference {
            writer.i32(reference[0]);
            writer.i32(reference[1]);
        }
        for loaded in &self.affine_loaded_from {
            match loaded {
                Some([x, y]) => {
                    writer.bool(true);
                    writer.i32(*x);
                    writer.i32(*y);
                }
                None => writer.bool(false),
            }
        }
    }

    pub(crate) fn load_state(&mut self, reader: &mut StateReader) -> io::Result<()> {
        for reference in &mut self.affine_reference {
            reference[0] = reader.i32()?;
            reference[1] = reader.i32()?;
        }
        for loaded in &mut self.affine_loaded_from {
            *loaded = if reader.bool()? {
                Some([reader.i32()?, reader.i32()?])
            } else {
                None
            };
        }
        Ok(())
    }
}
