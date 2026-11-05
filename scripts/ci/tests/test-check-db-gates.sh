#!/usr/bin/env bash
# AZ-3782 AC-10 for check-db-gates.sh: the repository passes; a clean fixture tree passes; each seeded violation
# (scope setting name outside sc_db, run_system call off the allowlist, SQL text outside a repository module, stale
# or non-repository allowlist entries) fails the gate it belongs to and only that gate. Seeds live in temp dirs.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-check-db-gates]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

run_gate() { RUN_SYSTEM_ALLOWLIST="${2:-$work/allowlist.txt}" "$ci_dir/check-db-gates.sh" "$1" 2>&1; }

expect_pass() {
  local label="$1" root="$2" out
  out="$(run_gate "$root" "${3:-}")" || { echo "$out"; echo "$tag FAILED: $label exited non-zero" >&2; exit 1; }
  grep -q 'OK: 3 of 3 gates passed' <<<"$out" || { echo "$out"; echo "$tag FAILED: $label did not pass all gates" >&2; exit 1; }
}

# expect_fail LABEL ROOT GATE COUNT: exit 1, only GATE failed, with COUNT violations.
expect_fail() {
  local label="$1" root="$2" gate="$3" count="$4" out rc=0
  out="$(run_gate "$root")" || rc=$?
  echo "$out"
  [ "$rc" -eq 1 ] || { echo "$tag FAILED: $label exited $rc, expected 1" >&2; exit 1; }
  grep -q "FAILED: $gate: $count violation(s)" <<<"$out" || { echo "$tag FAILED: $label did not report $count violation(s) of $gate" >&2; exit 1; }
  grep -q 'FAILED: 1 of 3 gates failed' <<<"$out" || { echo "$tag FAILED: $label failed a gate besides $gate" >&2; exit 1; }
}

clean="$work/clean"
mkdir -p "$clean/src/shared/db/src" "$clean/src/components/01_identity/src/repo" "$clean/src/components/02_tree/src" \
  "$clean/src/components/02_tree/tests" "$clean/src/target/debug"
cat >"$clean/src/shared/db/src/scope.rs" <<'RS'
const SET_ACCOUNT: &str = "SELECT set_config('sunnycloud.account_id', $1, true)";
const SET_SYSTEM: &str = "SELECT set_config('sunnycloud.scope', 'system', true)";
pub async fn apply(c: &mut C) { sqlx::query(SET_ACCOUNT).execute(c).await; db.run_system("x", f); }
RS
cat >"$clean/src/components/01_identity/src/repo/sessions.rs" <<'RS'
pub async fn by_hash(db: &Db) { db.run_system("session_by_hash", async |tx| {
    sqlx::query_as::<_, Row>("SELECT id FROM sessions WHERE token_hash = $1").fetch_one(tx.conn()).await }).await }
RS
cat >"$clean/src/components/02_tree/src/lib.rs" <<'RS'
// run_system is documented in sc_db; a comment may name SELECT or run_system.
pub fn label() -> &'static str { "Select a file from the list" }
pub async fn list(db: &Db, repo: &Repo) { db.run(scope, async |tx| repo.list(tx).await).await }
RS
sed -i 's/ and sunnycloud.scope? no: comments count for names././' "$clean/src/components/02_tree/src/lib.rs"
printf 'async fn t() { sqlx::query("DELETE FROM t").execute(c).await; }\n' >"$clean/src/components/02_tree/tests/it.rs"
printf 'pub fn x() { sqlx::query("SELECT 1"); db.run_system("t", f); }\n// sunnycloud.scope\n' >"$clean/src/target/debug/seeded.rs"
printf '# reviewed\nsrc/components/01_identity/src/repo/sessions.rs   # session by token hash\n' >"$work/allowlist.txt"

echo "$tag the repository passes all three gates"
"$ci_dir/check-db-gates.sh" "$repo_root" >/dev/null || { echo "$tag FAILED: the repository violates a gate" >&2; "$ci_dir/check-db-gates.sh" "$repo_root" >&2 || true; exit 1; }

echo "$tag a tree with only allowed forms passes"
expect_pass "clean tree" "$clean"

