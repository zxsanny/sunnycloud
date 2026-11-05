#!/usr/bin/env bash
# supply-chain: the browser-side crates must build for wasm32-unknown-unknown (target pinned in src/rust-toolchain.toml).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root/src"
echo "[build-wasm] cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids"
cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids
echo "[build-wasm] OK"
