//! Audio processing unit: four legacy PSG channels plus two Direct Sound
//! FIFO channels, mixed to stereo at 32768 Hz (GBATEK "GBA Sound Controller",
//! mGBA `audio.c`).
//!
//! The I/O registers live in the memory bus, which lends them to the APU.

mod state;

use crate::mmu::registers::{
    SOUND1CNT_H, SOUND1CNT_L, SOUND1CNT_X, SOUND2CNT_H, SOUND2CNT_L, SOUND3CNT_H, SOUND3CNT_L,
    SOUND3CNT_X, SOUND4CNT_H, SOUND4CNT_L, SOUNDBIAS, SOUNDCNT_H, SOUNDCNT_L, SOUNDCNT_X,
};
use std::collections::VecDeque;

/// CPU cycles between mixed output samples (16.78 MHz / 32768 Hz).
pub const SAMPLE_CYCLES: u32 = 512;
pub const SAMPLE_RATE: u32 = 32_768;

const FRAME_SEQUENCER_SAMPLES: u32 = 64; // 512 Hz
const FIFO_CAPACITY: usize = 32;
/// A FIFO asks for more data once it holds this many bytes or fewer.
pub const FIFO_REFILL_LEVEL: usize = 16;
/// Output is dropped past this many queued stereo frames when nothing drains.
const MAX_QUEUED_FRAMES: usize = SAMPLE_RATE as usize;

// Register offsets inside the I/O block.
pub const AUDIO_REGISTERS_START: usize = 0x60;
pub const AUDIO_REGISTERS_END: usize = 0xA8;

const DUTY_PATTERNS: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 0],
];

fn reg16(registers: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([registers[offset], registers[offset + 1]])
}

#[derive(Default)]
struct Square {
    enabled: bool,
    length: u16,
    volume: u8,
    envelope_timer: u8,
    frequency: u16,
    phase_cycles: u32,
    duty_position: usize,
    sweep_shadow: u16,
    sweep_timer: u8,
    sweep_enabled: bool,
}

impl Square {
    /// `control` is the length/duty/envelope halfword.
    fn dac_enabled(control: u16) -> bool {
        control & 0xF800 != 0
    }

    fn trigger(&mut self, control: u16, frequency: u16, sweep: Option<u16>) {
        self.enabled = Self::dac_enabled(control);
        if self.length == 0 {
            self.length = 64;
        }
        self.frequency = frequency;
        self.phase_cycles = 0;
        self.volume = (control >> 12) as u8;
        self.envelope_timer = envelope_period(control);
        if let Some(sweep) = sweep {
            let period = ((sweep >> 4) & 7) as u8;
            let shift = sweep & 7;
            self.sweep_shadow = frequency;
            self.sweep_timer = if period == 0 { 8 } else { period };
            self.sweep_enabled = period != 0 || shift != 0;
            if shift != 0 && self.sweep_target(sweep).is_none() {
                self.enabled = false;
            }
        }
    }

    /// Next swept frequency, or `None` if it would overflow 11 bits.
    fn sweep_target(&self, sweep: u16) -> Option<u16> {
        let delta = self.sweep_shadow >> (sweep & 7);
        let target = if sweep & 8 != 0 {
            self.sweep_shadow.wrapping_sub(delta)
        } else {
            self.sweep_shadow + delta
        };
        (target <= 2047).then_some(target)
    }

    fn clock_sweep(&mut self, sweep: u16) {
        if !self.sweep_enabled {
            return;
        }
        self.sweep_timer = self.sweep_timer.saturating_sub(1);
        if self.sweep_timer != 0 {
            return;
        }
        let period = ((sweep >> 4) & 7) as u8;
        self.sweep_timer = if period == 0 { 8 } else { period };
        if period == 0 {
            return;
        }
        match self.sweep_target(sweep) {
            None => self.enabled = false,
            Some(target) if sweep & 7 != 0 => {
                self.sweep_shadow = target;
                self.frequency = target;
                if self.sweep_target(sweep).is_none() {
                    self.enabled = false;
                }
            }
            Some(_) => {}
        }
    }

