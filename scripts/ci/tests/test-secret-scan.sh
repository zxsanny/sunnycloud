#!/usr/bin/env bash
# AC-8 for secret-scan.sh on scratch git repositories: a clean repository passes; a commit that adds a private key
# fails and the output names the file; the key stays in history after a later commit removes it, so the scan still fails;
# a shallow clone is refused. Needs gitleaks on PATH. The key is generated per run, never stored in this repository.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-secret-scan]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "$tag FAILED: $*" >&2; exit 1; }
git_in() { git -C "$work/repo" -c user.name=ci -c user.email=ci@example.invalid "$@"; }

command -v openssl >/dev/null || fail "openssl is needed to generate the throw-away key"
mkdir "$work/repo"
git -C "$work/repo" init --initial-branch=dev
printf 'hello\n' >"$work/repo/README.txt"
git_in add README.txt
git_in commit -m "clean commit"

echo "$tag clean repository must pass"
SECRET_SCAN_REPO="$work/repo" "$ci_dir/secret-scan.sh"

echo "$tag committing a fake private key"
mkdir -p "$work/repo/config"
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$work/repo/config/deploy_key.pem"
git_in add config/deploy_key.pem
git_in commit -m "add key"
set +e
SECRET_SCAN_REPO="$work/repo" "$ci_dir/secret-scan.sh" >"$work/leak.out" 2>&1
rc=$?
set -e
cat "$work/leak.out"
[ "$rc" -ne 0 ] || fail "the scan passed although a private key is committed"
grep -F "config/deploy_key.pem" "$work/leak.out" >/dev/null || fail "the output does not name config/deploy_key.pem"
if grep -F "BEGIN PRIVATE KEY" "$work/leak.out"; then fail "the key material was printed"; fi
echo "$tag leak run: exit $rc, file named"

echo "$tag removing the key in a later commit must not hide it"
git_in rm config/deploy_key.pem
git_in commit -m "remove key"
set +e
SECRET_SCAN_REPO="$work/repo" "$ci_dir/secret-scan.sh" >"$work/removed.out" 2>&1
rc=$?
set -e
[ "$rc" -ne 0 ] || fail "the scan passed although the key is still in history"
grep -F "config/deploy_key.pem" "$work/removed.out" >/dev/null || fail "history finding does not name the file"

echo "$tag shallow clone must be refused"
git clone --depth 1 "file://$work/repo" "$work/shallow"
set +e
SECRET_SCAN_REPO="$work/shallow" "$ci_dir/secret-scan.sh" >"$work/shallow.out" 2>&1
rc=$?
set -e
cat "$work/shallow.out"
[ "$rc" -eq 2 ] || fail "expected exit 2 for a shallow clone, got $rc"
echo "$tag OK"
