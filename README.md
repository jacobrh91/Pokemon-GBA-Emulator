Building a Game Boy Advance (GBA) emulator is an exceptional systems project. The GBA is powered by an ARM7TDMI processor running at 16.78 MHz, a custom 2D graphics engine (PPU), and a multi-channel sound processor (APU).
To make your emulator highly performant, capable of handling Fast-Forward Mode, and extensible enough for a Cheat Engine (GameShark/Action Replay), you should break the system down into decoupled, testable modules.
Here is the step-by-step modular architecture for your Rust GBA emulator.

🧩 System Modules Architecture


1. The CPU Module (`cpu`)

The heart of the system. The ARM7TDMI executes two distinct instruction sets: 32-bit ARM mode and compressed 16-bit Thumb mode.
• Instruction Decoder: Implement a clean, zero-allocation decoding pipeline using bit-masking macros to decode binary opcodes into Rust enum variants.
• Execution State Machine: Maintain registers (r0 to r15), the Current Program Status Register (CPSR), and pipeline emulation (Fetch-Decode-Execute).
• Execution Loop: Instead of relying on actual OS clocks, write a deterministic cycle-counting loop. Every instruction executed adds a precise number of internal clock cycles to a global counter.

2. The MMU Module (`mmu`)

The GBA uses a unified 32-bit memory map connecting the CPU to internal RAM, external RAM, registers, and the game cartridge.
• Memory Bus Routing: Build a centralized bus component with a fast matching pattern to intercept read/write calls and route them to their respective locations (e.g., 0x02000000 for On-Board WRAM).
• Zero-Copy ROM Mapping: Use memmap2 to map your Pokémon FireRed ROM directly into memory space, avoiding heavy memory copying allocations.
• DMA (Direct Memory Access) Controller: Emulate the 4 automatic DMA channels that instantly transfer blocks of data (like moving sprite graphics to VRAM) without using CPU execution cycles.

3. The PPU Module (`ppu`)

Pokémon FireRed operates primarily in Tile Modes (Modes 0, 1, or 2), utilizing 4 scrolling background layers, texturing maps, and sprite objects (OAM).
• Scanline Renderer: To support high-ratio Fast-Forwarding, do not render frame-by-frame. Render scanline-by-scanline (240 pixels horizontally) matching internal timing metrics, checking background priority registers and window transparency rules.
• VRAM / OAM Parsers: Interpret layout segments out of memory arrays to locate sprite configurations, handling flips, palettes, and affine scaling metrics.

4. The Audio Unit (APU) & Input Module (gba_apu, gba_joypad)

• Sound Channels: Emulate 4 legacy Game Boy Sound Font channels (Square waves, Noise) combined with 2 modern GBA Direct Sound FIFO digital channels.
• Keypad Register: Monitor button mappings (0x04000130), updating key indicators based on standard user keyboard configurations.

5. The Infrastructure Utility Module (gba_utils)

This module handles features like cheat codes, state recording, and speed tuning.
• Cheat Code Engine: Hook directly into the `mmu` write/read pipelines. When a cheat is enabled (e.g., GameShark/Action Replay text parsed into an address and value), the engine intercepts lookups to that address and forces the cheat value instead (e.g., infinite item counts).
• Fast-Forward Scheduler: By skipping OS frame synchronization limits (e.g., ignoring vertical sync intervals up to a specific frame skip cap), you let the execution loop process as many CPU cycles as your desktop thread can manage.
• Save State Engine (serde): Serialize the entire CPU register set, internal RAM arrays, and PPU flags into a binary snapshot file to enable instant load/save capabilities.

🚀 Step-by-Step Implementation Roadmap

Phase 1: Bootstrapping  ──> Phase 2: Graphics & Input ──> Phase 3: Game Mechanics & Tuning
(CPU, Thumb, Memory Bus)    (Scanline PPU, Keypad, UI)    (DMA, Timer Irqs, Cheats Engine)

Phase 1: Bootstrapping & Core CPU

