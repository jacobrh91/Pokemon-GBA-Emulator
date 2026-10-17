//! High-level emulation of the GBA BIOS entry points.
//!
//! No BIOS image is loaded, so the CPU intercepts the two places where
//! cartridge code transfers control into it, the same way mGBA's HLE BIOS does:
//!
//! * the SVC vector (`0x08`), which dispatches software-interrupt services, and
//! * the IRQ return stub (`BIOS_IRQ_RETURN`), which the BIOS IRQ trampoline
//!   hands to the game's handler as its link register.

use super::*;

/// Address the BIOS IRQ trampoline passes to the game handler as LR
/// (`ldmfd sp!, {r0-r3, r12, lr}; subs pc, lr, #4` lives here on hardware).
pub const BIOS_IRQ_RETURN: u32 = 0x0000_0138;
/// Interrupt flags the game IRQ handler accumulates for `IntrWait`.
const BIOS_IF: u32 = 0x0300_7FF8;
const REG_IME: u32 = 0x0400_0208;

const SWI_SOFT_RESET: u8 = 0x00;
const SWI_REGISTER_RAM_RESET: u8 = 0x01;
const SWI_HALT: u8 = 0x02;
const SWI_STOP: u8 = 0x03;
const SWI_INTR_WAIT: u8 = 0x04;
const SWI_VBLANK_INTR_WAIT: u8 = 0x05;
const SWI_DIV: u8 = 0x06;
const SWI_DIV_ARM: u8 = 0x07;
const SWI_SQRT: u8 = 0x08;
const SWI_ARCTAN: u8 = 0x09;
const SWI_ARCTAN2: u8 = 0x0A;
const SWI_BG_AFFINE_SET: u8 = 0x0E;
const SWI_OBJ_AFFINE_SET: u8 = 0x0F;
const SWI_CPU_SET: u8 = 0x0B;
const SWI_CPU_FAST_SET: u8 = 0x0C;
const SWI_GET_BIOS_CHECKSUM: u8 = 0x0D;
const SWI_LZ77_WRAM: u8 = 0x11;
const SWI_LZ77_VRAM: u8 = 0x12;
const SWI_RL_WRAM: u8 = 0x14;
const SWI_RL_VRAM: u8 = 0x15;

impl Cpu {
    /// Runs a BIOS HLE hook if the PC sits on one.
    ///
    /// Returns `Some(cycles)` when a hook consumed this step.
    pub fn step_bios_hle(&mut self, bus: &mut MemoryBus) -> io::Result<Option<u32>> {
        let pc = self.registers[15];
        // Both hooks live in the first 0x200 bytes of the BIOS region.
        if pc >= 0x200 {
            return Ok(None);
        }
        if self.mode() == MODE_SUPERVISOR && pc == VECTOR_SWI {
            let service = self.pending_swi_service(bus)?;
            self.handle_bios_swi(service, bus)?;
            return Ok(Some(0));
        }
        if self.mode() == MODE_IRQ && pc == BIOS_IRQ_RETURN && self.cpsr & T_FLAG == 0 {
            self.return_from_bios_irq(bus)?;
            return Ok(Some(0));
        }
        Ok(None)
    }

