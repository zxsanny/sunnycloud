#!/usr/bin/env bash
# Runs the workspace tests under cargo llvm-cov nextest (the rust-test job step: output lists every test by name) and
# fails when line coverage is below COVERAGE_MIN_OVERALL (75) or a critical path is below its minimum (its own, else
# COVERAGE_MIN_CRITICAL, 90).
# Prints the measured values. Needs cargo-llvm-cov, cargo-nextest and the llvm-tools-preview component (src/rust-toolchain.toml).
# CRITICAL_PATHS_FILE      default scripts/ci/critical-paths.txt
# COVERAGE_SUMMARY_JSON    apply the gate to an existing `cargo llvm-cov report --json --summary-only` file; no test run (tests of the gate)
# WORKSPACE_DIR            default src
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
workspace_dir="${WORKSPACE_DIR:-$repo_root/src}"
critical_paths="${CRITICAL_PATHS_FILE:-$ci_dir/critical-paths.txt}"
min_overall="${COVERAGE_MIN_OVERALL:-75}"
min_critical="${COVERAGE_MIN_CRITICAL:-90}"
tag="[check-coverage]"

if [ -n "${COVERAGE_SUMMARY_JSON:-}" ]; then
  echo "$tag step 1/3: using $COVERAGE_SUMMARY_JSON, no test run"
  summary="$COVERAGE_SUMMARY_JSON"
else
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
  summary="$work/summary.json"
  cd "$workspace_dir"
  # Most tests wait (database, timers, child processes), so twice the CPUs keeps the cores busy: 4 cores ran the
  # workspace in 751 s at 4 test threads and 470 s at 8.
  export NEXTEST_TEST_THREADS="${NEXTEST_TEST_THREADS:-$(( $(nproc) * 2 ))}"
  echo "$tag step 1/3: cargo llvm-cov nextest --workspace --locked --no-fail-fast (instrumented test run)"
  cargo llvm-cov nextest --workspace --locked --no-fail-fast --no-report
  echo "$tag step 2/3: cargo llvm-cov report --json --summary-only"
  cargo llvm-cov report --json --summary-only --output-path "$summary"
fi

echo "$tag step 3/3: apply thresholds overall>=${min_overall}% critical>=${min_critical}%"
if ! python3 -I "$ci_dir/coverage_gate.py" "$summary" "$workspace_dir" "$critical_paths" "$min_overall" "$min_critical"; then
  echo "$tag FAILED: coverage below the gate" >&2
  exit 1
fi
echo "$tag OK"
