#!/usr/bin/env bash
# sc_audit CI statement check (AZ-3784): the repository passes; a tree with only the audit crate's insert, export select
# and purge call, migrator DDL, and statements under tests/ passes; every seeded other statement against audit_log
# fails (UPDATE, DELETE, TRUNCATE, an extra grant, a purge function without a fixed search_path, the allowed insert in
# another file). Seeds only in temp dirs.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-check-audit-sql]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

run_gate() { "$ci_dir/check-audit-sql.sh" "$1" 2>&1; }

expect_pass() {
  local label="$1" root="$2" out
  out="$(run_gate "$root")" || { echo "$out"; echo "$tag FAILED: $label exited non-zero" >&2; exit 1; }
  echo "$out"
  grep -q 'OK:' <<<"$out" || { echo "$tag FAILED: $label did not print OK" >&2; exit 1; }
}

expect_fail() {
  local label="$1" root="$2" out rc=0
  out="$(run_gate "$root")" || rc=$?
  echo "$out"
  [ "$rc" -eq 1 ] || { echo "$tag FAILED: $label exited $rc, expected 1" >&2; exit 1; }
  grep -q 'FAILED: 1 statement' <<<"$out" || { echo "$tag FAILED: $label did not report exactly one statement" >&2; exit 1; }
}

