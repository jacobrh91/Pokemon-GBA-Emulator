use memmap2::Mmap;
use std::cell::Cell;
use std::fs::File;
use std::io;

mod display;
mod dma;
mod interrupts;
mod rtc;
use rtc::Rtc;
pub mod registers;
mod save;
mod state;
mod timers;
mod waitstates;
use crate::apu::{self, Apu};
pub use display::ScanlineSnapshot;
use display::{
    CYCLES_PER_FRAME, CYCLES_PER_SCANLINE, HBLANK_START_CYCLES, TOTAL_SCANLINES,
    VBLANK_START_SCANLINE,
};
use dma::{DmaChannel, DmaTiming};
use registers::{
    DISPCNT, DISPSTAT, FIFO_A, INTERRUPT_ENABLE, INTERRUPT_MASTER_ENABLE, INTERRUPT_REQUEST,
    KEYCNT, KEYINPUT, SOUNDBIAS, TIMER_BASE, VCOUNT, WAVE_RAM,
};
pub use save::SaveKind;
use save::Savedata;
use timers::Timer;
use waitstates::decode_waitcnt;

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
/// `msr cpsr_fc, r0`: the last opcode the BIOS fetches while booting.
pub const BIOS_OPEN_BUS_RESET: u32 = 0xE129_F000;
/// Last opcode fetched after a BIOS software interrupt returns.
pub const BIOS_OPEN_BUS_AFTER_SWI: u32 = 0xE3A0_2004;
/// Last opcode fetched after the BIOS IRQ handler returns.
pub const BIOS_OPEN_BUS_AFTER_IRQ: u32 = 0xE25E_F004;
const SOUNDBIAS_DEFAULT_HIGH: u8 = 0x02;
const DISPCNT_FORCED_BLANK: u8 = 0x80;

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
    /// BIOS ROM window; not readable by the cartridge code.
    Bios,
    /// No device drives the bus here.
    OpenBus,
}

pub struct MemoryBus {
    pub rom: Mmap,
    pub ewram: Box<[u8; 256 * 1024]>,
    pub iwram: Box<[u8; 32 * 1024]>,
    pub io_regs: Box<[u8; 1024]>,
    pub palette_ram: Box<[u8; 1024]>,
    pub vram: Box<[u8; 96 * 1024]>,
    pub oam: Box<[u8; 1024]>,
    savedata: Savedata,
    /// The cartridge's real-time clock, for the games that have one (see
    /// [`rtc`]); `None` for the rest.
    rtc: Option<Rtc>,
    /// Currently pressed buttons (`input` bit layout, 1 = pressed).
    pressed_buttons: u16,
    keypad_irq_active: bool,
    display_cycles: u32,
    /// Cycles the CPU has run that the display, timers and audio have not
    /// been told about yet. Always shorter than `cycles_until_event`, so no
    /// observable event hides inside it.
    pending_cycles: u32,
    /// Bus writes performed so far; lets the emulator prove a loop has no
    /// side effects.
    write_count: u64,
    /// Times pending time was applied (events and register writes). An
    /// iteration of a loop is only side-effect free if none happened in it.
    flush_count: u64,
    /// Reads of registers whose value changes with every cycle (timer
    /// counters); a loop that reads one is waiting on time, not an event.
    volatile_reads: Cell<u32>,
    /// Cycles from the last flush until the next scanline/HBlank boundary,
    /// timer overflow or audio sample.
    cycles_until_event: u32,
    /// Decoded WAITCNT: cycles per 16-bit Game Pak access, indexed by
    /// `[wait state 0..=2][sequential]`, and for SRAM/Flash.
    rom_cycles: [[u32; 2]; 3],
    /// Mask applied to Game Pak offsets: the ROM mirrors when its size is a
    /// power of two, otherwise the full 25-bit window is addressable.
    rom_mask: usize,
    sram_cycles: u32,
    timers: [Timer; 4],
    dma: [DmaChannel; 4],
    dma_cycles: u32,
    apu: Apu,
    /// Last value the CPU fetched; unmapped reads return it.
    open_bus: u32,
    scanlines: Vec<ScanlineSnapshot>,
    affine_reload: [bool; 2],
    /// Last BIOS opcode fetched, returned for BIOS reads from outside it.
    bios_open_bus: u32,
}

