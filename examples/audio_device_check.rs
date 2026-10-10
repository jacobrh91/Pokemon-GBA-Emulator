//! Plays a one-second 440 Hz tone through the real output path to confirm the
//! device opens and the resampler feeds it.
#[path = "../src/audio_output.rs"]
mod audio_output;

fn main() {
    let output = match audio_output::AudioOutput::new() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("could not open audio device: {error}");
            std::process::exit(1);
        }
    };
    let rate = gba_emulator::apu::SAMPLE_RATE as f32;
    let started = std::time::Instant::now();
    let mut produced = 0usize;
    while started.elapsed().as_secs_f32() < 1.0 {
        // Keep roughly 100 ms ahead of real time.
        let wanted = ((started.elapsed().as_secs_f32() + 0.1) * rate) as usize;
        let batch: Vec<[i16; 2]> = (produced..wanted.max(produced))
            .map(|n| {
                let value =
                    ((n as f32 * 440.0 * std::f32::consts::TAU / rate).sin() * 8000.0) as i16;
                [value, value]
            })
            .collect();
        produced += batch.len();
        output.push(&batch);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    println!("played {produced} frames");
}
