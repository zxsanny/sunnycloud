#!/usr/bin/env bash
# Cross-implementation vector gate of sync-core (AZ-3969, contract 13_sync_core/wasm_exports.md):
# the shared vector file must give 0 differences natively, in the WASM build under Node, Chromium, Firefox
# and WebKit, and (EMU tier) in the Kotlin UniFFI build. A difference fails the gate and names the case.
#
#   scripts/ci/check-wasm-vectors.sh [--vectors FILE] [--legs native,node,browsers,uniffi] [--require-browsers]
#
# Browsers run through Playwright 1.65.0 (PLAYWRIGHT_DIR=<node_modules/playwright>, else the npx cache);
# a browser that cannot start reports NOT-RUN(<reason>), which fails the gate only with --require-browsers.
# The UniFFI leg has no runner or emulator yet and always reports NOT-RUN(EMU).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
crate="$repo_root/src/sync-core-wasm"
vectors="$crate/vectors/sync_core_vectors.txt"
legs="native,node,browsers,uniffi"
require_browsers=0
pkg="${WASM_VECTORS_PKG_DIR:-$repo_root/src/target/wasm-vectors/pkg}"
tag="[check-wasm-vectors]"

while [ $# -gt 0 ]; do
  case "$1" in
    --vectors) vectors="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
    --legs) legs="$2"; shift 2 ;;
    --require-browsers) require_browsers=1; shift ;;
    *) echo "$tag unknown argument: $1" >&2; exit 2 ;;
  esac
done
has_leg() { [[ ",$legs," == *",$1,"* ]]; }
[ -f "$vectors" ] || { echo "$tag vector file not found: $vectors" >&2; exit 2; }

results="$(mktemp)"
trap 'rm -f "$results"' EXIT
leg() { echo "LEG $1 $2" | tee -a "$results"; }
# Runs a leg runner that prints its own LEG lines; a runner that exits non-zero without a FAIL line (a
# crash before it could compare) is recorded as FAIL so the gate never passes on silence.
run_legs() {
  local name="$1"; shift
  local rc=0 out
  out="$(mktemp)"
  "$@" >"$out" 2>&1 || rc=$?
  cat "$out"
  grep '^LEG ' "$out" >>"$results" || true
  if [ "$rc" -ne 0 ] && ! grep -q '^LEG [a-z]* FAIL' "$out"; then
    leg "$name" "FAIL(runner exit $rc)"
  fi
  rm -f "$out"
}

echo "$tag vectors: $vectors ($(wc -c <"$vectors") bytes); legs: $legs"
export CARGO_INCREMENTAL=0
cd "$repo_root/src"

echo "$tag step 1/6: dependency split of sync-core-wasm for wasm32 (no tokio, reqwest, mio, sync-core)"
forbidden="$(cargo tree --locked -p sync-core-wasm --target wasm32-unknown-unknown --edges normal,build --prefix none \
  | awk '$1 == "tokio" || $1 == "reqwest" || $1 == "mio" || $1 == "sync-core" { print $1 }' | sort -u)"
if [ -n "$forbidden" ]; then
  echo "$tag FAILED: forbidden crates in the wasm tree: $forbidden" >&2
  exit 1
fi

if has_leg node || has_leg browsers; then
  echo "$tag step 2/6: wasm-pack build --target web -> $pkg"
  wasm-pack build "$crate" --target web --release --out-dir "$pkg" --out-name sync_core_wasm -- --locked
  for export in "function hash_block" "function rollup" "function new_commit_id" "function client_key_for_mobile" \
    "function block_size" "class UploadPlanner"; do
    grep -q "export $export" "$pkg/sync_core_wasm.d.ts" || { echo "$tag FAILED: package lacks export $export" >&2; exit 1; }
  done
  echo "$tag package exports present ($(wc -c <"$pkg/sync_core_wasm_bg.wasm") bytes wasm)"
else
  echo "$tag step 2/6: skipped (no WASM leg selected)"
fi

echo "$tag step 3/6: native leg"
if has_leg native; then
  if SYNC_CORE_VECTORS="$vectors" cargo test --locked -p sync-core-wasm --test vectors; then
    leg native PASS
  else
    leg native FAIL
  fi
fi

echo "$tag step 4/6: Node leg ($(node --version))"
if has_leg node; then
  run_legs node node "$crate/js/node-runner.mjs" "$pkg" "$vectors"
fi

echo "$tag step 5/6: browser legs"
if has_leg browsers; then
  playwright="${PLAYWRIGHT_DIR:-}"
  if [ -z "$playwright" ]; then
    for candidate in "$(npm config get cache)"/_npx/*/node_modules/playwright; do
      if [ -f "$candidate/package.json" ] && grep -q '"version": "1.65.0"' "$candidate/package.json"; then
        playwright="$candidate"
        break
      fi
    done
  fi
  echo "$tag playwright: ${playwright:-not found}"
  run_legs browsers node "$crate/js/browser-runner.mjs" "$pkg" "$vectors" "$playwright"
fi

echo "$tag step 6/6: UniFFI leg"
if has_leg uniffi; then
  leg uniffi "NOT-RUN(EMU)"
fi

echo "$tag summary:"
grep '^LEG ' "$results" | sed "s/^/$tag   /"
if grep -q '^LEG [a-z]* FAIL' "$results"; then
  echo "$tag FAILED: differences above (DIFF lines name the case)" >&2
  exit 1
fi
if [ "$require_browsers" -eq 1 ] && grep -qE '^LEG (chromium|firefox|webkit) NOT-RUN' "$results"; then
  echo "$tag FAILED: a browser leg did not run and --require-browsers is set" >&2
  exit 1
fi
echo "$tag OK: 0 differences in the legs that ran"
