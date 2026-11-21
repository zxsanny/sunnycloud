#!/usr/bin/env bash
# The web job (AZ-3935): the same steps on a laptop and in CI. Needs bun (1.4.5, scripts/ci/install-tools.sh bun),
# wasm-pack (scripts/ci/install-tools.sh wasm-pack), the Rust toolchain of src/rust-toolchain.toml with the
# wasm32-unknown-unknown target, and Node >= 22: the bins of vite, vitest, eslint and svelte-check start with
# `#!/usr/bin/env node`, so `bun run` hands them to Node (vitest's forks pool does not start under `bun --bun`).
# usage: src/web/ci/web-ci.sh
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in bun wasm-pack node; do
  command -v "$tool" >/dev/null || { echo "[web] FAILED: $tool is not on PATH (scripts/ci/install-tools.sh bun wasm-pack)" >&2; exit 1; }
done
node_major="$(node --version | sed 's/^v\([0-9]*\).*/\1/')"
[ "$node_major" -ge 22 ] || { echo "[web] FAILED: Node $(node --version) found, the web tools need Node >= 22" >&2; exit 1; }
echo "[web] bun $(bun --version), $(wasm-pack --version), node $(node --version)"

echo "[web] bun install --frozen-lockfile"
bun install --frozen-lockfile

echo "[web] sync-core-wasm for the browser (wasm-pack, pinned Rust toolchain)"
bun run build:wasm

echo "[web] lint (eslint, prettier --check)"
bun run lint

echo "[web] typecheck (svelte-check)"
bun run typecheck

echo "[web] unit tests (includes the bundle scan of a real production build)"
bun run test

echo "[web] production build"
bun run build

echo "[web] generated client matches openapi/openapi.json"
bun run gen:api
git diff --exit-code -- openapi src/lib/api/generated

echo "[web] bun audit (high and above)"
bun audit --audit-level=high

echo "[web] done"
