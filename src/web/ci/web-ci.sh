#!/usr/bin/env bash
# The web job (AZ-3935): the same steps on a laptop and in CI. Needs Node >= 22 and npm.
# usage: src/web/ci/web-ci.sh
set -euo pipefail

cd "$(dirname "$0")/.."

echo "[web] npm ci"
npm ci --no-audit --no-fund

echo "[web] lint (eslint, prettier)"
npm run lint

echo "[web] svelte-check"
npm run check

echo "[web] unit tests (includes the bundle scan of a real production build)"
npm test

echo "[web] production build"
npm run build

echo "[web] generated client matches openapi/openapi.json"
npm run gen:api
git diff --exit-code -- openapi src/lib/api/generated

echo "[web] npm audit (high and above)"
npm audit --audit-level=high

echo "[web] done"
