//! Save states must capture the whole machine: a console restored from a
//! state has to behave exactly like the one the state was taken from.

mod common;

use common::{Snapshot, new_emulator, run_frame, snapshot};
use gba_emulator::emulator::Emulator;

const ROM: &str = "armwrestler-gba-fixed.gba";
/// Frames before the state is taken (the menu has been walked into a suite).
const BEFORE: u64 = 60;
/// Frames compared after the state is restored.
const AFTER: u64 = 50;

fn run_frames(emulator: &mut Emulator, from: u64, count: u64) -> Vec<Snapshot> {
    (from..from + count)
        .map(|frame| {
            run_frame(emulator, frame);
            snapshot(emulator)
        })
        .collect()
}

/// An emulator that has run `BEFORE` frames, with its audio drained.
fn advanced() -> Emulator {
    let mut emulator = new_emulator(ROM);
    run_frames(&mut emulator, 0, BEFORE);
    emulator
}

#[test]
fn a_fresh_console_restored_from_a_state_continues_identically() {
    let mut original = advanced();
    let state = original.save_state();
    let expected = run_frames(&mut original, BEFORE, AFTER);

    let mut restored = new_emulator(ROM);
    restored.load_state(&state).unwrap();
    let actual = run_frames(&mut restored, BEFORE, AFTER);

    for (frame, (expected, actual)) in expected.iter().zip(&actual).enumerate() {
        assert_eq!(
            expected,
            actual,
            "restored console diverged {} frames after the state",
            frame + 1
        );
    }
}

#[test]
fn loading_a_state_rewinds_a_console_that_has_moved_on() {
    let mut emulator = advanced();
    let state = emulator.save_state();
    let first = run_frames(&mut emulator, BEFORE, AFTER);
    // The run went somewhere different from where the state was taken.
    assert_ne!(first[0], first[AFTER as usize - 1]);

    emulator.load_state(&state).unwrap();
    let second = run_frames(&mut emulator, BEFORE, AFTER);
    assert_eq!(first, second);
}

#[test]
fn saving_does_not_disturb_the_running_console() {
    let mut with_save = advanced();
    let mut without = advanced();
    let _ = with_save.save_state();
    assert_eq!(
        run_frames(&mut with_save, BEFORE, 10),
        run_frames(&mut without, BEFORE, 10)
    );
}

#[test]
fn a_state_is_the_same_bytes_every_time() {
    assert_eq!(advanced().save_state(), advanced().save_state());
}

#[test]
fn damaged_or_foreign_states_are_rejected_without_touching_the_console() {
    let mut emulator = advanced();
    let state = emulator.save_state();
    let before = snapshot(&mut emulator);

    let mut flipped = state.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0x40;
    let mut bad_magic = state.clone();
    bad_magic[0] ^= 0xFF;
    let mut other_version = state.clone();
    other_version[8] ^= 0x01;

    let mut other_game = new_emulator("armwrestler.gba");
    run_frames(&mut other_game, 0, 5);
    let foreign = other_game.save_state();

    for (name, bytes) in [
        ("empty", Vec::new()),
        ("truncated", state[..state.len() - 1].to_vec()),
        ("header only", state[..48].to_vec()),
        ("bit flip", flipped),
        ("bad magic", bad_magic),
        ("other version", other_version),
        ("different game", foreign),
    ] {
        assert!(emulator.load_state(&bytes).is_err(), "{name} was accepted");
    }
    assert_eq!(
        snapshot(&mut emulator),
        before,
        "a rejected state changed the console"
    );
}
