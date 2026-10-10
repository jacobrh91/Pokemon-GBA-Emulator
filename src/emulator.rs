use crate::cpu::Cpu;
use crate::mmu::MemoryBus;
use crate::ppu::Ppu;
use std::io;

const MODE_SUPERVISOR: u32 = 0x13;
const THUMB_STATE: u32 = 1 << 5;

pub struct Emulator {
    cpu: Cpu,
    memory: MemoryBus,
    ppu: Ppu,
}

impl Emulator {
    /// Creates an emulator with a mapped ROM and default CPU/PPU state.
    ///
    /// Initializes the bus and devices without configuring CPU boot state.
    /// Call `initialize_gba_startup` or set the CPU PC and execution mode
    /// before stepping.
    pub fn new(rom_path: &str) -> io::Result<Self> {
        Ok(Self {
            cpu: Cpu::default(),
            memory: MemoryBus::new(rom_path)?,
            ppu: Ppu::default(),
        })
    }

    pub fn step_thumb(&mut self) -> io::Result<u32> {
        self.cpu.step_thumb(&mut self.memory)
    }

    pub fn initialize_gba_startup(&mut self, entry_point: u32) -> io::Result<()> {
        self.cpu.initialize_gba_startup(entry_point)
    }

    pub fn step(&mut self) -> io::Result<u32> {
        if self.cpu.mode() == MODE_SUPERVISOR && self.cpu.registers[15] == 0x08 {
            let saved_status = self.cpu.saved_status()?;
            let return_address = self.cpu.registers[14];
            let service = if saved_status & THUMB_STATE != 0 {
                let address = return_address.checked_sub(2).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid Thumb SWI return address",
                    )
                })?;
                let opcode = self.memory.read_u16(address)?;
                if opcode & 0xFF00 != 0xDF00 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "SVC vector return address does not point after a Thumb SWI",
                    ));
                }
                opcode as u8
            } else {
                let address = return_address.checked_sub(4).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid ARM SWI return address")
                })?;
                let opcode = self.memory.read_u32(address)?;
                if opcode & 0x0F00_0000 != 0x0F00_0000 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "SVC vector return address does not point after an ARM SWI",
                    ));
                }
                opcode as u8
            };
            self.cpu.handle_bios_swi(service, &mut self.memory)?;
            return Ok(0);
        }

        let mut cycles = if self.cpu.cpsr & (1 << 5) != 0 {
            self.step_thumb()
        } else {
            self.cpu.step_arm(&mut self.memory)
        }?;
        self.memory.advance(cycles);
        if self.memory.irq_pending() && self.cpu.request_irq(&self.memory)? {
            self.memory.advance(3);
            cycles += 3;
        }
        Ok(cycles)
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    pub fn memory(&self) -> &MemoryBus {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut MemoryBus {
        &mut self.memory
    }

    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    pub fn render_frame(&mut self) {
        self.ppu.render_frame(&self.memory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mmu::IO_REG_START;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);
    const ROM_START: u32 = 0x0800_0000;

    fn rom_file(opcode: u16) -> std::path::PathBuf {
        let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "gba-emulator-system-test-{}-{id}.gba",
            std::process::id()
        ));
        let mut rom = vec![0; 8];
        rom[..2].copy_from_slice(&opcode.to_le_bytes());
        rom[2..4].copy_from_slice(&0x2103u16.to_le_bytes());
        fs::write(&path, rom).unwrap();
        path
    }

    fn arm_rom_file(instruction: u32) -> std::path::PathBuf {
        let id = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "gba-emulator-system-arm-test-{}-{id}.gba",
            std::process::id()
        ));
        fs::write(&path, instruction.to_le_bytes()).unwrap();
        path
    }

    #[test]
    fn system_owner_steps_cpu_against_its_rom_bus() {
        let path = rom_file(0x2007);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.cpu_mut().registers[15] = ROM_START;

        assert_eq!(emulator.step_thumb().unwrap(), 4);
        assert_eq!(emulator.cpu().registers[0], 7);
        assert_eq!(emulator.cpu().registers[15], ROM_START + 2);
        assert_eq!(emulator.cpu().cycles, 4);
    }

    #[test]
    fn system_owner_dispatches_arm_mode_from_cpsr() {
        let path = arm_rom_file(0xE3A0_0007);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.cpu_mut().registers[15] = ROM_START;

        assert_eq!(emulator.step().unwrap(), 6);
        assert_eq!(emulator.cpu().registers[0], 7);
        assert_eq!(emulator.cpu().registers[15], ROM_START + 4);
    }

    #[test]
    fn gba_startup_initializes_cpu_mode_and_banked_stacks() {
        let path = arm_rom_file(0xE1A0_0000);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();

        emulator.initialize_gba_startup(ROM_START).unwrap();
        assert_eq!(emulator.cpu().mode(), 0x1F);
        assert_eq!(emulator.cpu().registers[13], 0x0300_7F00);
        assert_eq!(emulator.cpu().registers[15], ROM_START);

        emulator.cpu_mut().set_cpsr(0x12).unwrap();
        assert_eq!(emulator.cpu().registers[13], 0x0300_7FA0);
        emulator.cpu_mut().set_cpsr(0x13).unwrap();
        assert_eq!(emulator.cpu().registers[13], 0x0300_7FE0);
        emulator.cpu_mut().set_cpsr(0x1F).unwrap();
        assert_eq!(emulator.cpu().registers[13], 0x0300_7F00);
    }

    #[test]
    fn soft_reset_restarts_at_the_cartridge_entry_in_system_mode() {
        let path = rom_file(0xDF00);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();
        emulator.cpu_mut().registers[15] = ROM_START;

        emulator.step().unwrap();
        assert_eq!(emulator.cpu().registers[15], 0x08);
        assert_eq!(emulator.cpu().mode(), MODE_SUPERVISOR);

        assert_eq!(emulator.step().unwrap(), 0);
        assert_eq!(emulator.cpu().registers[15], ROM_START);
        assert_eq!(emulator.cpu().mode(), 0x1F);
        assert_eq!(emulator.cpu().registers[13], 0x0300_7F00);
        assert_eq!(emulator.cpu().registers[0], 0);
    }

    #[test]
    fn system_handles_thumb_register_ram_reset_swi_and_returns_from_svc() {
        let path = rom_file(0xDF01);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.cpu_mut().registers[15] = ROM_START;
        emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();
        emulator.cpu_mut().registers[0] = 0x03;
        emulator
            .memory_mut()
            .write_u8(crate::mmu::EWRAM_START, 0xA5)
            .unwrap();
        emulator
            .memory_mut()
            .write_u8(crate::mmu::IWRAM_START, 0x5A)
            .unwrap();

        emulator.step().unwrap();
        assert_eq!(emulator.cpu().mode(), MODE_SUPERVISOR);
        assert_eq!(emulator.cpu().registers[15], 0x08);

        assert_eq!(emulator.step().unwrap(), 0);
        assert_eq!(emulator.cpu().mode(), 0x1F);
        assert_eq!(emulator.cpu().cpsr & THUMB_STATE, THUMB_STATE);
        assert_eq!(emulator.cpu().registers[15], ROM_START + 2);
        assert_eq!(
            emulator.memory().read_u8(crate::mmu::EWRAM_START).unwrap(),
            0
        );
        assert_eq!(
            emulator.memory().read_u8(crate::mmu::IWRAM_START).unwrap(),
            0
        );
    }

    #[test]
    fn system_handles_thumb_vblank_wait_without_a_ppu_clock() {
        let path = rom_file(0xDF05);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.cpu_mut().registers[15] = ROM_START;
        emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();

        emulator.step().unwrap();
        assert_eq!(emulator.cpu().registers[15], 0x08);
        assert_eq!(emulator.step().unwrap(), 0);
        assert_eq!(emulator.cpu().mode(), 0x1F);
        assert_eq!(emulator.cpu().cpsr & THUMB_STATE, THUMB_STATE);
        assert_eq!(emulator.cpu().registers[15], ROM_START + 2);
    }

    #[test]
    fn system_handles_thumb_cpuset_copy_and_word_fill() {
        let path = rom_file(0xDF0B);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.cpu_mut().registers[15] = ROM_START;
        emulator.cpu_mut().set_cpsr(0x1F | THUMB_STATE).unwrap();
        emulator
            .memory_mut()
            .write_u16(crate::mmu::EWRAM_START, 0x1234)
            .unwrap();
        emulator
            .memory_mut()
            .write_u16(crate::mmu::EWRAM_START + 2, 0x5678)
            .unwrap();
        emulator.cpu_mut().registers[0] = crate::mmu::EWRAM_START;
        emulator.cpu_mut().registers[1] = crate::mmu::IWRAM_START;
        emulator.cpu_mut().registers[2] = 2;

        emulator.step().unwrap();
        emulator.step().unwrap();
        assert_eq!(
            emulator.memory().read_u32(crate::mmu::IWRAM_START).unwrap(),
            0x5678_1234
        );

        emulator.cpu_mut().registers[15] = ROM_START;
        emulator.cpu_mut().registers[0] = crate::mmu::EWRAM_START;
        emulator.cpu_mut().registers[1] = crate::mmu::IWRAM_START + 8;
        emulator.cpu_mut().registers[2] = (1 << 26) | (1 << 24) | 2;
        emulator.step().unwrap();
        emulator.step().unwrap();
        assert_eq!(
            emulator
                .memory()
                .read_u32(crate::mmu::IWRAM_START + 8)
                .unwrap(),
            0x5678_1234
        );
        assert_eq!(
            emulator
                .memory()
                .read_u32(crate::mmu::IWRAM_START + 12)
                .unwrap(),
            0x5678_1234
        );
    }

    #[test]
    fn system_decodes_arm_swi_service_from_low_byte() {
        let path = arm_rom_file(0xEF00_0001);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        emulator.cpu_mut().registers[0] = 1;
        emulator
            .memory_mut()
            .write_u8(crate::mmu::EWRAM_START, 0xA5)
            .unwrap();

        emulator.step().unwrap();
        assert_eq!(emulator.cpu().registers[15], 0x08);
        emulator.step().unwrap();

        assert_eq!(emulator.cpu().mode(), 0x1F);
        assert_eq!(emulator.cpu().registers[15], ROM_START + 4);
        assert_eq!(
            emulator.memory().read_u8(crate::mmu::EWRAM_START).unwrap(),
            0
        );
    }

    #[test]
    fn system_advances_timers_and_enters_irq_when_enabled() {
        let path = arm_rom_file(0xE1A0_0000);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.cpu_mut().registers[15] = ROM_START;
        let cpsr = emulator.cpu().cpsr & !(1 << 7);
        emulator.cpu_mut().set_cpsr(cpsr).unwrap();

        emulator
            .memory_mut()
            .write_u16(IO_REG_START + 0x100, 0xFFFE)
            .unwrap();
        emulator
            .memory_mut()
            .write_u16(IO_REG_START + 0x102, 0x00C0)
            .unwrap();
        emulator
            .memory_mut()
            .write_u16(IO_REG_START + 0x200, 1 << 3)
            .unwrap();
        emulator
            .memory_mut()
            .write_u8(IO_REG_START + 0x208, 1)
            .unwrap();

        assert_eq!(emulator.step().unwrap(), 9);
        assert_eq!(emulator.cpu().mode(), 0x12);
        assert_eq!(emulator.cpu().registers[15], 0x18);
        assert_eq!(emulator.cpu().cycles, 9);
    }

    #[test]
    fn vblank_irq_dispatches_to_the_bios_handler_pointer() {
        let path = arm_rom_file(0xE1A0_0000);
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        let handler = 0x0300_3580;
        emulator
            .memory_mut()
            .write_u32(0x0300_7FFC, handler)
            .unwrap();
        emulator
            .memory_mut()
            .write_u16(IO_REG_START + 0x004, 1 << 3)
            .unwrap();
        emulator
            .memory_mut()
            .write_u16(IO_REG_START + 0x200, 1)
            .unwrap();
        emulator
            .memory_mut()
            .write_u16(IO_REG_START + 0x208, 1)
            .unwrap();
        emulator.memory_mut().advance(1232 * 160 - 1);

        emulator.step().unwrap();

        assert_eq!(emulator.cpu().mode(), 0x12);
        assert_eq!(emulator.cpu().registers[15], handler);
        assert_eq!(emulator.cpu().saved_status().unwrap() & 0x1F, 0x1F);
        assert_eq!(
            emulator.memory().read_u16(IO_REG_START + 0x202).unwrap() & 1,
            1
        );
    }
}
