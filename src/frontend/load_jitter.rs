//! Randomness after loading a save state.
//!
//! Emulation is deterministic, so a restored state replays identically: the
//! same inputs on the same frame give the same result, including the game's
//! random number generator (Gen 3 Pokémon advances its generator once per
//! frame, so a failed catch fails the same way every time). To get a fresh
//! outcome, the console is run a few extra frames, silently, before the player
//! gets control back; the generator is then somewhere else for the same
//! inputs.
//!
//! The extra frame counts come from a shuffled bag, so a count is not repeated
//! until every count has been used once.

use gba_emulator::emulator::Emulator;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;

/// Longest extra run supported (10 seconds at 60 Hz).
pub(super) const MAX_JITTER_FRAMES: u32 = 600;

pub(super) struct JitterBag {
    remaining: Vec<u32>,
    /// The `max_frames` the bag was filled for.
    filled_for: u32,
    state: u64,
}

impl JitterBag {
    pub(super) fn new() -> Self {
        // `RandomState` is seeded from the operating system.
        let seed = RandomState::new().build_hasher().finish();
        Self::with_seed(seed)
    }

    pub(super) fn with_seed(seed: u64) -> Self {
        Self {
            remaining: Vec::new(),
            filled_for: 0,
            // xorshift needs a non-zero state.
            state: seed | 1,
        }
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// The next extra-frame count in `0..=max_frames`.
    pub(super) fn next(&mut self, max_frames: u32) -> u32 {
        let max_frames = max_frames.min(MAX_JITTER_FRAMES);
        if max_frames == 0 {
            return 0;
        }
        if self.remaining.is_empty() || self.filled_for != max_frames {
            self.remaining = (0..=max_frames).collect();
            self.filled_for = max_frames;
            // Fisher-Yates.
            for index in (1..self.remaining.len()).rev() {
                let other = (self.next_random() % (index as u64 + 1)) as usize;
                self.remaining.swap(index, other);
            }
        }
        self.remaining.pop().unwrap_or(0)
    }

    /// Starts a fresh round (for example when a new state is saved).
    pub(super) fn reset(&mut self) {
        self.remaining.clear();
    }
}

/// Runs `frames` whole frames with no buttons held, discarding the audio. Only
/// the last frame is drawn, which is all that is ever shown.
pub(super) fn advance_frames(emulator: &mut Emulator, frames: u32) -> io::Result<()> {
    if frames == 0 {
        return Ok(());
    }
    emulator.set_buttons(0);
    for frame in 0..frames {
        emulator.set_rendering(frame + 1 == frames);
        while !emulator.take_frame_ready() {
            emulator.step()?;
        }
    }
    // The audio belongs to frames the player never heard.
    let _ = emulator.take_audio_samples();
    emulator.set_rendering(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gba_emulator::mmu::ROM_START;

    #[test]
    fn every_count_is_used_once_before_any_repeats() {
        let mut bag = JitterBag::with_seed(12345);
        for _round in 0..3 {
            let mut seen: Vec<u32> = (0..=9).map(|_| bag.next(9)).collect();
            seen.sort_unstable();
            assert_eq!(seen, (0..=9).collect::<Vec<u32>>());
        }
    }

    #[test]
    fn rounds_are_shuffled_not_sequential() {
        let mut bag = JitterBag::with_seed(99);
        let first: Vec<u32> = (0..=29).map(|_| bag.next(29)).collect();
        let sorted: Vec<u32> = (0..=29).collect();
        let reversed: Vec<u32> = (0..=29).rev().collect();
        assert_ne!(first, sorted);
        assert_ne!(first, reversed);
        let second: Vec<u32> = (0..=29).map(|_| bag.next(29)).collect();
        assert_ne!(first, second, "each round is shuffled afresh");
    }

    #[test]
    fn zero_disables_the_jitter_and_large_values_are_capped() {
        let mut bag = JitterBag::with_seed(7);
        assert!((0..20).all(|_| bag.next(0) == 0));
        assert!((0..2000).all(|_| bag.next(1_000_000) <= MAX_JITTER_FRAMES));
    }

    #[test]
    fn changing_the_maximum_starts_a_new_round() {
        let mut bag = JitterBag::with_seed(3);
        bag.next(5);
        assert!((0..50).all(|_| bag.next(2) <= 2));
    }

    #[test]
    fn different_seeds_give_different_orders() {
        let order = |seed| {
            let mut bag = JitterBag::with_seed(seed);
            (0..=30).map(|_| bag.next(30)).collect::<Vec<_>>()
        };
        assert_ne!(order(1), order(2));
    }

    fn spin_emulator() -> Emulator {
        let path = std::env::temp_dir().join(format!(
            "gba-jitter-test-{}-{:?}.gba",
            std::process::id(),
            std::thread::current().id()
        ));
        // ARM `b .` forever.
        std::fs::write(&path, 0xEAFF_FFFEu32.to_le_bytes()).unwrap();
        let mut emulator = Emulator::new(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        emulator.initialize_gba_startup(ROM_START).unwrap();
        emulator
    }

    #[test]
    fn advancing_runs_exactly_that_many_whole_frames() {
        const FRAME_CYCLES: u64 = 1232 * 228;
        let mut emulator = spin_emulator();
        advance_frames(&mut emulator, 1).unwrap();
        let start = emulator.cpu().cycles;
        advance_frames(&mut emulator, 7).unwrap();
        assert_eq!(emulator.cpu().cycles - start, 7 * FRAME_CYCLES);
        advance_frames(&mut emulator, 0).unwrap();
        assert_eq!(emulator.cpu().cycles - start, 7 * FRAME_CYCLES);
    }
}
