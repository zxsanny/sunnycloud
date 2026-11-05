#!/usr/bin/env bash
# AC-5: the workflows and the CI scripts contain no `cargo test -q`, no `--quiet` and no `pytest -q`, and the
# rust-test path (check-coverage.sh) runs the tests without a quiet flag so each test is listed by name.
# Searches .github/ and scripts/ci/*.sh, *.py (not this tests directory, whose fixtures spell the patterns out).
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-ci-no-quiet]"

targets=()
while IFS= read -r file; do targets+=("$file"); done < <(find "$repo_root/.github" "$ci_dir" -maxdepth 3 -type f \( -name '*.yml' -o -name '*.sh' -o -name '*.py' \) -not -path "$ci_dir/tests/*" | sort)
[ "${#targets[@]}" -gt 0 ] || { echo "$tag FAILED: nothing to search" >&2; exit 1; }
echo "$tag searching ${#targets[@]} files"

hits="$(grep -nE -e '--quiet' -e 'cargo[^|;&#]*[[:space:]]test[^|;&#]*[[:space:]]-q([[:space:]]|$)' -e 'pytest[^|;&#]*[[:space:]]-q([[:space:]]|$)' "${targets[@]}" || true)"
if [ -n "$hits" ]; then
  echo "$tag FAILED: quiet test flags found:" >&2
  echo "$hits" >&2
  exit 1
fi

grep -F 'cargo llvm-cov test --workspace --locked --no-report' "$ci_dir/check-coverage.sh" >/dev/null \
  || { echo "$tag FAILED: check-coverage.sh does not run the verbose workspace test command" >&2; exit 1; }
echo "$tag OK: 0 hits for cargo test -q, --quiet, pytest -q"
