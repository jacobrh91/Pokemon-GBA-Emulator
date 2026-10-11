//! Interrupt controller (IE/IF/IME) and the keypad interrupt (GBATEK
//! "Interrupt Control", "Keypad Interrupt").

use super::*;

/// IF bit 12 (high byte bit 4).
pub(super) const KEYPAD_IRQ_BIT: u8 = 1 << 4;

impl MemoryBus {
    /// True when any enabled interrupt source is requested (`IE & IF`),
    /// regardless of IME. This is the condition that wakes a halted CPU.
    pub fn interrupt_requested(&self) -> bool {
        let ie = u16::from_le_bytes([
            self.io_regs[INTERRUPT_ENABLE],
            self.io_regs[INTERRUPT_ENABLE + 1],
        ]);
        let requested = u16::from_le_bytes([
            self.io_regs[INTERRUPT_REQUEST],
            self.io_regs[INTERRUPT_REQUEST + 1],
        ]);
        ie & requested != 0
    }

    pub fn irq_pending(&self) -> bool {
        self.io_regs[INTERRUPT_MASTER_ENABLE] & 1 != 0 && self.interrupt_requested()
    }

    /// Sets the pressed buttons (see [`crate::input`]); KEYINPUT reads them
    /// active-low. Raises the keypad interrupt when KEYCNT's condition
    /// becomes true.
    pub fn set_pressed_buttons(&mut self, pressed: u16) {
        self.pressed_buttons = pressed & crate::input::ALL;
        self.evaluate_keypad_irq();
    }

    pub(super) fn evaluate_keypad_irq(&mut self) {
        let control = u16::from_le_bytes([self.io_regs[KEYCNT], self.io_regs[KEYCNT + 1]]);
        let selected = control & crate::input::ALL;
        let satisfied = control & (1 << 14) != 0
            && selected != 0
            && if control & (1 << 15) != 0 {
                // AND mode: every selected button held.
                self.pressed_buttons & selected == selected
            } else {
                self.pressed_buttons & selected != 0
            };
        if satisfied && !self.keypad_irq_active {
            self.io_regs[INTERRUPT_REQUEST + 1] |= KEYPAD_IRQ_BIT;
        }
        self.keypad_irq_active = satisfied;
    }
}