1. [x] Implement the initial Memory Bus: map internal RAM, I/O, graphics RAM, and cartridge ROM; support basic reads/writes and wait-state estimates.
2. [ ] Validate ARM7TDMI ARM/Thumb semantics: test all implemented instruction families, architectural edge cases, exceptions, and pipeline behavior against trusted references.
3. [ ] Validate CPU-visible GBA memory behavior and timing: cover region boundaries, mirrors, access widths, wait states, and timer/interrupt interaction.
4. [ ] Expand external conformance coverage: run the available Armwrestler cases and focused CPU tests; record unsupported cases and avoid claiming a full suite pass until they pass.
5. [x] Define and demonstrate the Phase 1 boot target: CPU-only cartridge execution is demonstrated by headless ROM integration tests; rendering the boot logo is a Phase 2 PPU/frontend milestone.

Current CPU progress: The CPU implements a broad but not yet fully validated ARM7TDMI subset: ARM data processing, branches, multiply/long multiply, single/halfword/block transfers, status-register transfers, swaps, banked registers, and SPSRs; Thumb arithmetic/ALU, shifts, branches, common load/store forms, stack operations, multiple transfers, and long branches. Thumb POP-to-PC preserves the loaded branch target; Thumb NEG/CMP/CMN update all arithmetic flags, including C and V. Undefined instructions, SWI, IRQ/FIQ, prefetch abort, and data abort enter their exception modes with saved status and return addresses. ARM LDM exception return and user-bank block transfers are implemented. ARM and Thumb ROM fall-through fetches now use sequential WAITCNT costs, while branch refills use non-sequential costs. Thumb and ARM base cycles follow the ARM7TDMI datasheet and 32-bit buses (BIOS, IWRAM, I/O, OAM) take one cycle per word. Performance: the bus is event driven (display, timers and audio are only updated when something observable is due, with time-dependent register reads computed from pending cycles), wait states are charged as accesses happen, and loops that provably leave the whole machine state unchanged (same registers, no bus writes, no event or timer-counter read) are skipped up to one iteration before the next event, which is cycle-exact; `cargo run --release --example idle_diff -- ROM FRAMES` runs two emulators in lockstep, with skipping on and off, and compares CPU, RAM, VRAM, save and audio state every frame. The release profile uses fat LTO and one codegen unit; do not add `panic = "abort"` to it (rustc 1.92 on aarch64-apple-darwin miscompiled the drawing code into an invalid instruction under that combination, so the app died with SIGILL shortly after launch), and run `scripts/check_codegen.sh` after changing profile settings, which scans the release binary for instructions that do not decode. The overall timing model remains provisional, not a fully cycle-accurate external bus/pipeline model. BIOS services are high-level emulated, following mGBA's HLE BIOS: the SVC vector dispatches SoftReset, RegisterRamReset, Halt/Stop, IntrWait/VBlankIntrWait (halt until the game's IRQ handler sets the BIOS IF flags), Div/DivArm, Sqrt, ArcTan/ArcTan2, CpuSet/CpuFastSet, LZ77 and run-length decompression, and GetBiosChecksum; IRQ entry emulates the BIOS trampoline (register frame push, return stub at 0x138) so game handlers can `bx lr`. Instruction cycle counts follow the ARM7TDMI datasheet (data-dependent multiply, LDM/STM, SWP, PC loads); NV-condition ARM instructions are skipped, unaligned LDRH/LDRSH follow ARM7 rotation rules, and MSR cannot flip CPSR.T. Unimplemented BIOS services (affine sets, BitUnPack, Huffman, diff filters, sound) still error. DMA channels 0-3 run immediate, VBlank and HBlank transfers (repeat, address control, IRQ on end, CPU stall cycles) as in mGBA's `dma.c`; special-timing (sound FIFO/video capture) DMA is not triggered. Display timing raises VBlank, HBlank and VCount IRQs and DISPSTAT flags. Unmapped, BIOS-region and past-ROM reads return open-bus values (the GBA has no memory aborts); BIOS reads return the last-fetched BIOS opcode. Audio (`src/apu`): PSG channels 1-4 (sweep, envelopes, length counters, wave RAM banks, 7/15-bit noise), Direct Sound A/B FIFOs clocked by timer 0/1 overflow and refilled by special-timing DMA1/DMA2, SOUNDCNT/SOUNDBIAS mixing to stereo at 32768 Hz. The frontend plays it through `cpal` with linear resampling and a bounded queue. Mixing is point-sampled at 32768 Hz regardless of the SOUNDBIAS resolution bits, PSG channels are not band-limited, and the frontend has no audio-driven speed sync. `examples/audio_probe` writes a ROM's audio to a WAV and prints FIFO underrun/overrun counters. Cartridge ROM windows mirror power-of-two ROM images; backup memory is emulated for SRAM (32 KiB) and Flash (64/128 KiB Macronix ID, byte program that only clears bits, sector and chip erase, bank switching), chosen from the ROM's library ID string (`FLASH1M_V`, `FLASH512_V`, `FLASH_V`, `SRAM_V`; unknown ROMs get SRAM) and persisted to a `.sav` file; EEPROM is detected but not emulated. The backup chip is an 8-bit device, so wider reads see the byte repeated and wider writes store one lane. Open-bus behavior and cartridge-specific fault signaling remain incomplete. ARM barrel-shifter boundaries and data-processing results/flags have focused tests. `Emulator::step` dispatches from CPSR.T and advances timer/IRQ state.

