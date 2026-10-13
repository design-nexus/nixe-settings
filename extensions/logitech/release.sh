#!/bin/bash
# Build settings-logitech and publish it as release ext-logitech-v<version> in nixe, which install.sh downloads.
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
asset="settings-logitech-$(uname -m)"
cp target/release/settings-logitech "target/$asset"
gh release create "ext-logitech-v$version" "target/$asset" \
  --repo design-nexus/nixe \
  --title "Logitech extension $version" \
  --notes "The settings-logitech helper for $(uname -m) Linux. Settings downloads it when installing or updating the Logitech extension."
