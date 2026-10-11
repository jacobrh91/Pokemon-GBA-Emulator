//! Helpers shared by the integration tests.
#![allow(dead_code)]

use gba_emulator::emulator::Emulator;
use gba_emulator::input;
use gba_emulator::mmu::ROM_START;
use std::path::PathBuf;

pub fn rom_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join(name)
}

pub fn new_emulator(rom: &str) -> Emulator {
    let path = rom_path(rom);
    let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
    emulator.initialize_gba_startup(ROM_START).unwrap();
    emulator
}

/// Button presses `(first frame, buttons, frames held)` that walk through the
/// Armwrestler menu and start a test, so more than the idle menu is exercised.
pub const MENU_SCRIPT: [(u64, input::Buttons, u64); 4] = [
    (20, input::DOWN, 4),
    (40, input::START, 4),
    (80, input::START, 4),
    (110, input::A, 4),
];

pub fn buttons_at(frame: u64) -> input::Buttons {
    MENU_SCRIPT
        .iter()
        .filter(|(first, _, hold)| (*first..*first + *hold).contains(&frame))
        .fold(0, |buttons, (_, pressed, _)| buttons | pressed)
}

/// Runs one frame with the scripted buttons for `frame`.
pub fn run_frame(emulator: &mut Emulator, frame: u64) {
    emulator.set_buttons(buttons_at(frame));
    while !emulator.take_frame_ready() {
        emulator.step().unwrap();
    }
}

pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Everything observable about the machine at a frame boundary.
#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub cycles: u64,
    pub registers: [u32; 16],
    pub cpsr: u32,
    pub iwram: u64,
    pub ewram: u64,
    pub vram: u64,
    pub palette: u64,
    pub oam: u64,
    pub io: u64,
    pub framebuffer: u64,
    pub audio: u64,
}

/// Captures the state (this takes the pending audio samples).
pub fn snapshot(emulator: &mut Emulator) -> Snapshot {
    let audio: Vec<u8> = emulator
        .take_audio_samples()
        .iter()
        .flatten()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let framebuffer: Vec<u8> = emulator
        .ppu()
        .framebuffer()
        .iter()
        .flatten()
        .copied()
        .collect();
    let memory = emulator.memory();
    Snapshot {
        cycles: emulator.cpu().cycles,
        registers: emulator.cpu().registers,
        cpsr: emulator.cpu().cpsr,
        iwram: hash(&memory.iwram[..]),
        ewram: hash(&memory.ewram[..]),
        vram: hash(&memory.vram[..]),
        palette: hash(&memory.palette_ram[..]),
        oam: hash(&memory.oam[..]),
        io: hash(&memory.io_regs[..]),
        framebuffer: hash(&framebuffer),
        audio: hash(&audio),
    }
}
