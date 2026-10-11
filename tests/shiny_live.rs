//! The all-shiny cheat on real FireRed ROMs, from the save states left in the
//! grass: walk until a wild Pokémon appears, with and without the cheat, and
//! compare. Each game is skipped when its ROM or state is not in `rom/`
//! (they are not in the repository).

use gba_emulator::cheats::firered::shiny::is_shiny;
use gba_emulator::cheats::options::CheatOptions;
use gba_emulator::emulator::Emulator;
use gba_emulator::input::{LEFT, RIGHT};
use std::path::Path;

/// `(ROM, state, address of gEnemyParty[0])`.
const GAMES: [(&str, &str, u32); 3] = [
    (
        "Pokemon Fire Red/Pokemon Fire Red (v1.0).gba",
        "Pokemon Fire Red/Pokemon Fire Red (v1.0).state",
        0x0202_402C,
    ),
    (
        "Pokemon Fire Red/Pokemon Fire Red (v1.1).gba",
        "Pokemon Fire Red/Pokemon Fire Red (v1.1).state",
        0x0202_402C,
    ),
    (
        "Pokemon Fire Red/Pokemon Fire Red (J).gba",
        "Pokemon Fire Red/Pokemon Fire Red (J).state",
        0x0202_3F8C,
    ),
];

/// Frames to walk before giving up on an encounter.
const MAX_FRAMES: u32 = 60 * 60 * 3;

/// Walks left and right from the state until the enemy party's first Pokémon
/// changes; returns its `(personality, OT ID)`, or `None` if it never does.
fn encounter(rom: &str, state: &[u8], enemy: u32, shiny: bool) -> Option<(u32, u32)> {
    let mut emulator = Emulator::new(rom).unwrap();
    emulator.initialize_gba_startup(0x0800_0000).unwrap();
    emulator.load_state(state).unwrap();
    emulator.set_rendering(false);
    emulator.set_cheat_options(CheatOptions {
        shiny,
        ..CheatOptions::default()
    });
    let read = |emulator: &Emulator| {
        let bus = emulator.memory();
        (
            bus.read_u32(enemy).unwrap(),
            bus.read_u32(enemy + 4).unwrap(),
        )
    };
    let before = read(&emulator);
    for frame in 0..MAX_FRAMES {
        // Eight frames per direction: a step is 16 frames on foot.
        emulator.set_buttons(if (frame / 16) % 2 == 0 { LEFT } else { RIGHT });
        while !emulator.take_frame_ready() {
            emulator.step().unwrap();
        }
        let now = read(&emulator);
        if now != before && now.0 != 0 {
            return Some(now);
        }
    }
    None
}

#[test]
fn the_first_wild_pokemon_from_each_grass_state_is_shiny_with_the_cheat() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rom");
    for (rom, state, enemy) in GAMES {
        let (rom, state) = (root.join(rom), root.join(state));
        let (Ok(_), Ok(state)) = (std::fs::metadata(&rom), std::fs::read(&state)) else {
            continue;
        };
        let rom = rom.to_str().unwrap();
        let plain = encounter(rom, &state, enemy, false)
            .unwrap_or_else(|| panic!("{rom}: no encounter without the cheat"));
        let cheated = encounter(rom, &state, enemy, true)
            .unwrap_or_else(|| panic!("{rom}: no encounter with the cheat"));
        eprintln!("{rom}: off {plain:08X?}, on {cheated:08X?}");
        assert!(is_shiny(cheated.0, cheated.1), "{rom}: not shiny");
        assert_eq!(cheated.1, plain.1, "{rom}: OT ID untouched");
        assert_eq!(cheated.0 & 0xFF, plain.0 & 0xFF, "{rom}: gender byte");
        assert_eq!(cheated.0 % 25, plain.0 % 25, "{rom}: nature");
    }
}
