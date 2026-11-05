#!/usr/bin/env bash
# AC-1/AC-2 for check-migrations.sh against a real PostgreSQL given by DATABASE_URL (admin role).
# Seeds migration files only in temporary directories; creates and drops its own databases.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${DATABASE_URL:?DATABASE_URL must point at a PostgreSQL 18 server with CREATE DATABASE rights}"
tag="[test-check-migrations]"
work="$(mktemp -d)"
base_url="${DATABASE_URL%/*}"
db_name=""

drop_db() {
  if [ -n "$db_name" ]; then
    echo "$tag dropping database $db_name"
    DATABASE_URL="$base_url/$db_name" sqlx database drop -y
    db_name=""
  fi
}
cleanup() {
  drop_db
  rm -rf "$work"
}
trap cleanup EXIT

new_db() {
  drop_db
  db_name="az3740_$1_$$"
  echo "$tag creating empty database $db_name"
  DATABASE_URL="$base_url/$db_name" sqlx database create
}

run_check() {
  MIGRATIONS_DIR="$1" DATABASE_URL="$base_url/$db_name" "$ci_dir/check-migrations.sh"
}

seed_two_pairs() {
  mkdir -p "$1"
  printf 'CREATE TABLE seed_a (id int PRIMARY KEY);\n' >"$1/20260101000001_create_seed_a.up.sql"
  printf 'DROP TABLE seed_a;\n' >"$1/20260101000001_create_seed_a.down.sql"
  printf 'CREATE TABLE seed_b (id int PRIMARY KEY);\n' >"$1/20260101000002_create_seed_b.up.sql"
  printf 'DROP TABLE seed_b;\n' >"$1/20260101000002_create_seed_b.down.sql"
}

echo "$tag AC-1: empty migrations directory"
mkdir "$work/empty"
new_db empty
out="$(run_check "$work/empty" 2>&1)"; echo "$out"
grep -q '0 migrations applied, reverted and re-applied' <<<"$out"

echo "$tag AC-2: two reversible pairs"
seed_two_pairs "$work/two"
new_db two
out="$(run_check "$work/two" 2>&1)"; echo "$out"
grep -q '2 migrations applied, reverted and re-applied' <<<"$out"
[ "$(grep -c 'Applied 2026010100000./revert' <<<"$out")" -eq 2 ]
[ "$(grep -c 'Applied 2026010100000./migrate' <<<"$out")" -eq 4 ]

echo "$tag AC-2: broken down file must fail and name its version"
seed_two_pairs "$work/broken"
printf 'DROP TABLE table_that_does_not_exist;\n' >"$work/broken/20260101000002_create_seed_b.down.sql"
new_db broken
set +e
out="$(run_check "$work/broken" 2>&1)"; rc=$?
set -e
echo "$out"
[ "$rc" -ne 0 ] || { echo "$tag FAILED: broken down file exited 0" >&2; exit 1; }
grep -q 'down migration of version 20260101000002 failed' <<<"$out"

echo "$tag AC-2: a simple (non-reversible) file must fail"
seed_two_pairs "$work/simple"
printf 'SELECT 1;\n' >"$work/simple/20260101000003_plain.sql"
new_db simple
set +e
out="$(run_check "$work/simple" 2>&1)"; rc=$?
set -e
echo "$out"
[ "$rc" -ne 0 ]

echo "$tag OK"
