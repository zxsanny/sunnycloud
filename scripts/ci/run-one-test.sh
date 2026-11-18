#!/usr/bin/env bash
# usage: run-one-test.sh CMD...
# Runs a `cargo test ... -- --exact NAME` command, passes its output through, and fails unless exactly one test
# passed. `--exact` with a name that matches nothing (a renamed or deleted test) exits 0 with `0 passed`; this
# wrapper turns that silent pass into a failure. A failing command keeps its own exit code; a wrong count exits 1.
set -euo pipefail

tag="[run-one-test]"
[ "$#" -gt 0 ] || { echo "$tag usage: run-one-test.sh CMD..." >&2; exit 2; }

out="$(mktemp)"
trap 'rm -f "$out"' EXIT

set +e
"$@" 2>&1 | tee "$out"
rc="${PIPESTATUS[0]}"
set -e
if [ "$rc" -ne 0 ]; then
  echo "$tag FAILED: the command exited $rc" >&2
  exit "$rc"
fi

passed="$(sed -nE 's/^test result: ok\. ([0-9]+) passed;.*/\1/p' "$out" | awk '{ total += $1 } END { print total + 0 }')"
if [ "$passed" -ne 1 ]; then
  echo "$tag FAILED: expected exactly 1 passed test, got $passed (does the --exact name still match a test?)" >&2
  exit 1
fi
echo "$tag OK: 1 test passed"
