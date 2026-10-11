#!/bin/sh
# Checks a release binary for compiler-introduced faults.
#
# 1. Scans it for instructions the disassembler cannot decode. A healthy build
#    has none; a hit means an invalid encoding (the app would die with SIGILL
#    when it reaches that code), as happened with fat LTO + codegen-units=1 +
#    panic=abort on rustc 1.92.
# 2. Launches it for a few seconds and checks it is still running. Wrong-but-
#    valid code is invisible to the scan: rustc 1.92 once computed a pixel
#    index from the colour register and the app segfaulted at startup, only in
#    release builds. This step needs a display and a ROM; it is skipped when
#    there is no `rom/*.gba` or `rom/*/*.gba` (or GBA_SMOKE_ROM) or SKIP_LAUNCH is set.
#    The binary is started with GBA_EMULATOR_SMOKE_UI=1, which opens the dock,
#    the cheat menu and its species list from the first frame, so the drawing
#    code (where the previous miscompile hit) runs without a mouse.
#
# usage: scripts/check_codegen.sh [path/to/binary]
#        (default: target/release/gba_emulator)
#   env: TRIPLE         disassembly triple (default arm64-apple-darwin)
#        GBA_SMOKE_ROM  ROM to launch
#        SMOKE_SECONDS  how long it must survive (default 8)
#        SKIP_LAUNCH    set to skip the launch check
set -eu
binary="${1:-target/release/gba_emulator}"
triple="${TRIPLE:-arm64-apple-darwin}"

[ -f "$binary" ] || { echo "no such binary: $binary (cargo build --release first)" >&2; exit 2; }
command -v objdump >/dev/null || { echo "objdump not found" >&2; exit 2; }

hits=$(objdump -d --triple="$triple" "$binary" 2>/dev/null | grep -c '<unknown>' || true)
if [ "$hits" -eq 0 ]; then
    echo "ok: $binary has no undecodable instructions"
else
    echo "FAIL: $binary has $hits undecodable instruction(s):" >&2
    objdump -d --triple="$triple" "$binary" 2>/dev/null | grep -B2 '<unknown>' >&2
    exit 1
fi

[ -z "${SKIP_LAUNCH:-}" ] || { echo "skipped: launch check (SKIP_LAUNCH set)"; exit 0; }
rom="${GBA_SMOKE_ROM:-}"
if [ -z "$rom" ]; then
    for candidate in rom/*.gba rom/*/*.gba; do
        [ -f "$candidate" ] && { rom="$candidate"; break; }
    done
fi
[ -n "$rom" ] && [ -f "$rom" ] || { echo "skipped: launch check (no ROM in rom/)"; exit 0; }

seconds="${SMOKE_SECONDS:-8}"
GBA_EMULATOR_SMOKE_UI=1 "$binary" "$rom" >/dev/null 2>&1 &
pid=$!
sleep "$seconds"
if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    echo "ok: $binary ran for ${seconds}s without crashing"
else
    status=0
    wait "$pid" || status=$?
    echo "FAIL: $binary exited after less than ${seconds}s (status $status; 139 is a segfault, 132 an illegal instruction)" >&2
    exit 1
fi