    /// Reads the comment field of the SWI that raised the pending SVC exception.
    fn pending_swi_service(&self, bus: &MemoryBus) -> io::Result<u8> {
        let thumb = self.saved_status()? & T_FLAG != 0;
        let width = if thumb { 2 } else { 4 };
        let address = self.registers[14].checked_sub(width).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid SWI return address")
        })?;
        let (valid, service) = if thumb {
            let opcode = bus.read_u16(address)?;
            (opcode & 0xFF00 == 0xDF00, opcode as u8)
        } else {
            let opcode = bus.read_u32(address)?;
            (opcode & 0x0F00_0000 == 0x0F00_0000, opcode as u8)
        };
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SVC vector return address does not point after a SWI",
            ));
        }
        Ok(service)
    }

    /// Emulates `ldmfd sp!, {r0-r3, r12, lr}; subs pc, lr, #4`.
    fn return_from_bios_irq(&mut self, bus: &MemoryBus) -> io::Result<()> {
        let mut sp = self.registers[13];
        for register in [0, 1, 2, 3, 12, 14] {
            self.registers[register] = bus.read_u32(sp)?;
            sp = sp.wrapping_add(4);
        }
        self.registers[13] = sp;
        let return_address = self.registers[14].wrapping_sub(4);
        let status = self.saved_status()?;
        self.set_cpsr(status)?;
        self.registers[15] = return_address;
        self.sequential_fetch = None;
        Ok(())
    }

    pub fn handle_bios_swi(&mut self, service: u8, bus: &mut MemoryBus) -> io::Result<()> {
        if self.mode() != MODE_SUPERVISOR || self.registers[15] != VECTOR_SWI {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "BIOS SWI handler invoked outside the SVC vector",
            ));
        }
        let saved_status = self.saved_status()?;
        let instruction_width = if saved_status & T_FLAG != 0 { 2 } else { 4 };
        let mut return_address = self.registers[14];

        match service {
            SWI_SOFT_RESET => {
                *self = Cpu::default();
                self.initialize_gba_startup(ROM_START)?;
                return Ok(());
            }
            SWI_REGISTER_RAM_RESET => bus.register_ram_reset(self.registers[0]),
            SWI_HALT | SWI_STOP => self.halted = true,
            SWI_INTR_WAIT | SWI_VBLANK_INTR_WAIT => {
                let (discard, mask) = if service == SWI_VBLANK_INTR_WAIT {
                    (true, 1)
                } else {
                    (self.registers[0] != 0, self.registers[1] as u16)
                };
                if !self.intr_waiting {
                    bus.write_u16(REG_IME, 1)?;
                    if discard {
                        let flags = bus.read_u16(BIOS_IF)?;
                        bus.write_u16(BIOS_IF, flags & !mask)?;
                    }
                }
                let flags = bus.read_u16(BIOS_IF)?;
                if flags & mask != 0 {
                    bus.write_u16(BIOS_IF, flags & !mask)?;
                    self.intr_waiting = false;
                } else {
                    // Re-execute the SWI once an interrupt wakes the CPU.
                    self.intr_waiting = true;
                    self.halted = true;
                    return_address = return_address.wrapping_sub(instruction_width);
                }
            }
            SWI_DIV | SWI_DIV_ARM => {
                let (numerator, denominator) = if service == SWI_DIV {
                    (self.registers[0] as i32, self.registers[1] as i32)
                } else {
                    (self.registers[1] as i32, self.registers[0] as i32)
                };
                let (quotient, remainder) = bios_divide(numerator, denominator);
                self.registers[0] = quotient as u32;
                self.registers[1] = remainder as u32;
                self.registers[3] = quotient.unsigned_abs();
            }
            SWI_SQRT => self.registers[0] = integer_sqrt(self.registers[0]),
            SWI_ARCTAN => self.registers[0] = arctan(self.registers[0] as i32) as u32,
            SWI_ARCTAN2 => {
                self.registers[0] =
                    arctan2(self.registers[0] as i32, self.registers[1] as i32) as u32;
            }
            SWI_CPU_SET => {
                bios_cpu_set(bus, self.registers[0], self.registers[1], self.registers[2])?
            }
            SWI_CPU_FAST_SET => {
                bios_cpu_fast_set(bus, self.registers[0], self.registers[1], self.registers[2])?
            }
            SWI_BG_AFFINE_SET => {
                bios_bg_affine_set(bus, self.registers[0], self.registers[1], self.registers[2])?
            }
            SWI_OBJ_AFFINE_SET => bios_obj_affine_set(
                bus,
                self.registers[0],
                self.registers[1],
                self.registers[2],
                self.registers[3],
            )?,
            SWI_GET_BIOS_CHECKSUM => self.registers[0] = 0xBAAE_187F,
            SWI_LZ77_WRAM | SWI_LZ77_VRAM => {
                let data = bios_lz77_decompress(bus, self.registers[0])?;
                write_decompressed(bus, self.registers[1], &data, service == SWI_LZ77_VRAM)?;
            }
            SWI_RL_WRAM | SWI_RL_VRAM => {
                let data = bios_rl_decompress(bus, self.registers[0])?;
                write_decompressed(bus, self.registers[1], &data, service == SWI_RL_VRAM)?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("unsupported BIOS software interrupt: {service:#04X}"),
                ));
            }
        }

        self.set_cpsr(saved_status)?;
        self.registers[15] = return_address;
        self.sequential_fetch = None;
        bus.set_bios_open_bus(BIOS_OPEN_BUS_AFTER_SWI);
        Ok(())
    }

    /// True when `IE & IF` is non-zero, which wakes a halted CPU even if
    /// IME or CPSR.I would block interrupt delivery.
    pub fn should_wake(&self, bus: &MemoryBus) -> bool {
        bus.interrupt_requested()
    }
}

