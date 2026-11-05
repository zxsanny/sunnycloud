#!/usr/bin/env bash
# SQL never calls now(): the application binds a Timestamp from sc_clock (data_model §1, sc_clock contract).
# Scans every *.sql file and every Rust file (query strings) under ROOT for a now() call, case-insensitive.
# Ignored: SQL `--` comment lines, Rust `//` comment lines, `fn now(` definitions, and paths such as
# `Instant::now()` or `clock.now()` (a `.` or `:` before `now` makes it a Rust call, not SQL).
# usage: check-no-sql-now.sh [ROOT]   (default: repository root)
# Exit code: 0 with no occurrence, 1 when any is found, 2 on a usage or grep error.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="${1:-$(cd "$ci_dir/../.." && pwd)}"
tag="[check-no-sql-now]"
[ -d "$root" ] || { echo "$tag FAILED: $root is not a directory" >&2; exit 2; }

echo "$tag step 1/3: scanning *.sql and *.rs under $root"
pattern='(^|[^[:alnum:]_.:])now[[:space:]]*\([[:space:]]*\)'
status=0
raw="$(grep -rnIiE --include='*.sql' --include='*.rs' \
  --exclude-dir=target --exclude-dir=.git --exclude-dir=node_modules -e "$pattern" "$root")" || status=$?
if [ "$status" -gt 1 ]; then
  echo "$tag FAILED: grep exited $status" >&2
  exit 2
fi

echo "$tag step 2/3: dropping comment lines and fn now definitions"
hits="$(awk '
  {
    path = $0
    sub(/^[^:]*:[0-9]+:/, "", path)
    body = path
    file = $0
    sub(/:[0-9]+:.*$/, "", file)
    if (file ~ /\.sql$/ && body ~ /^[[:space:]]*--/) next
    if (file ~ /\.rs$/ && body ~ /^[[:space:]]*\/\//) next
    if (body ~ /fn[[:space:]]+now[[:space:]]*\(/) next
    print
  }' <<<"$raw")"

echo "$tag step 3/3: result"
if [ -n "$hits" ]; then
  count="$(wc -l <<<"$hits")"
  echo "$hits" >&2
  echo "$tag FAILED: $count occurrence(s) of now() in SQL or query strings; pass a Timestamp from sc_clock instead" >&2
  exit 1
fi
echo "$tag OK: 0 occurrences of now() in SQL files and query strings"
