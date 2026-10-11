//! Idle-loop skipping must be invisible: a machine that skips provably idle
//! loop iterations has to match one that runs every instruction, cycle for
//! cycle, at every frame boundary (CPU state, memory, video and audio).

mod common;

use common::{new_emulator, snapshot};

const FRAMES: u64 = 150;

/// Runs `rom` with and without skipping, asserting identical state after every
/// frame. Returns `(steps with skipping, steps without)`.
fn assert_skipping_is_invisible(rom: &str) -> (u64, u64) {
    let mut skipping = new_emulator(rom);
    let mut plain = new_emulator(rom);
    skipping.set_idle_loop_skipping(true);
    plain.set_idle_loop_skipping(false);
    let (mut skipping_steps, mut plain_steps) = (0, 0);

    for frame in 0..FRAMES {
        for (emulator, steps) in [
            (&mut skipping, &mut skipping_steps),
            (&mut plain, &mut plain_steps),
        ] {
            emulator.set_buttons(common::buttons_at(frame));
            while !emulator.take_frame_ready() {
                emulator.step().unwrap();
                *steps += 1;
            }
        }
        assert_eq!(
            snapshot(&mut skipping),
            snapshot(&mut plain),
            "{rom}: state diverged at frame {frame} when idle loops were skipped"
        );
    }
    (skipping_steps, plain_steps)
}

#[test]
fn skipping_idle_loops_changes_nothing_observable() {
    let (skipping, plain) = assert_skipping_is_invisible("armwrestler-gba-fixed.gba");
    // The comparison is only meaningful if skipping actually happened. The
    // test suites are compute-bound, so only the menu and the VBlank waits
    // between results are idle.
    assert!(
        skipping + plain / 20 < plain,
        "expected skipping to remove steps, got {skipping} vs {plain}"
    );
}

#[test]
fn skipping_is_invisible_on_the_original_rom_too() {
    assert_skipping_is_invisible("armwrestler.gba");
}
