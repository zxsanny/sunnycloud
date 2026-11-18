#!/usr/bin/env bash
# run-one-test.sh over fake test commands (no build): one passed test passes; a name that selects nothing
# (0 passed, exit 0) fails; two passed tests fail; a failing command keeps its exit code.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-run-one-test]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# run NAME WANT_EXIT FAKE_OUTPUT FAKE_EXIT PATTERN...
run() {
  local name="$1" want="$2" output="$3" code="$4" rc pattern
  shift 4
  set +e
  # shellcheck disable=SC2016 # the fake command expands its own positional arguments
  "$ci_dir/run-one-test.sh" bash -c 'printf "%s\n" "$1"; exit "$2"' fake "$output" "$code" >"$work/$name.out" 2>&1
  rc=$?
  set -e
  cat "$work/$name.out"
  [ "$rc" -eq "$want" ] || fail "$name: expected exit $want, got $rc"
  for pattern in "$@"; do
    grep -F -- "$pattern" "$work/$name.out" >/dev/null || fail "$name: output lacks '$pattern'"
  done
  echo "$tag $name: exit $rc as expected"
}

run one-passed 0 $'running 1 test\ntest big ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out' 0 \
  "test big ... ok" "OK: 1 test passed"
run none-matched 1 $'running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out' 0 \
  "expected exactly 1 passed test, got 0"
run two-passed 1 $'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out' 0 \
  "expected exactly 1 passed test, got 2"
run no-result-line 1 'error: no test target named x' 0 "expected exactly 1 passed test, got 0"
run command-failed 101 $'test big ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored' 101 \
  "the command exited 101"
echo "$tag OK"
