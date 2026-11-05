#!/usr/bin/env bash
# Applies every migration to an empty PostgreSQL, reverts them all, applies them again.
# Needs DATABASE_URL (empty database) and sqlx-cli 0.9.
# MIGRATIONS_DIR overrides the directory (default: src/migrations).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
migrations_dir="${MIGRATIONS_DIR:-$repo_root/src/migrations}"
tag="[check-migrations]"

fail() {
  echo "$tag FAILED: $*" >&2
  exit 1
}

# Prints "<installed> <pending>" counts from sqlx migrate info.
migration_counts() {
  local info
  info="$(sqlx migrate info --source "$migrations_dir")"
  local installed pending
  installed="$(printf '%s\n' "$info" | grep -c '/installed' || true)"
  pending="$(printf '%s\n' "$info" | grep -c '/pending' || true)"
  echo "$installed $pending"
}

# Prints the highest installed version, empty when none.
latest_installed_version() {
  sqlx migrate info --source "$migrations_dir" | grep '/installed' | tail -n 1 | cut -d/ -f1 || true
}

echo "$tag step 1/9: verify DATABASE_URL is set"
[ -n "${DATABASE_URL:-}" ] || { echo "$tag FAILED: DATABASE_URL is not set" >&2; exit 2; }

echo "$tag step 2/9: verify sqlx is installed (need 0.9.x)"
sqlx --version || { echo "$tag FAILED: sqlx not found; see scripts/ci/README.md" >&2; exit 2; }
case "$(sqlx --version)" in
  *" 0.9."*) ;;
  *) echo "$tag FAILED: sqlx-cli must be 0.9.x to match sqlx 0.9" >&2; exit 2 ;;
esac

echo "$tag step 3/9: verify $migrations_dir holds only reversible pairs"
[ -d "$migrations_dir" ] || fail "migrations directory $migrations_dir does not exist"
shopt -s nullglob
total=0
for file in "$migrations_dir"/*.sql; do
  name="$(basename "$file")"
  [[ "$name" =~ ^[0-9]{14}_[a-z0-9_]+\.(up|down)\.sql$ ]] \
    || fail "$name is not a reversible YYYYMMDDHHMMSS_<verb>_<object>.up.sql or .down.sql file"
  if [[ "$name" == *.up.sql ]]; then
    [ -f "${file%.up.sql}.down.sql" ] || fail "$name has no matching .down.sql"
    total=$((total + 1))
  else
    [ -f "${file%.down.sql}.up.sql" ] || fail "$name has no matching .up.sql"
  fi
done
echo "$tag found $total reversible pairs"

echo "$tag step 4/9: ensure the application roles exist (migrations grant to sunnycloud)"
"$(dirname "${BASH_SOURCE[0]}")/ensure-db-roles.sh" || fail "ensure-db-roles.sh failed"

echo "$tag step 5/9: verify the database has no applied migrations"
read -r installed pending <<<"$(migration_counts)"
echo "$tag installed=$installed pending=$pending"
[ "$installed" -eq 0 ] || fail "database is not empty: $installed migrations already applied"

echo "$tag step 6/9: apply all ($total) migrations"
sqlx migrate run --source "$migrations_dir" || fail "applying migrations failed (sqlx output above names the version)"
read -r installed pending <<<"$(migration_counts)"
[ "$installed" -eq "$total" ] || fail "expected $total applied after first run, found $installed"

echo "$tag step 7/9: revert all migrations, newest first"
reverted=0
while [ "$(latest_installed_version)" != "" ]; do
  version="$(latest_installed_version)"
  echo "$tag reverting version $version"
  sqlx migrate revert --source "$migrations_dir" || fail "down migration of version $version failed"
  reverted=$((reverted + 1))
  [ "$reverted" -le "$total" ] || fail "reverted more than $total migrations; last version $version"
done
[ "$reverted" -eq "$total" ] || fail "expected to revert $total, reverted $reverted"

echo "$tag step 8/9: apply all ($total) migrations again"
sqlx migrate run --source "$migrations_dir" || fail "re-applying migrations failed (sqlx output above names the version)"
read -r installed pending <<<"$(migration_counts)"
[ "$installed" -eq "$total" ] || fail "expected $total applied after second run, found $installed"

echo "$tag step 9/9: report"
echo "$tag OK: $total migrations applied, reverted and re-applied"
