#!/usr/bin/env bash
# Builds the deck firmware, flashes it to the ESP32 over the USB cable, then
# starts sending this Mac's agent sessions to it.
#
# Usage: scripts/flash.sh [PORT]
#
#   PORT  The ESP32's serial port, such as /dev/cu.usbserial-0001. Without
#         it, espflash asks which port to use, and agent-deck uses the only
#         USB serial port it finds.
#
# Needs the esp Rust toolchain (espup) and espflash; see the setup chapter
# of esp32-libs' rust-esp32-book. Stop with Ctrl-C; run
# `cargo run -p deck --features host --bin agent-deck` later to send
# sessions again without flashing.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
port="${1:-}"

# espup writes the esp toolchain's environment (Xtensa linker) here.
if [[ -f "$HOME/export-esp.sh" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/export-esp.sh"
fi

echo "==> Building the firmware"
(cd "$root/firmware" && cargo build --release)

echo "==> Flashing the ESP32"
espflash flash --chip esp32 ${port:+--port "$port"} \
  "$root/firmware/target/xtensa-esp32-none-elf/release/agent-deck-firmware"

echo "==> Sending sessions to the deck"
cd "$root"
exec cargo run --quiet -p deck --features host --bin agent-deck -- ${port:+"$port"}
