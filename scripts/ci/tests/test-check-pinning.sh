#!/usr/bin/env bash
# AC-2 for check-pinning.sh on fixture trees: tag -> fail, SHA without comment -> fail, SHA with comment -> pass;
# plus unpinned images, permissions, pull_request_target with a fork checkout; then the real repository -> pass.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-check-pinning]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
sha="3d3c42e5aac5ba805825da76410c181273ba90b1"
digest="sha256:0000000000000000000000000000000000000000000000000000000000000001"

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# workflow NAME PERMISSIONS_BLOCK USES_LINE [EXTRA_ON]
make_tree() { # name, permissions block (may be empty), steps body, on-block
  local dir="$work/$1"
  mkdir -p "$dir/.github/workflows"
  {
    printf 'name: fixture\non:\n%s\n' "${4:-  pull_request:}"
    printf '%s\n' "$2"
    printf 'jobs:\n  j:\n    runs-on: ubuntu-24.04\n    steps:\n%s\n' "$3"
  } >"$dir/.github/workflows/ci.yml"
}

# expect NAME pass|fail [grep pattern in output]
expect() {
  local name="$1" want="$2" pattern="${3:-}" rc
  set +e
  "$ci_dir/check-pinning.sh" "$work/$name" >"$work/$name.out" 2>&1
  rc=$?
  set -e
  cat "$work/$name.out"
  if [ "$want" = pass ] && [ "$rc" -ne 0 ]; then fail "$name: expected pass, exit $rc"; fi
  if [ "$want" = fail ] && [ "$rc" -eq 0 ]; then fail "$name: expected fail, exit 0"; fi
  if [ -n "$pattern" ]; then grep -F -- "$pattern" "$work/$name.out" >/dev/null || fail "$name: output lacks '$pattern'"; fi
  echo "$tag $name: $want as expected (exit $rc)"
}

readonly_perms=$'permissions:\n  contents: read'

make_tree tag-v4 "$readonly_perms" '      - uses: actions/checkout@v4'
expect tag-v4 fail "actions/checkout@v4"
make_tree sha-no-comment "$readonly_perms" "      - uses: actions/checkout@$sha"
expect sha-no-comment fail "version comment"
make_tree sha-with-comment "$readonly_perms" "      - uses: actions/checkout@$sha # v7.0.1"
expect sha-with-comment pass "0 unpinned"
make_tree sha-short "$readonly_perms" "      - uses: actions/checkout@${sha:0:7} # v7.0.1"
expect sha-short fail
make_tree local-action "$readonly_perms" "      - uses: ./.github/actions/x"
expect local-action pass
make_tree perms-missing "" "      - uses: actions/checkout@$sha # v7.0.1"
expect perms-missing fail "permissions"
make_tree perms-write "permissions:
  contents: read
  packages: write" "      - uses: actions/checkout@$sha # v7.0.1"
expect perms-write fail "permissions"
make_tree perms-write-all "permissions: write-all" "      - uses: actions/checkout@$sha # v7.0.1"
expect perms-write-all fail "permissions"
make_tree ptarget-fork "$readonly_perms" "      - uses: actions/checkout@$sha # v7.0.1
        with:
          ref: \${{ github.event.pull_request.head.sha }}" "  pull_request_target:"
expect ptarget-fork fail "fork checkout"
make_tree ptarget-safe "$readonly_perms" "      - uses: actions/checkout@$sha # v7.0.1" "  pull_request_target:"
expect ptarget-safe pass
make_tree pr-head-ok "$readonly_perms" "      - uses: actions/checkout@$sha # v7.0.1
        with:
          ref: \${{ github.event.pull_request.head.sha }}"
expect pr-head-ok pass
make_tree service-image "$readonly_perms
" "      - uses: actions/checkout@$sha # v7.0.1"
printf '    services:\n      db:\n        image: postgres:18\n' >>"$work/service-image/.github/workflows/ci.yml"
expect service-image fail "postgres:18"

mkdir -p "$work/docker-tag/deploy"
make_tree docker-tag "$readonly_perms" "      - uses: actions/checkout@$sha # v7.0.1"
printf 'FROM rust:1.97.1-trixie AS build\nFROM build AS again\nFROM scratch\nFROM debian:trixie-slim@%s\n' "$digest" >"$work/docker-tag/deploy/a.Dockerfile"
expect docker-tag fail "rust:1.97.1-trixie"
printf 'FROM rust:1.97.1-trixie@%s AS build\nFROM build AS again\nFROM scratch\nFROM --platform=linux/amd64 debian:trixie-slim@%s\n' "$digest" "$digest" >"$work/docker-tag/deploy/a.Dockerfile"
expect docker-tag pass

printf 'services:\n  db:\n    image: postgres:18\n  api:\n    build: .\n    image: local-api:dev\n' >"$work/docker-tag/compose.yml"
expect docker-tag fail "postgres:18"
printf 'services:\n  db:\n    image: postgres:18@%s\n  api:\n    build: .\n    image: local-api:dev\n' "$digest" >"$work/docker-tag/compose.yml"
expect docker-tag pass

echo "$tag the real repository"
if [ -d "$repo_root/.github/workflows" ]; then
  "$ci_dir/check-pinning.sh" "$repo_root"
else
  fail "no .github/workflows in $repo_root"
fi
echo "$tag OK"
