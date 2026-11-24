#!/usr/bin/env bash
# Runs the IT-11 integration cases of 11 http_api (AZ-3835 AC-1): every case of the component's tests.md with its
# tests from src/components/11_http_api/tests/it_cases.txt, each reported with result, test count and duration.
# Fails when a case has no test or takes longer than its max execution time (it_suite.py states every rule).
# IT_SUITE_SPEC   default _docs/02_document/components/11_http_api/tests.md
# IT_SUITE_CASES  default src/components/11_http_api/tests/it_cases.txt
# IT_SUITE_PACKAGE default sunnycloud-http-api
# WORKSPACE_DIR   default src
# Exit 0 pass, 1 failure, 2 usage.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
spec="${IT_SUITE_SPEC:-$repo_root/_docs/02_document/components/11_http_api/tests.md}"
cases="${IT_SUITE_CASES:-$repo_root/src/components/11_http_api/tests/it_cases.txt}"
workspace_dir="${WORKSPACE_DIR:-$repo_root/src}"

exec python3 -I "$ci_dir/it_suite.py" "$spec" "$cases" "${IT_SUITE_PACKAGE:-sunnycloud-http-api}" "$workspace_dir"
