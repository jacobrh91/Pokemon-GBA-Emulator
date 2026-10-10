use memmap2::Mmap;
use std::fs::File;
use std::io;

pub const ROM_START: u32 = 0x0800_0000;
pub const ROM_END: u32 = 0x0DFF_FFFF;
pub const EWRAM_START: u32 = 0x0200_0000;
pub const EWRAM_MASK: u32 = 0x0003_FFFF;
pub const IWRAM_START: u32 = 0x0300_0000;
pub const IWRAM_MASK: u32 = 0x0000_7FFF;
pub const PALETTE_RAM_START: u32 = 0x0500_0000;
pub const VRAM_START: u32 = 0x0600_0000;
pub const OAM_START: u32 = 0x0700_0000;
pub const IO_REG_START: u32 = 0x0400_0000;
pub const IO_REG_END: u32 = 0x0400_03FF;
const TIMER_BASE: usize = 0x100;
const INTERRUPT_ENABLE: usize = 0x200;
const INTERRUPT_REQUEST: usize = 0x202;
const INTERRUPT_MASTER_ENABLE: usize = 0x208;
const DISPSTAT: usize = 0x004;
const VCOUNT: usize = 0x006;
const KEYINPUT: usize = 0x130;
const CYCLES_PER_SCANLINE: u32 = 1232;
const VBLANK_START_SCANLINE: u32 = 160;
const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * 228;
const VBLANK_START_CYCLES: u32 = CYCLES_PER_SCANLINE * VBLANK_START_SCANLINE;

#[derive(Default)]
struct Timer {
    reload: u16,
    counter: u16,
    control: u16,
    prescaler_cycles: u16,
}

#[derive(Clone, Copy)]
enum Region {
    Rom(usize),
    Ewram(usize),
    Iwram(usize),
    Io(usize),
    PaletteRam(usize),
    Vram(usize),
    Oam(usize),
    SaveRam(usize),
}

pub struct MemoryBus {
    pub rom: Mmap,
    pub ewram: Box<[u8; 256 * 1024]>,
    pub iwram: Box<[u8; 32 * 1024]>,
    pub io_regs: Box<[u8; 1024]>,
    pub palette_ram: Box<[u8; 1024]>,
    pub vram: Box<[u8; 96 * 1024]>,
    pub oam: Box<[u8; 1024]>,
    pub save_ram: Box<[u8; 64 * 1024]>,
    key_input: u16,
    display_cycles: u32,
    timers: [Timer; 4],
}

impl MemoryBus {
    pub fn new(rom_path: &str) -> io::Result<Self> {
        let file = File::open(rom_path)?;
        // Safety: The ROM mapping is read-only and assumes the file is not modified during use.
        let rom = unsafe { Mmap::map(&file)? };

        Ok(Self {
            rom,
            ewram: Box::new([0; 256 * 1024]),
            iwram: Box::new([0; 32 * 1024]),
            io_regs: Box::new([0; 1024]),
            palette_ram: Box::new([0; 1024]),
            vram: Box::new([0; 96 * 1024]),
            oam: Box::new([0; 1024]),
            save_ram: Box::new([0xFF; 64 * 1024]),
            key_input: 0x03FF,
            display_cycles: 0,
            timers: std::array::from_fn(|_| Timer::default()),
        })
    }

