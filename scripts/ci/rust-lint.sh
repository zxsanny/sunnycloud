#!/usr/bin/env bash
# rust-lint job body: rustfmt, clippy with -D warnings, then the offline sqlx data check.
# Needs DATABASE_URL (an empty PostgreSQL 18 database), sqlx-cli 0.9 (scripts/ci/install-tools.sh sqlx-cli) and the pinned toolchain.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tag="[rust-lint]"
: "${DATABASE_URL:?DATABASE_URL must point at an empty PostgreSQL 18 database}"

cd "$repo_root/src"
echo "$tag step 1/4: cargo fmt --check"
cargo fmt --all --check

echo "$tag step 2/4: cargo clippy --workspace --all-targets --locked -- -D warnings"
cargo clippy --workspace --all-targets --locked -- -D warnings

echo "$tag step 3/4: apply migrations to the PG 18 service (input of the offline data check)"
sqlx migrate run --source migrations

echo "$tag step 4/4: offline query data is current"
"$repo_root/scripts/ci/check-sqlx-prepare.sh"

echo "$tag OK"
