#!/usr/bin/env bash
# AC-4: neither check script uses a quiet flag (-q, --quiet, --silent); expects 0 hits.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scripts=("$ci_dir/check-sqlx-prepare.sh" "$ci_dir/check-migrations.sh")

echo "[test-no-quiet-flags] searching ${scripts[*]}"
hits="$(grep -nE '(^|[[:space:]])(-q|--quiet|--silent)([[:space:]=]|$)' "${scripts[@]}" || true)"
if [ -n "$hits" ]; then
  echo "[test-no-quiet-flags] FAILED: quiet flags found:" >&2
  echo "$hits" >&2
  exit 1
fi

echo "[test-no-quiet-flags] checking that every script prints step lines"
for script in "${scripts[@]}"; do
  steps="$(grep -c 'step [0-9]*/[0-9]*' "$script" || true)"
  echo "[test-no-quiet-flags] $(basename "$script"): $steps step lines"
  [ "$steps" -ge 4 ] || { echo "[test-no-quiet-flags] FAILED: $script prints too few steps" >&2; exit 1; }
done
echo "[test-no-quiet-flags] OK: 0 quiet-flag hits"
