#!/bin/bash
# Puts the settings-asus helper in bin/: the released build when there is one,
# otherwise built from source (needs Rust).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p bin

asset="settings-asus-$(uname -m)"
url="https://github.com/design-nexus/settings-extensions/releases/latest/download/$asset"
if curl -fsSL --max-time 60 -o bin/.settings-asus.tmp "$url" 2>/dev/null; then
  chmod +x bin/.settings-asus.tmp
  mv bin/.settings-asus.tmp bin/settings-asus
  exit 0
fi
rm -f bin/.settings-asus.tmp

if ! command -v cargo >/dev/null; then
  echo "Building the ASUS extension needs Rust: sudo pacman -S rust" >&2
  exit 1
fi
cargo build --release --locked --quiet
install -m 755 target/release/settings-asus bin/settings-asus
