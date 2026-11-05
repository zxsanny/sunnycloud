#!/usr/bin/env bash
# AC-3 (context part): planted .env, nested .env*, secrets folder, _docs, .git and target must not reach the build context.
# Needs: docker. Plants and removes its own files (deploy/tests/.env.planted); the caller may also plant .env and secrets-dev/.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$REPO_ROOT"

PROBE_TAG="deploy-context-probe:$$"
NESTED="deploy/tests/.env.planted"
cleanup() { rm -f "$NESTED"; docker rmi "$PROBE_TAG" >/dev/null || echo "could not remove $PROBE_TAG" >&2; }
trap cleanup EXIT

printf 'planted\n' > "$NESTED"
docker build -f deploy/tests/context-probe.Dockerfile -t "$PROBE_TAG" .
listing="$(docker run --rm --entrypoint find "$PROBE_TAG" /ctx -maxdepth 4)"

# the probe must see real files, otherwise "absent" proves nothing
for must in /ctx/src/Cargo.toml /ctx/src/Cargo.lock /ctx/deploy/docker/api.Dockerfile; do
  if grep -qx "$must" <<<"$listing"; then pass "in context: $must"; else fail "missing from context: $must"; fi
done
for banned in '/ctx/.env' '/ctx/secrets-dev' '/ctx/secrets' '/ctx/_docs' '/ctx/.git' '/ctx/src/target' '/ctx/deploy/tests/.env.planted' '/ctx/README.md'; do
  if grep -qx -F "$banned" <<<"$listing"; then fail "present in context: $banned"; else pass "not in context: $banned"; fi
done
if grep -E '/(\.env[^/]*|secrets[^/]*|node_modules|e2e-results)$' <<<"$listing"; then fail "pattern match in context"; fi
finish
