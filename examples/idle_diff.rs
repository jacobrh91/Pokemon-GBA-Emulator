//! Runs two emulators in lockstep, idle-loop skipping on and off, and reports
//! the first frame at which any CPU or memory state differs.
use gba_emulator::emulator::Emulator;
use gba_emulator::mmu::ROM_START;

fn state(emu: &Emulator) -> (u64, [u32; 16], u32, u64, u64) {
    let hash = |bytes: &[u8]| {
        bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        })
    };
    let cpu = emu.cpu();
    (
        cpu.cycles,
        cpu.registers,
        cpu.cpsr,
        hash(&emu.memory().iwram[..]),
        hash(&emu.memory().ewram[..])
            ^ hash(emu.memory().save_data())
            ^ hash(&emu.memory().vram[..]),
    )
}

fn parse_script() -> Vec<(u64, u16, u64)> {
    use gba_emulator::input as i;
    std::env::var("PRESS")
        .unwrap_or_default()
        .split(',')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut parts = entry.split(':');
            let at = parts.next().unwrap().parse().unwrap();
            let mask = parts.next().unwrap().split('+').fold(0, |mask, name| {
                mask | match name {
                    "a" => i::A,
                    "b" => i::B,
                    "select" => i::SELECT,
                    "start" => i::START,
                    "right" => i::RIGHT,
                    "left" => i::LEFT,
                    "up" => i::UP,
                    "down" => i::DOWN,
                    "r" => i::R,
                    "l" => i::L,
                    other => panic!("unknown button {other}"),
                }
            });
            let hold = parts.next().map_or(6, |h| h.parse().unwrap());
            (at, mask, hold)
        })
        .collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let rom = args.next().unwrap();
    let frames: u64 = args.next().unwrap().parse().unwrap();
    let mut a = Emulator::new(&rom).unwrap();
    let mut b = Emulator::new(&rom).unwrap();
    for e in [&mut a, &mut b] {
        e.initialize_gba_startup(ROM_START).unwrap();
        e.set_rendering(false);
    }
    a.set_idle_loop_skipping(true);
    b.set_idle_loop_skipping(false);

    let script = parse_script();
    let mut skipped_steps = 0u64;
    let mut plain_steps = 0u64;
    for frame in 0..frames {
        let held = script
            .iter()
            .filter(|(at, _, hold)| (*at..*at + *hold).contains(&frame))
            .fold(0, |mask, (_, buttons, _)| mask | buttons);
        a.set_buttons(held);
        b.set_buttons(held);
        while !a.take_frame_ready() {
            a.step().unwrap();
            skipped_steps += 1;
        }
        while !b.take_frame_ready() {
            b.step().unwrap();
            plain_steps += 1;
        }
        let audio_hash = |samples: Vec<[i16; 2]>| {
            samples
                .iter()
                .flatten()
                .fold(0xcbf29ce484222325u64, |h, s| {
                    (h ^ u64::from(*s as u16)).wrapping_mul(0x100000001b3)
                })
        };
        let (audio_a, audio_b) = (
            audio_hash(a.take_audio_samples()),
            audio_hash(b.take_audio_samples()),
        );
        if audio_a != audio_b {
            println!("DIVERGED at frame {frame}: audio samples differ");
            return;
        }
        let (sa, sb) = (state(&a), state(&b));
        if sa != sb {
            println!("DIVERGED at frame {frame}");
            println!("  cycles   {} vs {}", sa.0, sb.0);
            println!("  regs     {:08X?}\n           {:08X?}", sa.1, sb.1);
            println!("  cpsr     {:#X} vs {:#X}", sa.2, sb.2);
            println!("  iwram    {:#X} vs {:#X}", sa.3, sb.3);
            println!("  ewram    {:#X} vs {:#X}", sa.4, sb.4);
            return;
        }
    }
    println!(
        "identical for {frames} frames; steps with skipping {skipped_steps}, without {plain_steps} ({:.1}x fewer)",
        plain_steps as f64 / skipped_steps as f64
    );
}
