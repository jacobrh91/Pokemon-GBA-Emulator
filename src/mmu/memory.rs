use memmap2::Mmap;
use std::fs::File;
use std::io;

mod dma;
mod save;
use crate::apu::{self, Apu};
use dma::{DmaChannel, DmaTiming};
pub use save::SaveKind;
use save::Savedata;

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
/// `msr cpsr_fc, r0`: the last opcode the BIOS fetches while booting.
pub const BIOS_OPEN_BUS_RESET: u32 = 0xE129_F000;
/// Last opcode fetched after a BIOS software interrupt returns.
pub const BIOS_OPEN_BUS_AFTER_SWI: u32 = 0xE3A0_2004;
/// Last opcode fetched after the BIOS IRQ handler returns.
pub const BIOS_OPEN_BUS_AFTER_IRQ: u32 = 0xE25E_F004;
const SOUNDBIAS: usize = 0x88;
const SOUNDBIAS_DEFAULT_HIGH: u8 = 0x02;
const DISPCNT: usize = 0x000;
const DISPCNT_FORCED_BLANK: u8 = 0x80;
const DISPSTAT: usize = 0x004;
const VCOUNT: usize = 0x006;
const KEYINPUT: usize = 0x130;
const KEYCNT: usize = 0x132;
const KEYPAD_IRQ_BIT: u8 = 1 << 4; // IF bit 12 (high byte bit 4)
const CYCLES_PER_SCANLINE: u32 = 1232;
const VBLANK_START_SCANLINE: u32 = 160;
const TOTAL_SCANLINES: u32 = 228;
const HBLANK_START_CYCLES: u32 = 240 * 4;
#[cfg(test)]
const VBLANK_START_CYCLES: u32 = CYCLES_PER_SCANLINE * VBLANK_START_SCANLINE;
const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * TOTAL_SCANLINES;

/// Display registers (DISPCNT..BLDY) as they were when a visible scanline
/// finished drawing. HBlank DMA runs after the snapshot, so per-line scroll
/// and window effects land on the following line, as on hardware.
#[derive(Clone)]
pub struct ScanlineSnapshot {
    pub line: usize,
    pub registers: [u8; DISPLAY_REGISTER_BYTES],
}

