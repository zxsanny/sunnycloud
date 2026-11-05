#!/usr/bin/env bash
# supply-chain: LICENSE is present and unchanged (pinned SHA-256 in scripts/ci/licence.sha256).
# COPYING.iOS is checked only when the owner has supplied the text (E-00 risk 3): once the file exists it must be non-empty.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tag="[check-licence]"
cd "$repo_root"

[ -s LICENSE ] || { echo "$tag FAILED: LICENSE is missing or empty" >&2; exit 1; }
sha256sum --check scripts/ci/licence.sha256 || { echo "$tag FAILED: LICENSE differs from scripts/ci/licence.sha256" >&2; exit 1; }

if [ -e COPYING.iOS ]; then
  [ -s COPYING.iOS ] || { echo "$tag FAILED: COPYING.iOS exists but is empty" >&2; exit 1; }
  echo "$tag COPYING.iOS present"
else
  echo "$tag COPYING.iOS not supplied by the owner yet; its check is disabled"
fi
echo "$tag OK"
