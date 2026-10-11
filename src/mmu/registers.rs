//! Offsets of the memory-mapped I/O registers within the I/O region
//! (`IO_REG_START`..), shared by the bus, PPU, APU and DMA code (GBATEK "GBA
//! I/O Map").

/// LCD control.
pub const DISPCNT: usize = 0x000;
/// LCD status and interrupt enables.
pub const DISPSTAT: usize = 0x004;
/// Current scanline.
pub const VCOUNT: usize = 0x006;
/// BG0CNT; BGnCNT follows at 2-byte strides.
pub const BG_CONTROL_BASE: usize = 0x008;
/// BG0HOFS; scroll registers are 4 bytes per background.
pub const BG_HORIZONTAL_OFFSET_BASE: usize = 0x010;
/// BG0VOFS.
pub const BG_VERTICAL_OFFSET_BASE: usize = 0x012;
/// BG2PA; BG2 and BG3 affine parameters are 0x10 apart.
pub const AFFINE_BASE: usize = 0x020;
/// WIN0H.
pub const WINDOW_HORIZONTAL_BASE: usize = 0x040;
/// WIN0V.
pub const WINDOW_VERTICAL_BASE: usize = 0x044;
/// Inside-window layer enables.
pub const WININ: usize = 0x048;
/// Outside-window and OBJ-window layer enables.
pub const WINOUT: usize = 0x04A;
/// Mosaic sizes.
pub const MOSAIC: usize = 0x04C;
/// Blend control.
pub const BLDCNT: usize = 0x050;
/// Alpha coefficients.
pub const BLDALPHA: usize = 0x052;
/// Brightness coefficient.
pub const BLDY: usize = 0x054;
/// Sound channel register.
pub const SOUND1CNT_L: usize = 0x060;
/// Sound channel register.
pub const SOUND1CNT_H: usize = 0x062;
/// Sound channel register.
pub const SOUND1CNT_X: usize = 0x064;
/// Sound channel register.
pub const SOUND2CNT_L: usize = 0x068;
/// Sound channel register.
pub const SOUND2CNT_H: usize = 0x06C;
/// Sound channel register.
pub const SOUND3CNT_L: usize = 0x070;
/// Sound channel register.
pub const SOUND3CNT_H: usize = 0x072;
/// Sound channel register.
pub const SOUND3CNT_X: usize = 0x074;
/// Sound channel register.
pub const SOUND4CNT_L: usize = 0x078;
/// Sound channel register.
pub const SOUND4CNT_H: usize = 0x07C;
/// PSG volume and routing.
pub const SOUNDCNT_L: usize = 0x080;
/// Direct Sound control.
pub const SOUNDCNT_H: usize = 0x082;
/// Sound master enable and channel status.
pub const SOUNDCNT_X: usize = 0x084;
/// Output bias and sampling rate.
pub const SOUNDBIAS: usize = 0x088;
/// Wave pattern RAM.
pub const WAVE_RAM: usize = 0x090;
/// Direct Sound FIFO A.
pub const FIFO_A: usize = 0x0A0;
/// Direct Sound FIFO B.
pub const FIFO_B: usize = 0x0A4;
/// DMA0SAD; each channel takes 12 bytes.
pub const DMA_BASE: usize = 0x0B0;
/// TM0CNT_L; each timer takes 4 bytes.
pub const TIMER_BASE: usize = 0x100;
/// Pressed buttons (active low).
pub const KEYINPUT: usize = 0x130;
/// Keypad interrupt control.
pub const KEYCNT: usize = 0x132;
/// IE.
pub const INTERRUPT_ENABLE: usize = 0x200;
/// IF.
pub const INTERRUPT_REQUEST: usize = 0x202;
/// IME.
pub const INTERRUPT_MASTER_ENABLE: usize = 0x208;