/// Returns `(quotient, remainder)` using the BIOS conventions for the cases
/// real hardware cannot represent (mGBA's HLE makes the same choices).
fn bios_divide(numerator: i32, denominator: i32) -> (i32, i32) {
    if denominator == 0 {
        (if numerator < 0 { -1 } else { 1 }, numerator)
    } else if denominator == -1 && numerator == i32::MIN {
        (i32::MIN, 0)
    } else {
        (numerator / denominator, numerator % denominator)
    }
}

fn integer_sqrt(value: u32) -> u32 {
    let mut root = (f64::from(value).sqrt()) as u32;
    while u64::from(root) * u64::from(root) > u64::from(value) {
        root -= 1;
    }
    while u64::from(root + 1) * u64::from(root + 1) <= u64::from(value) {
        root += 1;
    }
    root
}

/// Fixed-point polynomial arctangent from the BIOS (input and output 1.14).
fn arctan(input: i32) -> i32 {
    let a = -((input.wrapping_mul(input)) >> 14);
    let mut b = ((0xA9 * a) >> 14) + 0x390;
    for constant in [0x91C, 0xFB6, 0x16AA, 0x2081, 0x3651, 0xA2F9] {
        b = ((b.wrapping_mul(a)) >> 14) + constant;
    }
    input.wrapping_mul(b) >> 16
}

fn arctan2(x: i32, y: i32) -> i32 {
    if y == 0 {
        return if x >= 0 { 0 } else { 0x8000 };
    }
    if x == 0 {
        return if y >= 0 { 0x4000 } else { 0xC000 };
    }
    let slope = |numerator: i32, denominator: i32| arctan(numerator.wrapping_shl(14) / denominator);
    if y >= 0 {
        if x >= 0 {
            if x >= y {
                return slope(y, x);
            }
        } else if -x >= y {
            return slope(y, x) + 0x8000;
        }
        0x4000 - slope(x, y)
    } else {
        if x <= 0 {
            if -x > -y {
                return slope(y, x) + 0x8000;
            }
        } else if x >= -y {
            return slope(y, x) + 0x10000;
        }
        0xC000 - slope(x, y)
    }
}

/// Sine of `index * 360 / 256` degrees in 1.14 fixed point, truncated like the
/// BIOS lookup table (which is not stored in the cartridge-visible address space).
fn sin_1_14(index: u8) -> i32 {
    (f64::sin(f64::from(index) * std::f64::consts::TAU / 256.0) * 16384.0) as i32
}

fn cos_1_14(index: u8) -> i32 {
    sin_1_14(index.wrapping_add(64))
}

/// ObjAffineSet: writes the rotation/scale matrix (pa, pb, pc, pd) as 8.8
/// fixed-point halfwords spaced `stride` bytes apart, one set per entry.
fn bios_obj_affine_set(
    bus: &mut MemoryBus,
    source: u32,
    destination: u32,
    count: u32,
    stride: u32,
) -> io::Result<()> {
    let (mut source, mut destination) = (source, destination);
    for _ in 0..count {
        let scale_x = i32::from(bus.read_u16(source)? as i16);
        let scale_y = i32::from(bus.read_u16(source.wrapping_add(2))? as i16);
        let angle = (bus.read_u16(source.wrapping_add(4))? >> 8) as u8;
        let (sin, cos) = (sin_1_14(angle), cos_1_14(angle));
        let matrix = [
            (scale_x * cos) >> 14,
            -((scale_x * sin) >> 14),
            (scale_y * sin) >> 14,
            (scale_y * cos) >> 14,
        ];
        for (index, value) in matrix.into_iter().enumerate() {
            let address = destination.wrapping_add(index as u32 * stride);
            bus.write_u16(address & !1, value as u16)?;
        }
        source = source.wrapping_add(8);
        destination = destination.wrapping_add(4 * stride);
    }
    Ok(())
}

