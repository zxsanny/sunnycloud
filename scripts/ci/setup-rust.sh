#!/usr/bin/env bash
# Installs the toolchain pinned in src/rust-toolchain.toml (channel, components, targets) with rustup
# and verifies that it is the active one. No third-party action decides the version.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tag="[setup-rust]"
cd "$repo_root/src"

echo "$tag step 1/3: read the pinned channel"
channel="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
[ -n "$channel" ] || { echo "$tag FAILED: no channel in src/rust-toolchain.toml" >&2; exit 1; }
echo "$tag channel $channel"

echo "$tag step 2/3: rustup toolchain install (reads src/rust-toolchain.toml)"
rustup toolchain install

echo "$tag step 3/3: verify the active toolchain"
active="$(rustc --version)"
echo "$tag $active"
case "$active" in
  "rustc $channel "*) ;;
  *) echo "$tag FAILED: active toolchain is not $channel" >&2; exit 1 ;;
esac
rustup component list --installed
rustup target list --installed
echo "$tag OK: $channel"
