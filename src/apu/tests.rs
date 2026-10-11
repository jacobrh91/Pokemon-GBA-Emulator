use super::*;
use crate::mmu::registers::{
    SOUND1CNT_H, SOUND1CNT_X, SOUND2CNT_H, SOUND2CNT_L, SOUND3CNT_L, SOUNDBIAS, SOUNDCNT_H,
    SOUNDCNT_L, SOUNDCNT_X,
};

fn registers() -> Vec<u8> {
    let mut registers = vec![0u8; 0x400];
    registers[SOUNDCNT_X] = 0x80;
    registers[SOUNDBIAS..SOUNDBIAS + 2].copy_from_slice(&0x0200u16.to_le_bytes());
    registers
}

fn write16(registers: &mut [u8], apu: &mut Apu, offset: usize, value: u16) {
    for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
        registers[offset + index] = byte;
        apu.register_written(offset + index, registers);
    }
}

#[test]
fn silence_mixes_to_the_bias_center() {
    let mut apu = Apu::default();
    let registers = registers();
    apu.advance(SAMPLE_CYCLES * 4, &registers);
    assert_eq!(apu.take_samples(), vec![[0, 0]; 4]);
}

#[test]
fn square_wave_alternates_at_the_programmed_frequency() {
    let mut apu = Apu::default();
    let mut registers = registers();
    write16(&mut registers, &mut apu, SOUNDCNT_L, 0xFF77); // all channels, max volume
    write16(&mut registers, &mut apu, SOUND1CNT_H, 0xF080); // vol 15, duty 50%
    // freq 1920: period = 16 * 128 = 2048 cycles per duty step = 4 samples.
    write16(&mut registers, &mut apu, SOUND1CNT_X, 0x8000 | 1920);
    apu.advance(SAMPLE_CYCLES * 64, &registers);
    let samples = apu.take_samples();
    assert!(samples.iter().any(|s| s[0] > 0));
    assert!(samples.iter().any(|s| s[0] == 0));
    assert_eq!(
        registers[SOUND1CNT_X + 1] & 0x80,
        0,
        "restart bit reads as 0"
    );
    assert_eq!(apu.channel_status() & 1, 1);
}

#[test]
fn length_counter_silences_the_channel() {
    let mut apu = Apu::default();
    let mut registers = registers();
    write16(&mut registers, &mut apu, SOUND2CNT_L, 0xF03F); // length = 64 - 63 = 1
    write16(&mut registers, &mut apu, SOUND2CNT_H, 0xC000 | 1000);
    assert_eq!(apu.channel_status() & 2, 2);
    // The sequencer clocks length at step 0 after 64 samples.
    apu.advance(SAMPLE_CYCLES * FRAME_SEQUENCER_SAMPLES, &registers);
    assert_eq!(apu.channel_status() & 2, 0);
}

#[test]
fn fifo_sample_is_latched_on_selected_timer_overflow() {
    let mut apu = Apu::default();
    let mut registers = registers();
    // A: 100% volume, left+right, timer 0.
    write16(&mut registers, &mut apu, SOUNDCNT_H, 0x0304);
    apu.push_fifo(0, 0x40);
    apu.push_fifo(0, 0xC0);
    assert_eq!(apu.timer_overflow(1, &registers), [false, false]);
    assert_eq!(apu.fifo_len(0), 2);
    let refill = apu.timer_overflow(0, &registers);
    assert!(refill[0], "an almost-empty FIFO requests DMA");
    apu.advance(SAMPLE_CYCLES, &registers);
    let sample = apu.take_samples()[0];
    assert_eq!(sample, [(0x40 * 2) as i16 * 64, (0x40 * 2) as i16 * 64]);
    apu.timer_overflow(0, &registers);
    apu.advance(SAMPLE_CYCLES, &registers);
    assert!(apu.take_samples()[0][0] < 0);
}

#[test]
fn fifo_reset_bit_clears_queue_and_latch() {
    let mut apu = Apu::default();
    let mut registers = registers();
    apu.push_fifo(0, 1);
    write16(&mut registers, &mut apu, SOUNDCNT_H, 1 << 11);
    assert_eq!(apu.fifo_len(0), 0);
    assert_eq!(registers[SOUNDCNT_H + 1] & 0x08, 0);
}

#[test]
fn wave_ram_access_targets_the_bank_not_being_played() {
    let mut apu = Apu::default();
    let mut registers = registers();
    // Bank 0 playing => CPU writes bank 1.
    registers[SOUND3CNT_L] = 0x00;
    apu.write_wave_ram(0, 0xAB, &registers);
    assert_eq!(apu.wave_ram[1][0], 0xAB);
    registers[SOUND3CNT_L] = 0x40;
    assert_eq!(apu.read_wave_ram(0, &registers), 0);
}

#[test]
fn master_disable_clears_psg_registers_and_output() {
    let mut apu = Apu::default();
    let mut registers = registers();
    write16(&mut registers, &mut apu, SOUND1CNT_H, 0xF080);
    write16(&mut registers, &mut apu, SOUND1CNT_X, 0x8000 | 1000);
    registers[SOUNDCNT_X] = 0;
    apu.register_written(SOUNDCNT_X, &mut registers);
    assert_eq!(apu.channel_status(), 0);
    assert_eq!(reg16(&registers, SOUND1CNT_H), 0);
}
