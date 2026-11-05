#!/usr/bin/env bash
# Scans the full git history of the repository for committed secrets with the pinned gitleaks
# (scripts/ci/install-tools.sh gitleaks). Secret values are redacted in the log; each finding names its file.
# Exit 0 clean, 1 findings, 2 cannot scan (gitleaks missing, shallow clone).
# SECRET_SCAN_REPO      repository to scan (default: this repository)
# GITLEAKS_CONFIG       gitleaks configuration (default: scripts/ci/gitleaks.toml = built-in rules + task-id allowlist)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo="${SECRET_SCAN_REPO:-$repo_root}"
config="${GITLEAKS_CONFIG:-$repo_root/scripts/ci/gitleaks.toml}"
tag="[secret-scan]"

echo "$tag step 1/3: verify gitleaks and a complete history in $repo"
command -v gitleaks || { echo "$tag FAILED: gitleaks not found; run scripts/ci/install-tools.sh gitleaks" >&2; exit 2; }
gitleaks version
if [ "$(git -C "$repo" rev-parse --is-shallow-repository)" = "true" ]; then
  echo "$tag FAILED: shallow clone; the scan would miss history (checkout with fetch-depth: 0)" >&2
  exit 2
fi
commits="$(git -C "$repo" rev-list --all --count)"
echo "$tag $commits commits in history"

echo "$tag step 2/3: gitleaks git (redacted)"
rc=0
gitleaks git --config "$config" --no-banner --no-color --redact --verbose --exit-code 1 "$repo" || rc=$?

echo "$tag step 3/3: report"
if [ "$rc" -eq 0 ]; then
  echo "$tag OK: no secrets in $commits commits"
  exit 0
fi
if [ "$rc" -eq 1 ]; then
  echo "$tag FAILED: gitleaks found secrets in the history (File: lines above name each file)" >&2
else
  echo "$tag FAILED: gitleaks exited with $rc" >&2
fi
exit 1
