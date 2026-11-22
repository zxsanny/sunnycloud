#!/usr/bin/env bash
# rust-lint job body: rustfmt, clippy with -D warnings (default features, then the server with `vault` and with `test-clock`, then `decoder-runner` with `test-freeze`, then `mobile_backup` with `uniffi`), then the
# offline sqlx data check.
# Needs DATABASE_URL (an empty PostgreSQL 18 database), sqlx-cli 0.9 (scripts/ci/install-tools.sh sqlx-cli) and the pinned toolchain.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tag="[rust-lint]"
: "${DATABASE_URL:?DATABASE_URL must point at an empty PostgreSQL 18 database}"

cd "$repo_root/src"
echo "$tag step 1/8: cargo fmt --check"
cargo fmt --all --check

echo "$tag step 2/8: cargo clippy --workspace --all-targets --locked -- -D warnings"
cargo clippy --workspace --all-targets --locked -- -D warnings

# The vault service (v1.2) is linked only with this feature, so the default-feature pass above never lints it.
echo "$tag step 3/8: cargo clippy -p sunnycloud-server --all-targets --locked --features vault -- -D warnings"
cargo clippy -p sunnycloud-server --all-targets --locked --features vault -- -D warnings
echo "$tag step 4/8: cargo clippy -p sunnycloud-server --all-targets --locked --features test-clock -- -D warnings"
cargo clippy -p sunnycloud-server --all-targets --locked --features test-clock -- -D warnings

# The test-stubs image builds the runner and the stub binary (decoder-test-stub) with this feature only (AZ-3865).
echo "$tag step 5/8: cargo clippy -p decoder-runner --all-targets --locked --features test-freeze -- -D warnings"
cargo clippy -p decoder-runner --all-targets --locked --features test-freeze -- -D warnings

echo "$tag step 6/8: cargo clippy -p mobile_backup --all-targets --locked --features uniffi -- -D warnings"
cargo clippy -p mobile_backup --all-targets --locked --features uniffi -- -D warnings

echo "$tag step 7/8: create the application roles, apply migrations to the PG 18 service (input of the offline data check)"
"$repo_root/scripts/ci/ensure-db-roles.sh"
sqlx migrate run --source migrations

echo "$tag step 8/8: offline query data is current"
"$repo_root/scripts/ci/check-sqlx-prepare.sh"

echo "$tag OK"
