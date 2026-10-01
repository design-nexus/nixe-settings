#!/bin/bash
# Build settings-asus and publish it as release asus-v<version>, which install.sh downloads.
# Bump `version` in both extension.toml and Cargo.toml first.
set -euo pipefail
cd "$(dirname "$0")"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' extension.toml)
cargo_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [[ $version != "$cargo_version" ]]; then
  echo "extension.toml says $version but Cargo.toml says $cargo_version" >&2
  exit 1
fi
cargo build --release --locked
asset="settings-asus-$(uname -m)"
cp target/release/settings-asus "target/$asset"
gh release create "asus-v$version" "target/$asset" \
  --repo design-nexus/settings-extensions \
  --title "ASUS $version" \
  --notes "The settings-asus helper for $(uname -m) Linux. Settings downloads it when installing or updating the ASUS extension."
