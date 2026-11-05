#!/usr/bin/env bash
# Advisory gate (AC-63): validates the dated ignores of the deny configuration, derives the cargo-audit
# ignore list from the same entries, then runs `cargo audit --deny warnings` and `cargo deny check`.
# deny.toml is the only place for advisory ignores; there is no second list.
# DENY_TOML        deny configuration (default: src/deny.toml)
# AUDIT_WORKSPACE  workspace to audit (default: src; tests)
# AUDIT_TODAY      override of today's date, YYYY-MM-DD (tests)
# AUDIT_VALIDATE_ONLY=1   stop after validating the ignores (tests)
# Needs cargo-audit and cargo-deny (scripts/ci/install-tools.sh), python3 >= 3.11.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
deny_toml="${DENY_TOML:-$repo_root/src/deny.toml}"
workspace_dir="${AUDIT_WORKSPACE:-$repo_root/src}"
today="${AUDIT_TODAY:-$(date -u +%F)}"
tag="[audit]"

echo "$tag step 1/4: validate ignore entries of $deny_toml (today $today)"
[ -f "$deny_toml" ] || { echo "$tag FAILED: $deny_toml does not exist" >&2; exit 2; }
if ! ids="$(python3 -I "$ci_dir/audit_ignores.py" "$deny_toml" "$today")"; then
  echo "$tag FAILED: invalid advisory ignore entries (see INVALID IGNORE lines above)" >&2
  exit 1
fi
ignore_args=()
count=0
while IFS= read -r id; do
  [ -z "$id" ] || { ignore_args+=(--ignore "$id"); count=$((count + 1)); }
done <<<"$ids"
echo "$tag $count valid ignore entries: ${ids:-none}"

echo "$tag step 2/4: ignore entries are valid"
if [ "${AUDIT_VALIDATE_ONLY:-0}" = "1" ]; then
  echo "$tag OK: AUDIT_VALIDATE_ONLY=1, cargo tools not run"
  exit 0
fi

echo "$tag step 3/4: cargo audit --deny warnings"
cargo audit --file "$workspace_dir/Cargo.lock" --deny warnings ${ignore_args[@]+"${ignore_args[@]}"}

echo "$tag step 4/4: cargo deny check advisories bans licenses sources"
cargo deny --manifest-path "$workspace_dir/Cargo.toml" --config "$deny_toml" check advisories bans licenses sources

echo "$tag OK: advisories, bans, licenses and sources pass"