impl MemoryBus {
    pub fn new(rom_path: &str) -> io::Result<Self> {
        let file = File::open(rom_path)?;
        // Safety: The ROM mapping is read-only and assumes the file is not modified during use.
        let rom = unsafe { Mmap::map(&file)? };

        let mut io_regs = Box::new([0; 1024]);
        // The BIOS hands over with the display in forced blank.
        io_regs[DISPCNT] = DISPCNT_FORCED_BLANK;
        io_regs[SOUNDBIAS + 1] = SOUNDBIAS_DEFAULT_HIGH;

        let savedata = Savedata::detect(&rom);
        let rtc = rom
            .get(0xAC..0xB0)
            .filter(|code| rtc::cartridge_has_clock(code))
            .map(|_| Rtc::new(rtc::host_time));
        let rom_mask = if rom.len().is_power_of_two() {
            rom.len() - 1
        } else {
            usize::MAX
        };

        Ok(Self {
            rom,
            ewram: Box::new([0; 256 * 1024]),
            iwram: Box::new([0; 32 * 1024]),
            io_regs,
            savedata,
            rtc,
            palette_ram: Box::new([0; 1024]),
            vram: Box::new([0; 96 * 1024]),
            oam: Box::new([0; 1024]),
            pressed_buttons: 0,
            keypad_irq_active: false,
            display_cycles: 0,
            pending_cycles: 0,
            write_count: 0,
            flush_count: 0,
            volatile_reads: Cell::new(0),
            cycles_until_event: HBLANK_START_CYCLES,
            rom_cycles: decode_waitcnt(0).0,
            rom_mask,
            sram_cycles: decode_waitcnt(0).1,
            timers: std::array::from_fn(|_| Timer::default()),
            dma: [DmaChannel::default(); 4],
            dma_cycles: 0,
            apu: Apu::default(),
            open_bus: 0,
            scanlines: Vec::new(),
            affine_reload: [false; 2],
            bios_open_bus: BIOS_OPEN_BUS_RESET,
        })
    }

    /// Whether the byte at cartridge `offset` is one the clock's port shows
    /// instead of the ROM's own.
    #[inline]
    fn clock_port_at(&self, offset: usize) -> bool {
        self.rtc
            .as_ref()
            .is_some_and(|rtc| rtc.readable() && Rtc::is_gpio(offset))
    }

    /// Whether any of the `length` bytes of `region` from its offset are.
    #[inline]
    fn clock_port_in(&self, region: Region, length: usize) -> bool {
        match region {
            Region::Rom(offset) => {
                self.rtc.is_some() && (offset..offset + length).any(|at| self.clock_port_at(at))
            }
            _ => false,
        }
    }

    /// A write into the cartridge's address space: the ROM ignores it, but the
    /// clock's port takes it.
    fn write_cartridge(&mut self, address: u32, bytes: &[u8]) {
        let Some(rtc) = &mut self.rtc else {
            return;
        };
        let offset = (address & 0x01FF_FFFF) as usize;
        for (index, byte) in bytes.iter().enumerate() {
            if Rtc::is_gpio(offset + index) {
                rtc.write(offset + index, *byte);
            }
        }
    }

    /// Replaces the clock the cartridge's real-time clock reads (tests).
    #[cfg(test)]
    pub(crate) fn set_clock(&mut self, clock: rtc::Clock) {
        if self.rtc.is_some() {
            self.rtc = Some(Rtc::new(clock));
        }
    }

