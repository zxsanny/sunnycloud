#!/usr/bin/env bash
# v1.0 and v1.1 link no vault code (E-27/c18, AZ-4002 AC-4; ADR-016, module-layout exception 6): lists the crates
# `mobile_backup` links in the Android build of every ABI (feature `uniffi`, normal and build edges, the release
# configuration of mobile-build-native.sh) and fails when `vault-core` is among them. It also fails when the list
# lacks `sync-core`, so an empty or wrong listing cannot pass.
# usage: mobile-check-no-vault.sh
# SRC_DIR overrides the workspace directory (the seeded-failure test adds the dependency in a copy).
set -euo pipefail

tag="[mobile-check-no-vault]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"
cd "$cargo_dir"
found=0
for entry in "${ABI_TABLE[@]}"; do
  IFS=: read -r abi target _ <<<"$entry"
  echo "$tag cargo tree --target $target ($abi)"
  tree="$(cargo tree --locked -p mobile_backup --features uniffi --target "$target" --edges normal,build --prefix none)"
  echo "$tag $abi links $(echo "$tree" | awk '{print $1}' | sort -u | wc -l) crates"
  echo "$tree" | awk '$1=="sync-core"{ok=1} END{exit !ok}' || fail "$abi: sync-core is not in the linked crates"
  if echo "$tree" | awk '$1=="vault-core"{hit=1} END{exit !hit}'; then
    echo "$tag $abi links vault-core:" >&2
    echo "$tree" | grep -n 'vault-core' >&2
    found=1
  fi
done
[ "$found" -eq 0 ] || fail "vault-core is linked into the Android build (ADR-016: not before v1.2)"
echo "$tag OK: vault-core is not linked for any ABI"
