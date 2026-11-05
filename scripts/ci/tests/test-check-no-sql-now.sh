#!/usr/bin/env bash
# AC-5 for check-no-sql-now.sh: the repository has 0 occurrences; a seeded now() in an SQL file or a Rust
# query string fails the gate; Rust time calls, comments and ignored directories do not. Seeds only in temp dirs.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-check-no-sql-now]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

run_gate() { "$ci_dir/check-no-sql-now.sh" "$1" 2>&1; }

expect_fail() {
  local label="$1" root="$2" out rc=0
  out="$(run_gate "$root")" || rc=$?
  echo "$out"
  [ "$rc" -eq 1 ] || { echo "$tag FAILED: $label exited $rc, expected 1" >&2; exit 1; }
  grep -q 'FAILED: 1 occurrence' <<<"$out" || { echo "$tag FAILED: $label did not report exactly one occurrence" >&2; exit 1; }
}

expect_pass() {
  local label="$1" root="$2" out
  out="$(run_gate "$root")" || { echo "$tag FAILED: $label exited non-zero" >&2; exit 1; }
  echo "$out"
  grep -q 'OK: 0 occurrences' <<<"$out" || { echo "$tag FAILED: $label did not report 0 occurrences" >&2; exit 1; }
}

echo "$tag AC-5: the repository has 0 occurrences"
expect_pass "repository" "$repo_root"

echo "$tag AC-5: a tree with only allowed forms passes"
clean="$work/clean"
mkdir -p "$clean/migrations" "$clean/crate/src" "$clean/target/debug" "$clean/.git"
printf 'CREATE TABLE t (id int, created_at timestamptz NOT NULL);\n-- never DEFAULT now() here\n' >"$clean/migrations/001_t.sql"
cat >"$clean/crate/src/lib.rs" <<'RS'
// SQL never calls now()
use std::time::{Instant, SystemTime};
pub fn probe() -> (Instant, SystemTime) { (Instant::now(), SystemTime::now()) }
pub fn tick(clock: &Clock) { let _ = clock.now(); }
pub fn now() {}
pub const QUERY: &str = "UPDATE t SET created_at = $1";
RS
printf 'INSERT INTO t VALUES (1, now());\n' >"$clean/target/debug/seeded.sql"
printf 'INSERT INTO t VALUES (1, now());\n' >"$clean/.git/seeded.sql"
expect_pass "clean tree" "$clean"

echo "$tag AC-5: seeded now() in an SQL file fails"
seeded="$work/seeded-sql"
cp -r "$clean" "$seeded"
printf 'ALTER TABLE t ADD COLUMN updated_at timestamptz DEFAULT now();\n' >"$seeded/migrations/002_seed.sql"
expect_fail "seeded sql" "$seeded"

echo "$tag AC-5: seeded NOW () in upper case fails"
upper="$work/seeded-upper"
cp -r "$clean" "$upper"
printf 'UPDATE t SET created_at = NOW ( );\n' >"$upper/migrations/002_seed.sql"
expect_fail "seeded upper case" "$upper"

echo "$tag AC-5: seeded now() in a Rust query string fails"
query="$work/seeded-rs"
cp -r "$clean" "$query"
printf 'pub const BAD: &str = "UPDATE t SET touched_at = now() WHERE id = $1";\n' >>"$query/crate/src/lib.rs"
expect_fail "seeded rust query" "$query"

echo "$tag OK"
