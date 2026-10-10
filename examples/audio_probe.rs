//! Runs a ROM headlessly and writes its audio as a 16-bit stereo 32768 Hz WAV.
//! usage: audio_probe ROM OUT.wav FRAMES
use gba_emulator::emulator::Emulator;
use gba_emulator::mmu::ROM_START;

fn main() {
    let mut args = std::env::args().skip(1);
    let rom = args.next().unwrap();
    let out = args.next().unwrap();
    let frames: u64 = args.next().unwrap().parse().unwrap();
    let mut emu = Emulator::new(&rom).unwrap();
    emu.initialize_gba_startup(ROM_START).unwrap();

    let mut samples: Vec<[i16; 2]> = Vec::new();
    for _ in 0..frames {
        while !emu.take_frame_ready() {
            emu.step().unwrap();
        }
        samples.extend(emu.take_audio_samples());
    }

    let peak = samples
        .iter()
        .flatten()
        .map(|s| s.unsigned_abs())
        .max()
        .unwrap_or(0);
    let nonzero = samples.iter().filter(|s| **s != [0, 0]).count();
    let rms = (samples
        .iter()
        .flatten()
        .map(|s| f64::from(*s).powi(2))
        .sum::<f64>()
        / (samples.len() * 2).max(1) as f64)
        .sqrt();
    println!(
        "{} frames of audio, {} non-silent, peak {peak}, rms {rms:.0}",
        samples.len(),
        nonzero
    );

    let reg = |o: usize| u16::from_le_bytes([emu.memory().io_regs[o], emu.memory().io_regs[o + 1]]);
    println!(
        "SOUNDCNT_L={:#06X} SOUNDCNT_H={:#06X} SOUNDCNT_X={:#06X} SOUNDBIAS={:#06X} TM0={:#06X}/{:#06X} TM1={:#06X}/{:#06X}",
        reg(0x80),
        reg(0x82),
        reg(0x84),
        reg(0x88),
        reg(0x100),
        reg(0x102),
        reg(0x104),
        reg(0x106)
    );
    println!("fifo stats: {:?}", emu.memory().audio_fifo_stats());

    let data_len = (samples.len() * 4) as u32;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + data_len).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(32768u32.to_le_bytes());
    wav.extend((32768u32 * 4).to_le_bytes());
    wav.extend(4u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(data_len.to_le_bytes());
    for [l, r] in samples {
        wav.extend(l.to_le_bytes());
        wav.extend(r.to_le_bytes());
    }
    std::fs::write(out, wav).unwrap();
}
