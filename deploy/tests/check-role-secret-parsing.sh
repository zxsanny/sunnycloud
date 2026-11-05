#!/usr/bin/env bash
# The role script takes the password from a URL secret: user check, percent-decoding, rejection of bad files.
# Runs the script's own function in a subshell; needs no database.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
SCRIPT="$REPO_ROOT/deploy/postgres/initdb.d/10-roles.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

func="$(sed -n '/^password_from_url_file()/,/^}/p' "$SCRIPT")"
run() { bash -c "set -euo pipefail; $func; password_from_url_file \"\$1\" \"\$2\"" _ "$@"; }

printf 'postgres://sunnycloud:p%%40ss%%2Fw%%25rd@db:5432/sunnycloud\n' > "$tmp/ok"
got="$(run "$tmp/ok" sunnycloud)"
if [[ "$got" == 'p@ss/w%rd' ]]; then pass "percent-decoded password"; else fail "decoded '$got'"; fi

printf 'postgres://other:pw@db:5432/sunnycloud' > "$tmp/wronguser"
printf 'postgres://sunnycloud@db:5432/sunnycloud' > "$tmp/nopw"
printf 'mysql://sunnycloud:pw@db/x' > "$tmp/scheme"
for case_name in wronguser nopw scheme; do
  if run "$tmp/$case_name" sunnycloud >/dev/null 2>"$tmp/err"; then fail "$case_name accepted"; else pass "$case_name rejected: $(cat "$tmp/err")"; fi
done
if run "$tmp/missing" sunnycloud >/dev/null 2>"$tmp/err"; then fail "missing file accepted"; else pass "missing file rejected: $(cat "$tmp/err")"; fi
finish