The memory bus maps EWRAM, IWRAM, I/O, palette RAM, VRAM, OAM, and the three Game Pak ROM wait-state windows. WAITCNT-dependent access costs are included in CPU cycle estimates. Four timers support prescalers, reloads, cascaded counting, overflow IRQ requests, and batched cycle advancement; IF is write-one-to-clear and IE/IME gate IRQ delivery. Display timing tracks VBlank status and VCOUNT scanlines, but does not yet model HBlank or synchronize PPU rendering to scanlines.

Phase 1 completion means reliable CPU-only execution of cartridge code with validated ARM/Thumb behavior and CPU-visible memory/timing—not display output. FireRed v1.0 now runs headlessly for 6000 frames (`cargo run --release --example boot_probe -- ROM 6000`) through its intro with a multi-colour framebuffer and no CPU/BIOS errors; it has not been verified visually against a reference. This is not a full game boot or hardware conformance. Instruction edge cases and cycles still need comparison against trusted ARM7TDMI references; exception bus/pipeline effects, cartridge-specific open bus, backup-chip protocols, and some I/O semantics remain approximate or missing. The Mode 0 PPU can render tiled backgrounds to an RGBA framebuffer, and the executable presents that framebuffer in a scaled desktop window. Audio is emulated (see below). Save states, cheats, and fast-forward controls remain unimplemented; the PPU renders all six video modes: text backgrounds (0-3), rotation/scaling backgrounds (BG2/BG3 in modes 1-2, with 28-bit reference points that advance by PB/PD each line and reload on mid-frame writes, wrap or transparent overflow, 128-1024 px sizes), and the bitmap modes 3/4/5 with page flipping and the BG2 matrix; sprites (OAM, 4/8bpp, 1D/2D mapping, flips, affine and double-size, priority against BGs, OBJ-window masking, the unusable first 512 tiles in bitmap modes, and the per-line cycle budget of 1210/954); WIN0/WIN1/OBJ windows; the color special effects unit (alpha blending, brightness increase/decrease, semi-transparent sprites, per-window effect enable); and BG/OBJ mosaic. Not modelled: mid-line register changes, the capture-time quirks of VRAM/OAM access, and the green-swap display hack; VRAM/palette/OAM are read when a line is drawn, so mid-line writes are not modelled. The sprite budget is applied per line, not per pixel clock.

### Armwrestler integration coverage

`cargo test --test armwrestler` boots both included Armwrestler ROMs, advances the GBA display timing needed by their VBlank polling loop, and checks ROM-rendered pass/fail output for ARM suites 0, 2, and 4 in the fixed ROM and suite 0 in the original ROM. It also checks Thumb ALU suite 0 (including the previously failing ROR row), load/store suite 1, and multiple-transfer suite 2 in the fixed ROM. The ROR investigation found register-form NEG/CMP/CMN did not update C/V; implementing their architectural arithmetic flags made the full Thumb ALU suite pass. The fixed ROM is the primary reference; the original is included as a comparison. This remains a subset of the interactive suite; no complete Armwrestler pass is claimed. User-bank ARM block transfers have focused unit coverage.

Phase 2: Visuals & Window Pipeline

1. [x] Complete the PPU: scanline-synchronised rendering of all video modes, sprites, windows, blending and mosaic; mid-line effects remain unmodelled.
2. [x] Hook Up Frontend UI: A minifb window presents the 240x160 framebuffer at 4x scale; press Escape or close the window to exit.
3. [x] Bind Inputs: keyboard keys map to KEYINPUT/KEYCNT (see below); `examples/screenshot` can script button presses with `PRESS="frame:button:hold,..."`.

### ROM library

