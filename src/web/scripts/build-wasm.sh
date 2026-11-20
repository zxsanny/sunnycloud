#!/usr/bin/env bash
# Builds sync-core-wasm (BLAKE3 block hashing, roll-up, commit IDs, the upload planner) for the browser into
# src/web/wasm/pkg (gitignored: no binaries in git). The transfer Worker imports the glue and the .wasm from there,
# and Vite gives both content-hashed names. Needs the pinned Rust toolchain with the wasm32-unknown-unknown target
# and wasm-pack, whose wasm-bindgen CLI must equal the wasm-bindgen pin of src/Cargo.lock (wasm-pack takes it from there).
# usage: src/web/scripts/build-wasm.sh        (CARGO_TARGET_DIR is honoured)
set -euo pipefail

web_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crate="$web_dir/../sync-core-wasm"
pkg="$web_dir/wasm/pkg"
tag="[build-wasm]"

command -v wasm-pack >/dev/null || { echo "$tag FAILED: wasm-pack is not installed" >&2; exit 1; }

pinned="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[^0-9.]/, "", $0); print; exit }' "$web_dir/../Cargo.lock")"
echo "$tag wasm-bindgen pin of src/Cargo.lock: $pinned"

rm -rf "$pkg"
wasm-pack build "$crate" --target web --release --out-dir "$pkg" --out-name sync_core_wasm -- --locked

for export in "function hash_block" "function rollup" "function new_commit_id" "function block_size" "class UploadPlanner"; do
  grep -q "export $export" "$pkg/sync_core_wasm.d.ts" || { echo "$tag FAILED: package lacks export $export" >&2; exit 1; }
done
echo "$tag OK: $pkg ($(wc -c <"$pkg/sync_core_wasm_bg.wasm") bytes of wasm)"