    fn clock_length(&mut self, length_enabled: bool) {
        if length_enabled && self.length > 0 {
            self.length -= 1;
            if self.length == 0 {
                self.enabled = false;
            }
        }
    }

    fn clock_envelope(&mut self, control: u16) {
        clock_envelope(&mut self.envelope_timer, &mut self.volume, control);
    }

    fn sample(&mut self, control: u16) -> u8 {
        let period = 16 * (2048 - u32::from(self.frequency)).max(1);
        self.phase_cycles += SAMPLE_CYCLES;
        let steps = self.phase_cycles / period;
        self.phase_cycles %= period;
        self.duty_position = (self.duty_position + steps as usize) & 7;
        if !self.enabled {
            return 0;
        }
        let duty = ((control >> 6) & 3) as usize;
        DUTY_PATTERNS[duty][self.duty_position] * self.volume
    }
}

fn envelope_period(control: u16) -> u8 {
    ((control >> 8) & 7) as u8
}

fn clock_envelope(timer: &mut u8, volume: &mut u8, control: u16) {
    let period = envelope_period(control);
    if period == 0 {
        return;
    }
    *timer = timer.saturating_sub(1);
    if *timer != 0 {
        return;
    }
    *timer = period;
    if control & (1 << 11) != 0 {
        *volume = (*volume + 1).min(15);
    } else {
        *volume = volume.saturating_sub(1);
    }
}

#[derive(Default)]
struct Wave {
    enabled: bool,
    length: u16,
    position: usize,
    phase_cycles: u32,
    frequency: u16,
}

#[derive(Default)]
struct Noise {
    enabled: bool,
    length: u16,
    volume: u8,
    envelope_timer: u8,
    lfsr: u16,
    phase_cycles: u32,
}

#[derive(Default)]
struct DirectSound {
    fifo: VecDeque<i8>,
    latch: i8,
}

impl DirectSound {
    fn push(&mut self, value: i8) {
        if self.fifo.len() < FIFO_CAPACITY {
            self.fifo.push_back(value);
        }
    }

    fn reset(&mut self) {
        self.fifo.clear();
        self.latch = 0;
    }
}

pub struct Apu {
    square: [Square; 2],
    wave: Wave,
    noise: Noise,
    direct: [DirectSound; 2],
    wave_ram: [[u8; 16]; 2],
    cycle_accumulator: u32,
    sequencer_samples: u32,
    sequencer_step: u8,
    output: Vec<[i16; 2]>,
    stats: FifoStats,
}

/// Direct Sound health counters, for diagnosing starved or overrun FIFOs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FifoStats {
    /// Timer overflows that consumed a sample.
    pub samples_played: u64,
    /// Timer overflows that found the selected FIFO empty.
    pub underruns: u64,
    /// Bytes dropped because the FIFO was full.
    pub overflow_bytes: u64,
}

impl Default for Apu {
    fn default() -> Self {
        Self {
            square: Default::default(),
            wave: Wave::default(),
            noise: Noise {
                lfsr: 0x7FFF,
                ..Noise::default()
            },
            direct: Default::default(),
            wave_ram: [[0; 16]; 2],
            cycle_accumulator: 0,
            sequencer_samples: 0,
            sequencer_step: 0,
            output: Vec::new(),
            stats: FifoStats::default(),
        }
    }
}

impl Apu {
    pub fn push_fifo(&mut self, channel: usize, value: u8) {
        if self.direct[channel].fifo.len() >= FIFO_CAPACITY {
            self.stats.overflow_bytes += 1;
        }
        self.direct[channel].push(value as i8);
    }

    pub fn fifo_stats(&self) -> FifoStats {
        self.stats
    }

    pub fn fifo_len(&self, channel: usize) -> usize {
        self.direct[channel].fifo.len()
    }

