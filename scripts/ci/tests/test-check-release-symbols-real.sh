#!/usr/bin/env bash
# AC-4 on the real release binaries `sunnycloud` and `sunnycloud-worker` (cargo build --release --locked), the
# same artifacts the api and worker images ship. Run separately (two release builds of the server and worker).
# 1. Clean build: the check must pass.
# 2. Build with `--features test-clock`: if the binaries link sc_clock, the check must fail; while they do not
#    link it yet (skeleton binaries) there is no symbol to find, the check passes, and that is reported, not hidden.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-check-release-symbols-real]"
bins=("$repo_root/src/target/release/sunnycloud" "$repo_root/src/target/release/sunnycloud-worker")

build() {
  (cd "$repo_root/src" && cargo build --release --locked -p sunnycloud-server -p sunnycloud-worker "$@")
}

links_sc_clock() {
  local status=0
  grep -aqF -e sc_clock "$1" || status=$?
  case "$status" in
    0) return 0 ;;
    1) return 1 ;;
    *) echo "$tag FAILED: grep exited $status" >&2; exit 2 ;;
  esac
}

echo "$tag step 1/4: release build, no features"
build
echo "$tag step 2/4: the check passes on the clean build"
"$ci_dir/check-release-symbols.sh" "${bins[@]}"

echo "$tag step 3/4: release build with test-clock"
build --features test-clock
linked=0
for bin in "${bins[@]}"; do
  if links_sc_clock "$bin"; then linked=1; fi
done

echo "$tag step 4/4: the check on the test-clock build"
rc=0
"$ci_dir/check-release-symbols.sh" "${bins[@]}" || rc=$?
if [ "$linked" -eq 1 ]; then
  [ "$rc" -eq 1 ] || { echo "$tag FAILED: the binaries link sc_clock with test-clock but the check exited $rc, expected 1" >&2; exit 1; }
  echo "$tag OK: the test-clock build is rejected"
else
  [ "$rc" -eq 0 ] || { echo "$tag FAILED: no binary links sc_clock but the check exited $rc" >&2; exit 1; }
  echo "$tag NOTE: the server and worker binaries do not link sc_clock yet, so the test-clock build has no symbol to find;"
  echo "$tag       the failing case is covered by test-check-release-symbols.sh on a scratch binary that links sc_clock"
  echo "$tag OK"
fi
