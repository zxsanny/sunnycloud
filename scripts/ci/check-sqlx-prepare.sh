#!/usr/bin/env bash
# Fails when the committed sqlx offline query data (src/.sqlx/) does not match the code.
# Needs DATABASE_URL (a PostgreSQL 18 with all migrations applied) and sqlx-cli 0.9.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workspace_dir="${WORKSPACE_DIR:-$repo_root/src}"

echo "[check-sqlx-prepare] step 1/4: verify DATABASE_URL is set"
if [ -z "${DATABASE_URL:-}" ]; then
  echo "[check-sqlx-prepare] FAILED: DATABASE_URL is not set" >&2
  exit 2
fi

echo "[check-sqlx-prepare] step 2/4: verify cargo sqlx is installed (need 0.9.x)"
if ! cargo sqlx --version; then
  echo "[check-sqlx-prepare] FAILED: cargo sqlx not found; see scripts/ci/README.md" >&2
  exit 2
fi
case "$(cargo sqlx --version)" in
  *" 0.9."*) ;;
  *)
    echo "[check-sqlx-prepare] FAILED: sqlx-cli must be 0.9.x to match sqlx 0.9" >&2
    exit 2
    ;;
esac

echo "[check-sqlx-prepare] step 3/4: enter workspace $workspace_dir"
cd "$workspace_dir"

echo "[check-sqlx-prepare] step 4/4: cargo sqlx prepare --check --workspace"
if ! cargo sqlx prepare --check --workspace; then
  echo "[check-sqlx-prepare] FAILED: cargo sqlx prepare --check failed (stale .sqlx data, or the workspace does not build); to refresh run 'cargo sqlx prepare --workspace' in src/ and commit .sqlx" >&2
  exit 1
fi

echo "[check-sqlx-prepare] OK: offline query data is up to date"
