//! The real-time clock of the Pokémon Ruby, Sapphire and Emerald cartridges (a
//! Seiko S-3511A on the cartridge's GPIO port), showing the host machine's
//! local time.
//!
//! The chip is wired to three pins of a GPIO port that overlays the ROM at
//! offsets `0xC4..=0xC9`: `SCK` (bit 0), `SIO` (bit 1) and `CS` (bit 2) of the
//! data register at `0xC4`; the direction of each pin at `0xC6` (1 = the console
//! drives it); and at `0xC8` whether the port can be read (otherwise the ROM's
//! bytes show). The games enable reading once at start-up. Here it is on from
//! the beginning, since the port is not part of a save state and a restored game
//! would otherwise never see the chip. A transfer goes bit by bit:
//!
//! 1. `CS` goes high.
//! 2. An 8-bit command is sent most significant bit first, a bit per rising edge
//!    of `SCK`: `0110`, a 3-bit register number, then 1 to read or 0 to write.
//! 3. The register's bytes follow, least significant bit first: sent by the game
//!    (taken on rising edges) or by the chip (changing on falling edges, so they
//!    can be read once `SCK` has risen).
//! 4. `CS` goes low.
//!
//! Registers: 0 reset, 1 status, 2 date and time (year, month, day, weekday,
//! hour, minute, second, in BCD), 3 time (the last three), 4 and 5 alarms. The
//! status keeps the 24-hour flag; the power-failure flag, which makes the games
//! say the battery has run dry, is never set. The date and time cannot be set.

/// The first and last byte offsets of the GPIO port within the cartridge.
const GPIO_FIRST: usize = 0xC4;
const GPIO_LAST: usize = 0xC9;
const DATA: usize = 0xC4;
const DIRECTION: usize = 0xC6;
const READ_ENABLE: usize = 0xC8;

const SCK: u8 = 1;
const SIO: u8 = 2;
const CS: u8 = 4;

/// Status register bits the chip keeps: the three interrupt enables and the
/// 24-hour flag. (Bit 7, power failure, is read-only and clear.)
const STATUS_MASK: u8 = 0x6A;
const STATUS_24_HOUR: u8 = 0x40;

/// The time of day and date, as the machine reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u32,
    /// 1 to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
    /// 0 (Sunday) to 6.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Where the chip gets the time from.
pub type Clock = fn() -> LocalTime;

/// Whether a cartridge with this game code (the header's `0xAC..0xB0`) has the
/// clock: Pokémon Ruby (`AXV?`), Sapphire (`AXP?`) and Emerald (`BPE?`).
pub fn cartridge_has_clock(game_code: &[u8]) -> bool {
    [b"AXV", b"AXP", b"BPE"]
        .iter()
        .any(|prefix| game_code.starts_with(*prefix))
}

pub struct Rtc {
    clock: Clock,
    status: u8,
    alarms: [[u8; 3]; 2],
    /// What the console last wrote to each pin, and which pins it drives.
    latch: u8,
    direction: u8,
    readable: bool,
    phase: Phase,
    /// What the chip drives on `SIO`.
    output: bool,
}

/// How far along the current transfer is.
#[derive(Clone, Copy)]
enum Phase {
    Idle,
    Command {
        bits: u8,
        value: u8,
    },
    Write {
        register: u8,
        length: usize,
        bytes: [u8; 7],
        done: usize,
        bit: u8,
    },
    Read {
        bytes: [u8; 7],
        length: usize,
        done: usize,
        bit: u8,
    },
}

impl Rtc {
    pub fn new(clock: Clock) -> Self {
        Self {
            clock,
            status: STATUS_24_HOUR,
            alarms: [[0; 3]; 2],
            latch: 0,
            direction: 0,
            readable: true,
            phase: Phase::Idle,
            output: false,
        }
    }

    /// Whether `offset` (into the cartridge) is a byte of the GPIO port.
    pub fn is_gpio(offset: usize) -> bool {
        (GPIO_FIRST..=GPIO_LAST).contains(&offset)
    }

    /// Whether the port currently shows through instead of the ROM.
    pub fn readable(&self) -> bool {
        self.readable
    }

    /// The byte of the port at `offset`.
    pub fn read(&self, offset: usize) -> u8 {
        match offset {
            DATA => {
                // Pins the console drives read back what it wrote; the others
                // are the chip's: `SIO` carries its output while it sends.
                let chip = if self.output { SIO } else { 0 };
                ((self.latch & self.direction) | (chip & !self.direction)) & 0x0F
            }
            DIRECTION => self.direction,
            READ_ENABLE => u8::from(self.readable),
            _ => 0,
        }
    }

    /// A write to the byte of the port at `offset`.
    pub fn write(&mut self, offset: usize, value: u8) {
        match offset {
            DATA => {
                let old = self.latch;
                self.latch = (old & !self.direction) | (value & self.direction & 0x0F);
                self.pins_changed(old, self.latch);
            }
            DIRECTION => self.direction = value & 0x0F,
            READ_ENABLE => self.readable = value & 1 != 0,
            _ => {}
        }
    }

    /// The edges between two states of the pins.
    fn pins_changed(&mut self, old: u8, new: u8) {
        let (old_cs, new_cs) = (old & CS != 0, new & CS != 0);
        if !old_cs && new_cs {
            self.phase = Phase::Command { bits: 0, value: 0 };
        } else if old_cs && !new_cs {
            self.phase = Phase::Idle;
            self.output = false;
        }
        if !new_cs {
            return;
        }
        let (old_sck, new_sck) = (old & SCK != 0, new & SCK != 0);
        if !old_sck && new_sck {
            self.clock_rose(new & SIO != 0);
        } else if old_sck && !new_sck {
            self.clock_fell();
        }
    }