    /// A timer overflow clocks every FIFO routed to it by one sample. Returns
    /// which FIFOs dropped to the refill level and want a DMA transfer.
    pub fn timer_overflow(&mut self, timer: usize, registers: &[u8]) -> [bool; 2] {
        let control = reg16(registers, SOUNDCNT_H);
        let mut refill = [false; 2];
        for (channel, request) in refill.iter_mut().enumerate() {
            let selected_timer = usize::from(control >> (10 + channel * 4) & 1);
            if selected_timer != timer {
                continue;
            }
            let direct = &mut self.direct[channel];
            match direct.fifo.pop_front() {
                Some(sample) => {
                    direct.latch = sample;
                    self.stats.samples_played += 1;
                }
                None => self.stats.underruns += 1,
            }
            *request = direct.fifo.len() <= FIFO_REFILL_LEVEL;
        }
        refill
    }

    /// Wave RAM is two 16-byte banks; the CPU sees the one not being played.
    pub fn read_wave_ram(&self, index: usize, registers: &[u8]) -> u8 {
        self.wave_ram[self.cpu_wave_bank(registers)][index]
    }

    pub fn write_wave_ram(&mut self, index: usize, value: u8, registers: &[u8]) {
        let bank = self.cpu_wave_bank(registers);
        self.wave_ram[bank][index] = value;
    }

    fn cpu_wave_bank(&self, registers: &[u8]) -> usize {
        usize::from(reg16(registers, SOUND3CNT_L) >> 6 & 1) ^ 1
    }

    /// Status bits for SOUNDCNT_X: one per PSG channel that is playing.
    pub fn channel_status(&self) -> u8 {
        u8::from(self.square[0].enabled)
            | u8::from(self.square[1].enabled) << 1
            | u8::from(self.wave.enabled) << 2
            | u8::from(self.noise.enabled) << 3
    }

    /// Called after a byte in the audio register block was stored.
    pub fn register_written(&mut self, offset: usize, registers: &mut [u8]) {
        match offset {
            // Square 1 trigger (high byte of SOUND1CNT_X).
            o if o == SOUND1CNT_X + 1 => {
                self.square[0].frequency = reg16(registers, SOUND1CNT_X) & 0x7FF;
                if registers[o] & 0x80 != 0 {
                    let control = reg16(registers, SOUND1CNT_H);
                    self.square[0].length = 64 - (control & 0x3F);
                    self.square[0].trigger(
                        control,
                        reg16(registers, SOUND1CNT_X) & 0x7FF,
                        Some(reg16(registers, SOUND1CNT_L)),
                    );
                    registers[o] &= 0x7F;
                }
            }
            o if o == SOUND1CNT_X => {
                self.square[0].frequency = reg16(registers, SOUND1CNT_X) & 0x7FF;
            }
            o if o == SOUND2CNT_H + 1 => {
                self.square[1].frequency = reg16(registers, SOUND2CNT_H) & 0x7FF;
                if registers[o] & 0x80 != 0 {
                    let control = reg16(registers, SOUND2CNT_L);
                    self.square[1].length = 64 - (control & 0x3F);
                    self.square[1].trigger(control, reg16(registers, SOUND2CNT_H) & 0x7FF, None);
                    registers[o] &= 0x7F;
                }
            }
            o if o == SOUND2CNT_H => {
                self.square[1].frequency = reg16(registers, SOUND2CNT_H) & 0x7FF;
            }
            o if o == SOUND3CNT_X + 1 => {
                self.wave.frequency = reg16(registers, SOUND3CNT_X) & 0x7FF;
                if registers[o] & 0x80 != 0 {
                    self.wave.length = 256 - (reg16(registers, SOUND3CNT_H) & 0xFF);
                    self.wave.enabled = reg16(registers, SOUND3CNT_L) & 0x80 != 0;
                    self.wave.position = 0;
                    self.wave.phase_cycles = 0;
                    registers[o] &= 0x7F;
                }
            }
            o if o == SOUND3CNT_X => {
                self.wave.frequency = reg16(registers, SOUND3CNT_X) & 0x7FF;
            }
            o if o == SOUND3CNT_L => {
                if reg16(registers, SOUND3CNT_L) & 0x80 == 0 {
                    self.wave.enabled = false;
                }
            }
            o if o == SOUND4CNT_H + 1 => {
                if registers[o] & 0x80 != 0 {
                    let control = reg16(registers, SOUND4CNT_L);
                    self.noise.length = 64 - (control & 0x3F);
                    self.noise.enabled = Square::dac_enabled(control);
                    self.noise.volume = (control >> 12) as u8;
                    self.noise.envelope_timer = envelope_period(control);
                    self.noise.lfsr = 0x7FFF;
                    self.noise.phase_cycles = 0;
                    registers[o] &= 0x7F;
                }
            }
            // FIFO reset bits live in the high byte of SOUNDCNT_H.
            o if o == SOUNDCNT_H + 1 => {
                if registers[o] & 0x08 != 0 {
                    self.direct[0].reset();
                    registers[o] &= !0x08;
                }
                if registers[o] & 0x80 != 0 {
                    self.direct[1].reset();
                    registers[o] &= !0x80;
                }
            }
            // Clearing the master enable resets every PSG register.
            o if o == SOUNDCNT_X => {
                if registers[o] & 0x80 == 0 {
                    registers[SOUND1CNT_L..SOUNDCNT_X].fill(0);
                    self.square = Default::default();
                    self.wave = Wave::default();
                    self.noise = Noise {
                        lfsr: 0x7FFF,
                        ..Noise::default()
                    };
                }
            }
            _ => {}
        }
    }

