# GBA emulator

A Game Boy Advance emulator written in Rust. It boots and plays Pokémon ROMs with video, sound, input and battery saves, and runs far faster than real
time on a modern machine.

## Running

```text
cargo run --release -- path/to/game.gba      # run a ROM file
cargo run --release -- install path/to/game.gba [--force]
cargo run --release -- list
cargo run --release -- "My Pokemon ROM"      # run an installed ROM by (prefix of) name
```

Run it with `--release`; the debug build is far too slow to play.

### Game library

`install` copies a ROM into a per-user game library, one directory per game with a
`saves/` folder beside the ROM:

```text
<game library>/<My Pokemon ROM>/<My Pokemon ROM>.gba
<game library>/<My Pokemon ROM>/saves/
```

The game library is `~/Library/Application Support/gba_emulator` on macOS,
`$XDG_DATA_HOME/gba_emulator` (or `~/.local/share/gba_emulator`) on Linux and
`%APPDATA%\gba_emulator` on Windows. Set `GBA_EMULATOR_LIBRARY` to use another
directory. Installing validates the ROM header, does nothing for an identical
ROM, refuses to replace a different ROM of the same name without `--force`, and
never touches `saves/`.

Backup memory (SRAM, Flash) is loaded from `saves/<name>.sav` at startup and
written back, atomically, within about half a second of the game changing it and
again at shutdown. A ROM run straight from a path keeps its save beside it as
`<rom>.sav`.

## Settings

Per-game settings are kept in a plain-text file beside the ROM
(`<ROM name>.settings`) and restored the next time the game is played: volume,
mute, full screen, the windowed size and the fast-forward speed. The file is
`key = value` lines (see `src/settings.rs`); unknown or invalid entries are
ignored, and deleting it restores the defaults. Changes are written about once a
second and at exit.

## Save states

Press `0` to save the whole console to a state file beside the ROM
(`<ROM name>.state`) and `1` to play it back. There is one slot; saving again
replaces it. A floppy-disk icon in the top-right corner confirms a save, and the
same disk with a circular arrow confirms a load. If there is no state yet, `1`
does nothing (a note goes to the terminal).

A state is only accepted for the game that made it, and a truncated or damaged
file is rejected before anything changes, so a bad file never disturbs the
running game. Loading also writes the restored backup memory (battery save) back
to the `.sav` file. States are tied to this version of the emulator's internal
format; after an update that changes it, old states are refused rather than
misread.

## Controls

| Key | Button |
|---|---|
| W A S D | D-pad |
| Space | A |
| C | B |
| Q / E | L / R |
| X | Start |
| V | Select |
| Shift (hold) | Fast-forward |
| 0 | Save state (floppy-disk icon, top right) |
| 1 | Load the saved state (disk with a circular arrow) |
| F11 or Cmd+F | Toggle full screen |
| Escape | Leave full screen; quit when windowed |

Moving the mouse over the window slides up a dock with, left to right: power
(saves and quits), restart (power-cycles the console, keeping the save), full
screen, volume (drag the slider, scroll the wheel, or click the speaker to
mute), and the fast-forward speed (2X, 4X, 8X or MAX). While fast-forwarding, a
`>>` badge in the top-left corner shows the measured speed. Audio is muted while
the key is held.

The window is resizable with its 3:2 aspect ratio locked and a half-size minimum
(the aspect lock, full screen and exact cursor mapping use AppKit calls and are
macOS-only; elsewhere the picture is letterboxed).

## Architecture

The core is a library crate with no windowing or audio dependencies; the
desktop frontend is the binary.

```text
src/
  cpu/         ARM7TDMI interpreter (ARM and Thumb), cycle accounting, HLE BIOS
  mmu/         Memory bus: regions, wait states, timers, interrupts, display
               timing, DMA, Flash/SRAM saves, and the I/O register offsets (registers.rs)
  ppu/         Scanline renderer: backgrounds (text, affine, bitmap), sprites,
               windows, blending, mosaic
  apu/         PSG channels 1-4 and the Direct Sound FIFOs, mixed at 32768 Hz
  emulator/    Ties CPU, bus and PPU together; exact idle-loop skipping
  input.rs     Button bit layout
  game_library.rs  Game library and save-file handling
  settings.rs  Per-game settings file (volume, window, speed)
  state.rs     Binary codec for save states (each component saves its own fields)
  cli.rs       Command-line parsing
  frontend/    Desktop UI (minifb window, cpal audio)
    emulation_thread.rs   Runs the core, paced by audio or the wall clock
    dock/                 The control dock: state, layout, drawing
    window_control/       Native window features minifb lacks (macOS)
tests/         Integration tests (CPU, Armwrestler ROMs in tests/roms)
scripts/       check_codegen.sh
```

Notable design points:

- **Event-driven bus.** CPU cycles are accumulated and applied to the display,
  timers and audio only when something observable is due, which is what makes
  the core fast.
- **Exact idle-loop skipping.** A backward jump whose iteration provably changed
  no state is repeated arithmetically up to the next event. It is validated to
  produce cycle-identical results (see `emulator/tests.rs`).
- **Scanline-synchronised video.** The bus snapshots the display registers as
  each visible line finishes, before HBlank DMA runs, and the PPU draws from the
  snapshot, so per-line scroll and window effects behave as on hardware.
- **HLE BIOS.** The BIOS services games call are implemented directly; no BIOS
  image is needed.

## Testing

```text
cargo test
```

Unit tests live beside the code. The integration tests in `tests/` run the
Armwrestler ROMs in `tests/roms` through the whole emulator: `armwrestler.rs`
checks the pass/fail output of every suite, and `idle_skip.rs` runs the ROMs with
and without idle-loop skipping and requires identical CPU, memory, video and
audio state after every frame. Commercial ROMs are not included.

### Release-build codegen check

With rustc 1.92 on aarch64-apple-darwin, `lto = "fat"` combined with
`panic = "abort"` produced an invalid instruction and the app died with SIGILL
shortly after launch, so `panic = "abort"` is deliberately not set (see
`Cargo.toml`). After changing the release profile or toolchain, build and run
`scripts/check_codegen.sh` to scan the binary for undecodable instructions.

## Known limitations

- Mid-scanline register changes are not modelled; effects are per line.
- PSG channels are not band-limited, and there is no audio-driven speed sync.
- One save-state slot per game; no cheats, link cable or rumble.
- Verified mostly against Pokémon FireRed v1.0; other games may hit
  unimplemented behaviour.
