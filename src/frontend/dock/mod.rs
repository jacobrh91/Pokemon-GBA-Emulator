//! A control dock (power, restart, full screen, volume, fast-forward speed) that slides up from
//! the bottom of the window whenever the mouse moves over it, and slides away
//! again once the mouse has been still for a moment or leaves the window.
//!
//! Interaction logic ([`Dock::update`]) is separate from drawing
//! ([`Dock::draw`], in `draw.rs`) so it can be unit tested without a window.

mod draw;
mod layout;
mod speed;

use layout::{KNOB_RADIUS, Layout};
pub use speed::SpeedSetting;

/// Height of the strip along the bottom of the window that keeps the dock
/// open for as long as the mouse is anywhere in it.
const HOVER_ZONE: f32 = 110.0;
/// Frames of mouse stillness (about 2.5 s at 60 Hz) before the dock hides.
const HIDE_AFTER_IDLE_FRAMES: u32 = 150;
/// Fraction of the slide animation advanced per frame.
const REVEAL_STEP: f32 = 0.18;
const WHEEL_STEP: f32 = 0.05;

#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    /// Cursor position in window pixels, or `None` when outside the window.
    pub position: Option<(f32, f32)>,
    pub left_down: bool,
    /// Vertical wheel movement since the last frame (positive = up).
    pub scroll: f32,
}

pub struct Dock {
    volume: f32,
    muted: bool,
    /// 0.0 hidden .. 1.0 fully shown.
    reveal: f32,
    dragging: bool,
    was_down: bool,
    speed: SpeedSetting,
    last_position: Option<(f32, f32)>,
    idle_frames: u32,
    restart_requested: bool,
    quit_requested: bool,
    fullscreen_requested: bool,
}

impl Dock {
    pub fn new(initial_volume: f32) -> Self {
        Self {
            volume: initial_volume.clamp(0.0, 1.0),
            muted: false,
            reveal: 0.0,
            dragging: false,
            was_down: false,
            speed: SpeedSetting::default(),
            last_position: None,
            idle_frames: HIDE_AFTER_IDLE_FRAMES,
            restart_requested: false,
            quit_requested: false,
            fullscreen_requested: false,
        }
    }

    /// True once per click of the full-screen button.
    pub fn take_fullscreen_request(&mut self) -> bool {
        std::mem::take(&mut self.fullscreen_requested)
    }

    /// True once per click of the power button (the app should exit).
    pub fn take_quit_request(&mut self) -> bool {
        std::mem::take(&mut self.quit_requested)
    }

    /// True once per click of the restart button.
    pub fn take_restart_request(&mut self) -> bool {
        std::mem::take(&mut self.restart_requested)
    }

    /// The selected fast-forward speed.
    pub fn speed(&self) -> SpeedSetting {
        self.speed
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Restores the mute state (from saved settings).
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Restores the selected fast-forward speed (from saved settings).
    pub fn set_speed(&mut self, speed: SpeedSetting) {
        self.speed = speed;
    }

    #[cfg(test)]
    pub fn reveal(&self) -> f32 {
        self.reveal
    }

    /// Linear gain to apply to the audio. The slider is perceptual, so the
    /// gain follows a squared curve.
    pub fn gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume * self.volume
        }
    }

    /// Advances the slide animation and processes pointer input for one frame.
    pub fn update(&mut self, pointer: Pointer, width: usize, height: usize) {
        let layout = Layout::new(width, height);

        // The dock follows the mouse: any movement over the window shows it,
        // staying near the bottom keeps it, and stillness hides it again.
        if pointer.position.is_some() && pointer.position != self.last_position {
            self.idle_frames = 0;
        } else {
            self.idle_frames = self.idle_frames.saturating_add(1);
        }
        self.last_position = pointer.position;
        let in_hover_zone = pointer
            .position
            .is_some_and(|(_, y)| y >= height as f32 - HOVER_ZONE);
        let active = pointer.position.is_some()
            && (in_hover_zone || self.idle_frames < HIDE_AFTER_IDLE_FRAMES);
        let target = if active || self.dragging { 1.0 } else { 0.0 };
        let delta: f32 = target - self.reveal;
        self.reveal = (self.reveal + delta.clamp(-REVEAL_STEP, REVEAL_STEP)).clamp(0.0, 1.0);

        let pressed = pointer.left_down && !self.was_down;
        self.was_down = pointer.left_down;
        if !pointer.left_down {
            self.dragging = false;
        }

        // Ignore input while the dock is still sliding mostly out of view.
        if self.reveal < 0.5 {
            return;
        }
        if let Some((x, y)) = pointer.position {
            if pressed {
                if layout.power.contains(x, y) {
                    self.quit_requested = true;
                } else if layout.restart.contains(x, y) {
                    self.restart_requested = true;
                } else if layout.fullscreen.contains(x, y) {
                    self.fullscreen_requested = true;
                } else if layout.icon.inflated(6).contains(x, y) {
                    self.muted = !self.muted;
                } else if layout.track.inflated(KNOB_RADIUS).contains(x, y) {
                    self.dragging = true;
                    self.muted = false;
                } else if let Some(index) = layout
                    .speed_buttons
                    .iter()
                    .position(|button| button.contains(x, y))
                {
                    self.speed = SpeedSetting::ALL[index];
                }
            }
            if pointer.scroll != 0.0 && layout.dock.contains(x, y) {
                self.volume = (self.volume + pointer.scroll.signum() * WHEEL_STEP).clamp(0.0, 1.0);
                self.muted = false;
            }
        }
        if self.dragging
            && let Some((x, _)) = pointer.position
        {
            self.volume = ((x - layout.track.x as f32) / layout.track.width as f32).clamp(0.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests;
