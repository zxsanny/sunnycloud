#!/usr/bin/env bash
# AC-6 threshold logic of check-coverage.sh on fixture llvm-cov summaries (no cargo run):
# 80% overall passes, 70% fails and prints the measured value, critical paths below 90% / matching nothing fail;
# an entry with its own minimum (`PATH MIN`, AZ-3835 per-crate floor) is held to that minimum;
# the lcov files of test shards are merged (hits summed per line) before the gate applies.
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
run own-minimum-ok pass "$work/eighty.json" $'other 70   # a per-crate floor\n' "critical path other: 70.00% (70/100), minimum 70%: OK"
run own-minimum-below fail "$work/eighty.json" $'vault\nother 75\n' "critical path other: 70.00% (70/100), minimum 75%: BELOW"
run own-minimum-not-a-number fail "$work/eighty.json" $'other many\n' "is not a number"

# Two shards: a.rs lines 1-3 (shard 1 hits line 1, shard 2 hits line 2), b.rs line 1 only known to shard 2.
mkdir -p "$work/shards"
printf 'SF:%s/vault/a.rs\nDA:1,3\nDA:2,0\nDA:3,0\nLF:3\nLH:1\nend_of_record\n' "$ws" >"$work/shards/1.lcov"
printf 'SF:%s/vault/a.rs\nDA:1,0\nDA:2,5\nDA:3,0\nLF:3\nLH:1\nend_of_record\nSF:%s/other/b.rs\nDA:7,1\nLF:1\nLH:1\nend_of_record\n' "$ws" "$ws" >"$work/shards/2.lcov"
printf 'vault 60\nother\n' >"$work/shards.paths"
set +e
COVERAGE_LCOV_DIR="$work/shards" WORKSPACE_DIR="$ws" CRITICAL_PATHS_FILE="$work/shards.paths" "$ci_dir/check-coverage.sh" >"$work/shards.out" 2>&1
rc=$?
set -e
cat "$work/shards.out"
[ "$rc" -eq 0 ] || fail "merged shards: expected pass, exit $rc"
grep -F "overall line coverage 75.00% (3/4)" "$work/shards.out" >/dev/null || fail "merged shards: a line hit in either shard is not covered once"
grep -F "critical path vault: 66.67% (2/3), minimum 60%: OK" "$work/shards.out" >/dev/null || fail "merged shards: the critical path is not measured on the merge"
echo "$tag merged shards: 3 of 4 lines covered as expected"
mkdir -p "$work/empty"
if COVERAGE_LCOV_DIR="$work/empty" WORKSPACE_DIR="$ws" "$ci_dir/check-coverage.sh" >"$work/empty.out" 2>&1; then fail "no shard files: expected failure"; fi
echo "$tag no shard files: fails as expected"
echo "$tag OK"
