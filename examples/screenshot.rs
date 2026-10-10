//! Runs a ROM and writes the presented framebuffer of selected frames as PPM.
//! usage: screenshot ROM OUT_DIR FRAME[,FRAME...]
//! Set PRESS="frame:buttons:hold,..." (e.g. "2000:start:6,2100:a:6") to script input;
//! buttons are joined with '+' (a, b, select, start, right, left, up, down, r, l).
use gba_emulator::emulator::Emulator;
use gba_emulator::mmu::ROM_START;

fn main() {
    let mut args = std::env::args().skip(1);
    let rom = args.next().unwrap();
    let out = args.next().unwrap();
    let frames: Vec<u64> = args
        .next()
        .unwrap()
        .split(',')
        .map(|f| f.parse().unwrap())
        .collect();
    let last = *frames.iter().max().unwrap();
    let mut emu = Emulator::new(&rom).unwrap();
    emu.initialize_gba_startup(ROM_START).unwrap();
    if let Ok(path) = std::env::var("SAVE_IN") {
        emu.load_save(&std::fs::read(path).unwrap());
    }
    let script: Vec<(u64, u16, u64)> = std::env::var("PRESS")
        .unwrap_or_default()
        .split(',')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut parts = entry.split(':');
            let at = parts.next().unwrap().parse().unwrap();
            let mask = parts.next().unwrap().split('+').fold(0, |mask, name| {
                mask | match name {
                    "a" => gba_emulator::input::A,
                    "b" => gba_emulator::input::B,
                    "select" => gba_emulator::input::SELECT,
                    "start" => gba_emulator::input::START,
                    "right" => gba_emulator::input::RIGHT,
                    "left" => gba_emulator::input::LEFT,
                    "up" => gba_emulator::input::UP,
                    "down" => gba_emulator::input::DOWN,
                    "r" => gba_emulator::input::R,
                    "l" => gba_emulator::input::L,
                    other => panic!("unknown button {other}"),
                }
            });
            let hold = parts.next().map_or(6, |h| h.parse().unwrap());
            (at, mask, hold)
        })
        .collect();
    let mut frame = 0u64;
    while frame <= last {
        let held = script
            .iter()
            .filter(|(at, _, hold)| (*at..*at + *hold).contains(&frame))
            .fold(0, |mask, (_, buttons, _)| mask | buttons);
        emu.set_buttons(held);
        while !emu.take_frame_ready() {
            emu.step().unwrap();
        }
        if frames.contains(&frame) {
            let mut data = format!("P6\n240 160\n255\n").into_bytes();
            for p in emu.ppu().framebuffer() {
                data.extend_from_slice(&p[..3]);
            }
            std::fs::write(format!("{out}/frame_{frame}.ppm"), data).unwrap();
            let io = |o: usize| {
                u16::from_le_bytes([emu.memory().io_regs[o], emu.memory().io_regs[o + 1]])
            };
            println!(
                "frame {frame}: DISPCNT={:#06X} BG0CNT={:#06X} BG1CNT={:#06X} BG2CNT={:#06X} BG3CNT={:#06X} VOFS={:03X},{:03X},{:03X},{:03X} HOFS={:03X},{:03X},{:03X},{:03X}",
                io(0),
                io(8),
                io(10),
                io(12),
                io(14),
                io(0x12),
                io(0x16),
                io(0x1A),
                io(0x1E),
                io(0x10),
                io(0x14),
                io(0x18),
                io(0x1C)
            );
            println!(
                "          BLDCNT={:#06X} BLDALPHA={:#06X} BLDY={:#06X} WININ={:#06X} WINOUT={:#06X} WIN0H={:#06X} WIN0V={:#06X} WIN1H={:#06X} WIN1V={:#06X}",
                io(0x50),
                io(0x52),
                io(0x54),
                io(0x48),
                io(0x4A),
                io(0x40),
                io(0x44),
                io(0x42),
                io(0x46)
            );
        }
        frame += 1;
    }
    if let Ok(path) = std::env::var("SAVE_OUT") {
        std::fs::write(&path, emu.memory().save_data()).unwrap();
        println!(
            "wrote {} byte save to {path}",
            emu.memory().save_data().len()
        );
    }
    if let Ok(path) = std::env::var("SAVE_IN") {
        println!("(save was loaded from {path})");
    }
}
