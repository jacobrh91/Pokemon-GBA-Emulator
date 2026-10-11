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
mute, full screen, the windowed size, the fast-forward speed and the
save-state randomness range. The file is
`key = value` lines (see `src/settings.rs`); unknown or invalid entries are
ignored, and deleting it restores the defaults. Changes are written about once a
second and at exit.

## Cheat menu

Press **Tab**, or click the **CHEATS** button at the right end of the dock, to
open a menu of cheats above the dock. Every row has its own **checkbox at the
left** that switches that cheat on and off; there is no global switch. The menu
closes with Tab, the button, Escape, C (the B button), or a click anywhere else, and the choices
apply immediately and are remembered in the settings file for the next time the
game is played.

| Row (checkbox at its left) | Other controls | Status |
|---|---|---|
| *n* codes from the cheats file (only when the file has codes) | | **active**; never remembered, so it starts off every session |
| Wild Pokémon is *species* at level *n* | dropdown: ANY, then the 251 species; `-` / `+`, mouse wheel, `X` to clear for the level (empty keeps the game's) | **active** (FireRed) |
| Perfect stats for | dropdown: WILD POKEMON, OPPONENTS POKEMON or BOTH (wild first, the default) | **active** (FireRed) |
| Infinite money | | **active** (FireRed) |
| 100% catch rate | | **active** (FireRed) |
| All experience is boosted | | **active** (FireRed); every Pokémon counts as traded (×1.5) |
| All Pokémon are shiny | | **active** (FireRed) |
| No random encounters | | **active** (FireRed) |
| TMs are infinitely usable | | **always on** (FireRed): shown ticked at the bottom, with no switch |

- **From the keyboard**, while the menu is open the game gets no buttons and
  the movement keys drive the menu. The first press only shows which row has the
  focus (a highlighted band, starting on the top row). Then **W / S** (or Up /
  Down) move between the rows and wrap, and **Space** (or Enter) is the A
  button: it switches the focused row on or off. On the wild row, **A / D** (or
  Left / Right) move between its checkbox, species field and level down / up
  buttons, and A uses the one in focus: on a level button it steps the level, on
  the species field it opens the species list, where W / S move through the
  Pokémon, A chooses the one under the cursor and **C** (the B button) leaves the
  list without choosing. On the perfect-stats row, A / D cycle the target. With
  no list open, C, Backspace or Escape close the menu (as does Tab). After it
  closes the game ignores any key that is still held, until it is released, so
  the press that closed the menu is not taken by the game.
- **A row that is switched off keeps its choices**, shown dimmed, so you can
  turn it back on without choosing again.
- **Choosing in a row switches it on**: picking a species or a level, or
  choosing a different perfect-stats target, ticks the row's checkbox too, so a
  choice is never silently inactive. Choosing the target that is already
  selected changes nothing.
- **The species list** scrolls with the mouse wheel or by pressing and dragging
  its right edge. Species and level are independent: a species alone keeps the
  area's level, and a level alone keeps the species.
- **Wild encounters** are changed through the function hook described under
  *Wild species override*, in the FireRed builds listed there; they need no
  `.cheats` file.
- **Rows tagged SOON** would be stored and shown but not change the game yet; every row is active at present, so none is tagged.

## Cheats

Put cheat codes in a text file beside the ROM, named `<ROM name>.cheats`, one
CodeBreaker-format code (`XXXXXXXX YYYY`, 8 + 4 hex digits) per line; `#` starts
a comment. They are read at startup and applied once per frame, so they keep
winning against the game rewriting the value. At launch the terminal reports how
many writes were loaded and any line that could not be used.

| Code | Meaning |
|---|---|
| `3aaaaaaa 00vv` | write the byte `vv` to `aaaaaaa` |
| `8aaaaaaa vvvv` | write the halfword `vvvv` to `aaaaaaa` (even address) |
| `0000xxxx 000y` | master code 1, the game's identity (ignored) |
| `1aaaaaaa yyyy` | master code 2, the hook: ROM offset `aaaaaaa` |

On real hardware a CodeBreaker cartridge must get its code handler to run
regularly, so its master code names a ROM address that the game executes every
frame, where the cartridge hooks itself in, and the cheat writes happen from
that point in the program. The emulator does the same: when a code of the form
`1aaaaaaa yyyy` is present, the writes are performed each time execution reaches
that address; without one they are performed once per frame. Writes are limited
to RAM (`0x02......` EWRAM and `0x03......` IWRAM); any other line, including
code types not listed above, is reported and skipped rather than guessed at.

**The codes start off every session; tick the "codes from the cheats file" row
of the cheat menu (Tab) to switch them on.** That row appears only when the file
has codes, and is deliberately never remembered between sessions. Many codes do
not poke data but patch routines of the game that sit in RAM, and those patches
are only valid while the game has that routine loaded. Applied from the first
frame, a patch like the popular wild-encounter code crashes the game during the
intro; switched on after the game is up it runs normally. Use such cheats on
demand, the way a cartridge's on/off menu did.

### 100% catch rate (Pokémon FireRed)

A thrown ball is resolved by one battle-script command: it works out `odds`
from the target's catch rate, the ball and the target's HP and status, and then
tests `if (odds > 254)`. Above 254 the Pokémon is caught on the spot; otherwise
a shake roll decides. With the row switched on, `odds` is set to 255 at that
test, so every ball is a catch, whatever the species, level, HP or ball. It
works in the same builds as the wild species override (the address of the test
is found by a signature scan of the ROM, like `CreateWildMon`; the table in
`src/cheats/firered/mod.rs` lists the addresses).

In the game, a thrown Poké Ball at a wild level-100 Mewtwo at full health, which
breaks free ("Oh, no! The POKéMON broke free!") with the cheat off, is caught
("Gotcha! MEWTWO was caught!") with it on, on v1.0, v1.1 and the Japanese build.
Trainers' Pokémon cannot be caught either way: the game refuses the ball
before this test.

### Infinitely usable TMs (Pokémon FireRed)

When a Pokémon has learned its move from a TM, the game takes the item out of
the bag unless it is an HM (`RemoveBagItem(item, 1)`, guarded by "item is at
most TM50"). The emulator sets the count of that call to 0, so the TM stays at
its count. This cheat is **always on** while a FireRed ROM is recognised: it
has no switch and the menu lists it last, ticked and tagged ALWAYS ON. The
code appears twice in every build (one for each way of finishing the lesson:
a free move slot, or replacing a move) and both places are hooked, found by a
signature scan (`src/cheats/firered/tm.rs`). Selling or tossing a TM is not
affected, only using it.

Checked on v1.1 from the save state about to teach TM39 Rock Tomb to a Mewtwo:
without the hook the game calls `RemoveBagItem(327, 1)`; with it the call is
`RemoveBagItem(327, 0)`. The other builds have the same bytes (two matches
each) but were not run in the game.

### No random encounters (Pokémon FireRed)

Every step on an encounter tile asks one small function whether a wild
encounter happens: it compares `Random() % 1600` with the encounter rate (after
the tile, a Repel, the lead Pokémon's ability and so on have scaled it) and
returns 1 for "yes". With the row on, the emulator sets that answer to 0 at the
end of the function, so the game carries on as if the dice had said no. Steps
are still counted, a Repel still wears off, and fishing and scripted or static
encounters (which do not ask) still work. The function is found by a signature
scan (`src/cheats/firered/encounters.rs`).

The hook is covered by a unit test that runs the function's real bytes in the
emulator (`the_encounter_cheat_turns_every_yes_into_no`), the signature matches
once in each of the three builds, and it was tried in the game on v1.1.

### Boosted experience (Pokémon FireRed)

A Pokémon that was received in a trade earns 1.5 times the experience (the
game shows "a boosted Exp. Points"). The game stores no "traded" flag: it
compares the Pokémon's original-trainer ID with the player's own ID (and, if
those match, the trainer's name with the player's). With the row on, the
emulator treats every Pokémon of the player's as traded by giving it an ID that
is not the player's, at the moment the game asks: `IsTradedMon` fetches the
Pokémon's OT ID and the emulator inverts it before the comparison. Nothing is
changed in the Pokémon, so the Pokémon already in the party are covered too,
and the check is only made when experience is awarded, so obedience is not
affected (the player's Pokémon do not start disobeying). The place is found by
a signature scan (`src/cheats/firered/experience.rs`); the bonus itself is the
game's, so it stacks with the Lucky Egg and the trainer-battle bonus as usual.

Checked on v1.1 from the save state in a battle against a wild level-5 Pokémon
with a level-95 Pokémon of the player's: the award was 41 points without the
row and 61 with it (41 × 150 / 100, rounded down). A Pokémon that is level 100
gains nothing either way. The other builds have the same bytes (the signature
matches once in each) but were not run in the game.

### Infinite money (Pokémon FireRed)

Every reader of the balance goes through one small function, `GetMoney`, which
decrypts the stored balance with the save's key. With the row on, the emulator
sets its return value to 999,999 at the end of the function. The game then
sees a balance of ¥999,999 everywhere: the money box shows it, the affordability
check says yes, and the shop's quantity prompt works out the most you can buy
from it, so it is capped by the game's own limit of 99 rather than your wallet
(at ×1, pressing Down wraps to ×99). Payments are written back like any
balance, so the real balance becomes 999,999 minus what was spent; it is hidden
while the cheat is on, and is what is left when it is switched off. The
function is found from `IsEnoughMoney`, whose bytes are unique and which
begins by calling it (`src/cheats/firered/money.rs`).

A shop that is already open keeps the number it drew until the game redraws the
money box (after a purchase, or on leaving and re-entering), so switch the row
on before entering a shop.

Checked on v1.1 from the save state at a Poké Mart counter with ¥32: the
quantity prompt starts at ×01 ¥200, Down goes to ×99 ¥19800, the purchase
completes ("Here you are! Thank you!") and the money box then shows ¥999999.
The other builds have the same bytes (the signatures match once in each) but
were not run in the game.

### Perfect stats (Pokémon FireRed)

`CreateBoxMon` keeps its `fixed_iv` argument in a local and tests it: above 31
each IV is drawn at random, otherwise all six IVs are that value. With the row
on, the emulator stores 31 there at the same place the shiny cheat uses, so the
game's own code produces six perfect IVs (nature, gender and the rest are
untouched). The dropdown chooses whose Pokémon:

- **Wild Pokémon**: those built while `CreateWildMon` runs (grass, surfing,
  fishing, rock smash; the same set as the wild-species override). Scripted
  encounters such as a static Snorlax are built elsewhere and are not covered.
- **Opponents' Pokémon**: those built with OT-ID type 2, which the game uses
  for trainers' parties only.
- **Both**.

Checked in the game's memory from the save states in the grass: the first wild
Pokémon has IVs `[31, 31, 31, 31, 31, 31]` with the row on and random ones
without it, on v1.0, v1.1 and the Japanese build (with the target set to
opponents only, the wild Pokémon is unchanged). The
opponents' path was run against a real trainer battle on v1.1 (a step up from
the grass state starts a trainer battle): the trainer's two Pokémon have IVs of
0 without the cheat and `[31, 31, 31, 31, 31, 31]` with the target set to
opponents or both, and are unchanged with it set to wild only.

### All Pokémon are shiny (Pokémon FireRed)

In generation III a Pokémon is shiny when `OT ID high ^ OT ID low ^ PID high ^
PID low` is below 8; shininess is not stored, it follows from the personality
value (PID) and the original trainer's ID. `CreateBoxMon`, which builds every
Pokémon, draws the PID and picks the OT ID; with the row switched on the
emulator stops just after both are known and rewrites the PID so the sum is
below 8. The OT ID is left alone (the player's Pokémon stay the player's), and
the new PID keeps the low byte (gender, ability slot) and `PID % 25` (nature),
so only the colours change. Unown is skipped: its letter is made of the PID's
low bits. It applies to every Pokémon the game creates from then on (wild,
trainers', gifts, eggs), not to ones already in the party or boxes. The place
is found by a signature scan like the others (`src/cheats/firered/shiny.rs`).

From the save states in the grass, the first wild Pokémon is shiny with the
row on, with the same OT ID, gender and nature as without it, on v1.0, v1.1 and
the Japanese build. These checks were run by hand from save states and are not
automated tests, since they depend on a save state being in a particular place.

### Wild species override (Pokémon FireRed)

```text
wild_species = Dratini
```

A built-in cheat that makes every wild encounter (grass, surfing, fishing, rock
smash) the species you name. It is the wild-Pokémon row of the cheat menu; the
line above, in the `.cheats` file, only **preselects the dropdown** (a name, a
dex number from 1 to 251, or `0x93`) when no species was saved, and you tick the
row's checkbox to use it. Encounter levels, personalities and everything else
about the encounter are unchanged. Static encounters such as legendaries and
Snorlax are not wild encounters in this sense and are left alone.

How it works: while the row is on, whenever execution reaches the entry of the
game's `CreateWildMon` function, the species argument register is replaced. It
does not depend on the stack or RAM layout. The function's address differs
between builds, so it is found when the ROM is opened by a **signature scan**:
the function's first 16 bytes (a compiler prologue with no branch or pool
operands, so identical in every build) are searched for in the ROM, and the
override is used only if there is exactly one match in a `BPR?` (FireRed)
cartridge. Otherwise it is ignored, with a message in the terminal. The full
explanation is in `src/cheats/firered/wild_species.rs`.

| Build | Game code | Version | Function at | Checked |
|---|---|---|---|---|
| FireRed v1.0 (US) | `BPRE` | 0 | `0x080829FC` | live encounter |
| FireRed v1.1 (US) | `BPRE` | 1 | `0x08082A10` | live encounter |
| FireRed (Japan) | `BPRJ` | 0 | `0x080825E8` | live encounter |

In a headless test from a real save standing in grass, every encounter on v1.0,
v1.1 and the Japanese build was Dratini where the control runs gave Rattata or
Pidgey, at the same levels and with the same personality values.

**The cheats file belongs to one ROM file.** It is read from `<ROM file
name>.cheats` beside the ROM, so each revision or language needs its own copy
(`Pokemon Fire Red (J).cheats` for `Pokemon Fire Red (J).gba`). A ROM with no
such file simply has no cheats, and the terminal prints no `cheats` line for it.

**About the popular "wild Pokémon modifier" CodeBreaker codes** (`83007D4E xxxx`
plus a master code): in this emulator they write to a stack slot of an earlier
caller of the encounter code, the species never passes through that address, and
they do not change encounters, whether applied at the master-code hook, at the
frame boundary or before every instruction. They are accepted and applied as
written (so codes that do work are unaffected), but use `wild_species` for this
effect.

## Save states

Press `0` to save the whole console to a state file beside the ROM
(`<ROM name>.state`) and `1` to play it back. There is one slot; saving again
replaces it. A floppy-disk icon in the top-right corner confirms a save, and the
same disk with a circular arrow confirms a load. If there is no state yet, `1`
does nothing (a note goes to the terminal).

**Different outcomes each time.** Emulation is deterministic, so a restored
state would replay identically: the same inputs on the same frame give the same
result, including the game's random numbers. Gen 3 Pokémon keeps its generator
in RAM and advances it every frame (in FireRed v1.0 it is the word at
`0x03005000`, which changed on every one of 1,799 consecutive frames), so a
failed catch would fail the same way on every reload.

To avoid that, loading silently runs a random number of extra frames, 0 to 30 by
default (half a second at most), before you get control back. The game's
generator is then somewhere else for the same inputs. For example, one restored
FireRed state read `0x8715E898` after 20 identical frames on every load without
the extra frames, and five different values with them. The offsets come from a
shuffled bag, so none repeats until all 31 have been used, and saving a new state
starts a fresh round. This works for any game, since it only shifts time.

Things to know:

- **The game is slightly ahead of where you saved.** Up to half a second of game
  time passes instantly on load. That is invisible in an idle menu (such as a
  battle menu) but means a state saved in the middle of an animation resumes a
  little further along.
- **Buttons held during the extra frames are ignored.** They are run with no
  input; your next input arrives after them.
- **It changes where the generator is, not how the game uses it.** Outcomes still
  follow the game's own odds.
- **Turn it off for exact replays.** Set `state_jitter_frames` in the settings
  file to `0` (the range is 0 to 600), for example for tool-assisted work.

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
| Tab | Open or close the cheat menu |
| W / S, A / D, Space, C (or the arrow keys, Enter, Backspace) | While the cheat menu is open, drive it instead of the game (see below) |
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
  cheats/      Cheats, by mechanism:
    codebreaker.rs   CodeBreaker-format codes from a `.cheats` file (RAM writes)
    firered/         Built-in FireRed cheats found by signature scan:
      wild_species.rs  wild-encounter override (function arguments at its entry)
      catch_rate.rs    100% catch rate (a value inside the ball-throw routine)
      encounters.rs    no random encounters (the answer of the per-step dice roll)
      experience.rs    boosted experience (the OT ID IsTradedMon compares)
      money.rs         infinite money (the balance GetMoney returns)
      perfect_stats.rs perfect IVs (the fixed-IV local of the same routine)
      tm.rs            infinitely usable TMs (the count of the bag removal; always on)
      shiny.rs         all Pokémon shiny (the PID, inside the routine that builds a Pokémon)
    options.rs       The cheat menu's options
    species.rs       Names of the 251 original species
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

rustc 1.92 on aarch64-apple-darwin has miscompiled this project's release build
twice. The first time `lto = "fat"` combined with `panic = "abort"` emitted an
invalid instruction and the app died with SIGILL shortly after launch, so
`panic = "abort"` is deliberately not set (see `Cargo.toml`). The second time the
arm64 backend, with the 960-pixel window width inlined as a constant, computed a
pixel index from the colour register instead of the row, and the release build
segfaulted at startup while the debug build was fine; the drawing primitives are
now `#[inline(never)]` to avoid it. After changing the release profile, the
drawing code, or the toolchain, build in release and run
`scripts/check_codegen.sh`: it scans the binary for undecodable instructions and
then launches it for a few seconds (needs a display and a ROM in `rom/`) to make
sure it does not crash.

## Known limitations

- Mid-scanline register changes are not modelled; effects are per line.
- PSG channels are not band-limited, and there is no audio-driven speed sync.
- One save-state slot per game. Only constant 8/16-bit RAM writes are
  supported as cheats (no conditional, slide or ROM-patch codes), and no link
  cable or rumble.
- Verified mostly against Pokémon FireRed v1.0; other games may hit
  unimplemented behaviour.