    pub fn read_u8(&self, address: u32) -> io::Result<u8> {
        match self.resolve(address)? {
            // Past the end of the cartridge the address bus echoes itself.
            Region::Rom(offset) if self.clock_port_at(offset) => {
                Ok(self.rtc.as_ref().map_or(0, |rtc| rtc.read(offset)))
            }
            Region::Rom(offset) => Ok(self.rom.get(offset).copied().unwrap_or_else(|| {
                let halfword = (address >> 1) as u16;
                halfword.to_le_bytes()[(address & 1) as usize]
            })),
            Region::Bios => Ok(self.bios_open_bus.to_le_bytes()[(address & 3) as usize]),
            Region::OpenBus => Ok(self.open_bus.to_le_bytes()[(address & 3) as usize]),
            Region::Ewram(offset) => Ok(self.ewram[offset]),
            Region::Iwram(offset) => Ok(self.iwram[offset]),
            Region::Io(offset) => Ok(self.read_io_byte(offset)),
            Region::PaletteRam(offset) => Ok(self.palette_ram[offset]),
            Region::Vram(offset) => Ok(self.vram[offset]),
            Region::Oam(offset) => Ok(self.oam[offset]),
            Region::SaveRam(offset) => Ok(self.savedata.read(offset as u32)),
        }
    }

    /// `length` bytes starting at `offset` of a plain (side-effect free)
    /// memory region, or `None` for I/O, backup, open bus or an access that
    /// would run off the end of the region's storage.
    #[inline]
    fn plain_bytes(&self, region: Region, length: usize) -> Option<&[u8]> {
        let (storage, offset): (&[u8], usize) = match region {
            Region::Rom(offset) => (&self.rom[..], offset),
            Region::Ewram(offset) => (&self.ewram[..], offset),
            Region::Iwram(offset) => (&self.iwram[..], offset),
            Region::PaletteRam(offset) => (&self.palette_ram[..], offset),
            Region::Vram(offset) => (&self.vram[..], offset),
            Region::Oam(offset) => (&self.oam[..], offset),
            _ => return None,
        };
        storage.get(offset..offset.checked_add(length)?)
    }

    /// Mutable counterpart of [`Self::plain_bytes`] for writable regions.
    #[inline]
    fn plain_bytes_mut(&mut self, region: Region, length: usize) -> Option<&mut [u8]> {
        let (storage, offset): (&mut [u8], usize) = match region {
            Region::Ewram(offset) => (&mut self.ewram[..], offset),
            Region::Iwram(offset) => (&mut self.iwram[..], offset),
            Region::PaletteRam(offset) => (&mut self.palette_ram[..], offset),
            Region::Vram(offset) => (&mut self.vram[..], offset),
            Region::Oam(offset) => (&mut self.oam[..], offset),
            _ => return None,
        };
        storage.get_mut(offset..offset.checked_add(length)?)
    }

    /// Instruction fetch: ROM, IWRAM and EWRAM directly, a normal read
    /// otherwise.
    #[inline]
    pub fn fetch_u16(&self, address: u32) -> io::Result<u16> {
        let bytes = match address >> 24 {
            0x08..=0x0D => {
                let offset = (address & 0x01FF_FFFF) as usize & self.rom_mask;
                self.rom.get(offset..offset + 2)
            }
            0x03 => {
                let offset = (address & IWRAM_MASK) as usize;
                self.iwram.get(offset..offset + 2)
            }
            0x02 => {
                let offset = (address & EWRAM_MASK) as usize;
                self.ewram.get(offset..offset + 2)
            }
            _ => None,
        };
        match bytes {
            Some(bytes) => Ok(u16::from_le_bytes([bytes[0], bytes[1]])),
            None => self.read_u16(address),
        }
    }

    /// 32-bit counterpart of [`Self::fetch_u16`].
    #[inline]
    pub fn fetch_u32(&self, address: u32) -> io::Result<u32> {
        let bytes = match address >> 24 {
            0x08..=0x0D => {
                let offset = (address & 0x01FF_FFFF) as usize & self.rom_mask;
                self.rom.get(offset..offset + 4)
            }
            0x03 => {
                let offset = (address & IWRAM_MASK) as usize;
                self.iwram.get(offset..offset + 4)
            }
            0x02 => {
                let offset = (address & EWRAM_MASK) as usize;
                self.ewram.get(offset..offset + 4)
            }
            _ => None,
        };
        match bytes {
            Some(bytes) => Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            None => self.read_u32(address),
        }
    }

