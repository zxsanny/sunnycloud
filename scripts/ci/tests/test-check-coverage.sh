#!/usr/bin/env bash
# AC-6 threshold logic of check-coverage.sh on fixture llvm-cov summaries (no cargo run):
# 80% overall passes, 70% fails and prints the measured value, critical paths below 90% / matching nothing fail.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-check-coverage]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
ws="$work/ws"
mkdir -p "$ws"

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# summary FILE covered1 count1 covered2 count2   (files vault/a.rs and other/b.rs)
summary() {
  cat >"$1" <<JSON
{"data":[{"files":[
 {"filename":"$ws/vault/a.rs","summary":{"lines":{"covered":$2,"count":$3}}},
 {"filename":"$ws/other/b.rs","summary":{"lines":{"covered":$4,"count":$5}}},
 {"filename":"/elsewhere/c.rs","summary":{"lines":{"covered":0,"count":1000}}}
]}]}
JSON
}

# run NAME EXPECT SUMMARY CRITICAL_CONTENT [PATTERN]
run() {
  local name="$1" want="$2" json="$3" paths="$4" pattern="${5:-}" rc
  printf '%s' "$paths" >"$work/$name.paths"
  set +e
  COVERAGE_SUMMARY_JSON="$json" WORKSPACE_DIR="$ws" CRITICAL_PATHS_FILE="$work/$name.paths" "$ci_dir/check-coverage.sh" >"$work/$name.out" 2>&1
  rc=$?
  set -e
  cat "$work/$name.out"
  if [ "$want" = pass ] && [ "$rc" -ne 0 ]; then fail "$name: expected pass, exit $rc"; fi
  if [ "$want" = fail ] && [ "$rc" -eq 0 ]; then fail "$name: expected fail, exit 0"; fi
  if [ -n "$pattern" ]; then grep -F -- "$pattern" "$work/$name.out" >/dev/null || fail "$name: output lacks '$pattern'"; fi
  echo "$tag $name: $want as expected (exit $rc)"
}

summary "$work/eighty.json" 90 100 70 100
summary "$work/seventy.json" 70 100 70 100
summary "$work/boundary.json" 75 100 75 100
summary "$work/vault-low.json" 80 100 100 100

run overall-80 pass "$work/eighty.json" $'# no critical paths\n' "overall line coverage 80.00%"
run overall-70 fail "$work/seventy.json" '' "overall line coverage 70.00%"
run overall-75-boundary pass "$work/boundary.json" '' "75.00%"
run critical-ok pass "$work/eighty.json" $'vault/a.rs   # 90%\n' "critical path vault/a.rs: 90.00%"
run critical-dir-prefix pass "$work/eighty.json" $'vault\n' "critical path vault: 90.00%"
run critical-below fail "$work/vault-low.json" $'vault\n' "critical path vault: 80.00%"
run critical-missing fail "$work/eighty.json" $'nothing/here.rs\n' "matches no file"
echo "$tag OK"
