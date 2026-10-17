//! A small binary codec for save states: little-endian integers and
//! length-prefixed byte runs. Each component writes its fields in a fixed
//! order and reads them back in the same order, hence the header's version.

use std::io;

fn corrupt(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

#[derive(Default)]
pub struct StateWriter {
    bytes: Vec<u8>,
}

impl StateWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn i8(&mut self, value: i8) {
        self.bytes.push(value as u8);
    }

    pub fn bool(&mut self, value: bool) {
        self.bytes.push(u8::from(value));
    }

    pub fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// A fixed-size run whose length the reader already knows.
    pub fn raw(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn u32s(&mut self, values: &[u32]) {
        for value in values {
            self.u32(*value);
        }
    }

    /// A run whose length is only known at runtime.
    pub fn sized(&mut self, bytes: &[u8]) {
        self.u32(bytes.len() as u32);
        self.raw(bytes);
    }
}

pub struct StateReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> StateReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn take(&mut self, length: usize) -> io::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| corrupt("save state is truncated"))?;
        let slice = &self.data[self.position..end];
        self.position = end;
        Ok(slice)
    }

    /// Bytes not yet read.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    pub fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn i8(&mut self) -> io::Result<i8> {
        Ok(self.u8()? as i8)
    }

    pub fn bool(&mut self) -> io::Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(corrupt("save state holds an invalid flag")),
        }
    }

    pub fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// Fills `destination` from the next `destination.len()` bytes.
    pub fn raw_into(&mut self, destination: &mut [u8]) -> io::Result<()> {
        destination.copy_from_slice(self.take(destination.len())?);
        Ok(())
    }

    pub fn u32s_into(&mut self, destination: &mut [u32]) -> io::Result<()> {
        for value in destination {
            *value = self.u32()?;
        }
        Ok(())
    }

    /// Reads a run written by [`StateWriter::sized`]; `limit` bounds the
    /// accepted length.
    pub fn sized(&mut self, limit: usize) -> io::Result<&'a [u8]> {
        let length = self.u32()? as usize;
        if length > limit {
            return Err(corrupt("save state holds an oversized block"));
        }
        self.take(length)
    }
}

/// FNV-1a, for the state's integrity checksum and ROM identity.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_in_order() {
        let mut writer = StateWriter::new();
        writer.u8(7);
        writer.i8(-3);
        writer.bool(true);
        writer.u16(0xBEEF);
        writer.u32(0xDEAD_BEEF);
        writer.i32(-123_456);
        writer.u64(u64::MAX - 1);
        writer.raw(&[1, 2, 3]);
        writer.sized(&[9, 8]);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        assert_eq!(reader.u8().unwrap(), 7);
        assert_eq!(reader.i8().unwrap(), -3);
        assert!(reader.bool().unwrap());
        assert_eq!(reader.u16().unwrap(), 0xBEEF);
        assert_eq!(reader.u32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(reader.i32().unwrap(), -123_456);
        assert_eq!(reader.u64().unwrap(), u64::MAX - 1);
        let mut raw = [0; 3];
        reader.raw_into(&mut raw).unwrap();
        assert_eq!(raw, [1, 2, 3]);
        assert_eq!(reader.sized(16).unwrap(), &[9, 8]);
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn truncated_or_invalid_data_is_an_error_not_a_panic() {
        let mut reader = StateReader::new(&[1, 2, 3]);
        assert!(reader.u32().is_err());
        assert!(StateReader::new(&[2]).bool().is_err());
        let mut writer = StateWriter::new();
        writer.sized(&[0; 10]);
        let bytes = writer.into_bytes();
        assert!(StateReader::new(&bytes).sized(4).is_err());
    }
}