echo "$tag scope setting name in another crate fails"
t="$work/scope-account"; cp -r "$clean" "$t"
printf 'pub const NAME: &str = "sunnycloud.account_id";\n' >>"$t/src/components/02_tree/src/lib.rs"
expect_fail "account setting" "$t" scope-names 1
t="$work/scope-system"; cp -r "$clean" "$t"
printf '/// sets sunnycloud.scope itself\npub fn f() {}\n' >>"$t/src/components/02_tree/src/lib.rs"
expect_fail "system setting in a doc comment" "$t" scope-names 1

echo "$tag run_system call site off the allowlist fails"
t="$work/run-system-unlisted"; cp -r "$clean" "$t"
printf 'pub async fn f(db: &Db) { db.run_system("janitor", g).await }\n' >"$t/src/components/02_tree/src/janitor.rs"
expect_fail "unlisted call site" "$t" run-system 1
t="$work/run-system-unlisted-repo"; cp -r "$clean" "$t"
mkdir -p "$t/src/components/02_tree/src/repo"
printf 'pub async fn f(db: &Db) { Db::run_system(db, "janitor", g).await }\n' >"$t/src/components/02_tree/src/repo/system.rs"
expect_fail "unlisted repository module" "$t" run-system 1

echo "$tag stale and non-repository allowlist entries fail"
printf 'src/components/01_identity/src/repo/sessions.rs\nsrc/components/01_identity/src/repo/gone.rs\n' >"$work/stale.txt"
out="$(RUN_SYSTEM_ALLOWLIST="$work/stale.txt" "$ci_dir/check-db-gates.sh" "$clean" 2>&1)" && { echo "$tag FAILED: stale entry passed" >&2; exit 1; }
grep -q 'gone.rs: allowlist entry has no run_system call (stale)' <<<"$out" || { echo "$out"; echo "$tag FAILED: stale entry not reported" >&2; exit 1; }
t="$work/non-repo-entry"; cp -r "$clean" "$t"
printf 'pub async fn f(db: &Db) { db.run_system("janitor", g).await }\n' >"$t/src/components/02_tree/src/janitor.rs"
printf 'src/components/01_identity/src/repo/sessions.rs\nsrc/components/02_tree/src/janitor.rs\n' >"$work/non-repo.txt"
out="$(RUN_SYSTEM_ALLOWLIST="$work/non-repo.txt" "$ci_dir/check-db-gates.sh" "$t" 2>&1)" && { echo "$tag FAILED: non-repository entry passed" >&2; exit 1; }
grep -q 'janitor.rs: allowlist entry is not a repository module' <<<"$out" || { echo "$out"; echo "$tag FAILED: non-repository entry not reported" >&2; exit 1; }

echo "$tag SQL text outside a repository module fails"
t="$work/sql-entry"; cp -r "$clean" "$t"
printf 'pub async fn f(c: &mut C) { sqlx::query(Q).execute(c).await; }\n' >"$t/src/components/02_tree/src/queries.rs"
expect_fail "query entry point" "$t" sql-text 1
t="$work/sql-literal"; cp -r "$clean" "$t"
printf 'pub const Q: &str = "UPDATE nodes SET name = $1 WHERE id = $2";\n' >"$t/src/components/02_tree/src/queries.rs"
expect_fail "SQL literal" "$t" sql-text 1
t="$work/sql-multiline"; cp -r "$clean" "$t"
printf 'pub const Q: &str = r#"\n    SELECT id\n    FROM nodes"#;\n' >"$t/src/components/02_tree/src/queries.rs"
expect_fail "multi-line SQL literal" "$t" sql-text 1
t="$work/sql-macro"; cp -r "$clean" "$t"
printf 'pub async fn f(c: &mut C) { sqlx::query_as!(Row, "id").fetch_one(c).await; query_scalar!("x"); }\n' >"$t/src/components/02_tree/src/queries.rs"
expect_fail "query macros" "$t" sql-text 1
t="$work/sql-in-repo"; cp -r "$clean" "$t"
printf 'pub const Q: &str = "SELECT id FROM nodes";\n' >"$t/src/components/02_tree/src/node_repo.rs"
expect_pass "SQL in a *_repo.rs module" "$t"

echo "$tag OK"
