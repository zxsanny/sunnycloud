#!/usr/bin/env bash
# Fails on an unpinned `uses:` or image, a non-read-only default workflow permission, or a fork checkout
# under pull_request_target (RESTRICT-30, NFT-SEC-37). Rules are listed in check_pinning.py.
# usage: check-pinning.sh [ROOT]   (default: repository root)
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="${1:-$(cd "$ci_dir/../.." && pwd)}"
echo "[check-pinning] scanning $root"
exec python3 -I "$ci_dir/check_pinning.py" "$root"
