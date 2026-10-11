//! Plays the emulator's 32768 Hz stereo samples through the default audio
//! device, resampling to the device rate with linear interpolation.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use gba_emulator::apu::SAMPLE_RATE;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// About a quarter second of audio; older samples are dropped past this so
/// latency cannot grow without bound when emulation runs ahead of the device.
const MAX_QUEUED_FRAMES: usize = SAMPLE_RATE as usize / 4;

/// Per-output-frame smoothing factor for gain changes (a few milliseconds).
const GAIN_SMOOTHING: f32 = 0.002;

type Queue = Arc<Mutex<VecDeque<[i16; 2]>>>;

/// Thread-safe handle for feeding the device and adjusting its gain.
///
/// The `cpal` stream itself must stay on the thread that created it, so the
/// emulation thread talks to the device only through this handle.
#[derive(Clone)]
pub struct AudioSink {
    queue: Queue,
    /// Target linear gain as `f32` bits; smoothed per frame in the callback.
    gain: Arc<AtomicU32>,
}

impl AudioSink {
    pub fn push(&self, samples: &[[i16; 2]]) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        queue.extend(samples.iter().copied());
        let excess = queue.len().saturating_sub(MAX_QUEUED_FRAMES);
        queue.drain(..excess);
    }

    /// A sink with no device behind it, for tests.
    #[cfg(test)]
    pub fn detached() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            gain: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
        }
    }

    /// Drops everything queued, so a restarted game does not play the tail of
    /// the previous one.
    pub fn clear(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }

    /// Source frames (32768 Hz) waiting to be played.
    pub fn queued_frames(&self) -> usize {
        self.queue.lock().map_or(0, |queue| queue.len())
    }

    /// Sets the output gain (1.0 = unchanged). Changes are smoothed.
    pub fn set_gain(&self, gain: f32) {
        self.gain
            .store(gain.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }
}

pub struct AudioOutput {
    sink: AudioSink,
    _stream: Stream,
}

impl AudioOutput {
    /// Opens the default output device, or returns the reason it could not.
    pub fn new() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no audio output device")?;
        let supported = device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
        let gain = Arc::new(AtomicU32::new(1.0_f32.to_bits()));

        let stream = match format {
            SampleFormat::F32 => Self::build::<f32>(&device, &config, &queue, &gain),
            SampleFormat::I16 => Self::build::<i16>(&device, &config, &queue, &gain),
            SampleFormat::U16 => Self::build::<u16>(&device, &config, &queue, &gain),
            other => return Err(format!("unsupported audio sample format {other}")),
        }?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self {
            sink: AudioSink { queue, gain },
            _stream: stream,
        })
    }

    pub fn sink(&self) -> AudioSink {
        self.sink.clone()
    }

    pub fn set_gain(&self, gain: f32) {
        self.sink.set_gain(gain);
    }

    fn build<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        queue: &Queue,
        gain: &Arc<AtomicU32>,
    ) -> Result<Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = usize::from(config.channels);
        let step = f64::from(SAMPLE_RATE) / f64::from(config.sample_rate);
        let queue = Arc::clone(queue);
        let gain = Arc::clone(gain);
        let mut resampler = Resampler::default();
        let mut current_gain = 1.0_f32;

        device
            .build_output_stream(
                *config,
                move |output: &mut [T], _| {
                    let mut queue = queue
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let target_gain = f32::from_bits(gain.load(Ordering::Relaxed));
                    for frame in output.chunks_mut(channels) {
                        // One-pole smoothing avoids zipper noise while dragging.
                        current_gain += (target_gain - current_gain) * GAIN_SMOOTHING;
                        let [left, right] = resampler.next(&mut queue, step);
                        let (left, right) = (left * current_gain, right * current_gain);
                        for (index, sample) in frame.iter_mut().enumerate() {
                            *sample = T::from_sample(match (channels, index) {
                                (1, _) => (left + right) * 0.5,
                                (_, 0) => left,
                                (_, 1) => right,
                                _ => 0.0,
                            });
                        }
                    }
                },
                |error| eprintln!("audio stream error: {error}"),
                None,
            )
            .map_err(|error| error.to_string())
    }
}

/// Linear-interpolating resampler that pulls source frames from the queue.
#[derive(Default)]
struct Resampler {
    previous: [f32; 2],
    current: [f32; 2],
    /// Fractional position between `previous` (0.0) and `current` (1.0).
    position: f64,
}

impl Resampler {
    fn next(&mut self, queue: &mut VecDeque<[i16; 2]>, step: f64) -> [f32; 2] {
        self.position += step;
        while self.position >= 1.0 {
            self.position -= 1.0;
            self.previous = self.current;
            // On underrun, fade toward silence rather than clicking.
            self.current = match queue.pop_front() {
                Some([left, right]) => [f32::from(left) / 32768.0, f32::from(right) / 32768.0],
                None => [self.current[0] * 0.995, self.current[1] * 0.995],
            };
        }
        let t = self.position as f32;
        [
            self.previous[0] + (self.current[0] - self.previous[0]) * t,
            self.previous[1] + (self.current[1] - self.previous[1]) * t,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_interpolates_between_source_frames() {
        let mut queue: VecDeque<[i16; 2]> = [[0, 0], [16384, -16384]].into();
        let mut resampler = Resampler::default();
        // Half-rate source: two output frames per input frame.
        for _ in 0..4 {
            resampler.next(&mut queue, 0.5);
        }
        let halfway = resampler.next(&mut queue, 0.5);
        assert!((halfway[0] - 0.25).abs() < 1e-6);
        assert!((halfway[1] + 0.25).abs() < 1e-6);
    }

    #[test]
    fn underrun_decays_instead_of_jumping_to_zero() {
        let mut queue: VecDeque<[i16; 2]> = [[32767, 32767]].into();
        let mut resampler = Resampler::default();
        for _ in 0..2 {
            resampler.next(&mut queue, 1.0);
        }
        let decayed = resampler.next(&mut queue, 1.0);
        assert!(decayed[0] > 0.9 && decayed[0] < 1.0);
    }
}