```text
gba_emulator install "Pokemon Fire Red (v1.0).gba"   # copy into the library
gba_emulator list                                    # installed ROMs
gba_emulator "Pokemon Fire Red"                      # run by (prefix of) name
gba_emulator path/to/game.gba                        # or run a file directly
```

Each installed ROM gets its own directory with a `saves/` folder beside it:

```text
<library>/Pokemon Fire Red (v1.0)/Pokemon Fire Red (v1.0).gba
<library>/Pokemon Fire Red (v1.0)/saves/            (battery saves and, later, save states)
```

The library is `~/Library/Application Support/gba_emulator` on macOS, `$XDG_DATA_HOME/gba_emulator` (or `~/.local/share/gba_emulator`) on Linux and `%APPDATA%\gba_emulator` on Windows; set `GBA_EMULATOR_LIBRARY` to use another directory. Installing validates the ROM header, is a no-op for an identical ROM, refuses to replace a different ROM of the same name without `--force`, and never modifies `saves/`. The backup-memory image is loaded from `saves/<name>.sav` at startup and written back (atomically, via a `.partial` file) within about half a second of the game changing it and again at shutdown. A ROM run directly from a path keeps its save beside it as `<rom>.sav`.

Phase 2 progress: Run `cargo run -- [ROM_PATH | NAME]` to boot a ROM and display it in a 4x-scaled window. The emulator runs on its own thread, paced by the audio queue level (wall-clock fallback without an audio device), and publishes finished frames to the window thread, so UI stalls such as macOS Space swipes do not interrupt audio;  the bus latches the display registers as each visible scanline finishes (before HBlank DMA runs) and the PPU renders that line from the snapshot; the frontend presents a frame once its last visible line is drawn. The window is resizable with its 3:2 aspect ratio locked (on macOS the frame itself keeps the ratio while you drag, with a minimum of half size; elsewhere the picture is letterboxed), and full screen is available from the dock's corner-brackets button, F11 or Cmd+F (macOS; other platforms ignore it); the mouse position is read from the window system relative to the picture itself and mapped back through the scaling, so the dock works at any size and after leaving full screen. Escape leaves full screen when the window is full screen and otherwise closes the window; Keypad: WASD = D-pad, Space = A, C = B, Q = L, E = R, X = Start, V = Select (opposing directions cancel). KEYINPUT is active-low and KEYCNT's OR/AND keypad interrupt is raised on the condition's rising edge. Hold either Shift key to fast-forward at the speed chosen in the dock (2X, 4X, 8X or MAX, default MAX; the fixed speeds are paced against the wall clock, MAX is unthrottled): emulation runs (roughly 15x real time in a release build on the reference machine, because games spend most of a frame in idle loops that are skipped exactly; the measured multiple is shown next to a `>>` badge in the top-left corner), only one frame in about sixteen milliseconds is drawn and shown, audio is muted while held. Moving the mouse over the window slides up a dock; from left to right it holds a red power button (a ring with a bar) that saves and quits, a restart button (a circular arrow) that power-cycles the console while keeping the save, the volume (drag the slider, scroll the wheel over it, or click the speaker to mute), and the fast-forward speed buttons; the dock stays open while the mouse is near the bottom of the window or moving, and slides away after about two and a half seconds of stillness or when the mouse leaves. The game is upscaled 4x in software so the dock is drawn at window resolution.

Phase 3: Game-Specific Execution & Advanced Features

1. Refine Timers & Interrupts: the CPU/MMU currently implement basic timers and IRQ delivery; precise peripheral timing and DMA-triggered behavior remain future work.
2. Build the Cheat Hook Engine: Write a text parsing utility that decodes master codes and standard RAM patching strings, checking addresses on every memory clock iteration.
3. [x] Optimize for Speed Loops: hold Shift to fast-forward (see below). Decouple your frame timing scheduler so pressing a hotkey lets the runtime run unrestricted, allowing the game to run at 2x or 4x standard speeds.

Development Phase	Milestone Goal	Critical Component	Rust Dependency / Primitives
Phase 1	CPU-only cartridge execution	ARM/Thumb Interpreter & MMU	memmap2, bit-masking macros
Phase 2	Render UI & Movement	Mode 0 Tile PPU & Input Interface	minifb
Phase 3	Pokémon Fully Playable	DMA, Interrupts, Cheats, Saves	serde, bincode, parking_lot