pub const DISPLAY_REGISTER_BYTES: usize = 0x60;
/// Snapshots queued without a consumer are dropped past this many.
const MAX_QUEUED_SCANLINES: usize = 512;

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
    /// Currently pressed buttons (`input` bit layout, 1 = pressed).
    pressed_buttons: u16,
    keypad_irq_active: bool,
    display_cycles: u32,
    timers: [Timer; 4],
    dma: [DmaChannel; 4],
    dma_cycles: u32,
    apu: Apu,
    /// Last value the CPU fetched; unmapped reads return it.
    open_bus: u32,
    scanlines: Vec<ScanlineSnapshot>,
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

        Ok(Self {
            rom,
            ewram: Box::new([0; 256 * 1024]),
            iwram: Box::new([0; 32 * 1024]),
            io_regs,
            savedata,
            palette_ram: Box::new([0; 1024]),
            vram: Box::new([0; 96 * 1024]),
            oam: Box::new([0; 1024]),
            pressed_buttons: 0,
            keypad_irq_active: false,
            display_cycles: 0,
            timers: std::array::from_fn(|_| Timer::default()),
            dma: [DmaChannel::default(); 4],
            dma_cycles: 0,
            apu: Apu::default(),
            open_bus: 0,
            scanlines: Vec::new(),
            bios_open_bus: BIOS_OPEN_BUS_RESET,
        })
    }

    pub fn read_u8(&self, address: u32) -> io::Result<u8> {
        match self.resolve(address)? {
            // Past the end of the cartridge the address bus echoes itself.
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

    pub fn read_u16(&self, address: u32) -> io::Result<u16> {
        // The backup chip sits on an 8-bit bus: wider reads see the byte
        // repeated on every lane.
        if matches!(self.resolve(address)?, Region::SaveRam(_)) {
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
        if matches!(self.resolve(address)?, Region::SaveRam(_)) {
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
        if let Region::SaveRam(offset) = self.resolve(address)? {
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

    pub fn transfer_cycles(&self, address: u32, width: u8, sequential: bool) -> io::Result<u32> {
        let region = self.resolve(address)?;
        let halfword_cycles = match region {
            Region::Bios
            | Region::OpenBus
            | Region::Iwram(_)
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

    /// True when any enabled interrupt source is requested (`IE & IF`),
    /// regardless of IME. This is the condition that wakes a halted CPU.
    pub fn interrupt_requested(&self) -> bool {
        let ie = u16::from_le_bytes([
            self.io_regs[INTERRUPT_ENABLE],
            self.io_regs[INTERRUPT_ENABLE + 1],
        ]);
        let requested = u16::from_le_bytes([
            self.io_regs[INTERRUPT_REQUEST],
            self.io_regs[INTERRUPT_REQUEST + 1],
        ]);
        ie & requested != 0
    }

    pub fn irq_pending(&self) -> bool {
        self.io_regs[INTERRUPT_MASTER_ENABLE] & 1 != 0 && self.interrupt_requested()
    }

    pub fn advance(&mut self, cycles: u32) {
        self.display_events(cycles);
        self.display_cycles = ((u64::from(self.display_cycles) + u64::from(cycles))
            % u64::from(CYCLES_PER_FRAME)) as u32;

        let mut cascade_ticks = 0;
        let mut audio_overflows = [0u64; 2];
        for index in 0..4 {
            let control = self.timers[index].control;
            if control & (1 << 7) == 0 {
                cascade_ticks = 0;
                continue;
            }
            if index > 0 && control & (1 << 2) != 0 {
                let source_ticks = cascade_ticks;
                cascade_ticks = self.advance_timer(index, source_ticks);
                if index < 2 {
                    audio_overflows[index] = cascade_ticks;
                }
                continue;
            }
            let prescaler = [1_u64, 64, 256, 1024][(control & 3) as usize];
            let accumulated = u64::from(self.timers[index].prescaler_cycles) + u64::from(cycles);
            let ticks = accumulated / prescaler;
            self.timers[index].prescaler_cycles = (accumulated % prescaler) as u16;
            cascade_ticks = self.advance_timer(index, ticks);
            if index < 2 {
                audio_overflows[index] = cascade_ticks;
            }
        }
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

    /// Raises display interrupts and starts VBlank/HBlank DMA for every
    /// scanline boundary crossed while `cycles` elapse.
    fn display_events(&mut self, cycles: u32) {
        let start = u64::from(self.display_cycles);
        let end = start + u64::from(cycles);
        let dispstat = u16::from_le_bytes([self.io_regs[DISPSTAT], self.io_regs[DISPSTAT + 1]]);
        let line_cycles = u64::from(CYCLES_PER_SCANLINE);
        let hblank_offset = u64::from(HBLANK_START_CYCLES);

        for scanline in start / line_cycles..=end / line_cycles {
            let line = (scanline % u64::from(TOTAL_SCANLINES)) as u32;
            let line_start = scanline * line_cycles;
            if line_start > start && line_start <= end {
                if line == VBLANK_START_SCANLINE {
                    if dispstat & (1 << 3) != 0 {
                        self.io_regs[INTERRUPT_REQUEST] |= 1;
                    }
                    self.trigger_dma(DmaTiming::VBlank);
                }
                if dispstat & (1 << 5) != 0 && line == u32::from(dispstat >> 8) {
                    self.io_regs[INTERRUPT_REQUEST] |= 1 << 2;
                }
            }
            let hblank = line_start + hblank_offset;
            if hblank > start && hblank <= end {
                if dispstat & (1 << 4) != 0 {
                    self.io_regs[INTERRUPT_REQUEST] |= 1 << 1;
                }
                if line < VBLANK_START_SCANLINE {
                    if self.scanlines.len() < MAX_QUEUED_SCANLINES {
                        let mut registers = [0; DISPLAY_REGISTER_BYTES];
                        registers.copy_from_slice(&self.io_regs[..DISPLAY_REGISTER_BYTES]);
                        self.scanlines.push(ScanlineSnapshot {
                            line: line as usize,
                            registers,
                        });
                    }
                    self.trigger_dma(DmaTiming::HBlank);
                }
            }
        }
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

    /// Scanlines that finished drawing since the last call, oldest first.
    pub fn take_scanlines(&mut self) -> Vec<ScanlineSnapshot> {
        std::mem::take(&mut self.scanlines)
    }

    /// Sets the pressed buttons (see [`crate::input`]); KEYINPUT reads them
    /// active-low. Raises the keypad interrupt when KEYCNT's condition
    /// becomes true.
    pub fn set_pressed_buttons(&mut self, pressed: u16) {
        self.pressed_buttons = pressed & crate::input::ALL;
        self.evaluate_keypad_irq();
    }

    fn evaluate_keypad_irq(&mut self) {
        let control = u16::from_le_bytes([self.io_regs[KEYCNT], self.io_regs[KEYCNT + 1]]);
        let selected = control & crate::input::ALL;
        let satisfied = control & (1 << 14) != 0
            && selected != 0
            && if control & (1 << 15) != 0 {
                // AND mode: every selected button held.
                self.pressed_buttons & selected == selected
            } else {
                self.pressed_buttons & selected != 0
            };
        if satisfied && !self.keypad_irq_active {
            self.io_regs[INTERRUPT_REQUEST + 1] |= KEYPAD_IRQ_BIT;
        }
        self.keypad_irq_active = satisfied;
    }

    pub fn register_ram_reset(&mut self, flags: u32) {
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
            self.timers = std::array::from_fn(|_| Timer::default());
            self.dma = [DmaChannel::default(); 4];
        }
    }

    pub fn write_u32(&mut self, address: u32, value: u32) -> io::Result<()> {
        if (ROM_START..=ROM_END).contains(&address) {
            return Ok(());
        }
        if let Region::SaveRam(offset) = self.resolve(address)? {
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
            ROM_START..=ROM_END => {
                let mut offset = (address & 0x01FF_FFFF) as usize;
                if self.rom.len().is_power_of_two() {
                    offset &= self.rom.len() - 1;
                }
                Ok(Region::Rom(offset))
            }
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
                if (apu::FIFO_A..apu::FIFO_A + 8).contains(&offset) {
                    self.apu.push_fifo((offset - apu::FIFO_A) / 4, value);
                    return Ok(());
                }
                if (apu::WAVE_RAM..apu::WAVE_RAM + 16).contains(&offset) {
                    self.apu
                        .write_wave_ram(offset - apu::WAVE_RAM, value, &self.io_regs[..]);
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
                self.dma_register_written(offset);
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
            let scanline = self.display_cycles / CYCLES_PER_SCANLINE;
            let in_hblank = self.display_cycles % CYCLES_PER_SCANLINE >= HBLANK_START_CYCLES;
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
        if (apu::WAVE_RAM..apu::WAVE_RAM + 16).contains(&offset) {
            return self
                .apu
                .read_wave_ram(offset - apu::WAVE_RAM, &self.io_regs[..]);
        }
        if (apu::FIFO_A..apu::FIFO_A + 8).contains(&offset) {
            return 0;
        }
        if offset == 0x84 {
            return (self.io_regs[offset] & 0x80) | self.apu.channel_status();
        }
        if offset == VCOUNT {
            return (self.display_cycles / CYCLES_PER_SCANLINE) as u8;
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
        // Past the cartridge the bus echoes the halfword address.
        let odd_sized = bus_with_rom(&[0; 6]);
        assert_eq!(odd_sized.read_u16(ROM_START + 0x100).unwrap(), 0x0080);
        // BIOS and unmapped regions return open bus rather than faulting.
        assert_eq!(bus.read_u32(0).unwrap(), BIOS_OPEN_BUS_RESET);
        bus.set_open_bus(0xCAFE_F00D);
        assert_eq!(bus.read_u32(IO_REG_END + 1).unwrap(), 0xCAFE_F00D);
        bus.write_u32(IO_REG_END + 1, 0).unwrap();
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

        bus.set_pressed_buttons(crate::input::A);
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

    fn program_dma(
        bus: &mut MemoryBus,
        channel: u32,
        source: u32,
        dest: u32,
        count: u16,
        control: u16,
    ) {
        let base = IO_REG_START + 0xB0 + channel * 12;
        bus.write_u32(base, source).unwrap();
        bus.write_u32(base + 4, dest).unwrap();
        bus.write_u16(base + 8, count).unwrap();
        bus.write_u16(base + 10, control).unwrap();
    }

    #[test]
    fn immediate_dma_copies_words_and_clears_enable() {
        let mut bus = bus_with_rom(&[0; 4]);
        for index in 0..4 {
            bus.write_u32(EWRAM_START + index * 4, 0x1111_0000 + index)
                .unwrap();
        }
        program_dma(&mut bus, 3, EWRAM_START, IWRAM_START, 4, 0x8000 | (1 << 10));

        for index in 0..4 {
            assert_eq!(
                bus.read_u32(IWRAM_START + index * 4).unwrap(),
                0x1111_0000 + index
            );
        }
        assert_eq!(bus.read_u16(IO_REG_START + 0xDE).unwrap() & 0x8000, 0);
        assert!(bus.take_dma_cycles() > 8);
        assert_eq!(bus.take_dma_cycles(), 0);
    }

    #[test]
    fn dma_zero_count_means_maximum_and_fixed_source_fills() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(EWRAM_START, 0xABCD).unwrap();
        // DMA0, count 0 => 0x4000 halfwords, fixed source.
        program_dma(&mut bus, 0, EWRAM_START, IWRAM_START, 0, 0x8000 | (2 << 7));
        assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0xABCD);
        assert_eq!(bus.read_u16(IWRAM_START + 0x7FFE).unwrap(), 0xABCD);
    }

    #[test]
    fn vblank_dma_waits_for_vblank_and_repeats_with_irq() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(EWRAM_START, 0x1234).unwrap();
        // 1 halfword, VBlank timing, repeat, IRQ, fixed destination.
        let control = 0x8000 | (1 << 14) | (1 << 12) | (1 << 9) | (2 << 7) | (2 << 5);
        program_dma(&mut bus, 1, EWRAM_START, IWRAM_START, 1, control);
        assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0);

        bus.advance(VBLANK_START_CYCLES + 1);
        assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0x1234);
        assert_ne!(bus.read_u16(IO_REG_START + 0x202).unwrap() & (1 << 9), 0);
        assert_ne!(bus.read_u16(IO_REG_START + 0xC6).unwrap() & 0x8000, 0);

        bus.write_u16(EWRAM_START, 0x5678).unwrap();
        bus.advance(CYCLES_PER_FRAME);
        assert_eq!(bus.read_u16(IWRAM_START).unwrap(), 0x5678);
    }

    #[test]
    fn hblank_dma_runs_once_per_visible_scanline() {
        let mut bus = bus_with_rom(&[0; 4]);
        for line in 0..3 {
            bus.write_u16(EWRAM_START + line * 2, 0x100 + line as u16)
                .unwrap();
        }
        // 1 halfword per HBlank, repeat, incrementing source and destination.
        program_dma(
            &mut bus,
            2,
            EWRAM_START,
            IWRAM_START,
            1,
            0x8000 | (2 << 12) | (1 << 9),
        );
        bus.advance(CYCLES_PER_SCANLINE * 3);
        for line in 0..3 {
            assert_eq!(
                bus.read_u16(IWRAM_START + line * 2).unwrap(),
                0x100 + line as u16
            );
        }
        assert_eq!(bus.read_u16(IWRAM_START + 6).unwrap(), 0);
    }

    #[test]
    fn dma_address_registers_are_write_only() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u32(IO_REG_START + 0xD4, 0x0200_0000).unwrap();
        assert_eq!(bus.read_u32(IO_REG_START + 0xD4).unwrap(), 0);
    }

    #[test]
    fn register_ram_reset_preserves_the_top_of_iwram() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u32(IWRAM_START + 0x7FFC, 0xDEAD_BEEF).unwrap();
        bus.write_u32(IWRAM_START, 0x1234).unwrap();
        bus.register_ram_reset(2);
        assert_eq!(bus.read_u32(IWRAM_START).unwrap(), 0);
        assert_eq!(bus.read_u32(IWRAM_START + 0x7FFC).unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn scanline_snapshots_precede_hblank_dma_register_updates() {
        let mut bus = bus_with_rom(&[0; 4]);
        // HBlank DMA: one halfword per line from EWRAM into BG0VOFS (fixed dest).
        bus.write_u16(EWRAM_START, 0x0005).unwrap();
        bus.write_u16(EWRAM_START + 2, 0x0007).unwrap();
        program_dma(
            &mut bus,
            0,
            EWRAM_START,
            IO_REG_START + 0x12,
            1,
            0x8000 | (2 << 12) | (1 << 9) | (2 << 5),
        );
        bus.advance(CYCLES_PER_SCANLINE * 2);

        let lines = bus.take_scanlines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line, 0);
        // Line 0 was latched before its HBlank DMA wrote VOFS = 5.
        assert_eq!(lines[0].registers[0x12], 0);
        assert_eq!(lines[1].registers[0x12], 5);
        assert_eq!(bus.io_regs[0x12], 7);
        assert!(bus.take_scanlines().is_empty());
    }

    #[test]
    fn timer_overflow_pops_the_fifo_and_special_dma_refills_it() {
        let mut bus = bus_with_rom(&[0; 4]);
        // 32 bytes of audio data in EWRAM.
        for index in 0..32 {
            bus.write_u8(EWRAM_START + index, (index as u8).wrapping_add(1))
                .unwrap();
        }
        // Sound on; FIFO A: 100%, both sides, timer 0.
        bus.write_u8(IO_REG_START + 0x84, 0x80).unwrap();
        bus.write_u16(IO_REG_START + 0x82, 0x0304).unwrap();
        // DMA1: special timing, repeat, 32-bit, fixed destination FIFO A.
        program_dma(
            &mut bus,
            1,
            EWRAM_START,
            IO_REG_START + 0xA0,
            4,
            0x8000 | (3 << 12) | (1 << 9) | (1 << 10) | (2 << 5),
        );
        // Nothing transferred until the FIFO requests data.
        assert_eq!(bus.apu.fifo_len(0), 0);

        // Timer 0: overflow every 512 cycles (reload 0xFE00), no prescaler.
        bus.write_u16(IO_REG_START + 0x100, 0xFE00).unwrap();
        bus.write_u16(IO_REG_START + 0x102, 0x0080).unwrap();
        bus.advance(512);

        // The first overflow found the FIFO empty, requested DMA, and four
        // words (16 bytes) arrived.
        assert_eq!(bus.apu.fifo_len(0), 16);
        let samples = bus.take_audio_samples();
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn audio_registers_round_trip_and_fifo_reads_zero() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(IO_REG_START + 0x80, 0xFF77).unwrap();
        assert_eq!(bus.read_u16(IO_REG_START + 0x80).unwrap(), 0xFF77);
        bus.write_u32(IO_REG_START + 0xA0, 0x7F7F_7F7F).unwrap();
        assert_eq!(bus.read_u32(IO_REG_START + 0xA0).unwrap(), 0);
        assert_eq!(bus.apu.fifo_len(0), 4);
        bus.write_u8(IO_REG_START + 0x84, 0x80).unwrap();
        assert_eq!(bus.read_u8(IO_REG_START + 0x84).unwrap() & 0x80, 0x80);
    }

    #[test]
    fn keyinput_is_active_low_for_each_button() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.set_pressed_buttons(crate::input::START | crate::input::L);
        let value = bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap();
        assert_eq!(value, 0x03FF & !(1 << 3) & !(1 << 9));
        bus.set_pressed_buttons(0);
        assert_eq!(
            bus.read_u16(IO_REG_START + KEYINPUT as u32).unwrap(),
            0x03FF
        );
    }

    fn keypad_flag(bus: &MemoryBus) -> bool {
        bus.io_regs[INTERRUPT_REQUEST + 1] & KEYPAD_IRQ_BIT != 0
    }

    #[test]
    fn keypad_irq_or_mode_fires_once_when_any_selected_button_is_pressed() {
        let mut bus = bus_with_rom(&[0; 4]);
        let selected = crate::input::A | crate::input::B;
        bus.write_u16(IO_REG_START + KEYCNT as u32, (1 << 14) | selected)
            .unwrap();

        bus.set_pressed_buttons(crate::input::START);
        assert!(!keypad_flag(&bus));
        bus.set_pressed_buttons(crate::input::B);
        assert!(keypad_flag(&bus));

        // Acknowledge; holding the button must not retrigger.
        bus.write_u16(IO_REG_START + INTERRUPT_REQUEST as u32, 1 << 12)
            .unwrap();
        bus.set_pressed_buttons(crate::input::B | crate::input::START);
        assert!(!keypad_flag(&bus));
        // Release and press again.
        bus.set_pressed_buttons(0);
        bus.set_pressed_buttons(crate::input::A);
        assert!(keypad_flag(&bus));
    }

    #[test]
    fn keypad_irq_and_mode_requires_every_selected_button() {
        let mut bus = bus_with_rom(&[0; 4]);
        let combo = crate::input::A | crate::input::B | crate::input::START | crate::input::SELECT;
        bus.write_u16(IO_REG_START + KEYCNT as u32, (1 << 15) | (1 << 14) | combo)
            .unwrap();
        bus.set_pressed_buttons(crate::input::A | crate::input::B | crate::input::START);
        assert!(!keypad_flag(&bus));
        bus.set_pressed_buttons(combo);
        assert!(keypad_flag(&bus));
    }

    #[test]
    fn keypad_irq_is_disabled_without_the_enable_bit() {
        let mut bus = bus_with_rom(&[0; 4]);
        bus.write_u16(IO_REG_START + KEYCNT as u32, crate::input::A)
            .unwrap();
        bus.set_pressed_buttons(crate::input::A);
        assert!(!keypad_flag(&bus));
    }

    fn flash_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x200];
        rom[0x100..0x10C].copy_from_slice(b"FLASH1M_V103");
        rom
    }

    fn flash_command(bus: &mut MemoryBus, address: u32, value: u8) {
        bus.write_u8(0x0E00_5555, 0xAA).unwrap();
        bus.write_u8(0x0E00_2AAA, 0x55).unwrap();
        bus.write_u8(0x0E00_0000 + address, value).unwrap();
    }

    #[test]
    fn flash_is_detected_and_answers_the_id_command_through_the_bus() {
        let mut bus = bus_with_rom(&flash_rom());
        assert_eq!(bus.save_kind(), SaveKind::Flash128);

        flash_command(&mut bus, 0x5555, 0x90);
        assert_eq!(bus.read_u8(0x0E00_0000).unwrap(), 0xC2);
        assert_eq!(bus.read_u8(0x0E00_0001).unwrap(), 0x09);
        flash_command(&mut bus, 0x5555, 0xF0);
        assert_eq!(bus.read_u8(0x0E00_0000).unwrap(), 0xFF);
    }

    #[test]
    fn flash_program_marks_dirty_and_persists_through_load() {
        let mut bus = bus_with_rom(&flash_rom());
        assert!(!bus.take_save_dirty());

        flash_command(&mut bus, 0x5555, 0xA0);
        bus.write_u8(0x0E00_0010, 0x42).unwrap();
        assert!(bus.take_save_dirty());
        assert_eq!(bus.save_data()[0x10], 0x42);

        let image = bus.save_data().to_vec();
        let mut restored = bus_with_rom(&flash_rom());
        restored.load_save_data(&image);
        assert_eq!(restored.read_u8(0x0E00_0010).unwrap(), 0x42);
        assert!(!restored.take_save_dirty());
    }

    #[test]
    fn backup_memory_uses_an_8_bit_bus() {
        let mut bus = bus_with_rom(&[0; 4]); // no tag: SRAM
        assert_eq!(bus.save_kind(), SaveKind::Sram);
        bus.write_u8(0x0E00_0000, 0xAB).unwrap();
        bus.write_u8(0x0E00_0001, 0xCD).unwrap();

        // Wider reads see the addressed byte on every lane.
        assert_eq!(bus.read_u16(0x0E00_0000).unwrap(), 0xABAB);
        assert_eq!(bus.read_u32(0x0E00_0001).unwrap(), 0xCDCD_CDCD);

        // Wider writes store only the addressed lane.
        bus.write_u32(0x0E00_0010, 0x4433_2211).unwrap();
        assert_eq!(bus.read_u8(0x0E00_0010).unwrap(), 0x11);
        assert_eq!(bus.read_u8(0x0E00_0011).unwrap(), 0xFF);
        bus.write_u16(0x0E00_0021, 0x7766).unwrap();
        assert_eq!(bus.read_u8(0x0E00_0021).unwrap(), 0x77);
        assert_eq!(bus.read_u8(0x0E00_0020).unwrap(), 0xFF);
    }
}