    pub fn read_u8(&self, address: u32) -> io::Result<u8> {
        match self.resolve(address)? {
            Region::Rom(offset) => self.rom.get(offset).copied().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read address is beyond the mapped ROM",
                )
            }),
            Region::Ewram(offset) => Ok(self.ewram[offset]),
            Region::Iwram(offset) => Ok(self.iwram[offset]),
            Region::Io(offset) => Ok(self.read_io_byte(offset)),
            Region::PaletteRam(offset) => Ok(self.palette_ram[offset]),
            Region::Vram(offset) => Ok(self.vram[offset]),
            Region::Oam(offset) => Ok(self.oam[offset]),
            Region::SaveRam(offset) => Ok(self.save_ram[offset]),
        }
    }

    pub fn read_u16(&self, address: u32) -> io::Result<u16> {
        let low = self.read_u8(address)?;
        let high_address = address.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "16-bit read address overflow",
            )
        })?;
        let high = self.read_u8(high_address)?;
        Ok(u16::from_le_bytes([low, high]))
    }

    pub fn read_u32(&self, address: u32) -> io::Result<u32> {
        let mut bytes = [0; 4];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let byte_address = address.checked_add(index as u32).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    "32-bit read address overflow",
                )
            })?;
            *byte = self.read_u8(byte_address)?;
        }
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn write_u8(&mut self, address: u32, value: u8) -> io::Result<()> {
        if (ROM_START..=ROM_END).contains(&address) {
            return Ok(());
        }
        let region = self.resolve(address)?;
        self.write_region(region, value)
    }

    pub fn write_u16(&mut self, address: u32, value: u16) -> io::Result<()> {
        if (ROM_START..=ROM_END).contains(&address) {
            return Ok(());
        }
        let bytes = value.to_le_bytes();
        let high_address = address.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "16-bit write address overflow",
            )
        })?;
        let regions = [self.resolve(address)?, self.resolve(high_address)?];
        if regions
            .iter()
            .any(|region| matches!(region, Region::Rom(_)))
        {
            return Ok(());
        }
        for (region, byte) in regions.into_iter().zip(bytes) {
            self.write_region(region, byte)?;
        }
        Ok(())
    }

    pub fn transfer_cycles(&self, address: u32, width: u8, sequential: bool) -> io::Result<u32> {
        let region = self.resolve(address)?;
        let halfword_cycles = match region {
            Region::Iwram(_)
            | Region::Io(_)
            | Region::PaletteRam(_)
            | Region::Vram(_)
            | Region::Oam(_) => 1,
            Region::Ewram(_) => 3,
            Region::SaveRam(_) => {
                let waitcnt = u16::from_le_bytes([self.io_regs[0x204], self.io_regs[0x205]]);
                [4, 3, 2, 8][(waitcnt & 0x3) as usize]
            }
            Region::Rom(_) => {
                let waitcnt = u16::from_le_bytes([self.io_regs[0x204], self.io_regs[0x205]]);
                let wait_state = match address {
                    0x0800_0000..=0x09FF_FFFF => 0,
                    0x0A00_0000..=0x0BFF_FFFF => 1,
                    _ => 2,
                };
                let first_bit = [2, 5, 8][wait_state];
                let sequential_bit = [4, 7, 10][wait_state];
                if sequential {
                    let selected = (waitcnt >> sequential_bit) & 1 != 0;
                    match wait_state {
                        0 => [2, 1][usize::from(selected)],
                        1 => [4, 1][usize::from(selected)],
                        _ => [8, 1][usize::from(selected)],
                    }
                } else {
                    let selection = ((waitcnt >> first_bit) & 3) as usize;
                    [4, 3, 2, 8][selection]
                }
            }
        };
        Ok(match width {
            1 | 2 => halfword_cycles,
            4 if matches!(region, Region::Iwram(_)) => 1,
            4 => {
                let second_address = address.checked_add(2).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::AddrNotAvailable,
                        "word access address overflow",
                    )
                })?;
                halfword_cycles + self.transfer_cycles(second_address, 2, true)?
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unsupported bus access width: {width}"),
                ));
            }
        })
    }

    pub fn irq_pending(&self) -> bool {
        let ie = u16::from_le_bytes([
            self.io_regs[INTERRUPT_ENABLE],
            self.io_regs[INTERRUPT_ENABLE + 1],
        ]);
        let requested = u16::from_le_bytes([
            self.io_regs[INTERRUPT_REQUEST],
            self.io_regs[INTERRUPT_REQUEST + 1],
        ]);
        self.io_regs[INTERRUPT_MASTER_ENABLE] & 1 != 0 && ie & requested != 0
    }

    pub fn advance(&mut self, cycles: u32) {
        let old_display_cycles = self.display_cycles;
        let distance_to_vblank = if old_display_cycles < VBLANK_START_CYCLES {
            VBLANK_START_CYCLES - old_display_cycles
        } else {
            CYCLES_PER_FRAME - old_display_cycles + VBLANK_START_CYCLES
        };
        if u64::from(cycles) >= u64::from(distance_to_vblank)
            && self.io_regs[DISPSTAT] & (1 << 3) != 0
        {
            self.io_regs[INTERRUPT_REQUEST] |= 1;
        }
        self.display_cycles = ((u64::from(self.display_cycles) + u64::from(cycles))
            % u64::from(CYCLES_PER_FRAME)) as u32;

        let mut cascade_ticks = 0;
        for index in 0..4 {
            let control = self.timers[index].control;
            if control & (1 << 7) == 0 {
                cascade_ticks = 0;
                continue;
            }
            if index > 0 && control & (1 << 2) != 0 {
                let source_ticks = cascade_ticks;
                cascade_ticks = self.advance_timer(index, source_ticks);
                continue;
            }
            let prescaler = [1_u64, 64, 256, 1024][(control & 3) as usize];
            let accumulated = u64::from(self.timers[index].prescaler_cycles) + u64::from(cycles);
            let ticks = accumulated / prescaler;
            self.timers[index].prescaler_cycles = (accumulated % prescaler) as u16;
            cascade_ticks = self.advance_timer(index, ticks);
        }
    }

    pub fn set_key_input(&mut self, pressed_keys: u16) {
        self.key_input = pressed_keys & 0x03FF;
    }

    pub fn register_ram_reset(&mut self, flags: u32) {
        if flags & 1 != 0 {
            self.ewram.fill(0);
        }
        if flags & 2 != 0 {
            self.iwram.fill(0);
        }
        if flags & 4 != 0 {
            self.palette_ram.fill(0);
        }
        if flags & 8 != 0 {
            self.vram.fill(0);
        }
        if flags & 0x10 != 0 {
            self.oam.fill(0);
        }
        if flags & 0x80 != 0 {
            self.io_regs.fill(0);
            self.timers = std::array::from_fn(|_| Timer::default());
        }
    }

    pub fn write_u32(&mut self, address: u32, value: u32) -> io::Result<()> {
        if (ROM_START..=ROM_END).contains(&address) {
            return Ok(());
        }
        let bytes = value.to_le_bytes();
        let mut regions = [Region::Ewram(0); 4];
        for (index, region) in regions.iter_mut().enumerate() {
            let byte_address = address.checked_add(index as u32).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    "32-bit write address overflow",
                )
            })?;
            *region = self.resolve(byte_address)?;
        }
        if regions
            .iter()
            .any(|region| matches!(region, Region::Rom(_)))
        {
            return Ok(());
        }

        for (region, byte) in regions.into_iter().zip(bytes) {
            self.write_region(region, byte)?;
        }
        Ok(())
    }

    fn resolve(&self, address: u32) -> io::Result<Region> {
        match address {
            EWRAM_START..=0x02FF_FFFF => Ok(Region::Ewram((address & EWRAM_MASK) as usize)),
            IWRAM_START..=0x03FF_FFFF => Ok(Region::Iwram((address & IWRAM_MASK) as usize)),
            IO_REG_START..=IO_REG_END => Ok(Region::Io((address - IO_REG_START) as usize)),
            PALETTE_RAM_START..=0x05FF_FFFF => Ok(Region::PaletteRam((address & 0x3FF) as usize)),
            VRAM_START..=0x06FF_FFFF => {
                let offset = address & 0x1_FFFF;
                let offset = if offset >= 0x1_8000 {
                    offset - 0x8000
                } else {
                    offset
                };
                Ok(Region::Vram(offset as usize))
            }
            OAM_START..=0x07FF_FFFF => Ok(Region::Oam((address & 0x3FF) as usize)),
            0x0E00_0000..=0x0EFF_FFFF => Ok(Region::SaveRam((address & 0xFFFF) as usize)),
            ROM_START..=ROM_END => {
                let mut offset = (address & 0x01FF_FFFF) as usize;
                if self.rom.len().is_power_of_two() {
                    offset &= self.rom.len() - 1;
                }
                Ok(Region::Rom(offset))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                format!("unmapped memory address: {address:#010X}"),
            )),
        }
    }

    fn write_region(&mut self, region: Region, value: u8) -> io::Result<()> {
        match region {
            Region::Ewram(offset) => self.ewram[offset] = value,
            Region::Iwram(offset) => self.iwram[offset] = value,
            Region::PaletteRam(offset) => self.palette_ram[offset] = value,
            Region::Vram(offset) => self.vram[offset] = value,
            Region::Oam(offset) => self.oam[offset] = value,
            Region::SaveRam(offset) => self.save_ram[offset] = value,
            Region::Io(offset) => {
                if (VCOUNT..VCOUNT + 2).contains(&offset)
                    || (KEYINPUT..KEYINPUT + 2).contains(&offset)
                {
                    return Ok(());
                }
                if (INTERRUPT_REQUEST..INTERRUPT_REQUEST + 2).contains(&offset) {
                    self.io_regs[offset] &= !value;
                } else {
                    self.io_regs[offset] = value;
                }
                self.update_timer_register(offset);
            }
            Region::Rom(_) => {}
        }
        Ok(())
    }

    fn read_io_byte(&self, offset: usize) -> u8 {
        if offset == DISPSTAT {
            let scanline = self.display_cycles / CYCLES_PER_SCANLINE;
            return (self.io_regs[offset] & !1) | u8::from(scanline >= VBLANK_START_SCANLINE);
        }
        if offset == VCOUNT {
            return (self.display_cycles / CYCLES_PER_SCANLINE) as u8;
        }
        if offset == VCOUNT + 1 {
            return 0;
        }
        if offset == KEYINPUT {
            return self.key_input as u8;
        }
        if offset == KEYINPUT + 1 {
            return (self.key_input >> 8) as u8;
        }
        if (TIMER_BASE..TIMER_BASE + 16).contains(&offset) {
            let timer_index = (offset - TIMER_BASE) / 4;
            let timer_register_offset = (offset - TIMER_BASE) % 4;
            if timer_register_offset < 2 {
                let counter = self.timers[timer_index].counter;
                return if timer_register_offset == 0 {
                    counter as u8
                } else {
                    (counter >> 8) as u8
                };
            }
        }
        self.io_regs[offset]
    }

    fn update_timer_register(&mut self, offset: usize) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);

    fn bus_with_rom(contents: &[u8]) -> MemoryBus {
        let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "gba-emulator-test-rom-{}-{id}.bin",
            std::process::id()
        ));
        fs::write(&path, contents).unwrap();
        let bus = MemoryBus::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        bus
    }

    #[test]
    fn reads_and_writes_ram_and_io_bytes_and_words() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u8(EWRAM_START, 0x12).unwrap();
        bus.write_u32(IWRAM_START, 0x7856_3412).unwrap();
        bus.write_u32(IO_REG_START, 0xDEAD_BEEF).unwrap();

        assert_eq!(bus.read_u8(EWRAM_START).unwrap(), 0x12);
        assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0x3412);
        assert_eq!(bus.read_u32(IWRAM_START).unwrap(), 0x7856_3412);
        assert_eq!(bus.read_u32(IO_REG_START).unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn save_ram_is_writable_and_mirrors_across_the_backup_memory_window() {
        let mut bus = bus_with_rom(&[0; 4]);
        let address = 0x0E00_5555;

        assert_eq!(bus.read_u8(address).unwrap(), 0xFF);
        bus.write_u8(address, 0xA5).unwrap();

        assert_eq!(bus.read_u8(0x0E01_5555).unwrap(), 0xA5);
        assert_eq!(bus.transfer_cycles(address, 1, false).unwrap(), 4);
    }

    #[test]
    fn ram_words_wrap_at_mirrored_bank_boundaries() {
        let mut bus = bus_with_rom(&[0; 4]);
        let address = EWRAM_START + EWRAM_MASK - 1;

        bus.write_u32(address, 0x7856_3412).unwrap();

        assert_eq!(bus.read_u32(address).unwrap(), 0x7856_3412);
        assert_eq!(bus.ewram[0], 0x56);
        assert_eq!(bus.ewram[1], 0x78);
    }

    #[test]
    fn ram_palette_oam_and_vram_mirror_aliases_share_storage() {
        let mut bus = bus_with_rom(&[0; 4]);

        bus.write_u8(EWRAM_START, 0x11).unwrap();
        bus.write_u8(IWRAM_START, 0x22).unwrap();
        bus.write_u8(PALETTE_RAM_START, 0x33).unwrap();
        bus.write_u8(VRAM_START + 0x1_0000, 0x44).unwrap();
        bus.write_u8(OAM_START, 0x55).unwrap();

        assert_eq!(bus.read_u8(EWRAM_START + 0x0040_0000).unwrap(), 0x11);
        assert_eq!(bus.read_u8(IWRAM_START + 0x0000_8000).unwrap(), 0x22);
        assert_eq!(bus.read_u8(PALETTE_RAM_START + 0x400).unwrap(), 0x33);
        assert_eq!(bus.read_u8(VRAM_START + 0x1_8000).unwrap(), 0x44);
        assert_eq!(bus.read_u8(OAM_START + 0x400).unwrap(), 0x55);
    }

    #[test]
    fn graphics_memory_regions_are_mapped_and_vram_mirrors_correctly() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(PALETTE_RAM_START, 0x1234).unwrap();
        bus.write_u16(VRAM_START + 0x1_8000, 0x5678).unwrap();
        bus.write_u16(OAM_START, 0x9ABC).unwrap();

        assert_eq!(bus.read_u16(PALETTE_RAM_START).unwrap(), 0x1234);
        assert_eq!(bus.read_u16(VRAM_START + 0x1_0000,).unwrap(), 0x5678);
        assert_eq!(bus.read_u16(OAM_START).unwrap(), 0x9ABC);
    }

    #[test]
    fn rom_writes_are_ignored_and_out_of_range_accesses_are_reported() {
        let mut bus = bus_with_rom(&[0x12, 0x34, 0x56, 0x78]);

        assert_eq!(bus.read_u32(ROM_START).unwrap(), 0x7856_3412);
        bus.write_u16(ROM_START + 0x01FE_2FFE, 0x1234).unwrap();
        bus.write_u8(ROM_START, 0).unwrap();
        bus.write_u16(ROM_START + 1, 0).unwrap();
        bus.write_u32(ROM_START, 0).unwrap();
        bus.write_u16(ROM_START + 0x01FE_2FFE, 0x1234).unwrap();
        assert!(
            bus.transfer_cycles(ROM_START + 0x01FE_2FFE, 2, false)
                .is_ok()
        );
        assert_eq!(bus.read_u32(ROM_START).unwrap(), 0x7856_3412);
        assert_eq!(bus.read_u8(ROM_START + 4).unwrap(), 0x12);
        assert_eq!(
            bus.read_u8(0).unwrap_err().kind(),
            io::ErrorKind::AddrNotAvailable
        );
        assert_eq!(
            bus.read_u8(IO_REG_END + 1).unwrap_err().kind(),
            io::ErrorKind::AddrNotAvailable
        );
        assert_eq!(bus.read_u8(ROM_END).unwrap(), 0x78);
    }

    #[test]
    fn dispstat_vblank_flag_tracks_frame_cycles() {
        let mut bus = bus_with_rom(&[0; 4]);

        bus.advance(CYCLES_PER_SCANLINE * (VBLANK_START_SCANLINE - 1));
        assert_eq!(bus.read_u16(IO_REG_START + DISPSTAT as u32).unwrap() & 1, 0);

        bus.advance(CYCLES_PER_SCANLINE);
        assert_eq!(bus.read_u16(IO_REG_START + DISPSTAT as u32).unwrap() & 1, 1);

        bus.advance(CYCLES_PER_FRAME - CYCLES_PER_SCANLINE * VBLANK_START_SCANLINE);
        assert_eq!(bus.read_u16(IO_REG_START + DISPSTAT as u32).unwrap() & 1, 0);
    }

    #[test]
    fn vblank_irq_is_requested_when_enabled_and_acknowledged_write_one_to_clear() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(IO_REG_START + DISPSTAT as u32, 1 << 3)
            .unwrap();
        bus.write_u16(IO_REG_START + INTERRUPT_ENABLE as u32, 1)
            .unwrap();
        bus.write_u16(IO_REG_START + INTERRUPT_MASTER_ENABLE as u32, 1)
            .unwrap();

        bus.advance(VBLANK_START_CYCLES - 1);
        assert!(!bus.irq_pending());
        bus.advance(1);
        assert!(bus.irq_pending());

        bus.write_u16(IO_REG_START + INTERRUPT_REQUEST as u32, 1)
            .unwrap();
        assert!(!bus.irq_pending());
    }

    #[test]
    fn vcount_tracks_scanlines_and_wraps_at_frame_boundary() {
        let mut bus = bus_with_rom(&[0; 4]);

        bus.advance(CYCLES_PER_SCANLINE * 224);
        assert_eq!(bus.read_u16(IO_REG_START + VCOUNT as u32).unwrap(), 224);

        bus.advance(CYCLES_PER_SCANLINE * 4);
        assert_eq!(bus.read_u16(IO_REG_START + VCOUNT as u32).unwrap(), 0);
    }

    #[test]
    fn keyinput_defaults_to_released_and_is_read_only() {
        let mut bus = bus_with_rom(&[0; 4]);
        assert_eq!(
            bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
            0x03FF
        );

        bus.set_key_input(0x03FE);
        assert_eq!(
            bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
            0x03FE
        );
        bus.write_u16(IO_REG_START + KEYINPUT as u32, 0).unwrap();
        assert_eq!(
            bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
            0x03FE
        );
    }

    #[test]
    fn failed_word_write_does_not_partially_modify_io_registers() {
        let mut bus = bus_with_rom(&[0; 4]);
        let result = bus.write_u32(IO_REG_END - 1, 0x1234_5678);

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AddrNotAvailable);
        assert_eq!(bus.io_regs[(IO_REG_END - IO_REG_START - 1) as usize], 0);
        assert_eq!(bus.io_regs[(IO_REG_END - IO_REG_START) as usize], 0);
    }

    #[test]
    fn timer_counts_reloads_and_requests_irq() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(IO_REG_START + 0x100, 0xFFFE).unwrap();
        bus.write_u16(IO_REG_START + 0x102, 0x00C0).unwrap();
        bus.advance(2);

        assert_eq!(bus.read_u16(IO_REG_START + 0x100).unwrap(), 0xFFFE);
        assert_ne!(
            u16::from_le_bytes([
                bus.io_regs[INTERRUPT_REQUEST],
                bus.io_regs[INTERRUPT_REQUEST + 1]
            ]) & (1 << 3),
            0
        );
    }

    #[test]
    fn cascaded_timers_count_each_source_overflow_once() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(IO_REG_START + 0x100, 0xFFFF).unwrap();
        bus.write_u16(IO_REG_START + 0x102, 0x0080).unwrap();
        bus.write_u16(IO_REG_START + 0x104, 0xFFFE).unwrap();
        bus.write_u16(IO_REG_START + 0x106, 0x00C4).unwrap();
        bus.write_u16(IO_REG_START + 0x108, 0xFFFE).unwrap();
        bus.write_u16(IO_REG_START + 0x10A, 0x0084).unwrap();

        bus.advance(2);

        assert_eq!(bus.read_u16(IO_REG_START + 0x104).unwrap(), 0xFFFE);
        assert_eq!(bus.read_u16(IO_REG_START + 0x108).unwrap(), 0xFFFF);
        assert_ne!(bus.io_regs[INTERRUPT_REQUEST] & (1 << 4), 0);
        assert_eq!(bus.io_regs[INTERRUPT_REQUEST] & (1 << 5), 0);
    }

    #[test]
    fn timer_advancement_handles_large_cycle_batches_without_per_tick_loops() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(IO_REG_START + 0x100, 0xFFFE).unwrap();
        bus.write_u16(IO_REG_START + 0x102, 0x00C0).unwrap();

        bus.advance(1_000_002);

        assert_eq!(bus.read_u16(IO_REG_START + 0x100).unwrap(), 0xFFFE);
        assert_ne!(bus.io_regs[INTERRUPT_REQUEST] & (1 << 3), 0);
    }

    #[test]
    fn interrupt_flags_are_write_one_to_clear_and_irq_requires_enable() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.io_regs[INTERRUPT_REQUEST] = 0b1010;
        bus.write_u16(IO_REG_START + INTERRUPT_REQUEST as u32, 0b0010)
            .unwrap();
        assert_eq!(bus.io_regs[INTERRUPT_REQUEST], 0b1000);
        assert!(!bus.irq_pending());

        bus.write_u16(IO_REG_START + INTERRUPT_ENABLE as u32, 0b1000)
            .unwrap();
        bus.write_u8(IO_REG_START + INTERRUPT_MASTER_ENABLE as u32, 1)
            .unwrap();
        assert!(bus.irq_pending());
    }

    #[test]
    fn transfer_cycle_cost_depends_on_memory_region_and_width() {
        let mut bus = bus_with_rom(&[0; 4]);
        assert_eq!(bus.transfer_cycles(EWRAM_START, 2, false).unwrap(), 3);
        assert_eq!(bus.transfer_cycles(IWRAM_START, 4, false).unwrap(), 1);
        assert_eq!(bus.transfer_cycles(ROM_START, 2, false).unwrap(), 4);
        assert_eq!(bus.transfer_cycles(ROM_START, 2, true).unwrap(), 2);

        assert_eq!(bus.transfer_cycles(0x0A00_0000, 2, true).unwrap(), 4);
        assert_eq!(bus.transfer_cycles(0x0C00_0000, 2, true).unwrap(), 8);

        bus.io_regs[0x204] = 0x9C;
        bus.io_regs[0x205] = 0x04;
        assert_eq!(bus.transfer_cycles(ROM_START, 2, false).unwrap(), 8);
        assert_eq!(bus.transfer_cycles(ROM_START, 2, true).unwrap(), 1);
        assert_eq!(bus.transfer_cycles(0x0A00_0000, 2, true).unwrap(), 1);
        assert_eq!(bus.transfer_cycles(0x0C00_0000, 2, true).unwrap(), 1);
    }

    #[test]
    fn all_gamepak_waitstate_regions_map_the_rom_window() {
        let bus = bus_with_rom(&[0x12, 0x34, 0x56, 0x78]);
        for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
            assert_eq!(bus.read_u32(base).unwrap(), 0x7856_3412);
        }
    }
}