# A tree with every allowed form.
make_clean() {
  local dir="$1"
  mkdir -p "$dir/src/shared/audit/src" "$dir/src/shared/audit/tests" "$dir/src/migrations" "$dir/src/other/src" "$dir/src/target/debug" "$dir/src/.git"
  cat >"$dir/src/shared/audit/src/repo.rs" <<'RS'
// UPDATE audit_log in a comment is not a statement
const INSERT: &str = "INSERT INTO audit_log \
    (at, event) VALUES ($1, $2)";
const EXPORT: &str = "SELECT id, (EXTRACT(EPOCH FROM at) * 1000000)::bigint, event \
    FROM audit_log \
    WHERE id > $1 ORDER BY id LIMIT $2";
const PURGE: &str = "SELECT audit_log_purge($1)";
RS
  printf 'fn privileges() { let _ = "UPDATE audit_log SET event = $1"; let _ = "DELETE FROM audit_log"; }\n' >"$dir/src/shared/audit/tests/privileges.rs"
  cat >"$dir/src/migrations/20261105100000_create_audit_log_table.up.sql" <<'SQL'
CREATE TABLE audit_log (id bigint GENERATED ALWAYS AS IDENTITY, at timestamptz NOT NULL, event text NOT NULL);
CREATE INDEX audit_log_at_idx ON audit_log (at);
GRANT INSERT, SELECT ON audit_log TO sunnycloud;
-- UPDATE audit_log in a comment
CREATE FUNCTION audit_log_purge(cutoff timestamptz) RETURNS bigint
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    DELETE FROM public.audit_log WHERE at < cutoff;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END
$$;
REVOKE ALL ON FUNCTION audit_log_purge(timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION audit_log_purge(timestamptz) TO sunnycloud;
SQL
  cat >"$dir/src/migrations/20261105100000_create_audit_log_table.down.sql" <<'SQL'
REVOKE EXECUTE ON FUNCTION audit_log_purge(timestamptz) FROM sunnycloud;
DROP FUNCTION audit_log_purge(timestamptz);
REVOKE INSERT, SELECT ON audit_log FROM sunnycloud;
DROP TABLE audit_log;
SQL
  printf 'pub fn unrelated() {}\n' >"$dir/src/other/src/lib.rs"
  printf 'UPDATE audit_log SET event = 1;\n' >"$dir/src/target/debug/seeded.sql"
  printf 'UPDATE audit_log SET event = 1;\n' >"$dir/src/.git/seeded.sql"
}

# seed LABEL FILE CONTENT: copies the clean tree, appends CONTENT to FILE (created when missing), expects a failure.
seed_fail() {
  local label="$1" file="$2" content="$3" dir="$work/$1"
  make_clean "$dir"
  mkdir -p "$(dirname "$dir/$file")"
  printf '%s\n' "$content" >>"$dir/$file"
  echo "$tag seeded: $label"
  expect_fail "$label" "$dir"
}

echo "$tag the repository passes"
expect_pass "repository" "$repo_root"

echo "$tag a tree with only the allowed forms passes"
make_clean "$work/clean"
expect_pass "clean tree" "$work/clean"

seed_fail "update-in-rust" "src/other/src/lib.rs" 'pub const BAD: &str = "UPDATE audit_log SET event = $1";'
seed_fail "update-upper-case-name" "src/other/src/lib.rs" 'pub const BAD: &str = "update AUDIT_LOG set event = $1";'
seed_fail "delete-in-rust" "src/other/src/lib.rs" 'pub const BAD: &str = "DELETE FROM audit_log WHERE id = $1";'
seed_fail "truncate-split-over-lines" "src/other/src/lib.rs" $'pub const BAD: &str = "TRUNCATE\n    audit_log";'
seed_fail "insert-outside-repo" "src/other/src/lib.rs" 'pub const BAD: &str = "INSERT INTO audit_log (at, event) VALUES ($1, $2)";'
seed_fail "select-outside-repo" "src/other/src/lib.rs" 'pub const BAD: &str = "SELECT id FROM audit_log";'
seed_fail "purge-call-outside-repo" "src/other/src/lib.rs" 'pub const BAD: &str = "SELECT audit_log_purge($1)";'
seed_fail "update-in-repo" "src/shared/audit/src/repo.rs" 'const BAD: &str = "UPDATE audit_log SET event = $1";'
seed_fail "delete-in-repo" "src/shared/audit/src/repo.rs" 'const BAD: &str = "DELETE FROM audit_log WHERE at < $1";'
seed_fail "delete-select-in-repo" "src/shared/audit/src/repo.rs" 'const BAD: &str = "WITH d AS (DELETE FROM audit_log RETURNING id) SELECT id FROM d";'
seed_fail "update-in-sql-outside-migrations" "src/other/seed.sql" 'UPDATE audit_log SET event = '"'"'sign_in'"'"';'
seed_fail "update-in-later-migration" "src/migrations/20261201000000_fix_things.up.sql" 'UPDATE audit_log SET event = '"'"'sign_in'"'"';'
seed_fail "delete-in-later-migration" "src/migrations/20261201000000_fix_things.up.sql" 'DELETE FROM audit_log WHERE at < now_cutoff;'
seed_fail "grant-update" "src/migrations/20261201000000_grant_more.up.sql" 'GRANT UPDATE ON audit_log TO sunnycloud;'
seed_fail "grant-all" "src/migrations/20261201000000_grant_more.up.sql" 'GRANT ALL ON audit_log TO sunnycloud;'
seed_fail "grant-with-grant-option" "src/migrations/20261201000000_grant_more.up.sql" 'GRANT SELECT ON audit_log TO sunnycloud WITH GRANT OPTION;'
seed_fail "trigger-on-table" "src/migrations/20261201000000_trigger_it.up.sql" 'CREATE TRIGGER t BEFORE INSERT ON audit_log FOR EACH ROW EXECUTE FUNCTION f();'
seed_fail "grant-purge-to-public" "src/migrations/20261201000000_grant_more.up.sql" 'GRANT EXECUTE ON FUNCTION audit_log_purge(timestamptz) TO PUBLIC;'

echo "$tag seeded: purge function without a fixed search_path"
nosp="$work/no-search-path"
make_clean "$nosp"
sed -i 's/ SET search_path = pg_catalog, pg_temp//' "$nosp/src/migrations/20261105100000_create_audit_log_table.up.sql"
expect_fail "no-search-path" "$nosp"

echo "$tag seeded: purge that deletes by more than the cutoff"
wide="$work/wide-purge"
make_clean "$wide"
sed -i 's/WHERE at < cutoff;/;/' "$wide/src/migrations/20261105100000_create_audit_log_table.up.sql"
expect_fail "wide-purge" "$wide"

echo "$tag OK"
