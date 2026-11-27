#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo "The Agent Deck app can only be built on macOS." >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/dist/Agent Deck.app"
contents="$app/Contents"
preview="$contents/Resources/Agent Deck Preview.app/Contents"

cd "$root"
cargo build -p deck --example preview --features sim --release

rm -rf "$app"
mkdir -p "$contents/MacOS" "$preview/MacOS"
xcrun swiftc macos/AgentDeckApp.swift \
  -framework AppKit \
  -o "$contents/MacOS/agent-deck-menu"
cp macos/Info.plist "$contents/Info.plist"
cp macos/PreviewInfo.plist "$preview/Info.plist"
cp target/release/examples/preview "$preview/MacOS/agent-deck-preview"

echo "Built $app"
echo "Open it with: open '$app'"
