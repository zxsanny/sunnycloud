#!/usr/bin/env bash
# Three gates that keep tenant scope and SQL text in their place (sc_db contract, ADR-014, AZ-3782 AC-10):
#   1. scope-names:  the tenant and system scope setting names appear in Rust files only under src/shared/db/.
#   2. run-system:   every file calling run_system is a repository module listed in the reviewed allowlist, and
#                    every listed file still calls it.
#   3. sql-text:     no sqlx query entry point and no SQL-looking string literal outside src/shared/db/, a tests/
#                    directory or a repository module.
# A repository module is a Rust file or directory named repo, repository or repositories, or a file ending in
# _repo.rs or _repository.rs.
# usage: check-db-gates.sh [ROOT]   (default: repository root; scans ROOT/src/**/*.rs)
# RUN_SYSTEM_ALLOWLIST   default scripts/ci/run-system-allowlist.txt
# Exit code: 0 clean, 1 any violation, 2 usage or grep error.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="${1:-$(cd "$ci_dir/../.." && pwd)}"
allowlist="${RUN_SYSTEM_ALLOWLIST:-$ci_dir/run-system-allowlist.txt}"
tag="[check-db-gates]"
[ -d "$root/src" ] || { echo "$tag FAILED: $root/src is not a directory" >&2; exit 2; }
[ -f "$allowlist" ] || { echo "$tag FAILED: allowlist $allowlist not found" >&2; exit 2; }

failures=0

is_repository_path() {
  case "$1" in
    */repo/* | */repository/* | */repositories/* | */repo.rs | */repository.rs | *_repo.rs | *_repository.rs) return 0 ;;
  esac
  return 1
}

in_sc_db() { case "$1" in src/shared/db/*) return 0 ;; esac; return 1; }

# Prints "relative/path:line:text" of every line in ROOT/src/**/*.rs matching the ERE $1. With $2 = skip-comments,
# lines that start with // (comments, doc comments) are dropped.
rust_matches() {
  local pattern="$1" mode="$2" raw status=0
  raw="$(grep -rnIE --include='*.rs' --exclude-dir=target --exclude-dir=.git --exclude-dir=node_modules \
    -e "$pattern" "$root/src")" || status=$?
  if [ "$status" -gt 1 ]; then
    echo "$tag FAILED: grep exited $status" >&2
    return 2
  fi
  [ -n "$raw" ] || return 0
  awk -v root="$root/" -v mode="$mode" '
    {
      line = $0
      if (index(line, root) == 1) line = substr(line, length(root) + 1)
      body = line
      sub(/^[^:]*:[0-9]+:/, "", body)
      if (mode == "skip-comments" && body ~ /^[[:space:]]*\/\//) next
      print line
    }' <<<"$raw"
}

file_of() { printf '%s' "${1%%:*}"; }

report() {
  local gate="$1" hint="$2" hits="$3" count
  if [ -z "$hits" ]; then
    echo "$tag OK: $gate: 0 violations"
    return 0
  fi
  count="$(wc -l <<<"$hits")"
  echo "$hits" >&2
  echo "$tag FAILED: $gate: $count violation(s); $hint" >&2
  failures=$((failures + 1))
}

echo "$tag step 1/3: scope setting names only under src/shared/db/"
raw="$(rust_matches 'sunnycloud\.(scope|account_id)' keep-comments)" || exit 2
scope_hits=""
while IFS= read -r hit; do
  [ -n "$hit" ] || continue
  in_sc_db "$(file_of "$hit")" || scope_hits+="$hit"$'\n'
done <<<"$raw"
report "scope-names" "the scope settings belong to sc_db; call Db::run or Db::run_system instead" "${scope_hits%$'\n'}"

echo "$tag step 2/3: run_system call sites against $allowlist"
raw="$(rust_matches '(^|[^[:alnum:]_])run_system([^[:alnum:]_]|$)' skip-comments)" || exit 2
call_files=""
while IFS= read -r hit; do
  [ -n "$hit" ] || continue
  file="$(file_of "$hit")"
  in_sc_db "$file" || call_files+="$file"$'\n'
done <<<"$raw"
call_files="$(sort -u <<<"$call_files" | sed '/^$/d')"
listed="$(sed -e 's/[[:space:]]*#.*$//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' "$allowlist" | sed '/^$/d' | sort -u)"
system_hits=""
while IFS= read -r file; do
  [ -n "$file" ] || continue
  grep -qxF -- "$file" <<<"$listed" || system_hits+="$file: calls run_system but is not on the allowlist"$'\n'
done <<<"$call_files"
while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  is_repository_path "$entry" || system_hits+="$entry: allowlist entry is not a repository module"$'\n'
  grep -qxF -- "$entry" <<<"$call_files" || system_hits+="$entry: allowlist entry has no run_system call (stale)"$'\n'
done <<<"$listed"
report "run-system" "add the repository module to $(basename "$allowlist") in review, or drop the entry" "${system_hits%$'\n'}"

echo "$tag step 3/3: SQL text only in sc_db, tests/ and repository modules"
# Three shapes: a sqlx query entry point, a string literal that opens with a SQL verb, a line that starts with one
# (the body of a multi-line literal). Verbs are upper case, so prose such as "Select a file" does not match.
verbs='SELECT[[:space:]]|INSERT[[:space:]]+INTO[[:space:]]|DELETE[[:space:]]+FROM[[:space:]]|UPDATE[[:space:]]+[A-Za-z_."]+[[:space:]]+SET[[:space:]]|TRUNCATE[[:space:]]|CREATE[[:space:]]+TABLE[[:space:]]|ALTER[[:space:]]+TABLE[[:space:]]|DROP[[:space:]]+TABLE[[:space:]]'
entry_point='sqlx::(\{[^}]*)?(query|query_as|query_scalar|raw_sql)([^[:alnum:]_]|$)|(^|[^[:alnum:]_])(query|query_as|query_scalar|query_file|query_file_as|query_file_scalar)!'
raw_entry="$(rust_matches "$entry_point" skip-comments)" || exit 2
raw_literal="$(rust_matches "\"[[:space:]]*($verbs)" skip-comments)" || exit 2
raw_start="$(rust_matches "^[[:space:]]*($verbs)" skip-comments)" || exit 2
sql_hits=""
while IFS= read -r hit; do
  [ -n "$hit" ] || continue
  file="$(file_of "$hit")"
  in_sc_db "$file" && continue
  case "$file" in */tests/*) continue ;; esac
  is_repository_path "$file" && continue
  sql_hits+="$hit"$'\n'
done < <(sort -u <<<"$raw_entry"$'\n'"$raw_literal"$'\n'"$raw_start")
report "sql-text" "move the query into a repository module (a repo/ or repository/ directory, repo.rs, *_repo.rs)" "${sql_hits%$'\n'}"

if [ "$failures" -gt 0 ]; then
  echo "$tag FAILED: $failures of 3 gates failed" >&2
  exit 1
fi
echo "$tag OK: 3 of 3 gates passed"
