#!/bin/sh
# Scans a release binary for instructions the disassembler cannot decode.
#
# A healthy build has none. A hit means the compiler emitted an invalid
# encoding (the app would die with SIGILL when it reaches that code), as
# happened with fat LTO + codegen-units=1 + panic=abort on rustc 1.92.
#
# usage: scripts/check_codegen.sh [path/to/binary]
#        (default: target/release/gba_emulator)
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