    /// Cycles until the next sample is mixed (always at least 1).
    pub fn cycles_until_sample(&self) -> u32 {
        SAMPLE_CYCLES - self.cycle_accumulator
    }

    /// Advances time, mixing one stereo sample per [`SAMPLE_CYCLES`].
    pub fn advance(&mut self, cycles: u32, registers: &[u8]) {
        self.cycle_accumulator += cycles;
        while self.cycle_accumulator >= SAMPLE_CYCLES {
            self.cycle_accumulator -= SAMPLE_CYCLES;
            self.mix_sample(registers);
        }
    }

    /// Drains the mixed stereo samples (32768 Hz) produced so far.
    pub fn take_samples(&mut self) -> Vec<[i16; 2]> {
        std::mem::take(&mut self.output)
    }

    fn clock_sequencer(&mut self, registers: &[u8]) {
        let step = self.sequencer_step;
        let length_control = [
            reg16(registers, SOUND1CNT_X),
            reg16(registers, SOUND2CNT_H),
            reg16(registers, SOUND3CNT_X),
            reg16(registers, SOUND4CNT_H),
        ];
        if step.is_multiple_of(2) {
            self.square[0].clock_length(length_control[0] & 0x4000 != 0);
            self.square[1].clock_length(length_control[1] & 0x4000 != 0);
            if length_control[2] & 0x4000 != 0 && self.wave.length > 0 {
                self.wave.length -= 1;
                if self.wave.length == 0 {
                    self.wave.enabled = false;
                }
            }
            if length_control[3] & 0x4000 != 0 && self.noise.length > 0 {
                self.noise.length -= 1;
                if self.noise.length == 0 {
                    self.noise.enabled = false;
                }
            }
        }
        if step == 2 || step == 6 {
            self.square[0].clock_sweep(reg16(registers, SOUND1CNT_L));
        }
        if step == 7 {
            self.square[0].clock_envelope(reg16(registers, SOUND1CNT_H));
            self.square[1].clock_envelope(reg16(registers, SOUND2CNT_L));
            clock_envelope(
                &mut self.noise.envelope_timer,
                &mut self.noise.volume,
                reg16(registers, SOUND4CNT_L),
            );
        }
        self.sequencer_step = (step + 1) & 7;
    }

