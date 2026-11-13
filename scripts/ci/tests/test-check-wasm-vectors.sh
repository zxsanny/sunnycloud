#!/usr/bin/env bash
# AC-6 of AZ-3969 for check-wasm-vectors.sh (native and Node legs, so it needs cargo, wasm-pack and node):
#   1. the committed vector file passes with 0 differences
#   2. a copy with one bit of the six_mib file hash flipped fails, and both legs name the case
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
vectors="$ci_dir/../../src/sync-core-wasm/vectors/sync_core_vectors.txt"
tag="[test-check-wasm-vectors]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

echo "$tag case 1: committed vector file, native and Node legs"
rc=0
"$ci_dir/check-wasm-vectors.sh" --legs native,node >"$work/clean.txt" 2>&1 || rc=$?
cat "$work/clean.txt"
[ "$rc" -eq 0 ] || fail "clean vector file: expected exit 0, got $rc"
grep -q '^LEG native PASS' "$work/clean.txt" || fail "clean run: native leg did not pass"
grep -q '^LEG node PASS' "$work/clean.txt" || fail "clean run: Node leg did not pass"

echo "$tag case 2: one bit flipped in the expected file hash of six_mib"
# A hex digit XOR 1 changes exactly one bit of the decoded hash; only the six_mib file line is touched.
awk '$1 == "file" && $2 == "six_mib" {
  digits = "0123456789abcdef"
  value = index(digits, substr($4, 1, 1)) - 1
  flipped = (value % 2 == 0) ? value + 1 : value - 1
  $4 = substr(digits, flipped + 1, 1) substr($4, 2)
} { print }' "$vectors" >"$work/seeded.txt"
[ "$(diff "$vectors" "$work/seeded.txt" | grep -c '^>')" -eq 1 ] || fail "seeding must change exactly one line"
rc=0
"$ci_dir/check-wasm-vectors.sh" --vectors "$work/seeded.txt" --legs native,node >"$work/seeded-run.txt" 2>&1 || rc=$?
cat "$work/seeded-run.txt"
[ "$rc" -ne 0 ] || fail "seeded vector file: gate exited 0"
grep -q '^LEG native FAIL' "$work/seeded-run.txt" || fail "seeded run: native leg did not fail"
grep -q '^LEG node FAIL' "$work/seeded-run.txt" || fail "seeded run: Node leg did not fail"
grep -q 'DIFF native file six_mib: file hash' "$work/seeded-run.txt" || fail "native leg did not name six_mib"
grep -q 'DIFF node file six_mib: file hash' "$work/seeded-run.txt" || fail "Node leg did not name six_mib"
echo "$tag OK: clean file passes; seeded one-bit difference fails and names six_mib"
