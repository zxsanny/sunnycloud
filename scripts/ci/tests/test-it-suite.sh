#!/usr/bin/env bash
# AZ-3835 AC-1: it_suite.py over a fixture spec, a fixture case list and a fake cargo (no build): a complete list
# passes and reports every case; a case with no line fails naming it; a prefix that selects nothing, a failing test
# and a case over its max execution time each fail naming the case.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-it-suite]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

cat >"$work/tests.md" <<'MD'
# Test Specification - 99 fixture

### IT-99-1: First
**Max execution time**: 5 s

### IT-99-2: Second
**Max execution time**: 1 s

## Performance Tests

### PT-99-1: Not an IT case
**Max execution time**: 1 s
MD

# The fake cargo: `--list` prints the tests of the target; a run passes every --exact name, fails a name containing
# `broken`, and takes 1.5 s when a name contains `slow`.
cat >"$work/cargo" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
names=()
listing=0
seen_separator=0
for arg in "$@"; do
  if [ "$seen_separator" -eq 1 ]; then
    if [ "$arg" = --list ]; then listing=1; elif [ "$arg" != --exact ]; then names+=("$arg"); fi
  elif [ "$arg" = -- ]; then
    seen_separator=1
  fi
done
if [ "$listing" -eq 1 ]; then
  printf '%s: test\n' alpha::one alpha::two beta::slow_one beta::broken_one gamma::three
  echo "5 tests, 0 benchmarks"
  exit 0
fi
failed=0
for name in "${names[@]}"; do
  case "$name" in
    *slow*) sleep 1.5 ;;
  esac
  case "$name" in
    *broken*) echo "test $name ... FAILED"; failed=$((failed + 1)) ;;
    *) echo "test $name ... ok" ;;
  esac
done
echo "test result: ok. $(( ${#names[@]} - failed )) passed; $failed failed; 0 ignored"
[ "$failed" -eq 0 ]
FAKE
chmod +x "$work/cargo"

# run NAME EXPECT CASES_CONTENT PATTERN...
run() {
  local name="$1" want="$2" cases="$3" rc pattern
  shift 3
  printf '%s\n' "$cases" >"$work/$name.cases"
  set +e
  CARGO="$work/cargo" python3 -I "$ci_dir/it_suite.py" "$work/tests.md" "$work/$name.cases" fixture "$work" >"$work/$name.out" 2>&1
  rc=$?
  set -e
  cat "$work/$name.out"
  if [ "$want" = pass ] && [ "$rc" -ne 0 ]; then fail "$name: expected pass, exit $rc"; fi
  if [ "$want" = fail ] && [ "$rc" -ne 1 ]; then fail "$name: expected exit 1, exit $rc"; fi
  for pattern in "$@"; do
    grep -F -- "$pattern" "$work/$name.out" >/dev/null || fail "$name: output lacks '$pattern'"
  done
  echo "$tag $name: $want as expected (exit $rc)"
}

run complete pass $'IT-99-1 it alpha::\nIT-99-2 it gamma::three' \
  "IT-99-1   PASS" "IT-99-2   PASS" "OK: 2 cases passed"
run missing-case fail 'IT-99-1 it alpha::' \
  "FAILED: IT-99-2 has no test" "IT-99-2   MISSING" "IT-99-1   PASS"
run empty-prefix fail $'IT-99-1 it alpha::\nIT-99-2 it delta::' \
  "FAILED: IT-99-2: prefix delta:: selects no test of target it"
run failing-test fail $'IT-99-1 it alpha:: beta::broken\nIT-99-2 it gamma::' \
  "FAILED: IT-99-1: target it failed" "IT-99-1   FAIL"
run over-budget fail $'IT-99-1 it alpha::\nIT-99-2 it beta::slow' \
  "over its max execution time of 1s" "IT-99-2   FAIL"
run unknown-case fail $'IT-99-1 it alpha::\nIT-99-2 it gamma::\nIT-99-7 it gamma::' \
  "FAILED: IT-99-7 is not a case of"
echo "$tag OK"
