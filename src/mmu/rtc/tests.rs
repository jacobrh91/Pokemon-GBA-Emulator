use super::*;

fn fixed_clock() -> LocalTime {
    LocalTime {
        year: 2026,
        month: 10,
        day: 17,
        weekday: 6,
        hour: 21,
        minute: 5,
        second: 9,
    }
}

/// A chip as the game sets the port up: `SCK` and `CS` driven by the console,
/// `SIO` too until it listens.
fn chip() -> Rtc {
    let mut rtc = Rtc::new(fixed_clock);
    rtc.write(READ_ENABLE, 1);
    rtc.write(DIRECTION, 0b111);
    rtc
}

fn begin(rtc: &mut Rtc) {
    rtc.write(DATA, SCK);
    rtc.write(DATA, SCK | CS);
}

fn end(rtc: &mut Rtc) {
    rtc.write(DATA, SCK);
}

/// One bit from the console, as the game sends it.
fn send_bit(rtc: &mut Rtc, bit: bool) {
    let sio = if bit { SIO } else { 0 };
    rtc.write(DATA, sio | CS);
    rtc.write(DATA, sio | CS | SCK);
}

fn send_command(rtc: &mut Rtc, command: u8) {
    for index in (0..8).rev() {
        send_bit(rtc, (command >> index) & 1 != 0);
    }
}

fn send_byte(rtc: &mut Rtc, byte: u8) {
    for index in 0..8 {
        send_bit(rtc, (byte >> index) & 1 != 0);
    }
}

/// One byte from the chip: the console listens on `SIO`.
fn read_byte(rtc: &mut Rtc) -> u8 {
    rtc.write(DIRECTION, SCK | CS);
    let mut byte = 0;
    for index in 0..8 {
        rtc.write(DATA, CS);
        rtc.write(DATA, CS | SCK);
        byte |= u8::from(rtc.read(DATA) & SIO != 0) << index;
    }
    rtc.write(DIRECTION, 0b111);
    byte
}

/// Reads register `register`: `length` bytes.
fn read_register(rtc: &mut Rtc, register: u8, length: usize) -> Vec<u8> {
    begin(rtc);
    send_command(rtc, 0x60 | (register << 1) | 1);
    let bytes = (0..length).map(|_| read_byte(rtc)).collect();
    end(rtc);
    bytes
}

fn write_register(rtc: &mut Rtc, register: u8, bytes: &[u8]) {
    begin(rtc);
    send_command(rtc, 0x60 | (register << 1));
    for byte in bytes {
        send_byte(rtc, *byte);
    }
    end(rtc);
}

#[test]
fn the_date_and_time_are_the_clocks_in_bcd() {
    let mut rtc = chip();
    assert_eq!(
        read_register(&mut rtc, 2, 7),
        [0x26, 0x10, 0x17, 0x06, 0x21, 0x05, 0x09],
        "year, month, day, weekday, hour, minute, second"
    );
    assert_eq!(read_register(&mut rtc, 3, 3), [0x21, 0x05, 0x09]);
}

#[test]
fn the_status_has_the_24_hour_flag_and_never_the_power_failure_flag() {
    let mut rtc = chip();
    let status = read_register(&mut rtc, 1, 1)[0];
    assert_eq!(status & 0x40, 0x40, "24-hour mode");
    assert_eq!(status & 0x80, 0, "the battery has not run dry");
}

#[test]
fn the_status_can_be_set_and_reset_clears_it() {
    let mut rtc = chip();
    // 12-hour mode with the frequency interrupt enabled; bit 7 is ignored.
    write_register(&mut rtc, 1, &[0x82]);
    assert_eq!(read_register(&mut rtc, 1, 1), [0x02]);
    // In 12-hour mode 21:05 is 9 PM.
    assert_eq!(read_register(&mut rtc, 3, 3)[0], 0x09 | 0x40);
    // The reset command clears the status; the game then sets 24-hour mode.
    begin(&mut rtc);
    send_command(&mut rtc, 0x60);
    end(&mut rtc);
    assert_eq!(read_register(&mut rtc, 1, 1), [0x00]);
    write_register(&mut rtc, 1, &[0x40]);
    assert_eq!(read_register(&mut rtc, 3, 3)[0], 0x21);
}