    fn wave_sample(&mut self, registers: &[u8]) -> u8 {
        let control = reg16(registers, SOUND3CNT_L);
        let period = 8 * (2048 - u32::from(self.wave.frequency)).max(1);
        self.wave.phase_cycles += SAMPLE_CYCLES;
        let steps = (self.wave.phase_cycles / period) as usize;
        self.wave.phase_cycles %= period;
        let two_banks = control & 0x20 != 0;
        let length = if two_banks { 64 } else { 32 };
        self.wave.position = (self.wave.position + steps) % length;
        if !self.wave.enabled || control & 0x80 == 0 {
            return 0;
        }
        let bank = if two_banks {
            self.wave.position / 32
        } else {
            usize::from(control >> 6 & 1)
        };
        let index = self.wave.position % 32;
        let byte = self.wave_ram[bank][index / 2];
        let nibble = if index.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0xF
        };
        let volume = reg16(registers, SOUND3CNT_H);
        if volume & 0x8000 != 0 {
            return nibble * 3 / 4;
        }
        match (volume >> 13) & 3 {
            0 => 0,
            1 => nibble,
            2 => nibble >> 1,
            _ => nibble >> 2,
        }
    }

    fn noise_sample(&mut self, registers: &[u8]) -> u8 {
        let control = reg16(registers, SOUND4CNT_H);
        let ratio = u32::from(control & 7);
        let shift = u32::from((control >> 4) & 0xF);
        if shift >= 14 {
            return 0;
        }
        let half_ratio = if ratio == 0 { 1 } else { ratio * 2 };
        let period = (16 * half_ratio) << (shift + 1);
        self.noise.phase_cycles += SAMPLE_CYCLES;
        let steps = self.noise.phase_cycles / period;
        self.noise.phase_cycles %= period;
        for _ in 0..steps.min(64) {
            let feedback = (self.noise.lfsr ^ (self.noise.lfsr >> 1)) & 1;
            self.noise.lfsr = (self.noise.lfsr >> 1) | (feedback << 14);
            if control & 8 != 0 {
                self.noise.lfsr = (self.noise.lfsr & !0x40) | (feedback << 6);
            }
        }
        if !self.noise.enabled || self.noise.lfsr & 1 != 0 {
            return 0;
        }
        self.noise.volume
    }

    fn mix_sample(&mut self, registers: &[u8]) {
        self.sequencer_samples += 1;
        if self.sequencer_samples == FRAME_SEQUENCER_SAMPLES {
            self.sequencer_samples = 0;
            self.clock_sequencer(registers);
        }

        let master_enabled = registers[SOUNDCNT_X] & 0x80 != 0;
        let psg = [
            self.square[0].sample(reg16(registers, SOUND1CNT_H)),
            self.square[1].sample(reg16(registers, SOUND2CNT_L)),
            self.wave_sample(registers),
            self.noise_sample(registers),
        ];
        if !master_enabled {
            self.push_output([0, 0]);
            return;
        }

        let control_l = reg16(registers, SOUNDCNT_L);
        let control_h = reg16(registers, SOUNDCNT_H);
        let psg_multiplier = match control_h & 3 {
            0 => 1,
            1 => 2,
            _ => 4,
        };
        let mut sides = [0i32; 2];
        for (side, total) in sides.iter_mut().enumerate() {
            // Right-side enables are bits 8-11, left-side 12-15.
            let enables = (control_l >> (8 + side * 4)) & 0xF;
            let volume = i32::from((control_l >> (4 * side)) & 7) + 1;
            let sum: i32 = psg
                .iter()
                .enumerate()
                .filter(|(channel, _)| enables & (1 << channel) != 0)
                .map(|(_, level)| i32::from(*level))
                .sum();
            *total = sum * volume * psg_multiplier / 4;
        }
        for (channel, direct) in self.direct.iter().enumerate() {
            let full_volume = control_h & (1 << (2 + channel)) != 0;
            let scaled = i32::from(direct.latch) * if full_volume { 2 } else { 1 };
            // Enables: A right=8, A left=9, B right=12, B left=13.
            let right = control_h & (1 << (8 + channel * 4)) != 0;
            let left = control_h & (1 << (9 + channel * 4)) != 0;
            if right {
                sides[0] += scaled;
            }
            if left {
                sides[1] += scaled;
            }
        }

        let bias = i32::from(reg16(registers, SOUNDBIAS) & 0x3FE);
        let convert = |value: i32| ((value + bias).clamp(0, 0x3FF) - 0x200) as i16 * 64;
        // `sides` is [right, left]; output frames are [left, right].
        self.push_output([convert(sides[1]), convert(sides[0])]);
    }

    fn push_output(&mut self, frame: [i16; 2]) {
        if self.output.len() < MAX_QUEUED_FRAMES {
            self.output.push(frame);
        }
    }
}

#[cfg(test)]
mod tests;
