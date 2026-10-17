# GBA emulator

A Game Boy Advance emulator written in Rust, with built-in cheats for the Gen 3 Pokemon games. You supply your own game
ROMs.

## Install

You need a recent [Rust toolchain](https://rustup.rs). Build and run in release
mode (the debug build is too slow to play):

```text
cargo run --release -- path/to/game.gba
```

To keep games in a library and start them by name:

```text
cargo run --release -- install path/to/game.gba    # copy a ROM into the library
cargo run --release -- list                        # show installed ROMs
cargo run --release -- "Pokemon Emerald"           # run one by (a prefix of) its name
```

Saves, settings and save states are kept beside each game. Moving the mouse over
the window slides up a dock with volume, full screen and fast-forward.

| Key | Action |
|---|---|
| W A S D / Space / C | D-pad / A / B |
| Q E / X / V | L R / Start / Select |
| Shift (hold) | Fast-forward |
| 0 / 1 | Save / load a state |
| Tab | Cheat menu |
| F11 | Full screen |
| Escape | Quit |

## Cheats

Open the cheat menu with **Tab** (or the CHEATS button in the dock). Each row has
its own checkbox, takes effect at once, and is remembered for the next time you
play that game.

Switchable in the menu:

- **Wild Pokémon is *species* at level *n*:** every wild encounter is the Pokémon
  and level you choose.
- **Perfect stats:** maximum IVs for wild Pokémon, opponents' Pokémon, or both.
- **Infinite money.**
- **100% catch rate.**
- **All experience is boosted:** every Pokémon earns the extra experience of a
  traded one.
- **Shiny chance:** from 1/1000 up to every Pokémon being shiny.
- **No random encounters.**
- **Max speed in battles:** emulator runs as fast as possible during battles.
- **Codes from a `.cheats` file:** CodeBreaker-style RAM codes placed beside the
  ROM, shown as a row when the file has any.

Always on (shown ticked at the bottom of the menu):

- **TMs are infinitely usable.**
- **The first Pokémon knows every HM**, and lists Cut, Surf, Flash, Fly, Dig,
  Teleport and the rest in its START-menu actions without knowing them.
- **No poison damage outside battle.**

