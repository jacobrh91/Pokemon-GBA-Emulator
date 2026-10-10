//! Headless boot probe: runs a ROM for a bounded number of frames and reports
//! where emulation stops and what the PPU produced.
use gba_emulator::emulator::Emulator;
use gba_emulator::mmu::ROM_START;
use std::collections::HashMap;

fn main() {
    let mut args = std::env::args().skip(1);
    let rom = args.next().expect("rom path");
    let frames: u64 = args.next().map(|v| v.parse().unwrap()).unwrap_or(60);
    let mut emu = Emulator::new(&rom).unwrap();
    emu.initialize_gba_startup(ROM_START).unwrap();
    let per_frame = 1232 * 228;
    let mut instructions = 0u64;
    let mut last_ptr = 0u32;
    let mut last_buf = 0u16;
    let mut last_dispstat = (0u16, 0u16);
    let mut ring: Vec<(u32, u32, u32)> = Vec::new();
    let mut prev = ([0u32; 16], 0u32);
    let mut hist: HashMap<u32, u64> = HashMap::new();
    for frame in 0..frames {
        let start = emu.cpu().cycles;
        while emu.cpu().cycles - start < per_frame {
            let pc = emu.cpu().registers[15];
            if pc < 0x20 && pc != 0x08 && std::env::var("TRAP").is_ok() {
                println!(
                    "TRAP vector {pc:#X} at frame {frame} insn {instructions}, lr={:#010X} mode={:#X} spsr={:#010X}",
                    emu.cpu().registers[14],
                    emu.cpu().mode(),
                    emu.cpu().saved_status().unwrap_or(0)
                );
                for (p, c, i) in &ring {
                    println!("  trace pc={p:#010X} cpsr={c:#010X} insn={i:#010X}");
                }
                for i in 0..24u32 {
                    let a = 0x0300_3580 + i * 4;
                    println!(
                        "  iwram {a:#010X}: {:#010X}",
                        emu.memory().read_u32(a).unwrap()
                    );
                }
                dump(&emu);
                println!(
                    "ptr@7FFC now = {:#010X}",
                    emu.memory().read_u32(0x0300_7FFC).unwrap()
                );
                println!("prev regs: {:08X?} cpsr={:#010X}", prev.0, prev.1);
                let at = emu.cpu().registers[14].wrapping_sub(8);
                println!(
                    "instr at {at:#010X}: {:#010X}",
                    emu.memory().read_u32(at).unwrap_or(0)
                );
                for i in -12i32..4 {
                    let a = at.wrapping_add((i * 4) as u32);
                    println!(
                        "  {a:#010X}: {:#010X}",
                        emu.memory().read_u32(a).unwrap_or(0)
                    );
                }
                return;
            }
            *hist.entry(pc & !0xFF).or_default() += 1;
            if let Ok(v) = emu.memory().read_u32(0x0300_7FFC) {
                if v != last_ptr {
                    println!(
                        "IRQ ptr {last_ptr:#010X} -> {v:#010X} at insn {instructions} pc={pc:#010X} frame {frame}"
                    );
                    last_ptr = v;
                }
            }
            let seq = ring
                .last()
                .map(|r| pc == r.0.wrapping_add(if r.1 & 0x20 != 0 { 2 } else { 4 }))
                .unwrap_or(false);
            if !seq {
                ring.push((
                    pc,
                    emu.cpu().cpsr,
                    if emu.cpu().cpsr & 0x20 != 0 {
                        emu.memory().read_u16(pc).unwrap_or(0) as u32
                    } else {
                        emu.memory().read_u32(pc).unwrap_or(0)
                    },
                ));
            } else {
                let l = ring.len();
                ring[l - 1].0 = pc;
            }
            if ring.len() > 24 {
                ring.remove(0);
            }
            {
                let v = u16::from_le_bytes([emu.memory().io_regs[4], emu.memory().io_regs[5]]);
                let ie =
                    u16::from_le_bytes([emu.memory().io_regs[0x200], emu.memory().io_regs[0x201]]);
                let buf = emu.memory().read_u16(0x0300_0004).unwrap();
                if buf != last_buf {
                    println!(
                        "BUF[0x03000004] {last_buf:#06X} -> {buf:#06X} at insn {instructions} pc={pc:#010X} lr={:#010X}",
                        emu.cpu().registers[14]
                    );
                    last_buf = buf;
                }
                if (v, ie) != last_dispstat {
                    println!(
                        "DISPSTAT/IE {:#06X}/{:#06X} -> {v:#06X}/{ie:#06X} at insn {instructions} pc={pc:#010X} lr={:#010X}",
                        last_dispstat.0,
                        last_dispstat.1,
                        emu.cpu().registers[14]
                    );
                    last_dispstat = (v, ie);
                }
            }
            if let Ok(r) = std::env::var("TRACE") {
                let (a, b) = r.split_once('-').unwrap();
                if instructions >= a.parse().unwrap() && instructions < b.parse().unwrap() {
                    let thumb = emu.cpu().cpsr & 0x20 != 0;
                    let raw = if thumb {
                        emu.memory().read_u16(pc).unwrap_or(0) as u32
                    } else {
                        emu.memory().read_u32(pc).unwrap_or(0)
                    };
                    println!(
                        "T {instructions} {pc:#010X} {raw:#06X} r0-3={:X?}",
                        &emu.cpu().registers[0..4]
                    );
                }
            }
            prev = (emu.cpu().registers, emu.cpu().cpsr);
            if let Err(e) = emu.step() {
                println!(
                    "STOP frame {frame} insn {instructions} pc={pc:#010X} cpsr={:#010X} mode={:#X}: {e}",
                    emu.cpu().cpsr,
                    emu.cpu().mode()
                );
                println!("regs: {:08X?}", emu.cpu().registers);
                report(&mut emu, &hist);
                return;
            }
            instructions += 1;
        }
        if frame % 20 == 0 {
            println!(
                "frame {frame} pc={:#010X} halted={} dispcnt={:#06X}",
                emu.cpu().registers[15],
                emu.cpu().halted,
                emu.memory().read_u16(0x0400_0000).unwrap()
            );
        }
    }
    println!("completed {frames} frames, {instructions} instructions");
    report(&mut emu, &hist);
}

