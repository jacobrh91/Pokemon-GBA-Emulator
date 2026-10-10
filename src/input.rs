//! GBA keypad buttons as bit masks, matching the KEYINPUT/KEYCNT bit layout
//! (a set bit means "pressed" in a [`Buttons`] mask).

pub type Buttons = u16;

pub const A: Buttons = 1 << 0;
pub const B: Buttons = 1 << 1;
pub const SELECT: Buttons = 1 << 2;
pub const START: Buttons = 1 << 3;
pub const RIGHT: Buttons = 1 << 4;
pub const LEFT: Buttons = 1 << 5;
pub const UP: Buttons = 1 << 6;
pub const DOWN: Buttons = 1 << 7;
pub const R: Buttons = 1 << 8;
pub const L: Buttons = 1 << 9;

pub const ALL: Buttons = 0x03FF;

/// Cancels opposing D-pad directions, which the hardware cannot produce.
pub fn without_opposing_directions(mut buttons: Buttons) -> Buttons {
    if buttons & (LEFT | RIGHT) == LEFT | RIGHT {
        buttons &= !(LEFT | RIGHT);
    }
    if buttons & (UP | DOWN) == UP | DOWN {
        buttons &= !(UP | DOWN);
    }
    buttons
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposing_directions_cancel_but_other_buttons_remain() {
        assert_eq!(without_opposing_directions(LEFT | RIGHT | A), A);
        assert_eq!(without_opposing_directions(UP | DOWN | LEFT), LEFT);
        assert_eq!(without_opposing_directions(UP | RIGHT), UP | RIGHT);
    }
}
