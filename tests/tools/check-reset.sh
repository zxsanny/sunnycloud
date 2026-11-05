#!/usr/bin/env bash
# AC-6 against a stack left up by `tests/run.sh --profile core --keep`: write rows, reset, check they are gone.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STATE_FILE="$ROOT/e2e-results/.stack-state"
[[ -f "$STATE_FILE" ]] || { echo "no stack is up; run: tests/run.sh --profile core --keep" >&2; exit 2; }
# shellcheck source=/dev/null
source "$STATE_FILE"
export E2E_SECRETS_DIR
db_sql() { docker compose -p sunnycloud-e2e -f "$ROOT/tests/compose.test.yml" --profile core exec -T db psql -v ON_ERROR_STOP=1 -U postgres -d sunnycloud -At -c "$1"; }
fail() { echo "FAIL $*" >&2; exit 1; }

echo "== writing rows after the golden snapshot"
db_sql "create table reset_probe (id int primary key, note text); insert into reset_probe select g, 'row ' || g from generate_series(1, 1000) g;"
rows="$(db_sql 'select count(*) from reset_probe')"
echo "rows before reset: $rows"
[[ "$rows" == 1000 ]] || fail "expected 1000 rows before reset, got $rows"

echo "== tests/run.sh reset"
started=$SECONDS
"$ROOT/tests/run.sh" reset
elapsed=$((SECONDS - started))

echo "== checking the snapshot state"
exists="$(db_sql "select to_regclass('public.reset_probe') is not null")"
echo "reset_probe exists after reset: $exists"
[[ "$exists" == f ]] || fail "reset_probe survived the reset"
roles="$(db_sql "select count(*) from pg_roles where rolname in ('sunnycloud','sunnycloud_migrator')")"
[[ "$roles" == 2 ]] || fail "the snapshot lost the application roles (found $roles)"
(( elapsed < 60 )) || fail "reset took ${elapsed} s (budget 60 s)"
echo "PASS reset restored the golden snapshot in ${elapsed} s"