fn dump(emu: &Emulator) {
    // DUMP=<hex addr>:<bytes> writes raw memory to $DUMP_OUT for disassembly.
    if let (Ok(spec), Ok(out)) = (std::env::var("DUMP"), std::env::var("DUMP_OUT")) {
        let (a, n) = spec.split_once(':').unwrap();
        let a = u32::from_str_radix(a, 16).unwrap();
        let bytes: Vec<u8> = (0..n.parse::<u32>().unwrap())
            .map(|i| emu.memory().read_u8(a + i).unwrap_or(0))
            .collect();
        std::fs::write(out, bytes).unwrap();
    }
}

fn report(emu: &mut Emulator, hist: &HashMap<u32, u64>) {
    dump(emu);
    let io = |o: u32| emu.memory().read_u16(0x0400_0000 + o).unwrap();
    println!(
        "cpsr={:#010X} IE={:#06X} IF={:#06X} IME={:#X} DISPSTAT={:#06X} DISPCNT={:#06X} IRQ_PTR={:#010X} BIOS_IF={:#06X} intrCheck={:#06X}",
        emu.cpu().cpsr,
        io(0x200),
        io(0x202),
        io(0x208),
        io(4),
        io(0),
        emu.memory().read_u32(0x0300_7FFC).unwrap(),
        emu.memory().read_u16(0x0300_7FF8).unwrap(),
        emu.memory().read_u16(0x0300_30F0 + 0x1C).unwrap(),
    );
    println!("regs: {:08X?}", emu.cpu().registers);
    let sp = emu.cpu().registers[13];
    for i in 0..6 {
        println!(
            "  [sp+{:#X}] = {:#010X}",
            i * 4,
            emu.memory().read_u32(sp + i * 4).unwrap_or(0)
        );
    }
    emu.render_frame();
    let fb = emu.ppu().framebuffer();
    let mut colors: HashMap<[u8; 4], usize> = HashMap::new();
    for p in fb {
        *colors.entry(*p).or_default() += 1;
    }
    println!("distinct framebuffer colors: {}", colors.len());
    let mut h: Vec<_> = hist.iter().collect();
    h.sort_by(|a, b| b.1.cmp(a.1));
    for (pc, n) in h.iter().take(6) {
        println!("  hot {pc:#010X}: {n}");
    }
}
