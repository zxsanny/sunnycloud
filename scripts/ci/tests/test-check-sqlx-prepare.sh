#!/usr/bin/env bash
# AC-3 for check-sqlx-prepare.sh: a seeded compile-time checked query without regenerated offline
# data must fail the check; after regenerating the data the same check must pass.
# Works in a scratch workspace under a temporary directory, never in the repository.
# DATABASE_URL must be an admin URL of a PostgreSQL server; the test creates and drops its own database.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${DATABASE_URL:?DATABASE_URL must point at a PostgreSQL server with CREATE DATABASE rights}"
tag="[test-check-sqlx-prepare]"
work="$(mktemp -d)"
base_url="${DATABASE_URL%/*}"
db_name="az3740_prepare_$$"
db_url="$base_url/$db_name"

cleanup() {
  echo "$tag dropping database $db_name"
  DATABASE_URL="$db_url" sqlx database drop -y || echo "$tag database drop failed (may not exist)" >&2
  rm -rf "$work"
}
trap cleanup EXIT

echo "$tag creating database $db_name and scratch workspace $work/ws"
DATABASE_URL="$db_url" sqlx database create
mkdir -p "$work/ws/src" "$work/ws/migrations"
cat >"$work/ws/Cargo.toml" <<'TOML'
[workspace]
resolver = "3"
members = ["."]

[package]
name = "scratch-queries"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
sqlx = { version = "0.9", default-features = false, features = ["postgres", "macros", "runtime-tokio", "tls-rustls"] }
TOML
printf '// no queries yet\n' >"$work/ws/src/lib.rs"
printf 'CREATE TABLE scratch (id int NOT NULL);\n' >"$work/ws/migrations/20260101000001_create_scratch.up.sql"
printf 'DROP TABLE scratch;\n' >"$work/ws/migrations/20260101000001_create_scratch.down.sql"
echo "$tag applying scratch migration"
DATABASE_URL="$db_url" sqlx migrate run --source "$work/ws/migrations"

echo "$tag AC-1 on scratch workspace without any query: expect exit 0"
DATABASE_URL="$db_url" WORKSPACE_DIR="$work/ws" "$ci_dir/check-sqlx-prepare.sh"

echo "$tag seeding a compile-time checked query without regenerating offline data"
cat >"$work/ws/src/lib.rs" <<'RS'
pub async fn ids(pool: &sqlx::PgPool) -> Result<Vec<i32>, sqlx::Error> {
    let rows = sqlx::query!("SELECT id FROM scratch").fetch_all(pool).await?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}
RS
set +e
out="$(DATABASE_URL="$db_url" WORKSPACE_DIR="$work/ws" "$ci_dir/check-sqlx-prepare.sh" 2>&1)"
rc=$?
set -e
echo "$out"
echo "$tag stale-data run exit code: $rc"
grep -q 'prepare check failed: .sqlx is missing one or more queries' <<<"$out" || { echo "$tag FAILED: failure was not reported as missing offline query data" >&2; exit 1; }
[ "$rc" -ne 0 ] || { echo "$tag FAILED: stale offline data exited 0" >&2; exit 1; }

echo "$tag positive control: regenerate offline data, then the check must pass"
(cd "$work/ws" && DATABASE_URL="$db_url" cargo sqlx prepare --workspace)
ls "$work/ws/.sqlx"
DATABASE_URL="$db_url" WORKSPACE_DIR="$work/ws" "$ci_dir/check-sqlx-prepare.sh"

echo "$tag OK"