    pub fn read_u16(&self, address: u32) -> io::Result<u16> {
        let region = self.resolve(address)?;
        if !self.clock_port_in(region, 2)
            && let Some(bytes) = self.plain_bytes(region, 2)
        {
            return Ok(u16::from_le_bytes([bytes[0], bytes[1]]));
        }
        // The backup chip sits on an 8-bit bus: wider reads see the byte
        // repeated on every lane.
        if matches!(region, Region::SaveRam(_)) {
            return Ok(u16::from(self.read_u8(address)?) * 0x0101);
        }
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
        let region = self.resolve(address)?;
        if !self.clock_port_in(region, 4)
            && let Some(bytes) = self.plain_bytes(region, 4)
        {
            return Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        }
        if matches!(region, Region::SaveRam(_)) {
            return Ok(u32::from(self.read_u8(address)?) * 0x0101_0101);
        }
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
        self.write_count += 1;
        if (ROM_START..=ROM_END).contains(&address) {
            self.write_cartridge(address, &[value]);
            return Ok(());
        }
        let region = self.resolve(address)?;
        self.write_region(region, value)
    }

    pub fn write_u16(&mut self, address: u32, value: u16) -> io::Result<()> {
        self.write_count += 1;
        if (ROM_START..=ROM_END).contains(&address) {
            self.write_cartridge(address, &value.to_le_bytes());
            return Ok(());
        }
        let first = self.resolve(address)?;
        if let Some(bytes) = self.plain_bytes_mut(first, 2) {
            bytes.copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Region::SaveRam(offset) = first {
            // Only one byte reaches the 8-bit chip: the lane being addressed.
            self.savedata
                .write(offset as u32, (value >> ((address & 1) * 8)) as u8);
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

    /// Accounts `cycles` of CPU time; the work is deferred until something
    /// observable is due.
    #[inline(always)]
    pub fn advance(&mut self, cycles: u32) {
        self.pending_cycles = self.pending_cycles.saturating_add(cycles);
        if self.pending_cycles >= self.cycles_until_event {
            self.flush_pending();
        }
    }

    /// Brings the display, timers and audio up to date.
    pub fn sync(&mut self) {
        self.flush_pending();
    }

    #[inline(never)]
    fn flush_pending(&mut self) {
        let elapsed = std::mem::take(&mut self.pending_cycles);
        if elapsed != 0 {
            self.flush_count += 1;
            self.advance_elapsed(elapsed);
            self.reschedule();
        }
    }

    /// Recomputes how long the bus can run before the next observable event.
    fn reschedule(&mut self) {
        let in_line = self.display_cycles % CYCLES_PER_SCANLINE;
        let mut distance = if in_line < HBLANK_START_CYCLES {
            HBLANK_START_CYCLES - in_line
        } else {
            CYCLES_PER_SCANLINE - in_line
        };
        distance = distance.min(self.apu.cycles_until_sample());
        distance = distance.min(self.timer_event_distance());
        self.cycles_until_event = distance.max(1);
    }

    /// Bus writes so far, for loop-invariance checks.
    pub fn write_count(&self) -> u64 {
        self.write_count
    }

    /// Times pending time was applied so far.
    pub fn flush_count(&self) -> u64 {
        self.flush_count
    }

    /// Reads of per-cycle-varying registers so far.
    pub fn volatile_read_count(&self) -> u32 {
        self.volatile_reads.get()
    }

    /// Cycles the CPU can still run before the next observable event.
    pub fn cycles_until_next_event(&self) -> u32 {
        self.cycles_until_event.saturating_sub(self.pending_cycles)
    }

    /// Applies `cycles` of elapsed time to the display, timers and audio.
    fn advance_elapsed(&mut self, cycles: u32) {
        self.display_events(cycles);
        self.display_cycles = ((u64::from(self.display_cycles) + u64::from(cycles))
            % u64::from(CYCLES_PER_FRAME)) as u32;

        let audio_overflows = self.advance_timers(cycles);
        self.advance_audio(cycles, audio_overflows);
    }

    /// Feeds timer 0/1 overflows to the Direct Sound FIFOs, refilling them
    /// through special-timing DMA, then mixes audio for the elapsed cycles.
    fn advance_audio(&mut self, cycles: u32, overflows: [u64; 2]) {
        for (timer, count) in overflows.into_iter().enumerate() {
            for _ in 0..count.min(64) {
                let refill = self.apu.timer_overflow(timer, &self.io_regs[..]);
                for (channel, wanted) in refill.into_iter().enumerate() {
                    if wanted {
                        self.trigger_fifo_dma(channel);
                    }
                }
            }
        }
        self.apu.advance(cycles, &self.io_regs[..]);
    }

    pub fn audio_fifo_stats(&self) -> apu::FifoStats {
        self.apu.fifo_stats()
    }

    /// Mixed stereo samples at [`apu::SAMPLE_RATE`] since the last call.
    pub fn take_audio_samples(&mut self) -> Vec<[i16; 2]> {
        self.apu.take_samples()
    }

    pub fn save_kind(&self) -> SaveKind {
        self.savedata.kind()
    }

    /// The backup memory image, for persisting to disk.
    pub fn save_data(&self) -> &[u8] {
        self.savedata.data()
    }

    /// Restores a previously persisted backup memory image.
    pub fn load_save_data(&mut self, image: &[u8]) {
        self.savedata.load(image);
    }

    /// True if the backup memory changed since the last call.
    pub fn take_save_dirty(&mut self) -> bool {
        self.savedata.take_dirty()
    }

    /// Records the value on the data bus after a CPU opcode fetch.
    pub fn set_open_bus(&mut self, value: u32) {
        self.open_bus = value;
    }

    /// Records the last opcode the (emulated) BIOS fetched.
    pub fn set_bios_open_bus(&mut self, value: u32) {
        self.bios_open_bus = value;
    }

    pub fn register_ram_reset(&mut self, flags: u32) {
        self.flush_pending();
        if flags & 1 != 0 {
            self.ewram.fill(0);
        }
        if flags & 2 != 0 {
            // The BIOS keeps the last 0x200 bytes (stacks, IRQ vector, flags).
            self.iwram[..0x7E00].fill(0);
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
            self.io_regs[DISPCNT] = DISPCNT_FORCED_BLANK;
            self.io_regs[SOUNDBIAS + 1] = SOUNDBIAS_DEFAULT_HIGH;
            self.refresh_waitcnt();
            self.timers = std::array::from_fn(|_| Timer::default());
            self.dma = [DmaChannel::default(); 4];
            self.reschedule();
        }
    }

    pub fn write_u32(&mut self, address: u32, value: u32) -> io::Result<()> {
        self.write_count += 1;
        if (ROM_START..=ROM_END).contains(&address) {
            self.write_cartridge(address, &value.to_le_bytes());
            return Ok(());
        }
        let first = self.resolve(address)?;
        if let Some(bytes) = self.plain_bytes_mut(first, 4) {
            bytes.copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Region::SaveRam(offset) = first {
            self.savedata
                .write(offset as u32, (value >> ((address & 3) * 8)) as u8);
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
            ROM_START..=ROM_END => Ok(Region::Rom(
                (address & 0x01FF_FFFF) as usize & self.rom_mask,
            )),
            0x0000_0000..=0x0000_3FFF => Ok(Region::Bios),
            _ => Ok(Region::OpenBus),
        }
    }

    fn write_region(&mut self, region: Region, value: u8) -> io::Result<()> {
        match region {
            Region::Ewram(offset) => self.ewram[offset] = value,
            Region::Iwram(offset) => self.iwram[offset] = value,
            Region::PaletteRam(offset) => self.palette_ram[offset] = value,
            Region::Vram(offset) => self.vram[offset] = value,
            Region::Oam(offset) => self.oam[offset] = value,
            Region::SaveRam(offset) => self.savedata.write(offset as u32, value),
            Region::Io(offset) => {
                // A register write must see (and be seen at) the right time.
                self.flush_pending();
                if (FIFO_A..FIFO_A + 8).contains(&offset) {
                    self.apu.push_fifo((offset - FIFO_A) / 4, value);
                    return Ok(());
                }
                if (WAVE_RAM..WAVE_RAM + 16).contains(&offset) {
                    self.apu
                        .write_wave_ram(offset - WAVE_RAM, value, &self.io_regs[..]);
                    return Ok(());
                }
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
                if (TIMER_BASE..TIMER_BASE + 16).contains(&offset) {
                    self.reschedule();
                }
                if (0x204..0x206).contains(&offset) {
                    self.refresh_waitcnt();
                }
                self.dma_register_written(offset);
                match offset {
                    0x28..=0x2F => self.affine_reload[0] = true,
                    0x38..=0x3F => self.affine_reload[1] = true,
                    _ => {}
                }
                if (KEYCNT..KEYCNT + 2).contains(&offset) {
                    self.evaluate_keypad_irq();
                }
                if (apu::AUDIO_REGISTERS_START..apu::AUDIO_REGISTERS_END).contains(&offset) {
                    self.apu.register_written(offset, &mut self.io_regs[..]);
                }
            }
            Region::Rom(_) | Region::Bios | Region::OpenBus => {}
        }
        Ok(())
    }

    fn read_io_byte(&self, offset: usize) -> u8 {
        if offset == DISPSTAT {
            let position = self.live_display_cycles();
            let scanline = position / CYCLES_PER_SCANLINE;
            let in_hblank = position % CYCLES_PER_SCANLINE >= HBLANK_START_CYCLES;
            let in_vblank = (VBLANK_START_SCANLINE..TOTAL_SCANLINES - 1).contains(&scanline);
            let lyc = u32::from(self.io_regs[DISPSTAT + 1]);
            return (self.io_regs[offset] & !7)
                | u8::from(in_vblank)
                | u8::from(in_hblank) << 1
                | u8::from(scanline == lyc) << 2;
        }
        if dma::dma_register_is_write_only(offset) {
            return 0;
        }
        if (WAVE_RAM..WAVE_RAM + 16).contains(&offset) {
            return self.apu.read_wave_ram(offset - WAVE_RAM, &self.io_regs[..]);
        }
        if (FIFO_A..FIFO_A + 8).contains(&offset) {
            return 0;
        }
        if offset == 0x84 {
            return (self.io_regs[offset] & 0x80) | self.apu.channel_status();
        }
        if offset == VCOUNT {
            return (self.live_display_cycles() / CYCLES_PER_SCANLINE) as u8;
        }
        if offset == VCOUNT + 1 {
            return 0;
        }
        let key_input = !self.pressed_buttons & crate::input::ALL;
        if offset == KEYINPUT {
            return key_input as u8;
        }
        if offset == KEYINPUT + 1 {
            return (key_input >> 8) as u8;
        }
        if (TIMER_BASE..TIMER_BASE + 16).contains(&offset) {
            let timer_index = (offset - TIMER_BASE) / 4;
            let timer_register_offset = (offset - TIMER_BASE) % 4;
            if timer_register_offset < 2 {
                self.volatile_reads
                    .set(self.volatile_reads.get().wrapping_add(1));
                let counter = self.live_timer_counter(timer_index);
                return if timer_register_offset == 0 {
                    counter as u8
                } else {
                    (counter >> 8) as u8
                };
            }
        }
        self.io_regs[offset]
    }
}

#[cfg(test)]
mod tests;
