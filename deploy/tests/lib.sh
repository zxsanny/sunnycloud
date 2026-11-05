# Shared helpers for the deploy/tests scripts (sourced). Exit codes of the scripts: 0 pass, 1 check failed, 2 usage.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FAILS=0
pass() { printf 'PASS %s\n' "$*"; }
fail() { printf 'FAIL %s\n' "$*" >&2; FAILS=$((FAILS + 1)); }
finish() {
  if [[ "$FAILS" -gt 0 ]]; then printf '%s check(s) failed\n' "$FAILS" >&2; exit 1; fi
  printf 'all checks passed\n'
}
