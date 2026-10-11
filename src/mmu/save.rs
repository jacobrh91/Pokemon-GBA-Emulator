//! Cartridge backup memory: SRAM and 64/128 KiB Flash (GBATEK "GBA Cart
//! Backup Media", mGBA `savedata.c`). EEPROM, which is accessed through the
//! ROM address space with a serial protocol, is not emulated.

const SRAM_SIZE: usize = 32 * 1024;
const FLASH_64K: usize = 64 * 1024;
const FLASH_128K: usize = 128 * 1024;
const BANK_SIZE: usize = 64 * 1024;
const SECTOR_SIZE: usize = 4 * 1024;

const COMMAND_ADDRESS_1: usize = 0x5555;
const COMMAND_ADDRESS_2: usize = 0x2AAA;

/// Macronix MX29L010 (128 KiB) and MX29L512 (64 KiB), which every game that
/// supports Flash accepts.
const MACRONIX_MANUFACTURER: u8 = 0xC2;
const MACRONIX_128K_DEVICE: u8 = 0x09;
const MACRONIX_64K_DEVICE: u8 = 0x1C;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind {
    /// No recognised backup library: treated as 32 KiB SRAM, the most
    /// common type and what homebrew usually expects.
    Sram,
    Flash64,
    Flash128,
    /// Detected but unsupported; reads return 0xFF and writes are ignored.
    Eeprom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sequence {
    Idle,
    /// 0xAA written to 0x5555.
    Unlocked1,
    /// 0x55 written to 0x2AAA; the next write is a command.
    Unlocked2,
}

pub struct Savedata {
    kind: SaveKind,
    data: Vec<u8>,
    sequence: Sequence,
    identify: bool,
    erase_armed: bool,
    program_armed: bool,
    bank_switch_armed: bool,
    bank: usize,
    dirty: bool,
}

impl Savedata {
    /// Chooses the backup type from the ID strings that Nintendo's save
    /// libraries embed in the ROM (word aligned).
    pub fn detect(rom: &[u8]) -> Self {
        Self::with_kind(detect_kind(rom))
    }

    pub fn with_kind(kind: SaveKind) -> Self {
        let size = match kind {
            SaveKind::Sram | SaveKind::Eeprom => SRAM_SIZE,
            SaveKind::Flash64 => FLASH_64K,
            SaveKind::Flash128 => FLASH_128K,
        };
        Self {
            kind,
            data: vec![0xFF; size],
            sequence: Sequence::Idle,
            identify: false,
            erase_armed: false,
            program_armed: false,
            bank_switch_armed: false,
            bank: 0,
            dirty: false,
        }
    }

    pub fn kind(&self) -> SaveKind {
        self.kind
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Replaces the contents with a previously saved image. A short image is
    /// padded with erased bytes and a long one truncated.
    pub fn load(&mut self, image: &[u8]) {
        let length = self.data.len().min(image.len());
        self.data.fill(0xFF);
        self.data[..length].copy_from_slice(&image[..length]);
        self.dirty = false;
    }

    /// True if the contents changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn read(&self, address: u32) -> u8 {
        let offset = address as usize & 0xFFFF;
        match self.kind {
            SaveKind::Eeprom => 0xFF,
            SaveKind::Sram => self.data[offset & (SRAM_SIZE - 1)],
            SaveKind::Flash64 | SaveKind::Flash128 => {
                if self.identify {
                    match offset {
                        0 => return MACRONIX_MANUFACTURER,
                        1 => return self.device_id(),
                        _ => {}
                    }
                }
                self.data[self.flash_index(offset)]
            }
        }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let offset = address as usize & 0xFFFF;
        match self.kind {
            SaveKind::Eeprom => {}
            SaveKind::Sram => {
                let index = offset & (SRAM_SIZE - 1);
                if self.data[index] != value {
                    self.data[index] = value;
                    self.dirty = true;
                }
            }
            SaveKind::Flash64 | SaveKind::Flash128 => self.write_flash(offset, value),
        }
    }

    fn device_id(&self) -> u8 {
        if self.kind == SaveKind::Flash128 {
            MACRONIX_128K_DEVICE
        } else {
            MACRONIX_64K_DEVICE
        }
    }

    fn flash_index(&self, offset: usize) -> usize {
        self.bank * BANK_SIZE + offset
    }

    fn write_flash(&mut self, offset: usize, value: u8) {
        match self.sequence {
            Sequence::Idle => {
                // An armed operation consumes the next write as its data,
                // even if the byte looks like a command (0xAA, 0xF0).
                if self.program_armed {
                    // Programming can only clear bits; erase sets them.
                    let index = self.flash_index(offset);
                    let programmed = self.data[index] & value;
                    if programmed != self.data[index] {
                        self.data[index] = programmed;
                        self.dirty = true;
                    }
                    self.program_armed = false;
                } else if self.bank_switch_armed {
                    if offset == 0 && self.kind == SaveKind::Flash128 {
                        self.bank = usize::from(value & 1);
                    }
                    self.bank_switch_armed = false;
                } else if offset == COMMAND_ADDRESS_1 && value == 0xAA {
                    self.sequence = Sequence::Unlocked1;
                } else if value == 0xF0 {
                    // Reset: leave ID mode and drop any half-finished command.
                    self.identify = false;
                    self.erase_armed = false;
                }
            }
            Sequence::Unlocked1 => {
                self.sequence = if offset == COMMAND_ADDRESS_2 && value == 0x55 {
                    Sequence::Unlocked2
                } else {
                    Sequence::Idle
                };
            }
            Sequence::Unlocked2 => {
                self.sequence = Sequence::Idle;
                self.run_command(offset, value);
            }
        }
    }

    fn run_command(&mut self, offset: usize, value: u8) {
        if self.erase_armed {
            match value {
                0x10 if offset == COMMAND_ADDRESS_1 => {
                    self.data.fill(0xFF);
                    self.dirty = true;
                }
                0x30 => {
                    let start = self.flash_index(offset & !(SECTOR_SIZE - 1));
                    self.data[start..start + SECTOR_SIZE].fill(0xFF);
                    self.dirty = true;
                }
                _ => {}
            }
            self.erase_armed = false;
            return;
        }
        if offset != COMMAND_ADDRESS_1 {
            return;
        }
        match value {
            0x90 => self.identify = true,
            0xF0 => self.identify = false,
            0x80 => self.erase_armed = true,
            0xA0 => self.program_armed = true,
            0xB0 => self.bank_switch_armed = true,
            _ => {}
        }
    }
}

fn detect_kind(rom: &[u8]) -> SaveKind {
    // Longest/most specific tags first: "FLASH1M_V" must win over "FLASH_V".
    const TAGS: [(&[u8], SaveKind); 6] = [
        (b"FLASH1M_V", SaveKind::Flash128),
        (b"FLASH512_V", SaveKind::Flash64),
        (b"FLASH_V", SaveKind::Flash64),
        (b"EEPROM_V", SaveKind::Eeprom),
        (b"SRAM_F_V", SaveKind::Sram),
        (b"SRAM_V", SaveKind::Sram),
    ];
    for (tag, kind) in TAGS {
        let found = rom
            .windows(tag.len())
            .step_by(4)
            .any(|window| window == tag);
        if found {
            return kind;
        }
    }
    SaveKind::Sram
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(save: &mut Savedata, address: u32, value: u8) {
        save.write(0x5555, 0xAA);
        save.write(0x2AAA, 0x55);
        save.write(address, value);
    }

    fn flash() -> Savedata {
        Savedata::with_kind(SaveKind::Flash128)
    }

    fn program(save: &mut Savedata, address: u32, value: u8) {
        command(save, 0x5555, 0xA0);
        save.write(address, value);
    }

    #[test]
    fn detects_backup_type_from_library_strings() {
        let with = |tag: &[u8]| {
            let mut rom = vec![0u8; 64];
            rom[16..16 + tag.len()].copy_from_slice(tag);
            detect_kind(&rom)
        };
        assert_eq!(with(b"FLASH1M_V103"), SaveKind::Flash128);
        assert_eq!(with(b"FLASH512_V131"), SaveKind::Flash64);
        assert_eq!(with(b"FLASH_V126"), SaveKind::Flash64);
        assert_eq!(with(b"EEPROM_V124"), SaveKind::Eeprom);
        assert_eq!(with(b"SRAM_V113"), SaveKind::Sram);
        assert_eq!(detect_kind(&[0; 64]), SaveKind::Sram);
        // Unaligned tags are not library markers.
        let mut rom = vec![0u8; 64];
        rom[17..26].copy_from_slice(b"FLASH1M_V");
        assert_eq!(detect_kind(&rom), SaveKind::Sram);
    }

    #[test]
    fn id_mode_reports_the_chip_and_exits_on_reset() {
        let mut save = flash();
        assert_eq!(save.read(0), 0xFF);
        command(&mut save, 0x5555, 0x90);
        assert_eq!(save.read(0), MACRONIX_MANUFACTURER);
        assert_eq!(save.read(1), MACRONIX_128K_DEVICE);
        save.write(0x5555, 0xF0);
        assert_eq!(save.read(0), 0xFF);

        let mut small = Savedata::with_kind(SaveKind::Flash64);
        command(&mut small, 0x5555, 0x90);
        assert_eq!(small.read(1), MACRONIX_64K_DEVICE);
        command(&mut small, 0x5555, 0xF0);
        assert_eq!(small.read(0), 0xFF);
    }

    #[test]
    fn program_requires_the_unlock_sequence_and_only_clears_bits() {
        let mut save = flash();
        // A bare write does nothing.
        save.write(0x0100, 0x12);
        assert_eq!(save.read(0x0100), 0xFF);

        program(&mut save, 0x0100, 0xF0);
        assert_eq!(save.read(0x0100), 0xF0);
        assert!(save.take_dirty());
        // Reprogramming without erasing ANDs with the stored value.
        program(&mut save, 0x0100, 0x3C);
        assert_eq!(save.read(0x0100), 0x30);
        // Only one byte is programmed per command.
        save.write(0x0101, 0x00);
        assert_eq!(save.read(0x0101), 0xFF);
    }

    #[test]
    fn data_bytes_that_look_like_commands_are_programmed_not_executed() {
        let mut save = flash();
        program(&mut save, 0x0200, 0xAA);
        assert_eq!(save.read(0x0200), 0xAA);
        program(&mut save, 0x0201, 0xF0);
        assert_eq!(save.read(0x0201), 0xF0);
        // A data byte of 0xAA at 0x5555 must not start an unlock sequence.
        program(&mut save, 0x5555, 0xAA);
        save.write(0x2AAA, 0x55);
        save.write(0x5555, 0xA0);
        save.write(0x0300, 0x00);
        assert_eq!(save.read(0x0300), 0xFF);
    }

    #[test]
    fn sector_erase_resets_exactly_one_4k_sector() {
        let mut save = flash();
        program(&mut save, 0x0000, 0x00);
        program(&mut save, 0x0FFF, 0x00);
        program(&mut save, 0x1000, 0x00);

        command(&mut save, 0x5555, 0x80);
        command(&mut save, 0x0800, 0x30); // any address inside sector 0

        assert_eq!(save.read(0x0000), 0xFF);
        assert_eq!(save.read(0x0FFF), 0xFF);
        assert_eq!(save.read(0x1000), 0x00, "next sector untouched");
    }

    #[test]
    fn chip_erase_clears_both_banks() {
        let mut save = flash();
        program(&mut save, 0x0010, 0x00);
        command(&mut save, 0x5555, 0xB0);
        save.write(0x0000, 1);
        program(&mut save, 0x0010, 0x00);

        command(&mut save, 0x5555, 0x80);
        command(&mut save, 0x5555, 0x10);

        assert!(save.data().iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn bank_switching_exposes_the_second_64k_of_a_128k_chip() {
        let mut save = flash();
        program(&mut save, 0x0042, 0x11);
        command(&mut save, 0x5555, 0xB0);
        save.write(0x0000, 1);
        assert_eq!(save.read(0x0042), 0xFF);
        program(&mut save, 0x0042, 0x22);
        assert_eq!(save.data()[BANK_SIZE + 0x42], 0x22);

        command(&mut save, 0x5555, 0xB0);
        save.write(0x0000, 0);
        assert_eq!(save.read(0x0042), 0x11);
    }

    #[test]
    fn a_broken_unlock_sequence_is_ignored() {
        let mut save = flash();
        save.write(0x5555, 0xAA);
        save.write(0x1234, 0x55); // wrong address
        save.write(0x5555, 0xA0);
        save.write(0x0000, 0x00);
        assert_eq!(save.read(0x0000), 0xFF);
    }

    #[test]
    fn sram_is_plain_32k_memory_mirrored_across_the_window() {
        let mut save = Savedata::with_kind(SaveKind::Sram);
        save.write(0x0000_0123, 0x5A);
        assert_eq!(save.read(0x0123), 0x5A);
        assert_eq!(save.read(0x8123), 0x5A, "32 KiB mirror");
        assert!(save.take_dirty());
        assert!(!save.take_dirty());
        // Writing the same value does not mark it dirty again.
        save.write(0x0123, 0x5A);
        assert!(!save.take_dirty());
    }

    #[test]
    fn load_pads_and_truncates_and_clears_dirty() {
        let mut save = flash();
        program(&mut save, 0, 0);
        save.load(&[1, 2, 3]);
        assert_eq!(&save.data()[..4], &[1, 2, 3, 0xFF]);
        assert_eq!(save.data().len(), FLASH_128K);
        assert!(!save.take_dirty());

        save.load(&vec![7; FLASH_128K + 100]);
        assert_eq!(save.data().len(), FLASH_128K);
        assert_eq!(save.data()[FLASH_128K - 1], 7);
    }
}