/// BgAffineSet: 20-byte source entries become 16-byte (pa, pb, pc, pd, dx, dy)
/// background parameters.
fn bios_bg_affine_set(
    bus: &mut MemoryBus,
    source: u32,
    destination: u32,
    count: u32,
) -> io::Result<()> {
    let (mut source, mut destination) = (source, destination);
    for _ in 0..count {
        let origin_x = bus.read_u32(source)? as i32;
        let origin_y = bus.read_u32(source.wrapping_add(4))? as i32;
        let center_x = i32::from(bus.read_u16(source.wrapping_add(8))? as i16);
        let center_y = i32::from(bus.read_u16(source.wrapping_add(10))? as i16);
        let scale_x = i32::from(bus.read_u16(source.wrapping_add(12))? as i16);
        let scale_y = i32::from(bus.read_u16(source.wrapping_add(14))? as i16);
        let angle = (bus.read_u16(source.wrapping_add(16))? >> 8) as u8;
        let (sin, cos) = (sin_1_14(angle), cos_1_14(angle));

        let pa = (scale_x * cos) >> 14;
        let pb = -((scale_x * sin) >> 14);
        let pc = (scale_y * sin) >> 14;
        let pd = (scale_y * cos) >> 14;
        let dx = origin_x
            .wrapping_sub(center_x.wrapping_mul(pa))
            .wrapping_sub(center_y.wrapping_mul(pb));
        let dy = origin_y
            .wrapping_sub(center_x.wrapping_mul(pc))
            .wrapping_sub(center_y.wrapping_mul(pd));

        for (index, value) in [pa, pb, pc, pd].into_iter().enumerate() {
            bus.write_u16(
                destination.wrapping_add(index as u32 * 2) & !1,
                value as u16,
            )?;
        }
        bus.write_u32(destination.wrapping_add(8), dx as u32)?;
        bus.write_u32(destination.wrapping_add(12), dy as u32)?;
        source = source.wrapping_add(20);
        destination = destination.wrapping_add(16);
    }
    Ok(())
}

fn bios_cpu_set(
    bus: &mut MemoryBus,
    source: u32,
    destination: u32,
    control: u32,
) -> io::Result<()> {
    let word_transfer = control & (1 << 26) != 0;
    let fill = control & (1 << 24) != 0;
    let count = control & 0x001F_FFFF;
    let (mut source, mut destination) = (source, destination);

    for _ in 0..count {
        if word_transfer {
            let value = bus.read_u32(source & !3)?;
            bus.write_u32(destination & !3, value)?;
            if !fill {
                source = source.wrapping_add(4);
            }
            destination = destination.wrapping_add(4);
        } else {
            let value = bus.read_u16(source & !1)?;
            bus.write_u16(destination & !1, value)?;
            if !fill {
                source = source.wrapping_add(2);
            }
            destination = destination.wrapping_add(2);
        }
    }
    Ok(())
}

/// Word copy/fill in blocks of eight words; the count rounds up to a block.
fn bios_cpu_fast_set(
    bus: &mut MemoryBus,
    source: u32,
    destination: u32,
    control: u32,
) -> io::Result<()> {
    let fill = control & (1 << 24) != 0;
    let count = ((control & 0x001F_FFFF) + 7) & !7;
    let (mut source, mut destination) = (source & !3, destination & !3);

    for _ in 0..count {
        let value = bus.read_u32(source)?;
        bus.write_u32(destination, value)?;
        if !fill {
            source = source.wrapping_add(4);
        }
        destination = destination.wrapping_add(4);
    }
    Ok(())
}

/// Parses the 4-byte BIOS compression header: `(type nibble, output length)`.
fn compression_header(bus: &MemoryBus, source: u32) -> io::Result<(u8, usize)> {
    let header = bus.read_u32(source)?;
    Ok(((header >> 4) as u8 & 0x0F, (header >> 8) as usize))
}

/// Like the real BIOS, takes the length from the header and does not check its
/// type nibble: a game may hand over zeroed data (Emerald's player back picture
/// table does), which decompresses to nothing.
fn bios_lz77_decompress(bus: &MemoryBus, source: u32) -> io::Result<Vec<u8>> {
    let (_, output_length) = compression_header(bus, source)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_length)
        .map_err(io::Error::other)?;
    let mut input = checked_bios_address(source, 4)?;

    while output.len() < output_length {
        let flags = bus.read_u8(input)?;
        input = checked_bios_address(input, 1)?;
        for bit in (0..8).rev() {
            if output.len() == output_length {
                break;
            }
            if flags & (1 << bit) == 0 {
                output.push(bus.read_u8(input)?);
                input = checked_bios_address(input, 1)?;
                continue;
            }

            let first = bus.read_u8(input)?;
            let second = bus.read_u8(checked_bios_address(input, 1)?)?;
            input = checked_bios_address(input, 2)?;
            let copy_length = usize::from(first >> 4) + 3;
            let displacement = usize::from((u16::from(first & 0x0F) << 8) | u16::from(second)) + 1;
            if displacement > output.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "BIOS LZ77 back-reference precedes the decompressed output",
                ));
            }
            for _ in 0..copy_length {
                if output.len() == output_length {
                    break;
                }
                output.push(output[output.len() - displacement]);
            }
        }
    }
    Ok(output)
}

