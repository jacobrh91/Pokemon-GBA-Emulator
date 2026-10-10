//! Measures headless emulation speed in frames per second (no pacing).
//! usage: speed_probe ROM FRAMES [no-render]
use gba_emulator::emulator::Emulator;
use gba_emulator::mmu::ROM_START;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let rom = args.next().unwrap();
    let frames: u64 = args.next().unwrap().parse().unwrap();
    let render = args.next().as_deref() != Some("no-render");

    let mut emu = Emulator::new(&rom).unwrap();
    emu.initialize_gba_startup(ROM_START).unwrap();
    emu.set_rendering(render);
    let started = Instant::now();
    for _ in 0..frames {
        while !emu.take_frame_ready() {
            emu.step().unwrap();
        }
        // Keep the audio buffer from growing without bound.
        emu.take_audio_samples();
    }
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "{frames} frames in {seconds:.2}s = {:.0} fps ({:.1}x real time), rendering {}",
        frames as f64 / seconds,
        frames as f64 / seconds / 59.7275,
        if render { "on" } else { "off" }
    );
}