    /// `SCK` rose: the console's bit on `SIO` is taken.
    fn clock_rose(&mut self, bit: bool) {
        match &mut self.phase {
            Phase::Command { bits, value } => {
                *value = (*value << 1) | u8::from(bit);
                *bits += 1;
                if *bits == 8 {
                    let command = *value;
                    self.command(command);
                }
            }
            Phase::Write {
                register,
                length,
                bytes,
                done,
                bit: at,
            } => {
                if *done < *length {
                    bytes[*done] |= u8::from(bit) << *at;
                    *at += 1;
                    if *at == 8 {
                        *at = 0;
                        *done += 1;
                        if *done == *length {
                            let (register, bytes) = (*register, *bytes);
                            self.write_register(register, &bytes);
                        }
                    }
                }
            }
            Phase::Idle | Phase::Read { .. } => {}
        }
    }

    /// `SCK` fell: while the chip sends, its next bit goes on `SIO`.
    fn clock_fell(&mut self) {
        if let Phase::Read {
            bytes,
            length,
            done,
            bit,
        } = &mut self.phase
        {
            self.output = *done < *length && (bytes[*done] >> *bit) & 1 != 0;
            *bit += 1;
            if *bit == 8 {
                *bit = 0;
                *done += 1;
            }
        }
    }

    /// A whole command has arrived.
    fn command(&mut self, command: u8) {
        if command >> 4 != 0b0110 {
            self.phase = Phase::Idle;
            return;
        }
        let register = (command >> 1) & 7;
        let length = register_length(register);
        if command & 1 != 0 {
            self.phase = Phase::Read {
                bytes: self.read_register(register),
                length,
                done: 0,
                bit: 0,
            };
        } else if length == 0 {
            // Register 0, reset: the status and alarms go back to zero.
            self.status = 0;
            self.alarms = [[0; 3]; 2];
            self.phase = Phase::Idle;
        } else {
            self.phase = Phase::Write {
                register,
                length,
                bytes: [0; 7],
                done: 0,
                bit: 0,
            };
        }
    }

    fn read_register(&self, register: u8) -> [u8; 7] {
        let mut bytes = [0; 7];
        match register {
            1 => bytes[0] = self.status,
            2 | 3 => {
                let now = (self.clock)();
                let hour = self.hour_byte(now.hour);
                let time = [hour, bcd(now.minute), bcd(now.second)];
                if register == 2 {
                    bytes = [
                        bcd((now.year % 100) as u8),
                        bcd(now.month),
                        bcd(now.day),
                        bcd(now.weekday),
                        time[0],
                        time[1],
                        time[2],
                    ];
                } else {
                    bytes[..3].copy_from_slice(&time);
                }
            }
            4 | 5 => bytes[..3].copy_from_slice(&self.alarms[usize::from(register - 4)]),
            _ => {}
        }
        bytes
    }

    /// The hour as the chip reports it: 0 to 23 in 24-hour mode, else 0 to 11
    /// with bit 6 set in the afternoon.
    fn hour_byte(&self, hour: u8) -> u8 {
        if self.status & STATUS_24_HOUR != 0 {
            bcd(hour)
        } else {
            bcd(hour % 12) | (u8::from(hour >= 12) << 6)
        }
    }

    fn write_register(&mut self, register: u8, bytes: &[u8; 7]) {
        match register {
            1 => self.status = bytes[0] & STATUS_MASK,
            4 | 5 => self.alarms[usize::from(register - 4)].copy_from_slice(&bytes[..3]),
            // The date and time are the machine's.
            _ => {}
        }
    }
}

/// How many bytes a register holds.
fn register_length(register: u8) -> usize {
    match register {
        0 => 0,
        1 => 1,
        2 => 7,
        3..=5 => 3,
        _ => 0,
    }
}

/// A number below 100 as two BCD digits.
fn bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

/// The time of the machine the emulator runs on, in its time zone.
#[cfg(unix)]
pub fn host_time() -> LocalTime {
    // Safety: `localtime_r` writes only the `tm` it is given.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&now, &mut tm).is_null() {
            return LocalTime {
                year: (tm.tm_year + 1900) as u32,
                month: (tm.tm_mon + 1) as u8,
                day: tm.tm_mday as u8,
                weekday: tm.tm_wday as u8,
                hour: tm.tm_hour as u8,
                minute: tm.tm_min as u8,
                second: tm.tm_sec as u8,
            };
        }
    }
    utc_time()
}

/// The time of the machine the emulator runs on (UTC: this platform's time
/// zone is not available).
#[cfg(not(unix))]
pub fn host_time() -> LocalTime {
    utc_time()
}

/// The current UTC time.
fn utc_time() -> LocalTime {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    time_from_unix(seconds)
}

/// The UTC date and time of a Unix time (days from the civil calendar: Howard
/// Hinnant's algorithm).
pub fn time_from_unix(seconds: u64) -> LocalTime {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u8;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u8;
    let year = (year_of_era + era * 400 + i64::from(month <= 2)) as u32;
    LocalTime {
        year,
        month,
        day,
        // 1970-01-01 was a Thursday.
        weekday: ((days + 4).rem_euclid(7)) as u8,
        hour: (rest / 3_600) as u8,
        minute: (rest % 3_600 / 60) as u8,
        second: (rest % 60) as u8,
    }
}

#[cfg(test)]
mod tests;