#[test]
fn the_date_and_time_cannot_be_set_and_alarms_are_kept() {
    let mut rtc = chip();
    write_register(&mut rtc, 2, &[0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00]);
    assert_eq!(
        read_register(&mut rtc, 3, 3),
        [0x21, 0x05, 0x09],
        "still the clock's"
    );
    write_register(&mut rtc, 4, &[0x03, 0x12, 0x30]);
    assert_eq!(read_register(&mut rtc, 4, 3), [0x03, 0x12, 0x30]);
    assert_eq!(read_register(&mut rtc, 5, 3), [0, 0, 0], "the other alarm");
}

#[test]
fn a_command_that_is_not_the_chips_is_ignored_and_a_new_transfer_starts_fresh() {
    let mut rtc = chip();
    begin(&mut rtc);
    send_command(&mut rtc, 0x12); // not 0110 in the high nibble
    end(&mut rtc);
    // The next transfer works as usual.
    assert_eq!(read_register(&mut rtc, 3, 3), [0x21, 0x05, 0x09]);
    // A transfer cut short by CS going low writes nothing.
    begin(&mut rtc);
    send_command(&mut rtc, 0x62); // write the status
    for index in 0..4 {
        send_bit(&mut rtc, (0x02u8 >> index) & 1 != 0);
    }
    end(&mut rtc);
    assert_eq!(read_register(&mut rtc, 1, 1)[0], 0x40);
}

#[test]
fn the_port_reads_back_what_the_console_drives_and_the_rom_shows_when_it_is_off() {
    let mut rtc = Rtc::new(fixed_clock);
    assert!(rtc.readable(), "on from the start");
    rtc.write(READ_ENABLE, 0);
    assert!(
        !rtc.readable(),
        "the ROM shows through when the game turns it off"
    );
    rtc.write(READ_ENABLE, 1);
    assert!(rtc.readable());
    assert_eq!(rtc.read(READ_ENABLE), 1);
    rtc.write(DIRECTION, 0b101);
    assert_eq!(rtc.read(DIRECTION), 0b101);
    rtc.write(DATA, 0b111);
    // Only the pins it drives take what was written (`SIO` is the chip's).
    assert_eq!(rtc.read(DATA), 0b101);
    assert!(Rtc::is_gpio(0xC4) && Rtc::is_gpio(0xC9));
    assert!(!Rtc::is_gpio(0xC3) && !Rtc::is_gpio(0xCA));
}

#[test]
fn the_cartridges_with_a_clock_are_ruby_sapphire_and_emerald() {
    for code in [b"AXVE", b"AXPJ", b"BPEE", b"BPEJ"] {
        assert!(cartridge_has_clock(code), "{code:?}");
    }
    for code in [b"BPRE", b"BPGE", b"AMTE", b"    "] {
        assert!(!cartridge_has_clock(code), "{code:?}");
    }
}

#[test]
fn unix_time_becomes_the_calendar_date_and_time() {
    // 2026-10-17 21:05:09 UTC, a Saturday.
    let seconds = 1_792_271_109;
    let time = time_from_unix(seconds);
    assert_eq!(
        time,
        LocalTime {
            year: 2026,
            month: 10,
            day: 17,
            weekday: 6,
            hour: 21,
            minute: 5,
            second: 9
        }
    );
    // The epoch, a leap day, and the end of a year.
    assert_eq!(
        (time_from_unix(0).year, time_from_unix(0).weekday),
        (1970, 4)
    );
    let leap = time_from_unix(1_709_164_800); // 2024-02-29 00:00:00
    assert_eq!((leap.year, leap.month, leap.day), (2024, 2, 29));
    let end = time_from_unix(1_798_761_599); // 2026-12-31 23:59:59
    assert_eq!((end.year, end.month, end.day, end.hour), (2026, 12, 31, 23));
}

#[test]
fn the_host_time_is_a_valid_date() {
    let now = host_time();
    assert!(now.year >= 2024);
    assert!((1..=12).contains(&now.month) && (1..=31).contains(&now.day));
    assert!(now.weekday < 7 && now.hour < 24 && now.minute < 60 && now.second < 61);
}
