//! Runs a ROM and writes the presented framebuffer of selected frames as PPM.
//! usage: screenshot ROM OUT_DIR FRAME[,FRAME...]
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
    let mut frame = 0u64;
    while frame <= last {
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
        }
        frame += 1;
    }
}
