#!/usr/bin/env bash
# Runs the tests of the CI scripts that need neither a database nor Docker nor cargo-llvm-cov (the supply-chain job runs this).
# gitleaks must be on PATH for test-secret-scan.sh (scripts/ci/install-tools.sh gitleaks).
set -euo pipefail

tests_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for test in test-no-quiet-flags test-ci-no-quiet test-check-pinning test-audit test-check-coverage test-watchdog test-secret-scan; do
  echo "[run-script-tests] === $test"
  "$tests_dir/$test.sh"
done
echo "[run-script-tests] OK"
