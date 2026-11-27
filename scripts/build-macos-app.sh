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
# Ask Cargo for the target directory, so CARGO_TARGET_DIR and
# build.target-dir are honoured.
target="$(cargo metadata --no-deps --format-version 1 |
  sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
pkgid="$(cargo pkgid -p deck)"
version="${pkgid##*[#@]}"

rm -rf "$app"
mkdir -p "$contents/MacOS" "$preview/MacOS"
xcrun swiftc macos/AgentDeckApp.swift \
  -framework AppKit \
  -o "$contents/MacOS/agent-deck-menu"
cp macos/Info.plist "$contents/Info.plist"
cp macos/PreviewInfo.plist "$preview/Info.plist"
cp "$target/release/examples/preview" "$preview/MacOS/agent-deck-preview"
for plist in "$contents/Info.plist" "$preview/Info.plist"; do
  plutil -replace CFBundleShortVersionString -string "$version" "$plist"
done

echo "Built $app"
echo "Open it with: open '$app'"