fn bios_rl_decompress(bus: &MemoryBus, source: u32) -> io::Result<Vec<u8>> {
    let (kind, output_length) = compression_header(bus, source)?;
    if kind != 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid BIOS run-length header type: {kind:#X}"),
        ));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_length)
        .map_err(io::Error::other)?;
    let mut input = checked_bios_address(source, 4)?;

    while output.len() < output_length {
        let flag = bus.read_u8(input)?;
        input = checked_bios_address(input, 1)?;
        let remaining = output_length - output.len();
        if flag & 0x80 != 0 {
            let length = usize::from(flag & 0x7F) + 3;
            let value = bus.read_u8(input)?;
            input = checked_bios_address(input, 1)?;
            output.extend(std::iter::repeat_n(value, length.min(remaining)));
        } else {
            let length = (usize::from(flag) + 1).min(remaining);
            for _ in 0..length {
                output.push(bus.read_u8(input)?);
                input = checked_bios_address(input, 1)?;
            }
        }
    }
    Ok(output)
}

/// Writes decompressed bytes, using 16-bit stores for the VRAM-safe variants.
fn write_decompressed(
    bus: &mut MemoryBus,
    destination: u32,
    data: &[u8],
    halfwords: bool,
) -> io::Result<()> {
    if !halfwords {
        for (index, value) in data.iter().enumerate() {
            bus.write_u8(checked_bios_address(destination, index as u32)?, *value)?;
        }
        return Ok(());
    }
    let destination = destination & !1;
    for (index, pair) in data.chunks(2).enumerate() {
        let address = checked_bios_address(destination, index as u32 * 2)?;
        let high = match pair.get(1) {
            Some(byte) => *byte,
            None => bus.read_u8(address + 1)?,
        };
        bus.write_u16(address, u16::from_le_bytes([pair[0], high]))?;
    }
    Ok(())
}

fn checked_bios_address(address: u32, offset: u32) -> io::Result<u32> {
    address.checked_add(offset).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "BIOS service address overflow",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divide_follows_bios_edge_cases() {
        assert_eq!(bios_divide(7, 2), (3, 1));
        assert_eq!(bios_divide(-7, 2), (-3, -1));
        assert_eq!(bios_divide(5, 0), (1, 5));
        assert_eq!(bios_divide(-5, 0), (-1, -5));
        assert_eq!(bios_divide(i32::MIN, -1), (i32::MIN, 0));
    }

    #[test]
    fn integer_sqrt_is_exact_at_boundaries() {
        for value in [0, 1, 2, 3, 4, 15, 16, 17, 0xFFFF_FFFF, 0xFFFE_0001] {
            let root = integer_sqrt(value);
            assert!(u64::from(root).pow(2) <= u64::from(value));
            assert!(u64::from(root + 1).pow(2) > u64::from(value));
        }
    }

    #[test]
    fn sine_table_matches_known_bios_entries() {
        assert_eq!(sin_1_14(0), 0);
        assert_eq!(sin_1_14(1), 0x192);
        assert_eq!(sin_1_14(2), 0x323);
        assert_eq!(sin_1_14(64), 0x4000);
        assert_eq!(cos_1_14(0), 0x4000);
        assert_eq!(sin_1_14(192), -0x4000);
    }

    #[test]
    fn arctan2_hits_the_cardinal_directions() {
        assert_eq!(arctan2(1, 0), 0);
        assert_eq!(arctan2(-1, 0), 0x8000);
        assert_eq!(arctan2(0, 1), 0x4000);
        assert_eq!(arctan2(0, -1), 0xC000);
        assert!((arctan2(1, 1) - 0x2000).abs() < 0x20);
    }
}
