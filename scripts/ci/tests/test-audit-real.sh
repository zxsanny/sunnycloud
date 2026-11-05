#!/usr/bin/env bash
# AC-3 end to end with the real cargo-audit and cargo-deny on scratch workspaces (needs network and both tools):
#   a dependency with a known advisory and no ignore -> fail; the same with an ignore dated 91 days ago -> fail;
#   the same with an ignore dated 89 days ago -> pass; a pin below the bytes floor (bytes 1.11.0) -> fail.
# Never touches the repository workspace.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-audit-real]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
today="$(date -u +%F)"
fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# scratch NAME DEPENDENCY_LINE
scratch() {
  mkdir -p "$work/$1/src"
  cat >"$work/$1/Cargo.toml" <<TOML
[package]
name = "scratch-$1"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
$2
TOML
  printf '// scratch\n' >"$work/$1/src/lib.rs"
  cp "$repo_root/src/rust-toolchain.toml" "$work/$1/rust-toolchain.toml"
  (cd "$work/$1" && cargo generate-lockfile)
}

# deny_with IGNORE_ENTRY -> path of a deny configuration identical to src/deny.toml except for the ignore list
deny_with() {
  local file="$work/deny-$RANDOM.toml"
  python3 -I - "$repo_root/src/deny.toml" "$file" "$1" <<'PY'
import sys
source, target, entry = sys.argv[1:]
text = open(source).read().replace("ignore = []", f"ignore = [{entry}]")
open(target, "w").write(text)
PY
  printf '%s' "$file"
}

# expect NAME pass|fail WORKSPACE DENY_FILE
expect() {
  local name="$1" want="$2" rc
  set +e
  AUDIT_WORKSPACE="$work/$3" DENY_TOML="$4" "$ci_dir/audit.sh" >"$work/$name.out" 2>&1
  rc=$?
  set -e
  tail -15 "$work/$name.out"
  if [ "$want" = pass ] && [ "$rc" -ne 0 ]; then fail "$name: expected pass, exit $rc"; fi
  if [ "$want" = fail ] && [ "$rc" -eq 0 ]; then fail "$name: expected fail, exit 0"; fi
  echo "$tag $name: $want as expected (exit $rc)"
}

days_ago() { date -u -d "$today - $1 days" +%F; }
days_ahead() { date -u -d "$today + $1 days" +%F; }
entry() { printf '{ id = "RUSTSEC-2020-0071", reason = "added=%s; expires=%s; why=scratch test" }' "$(days_ago "$1")" "$(days_ahead 30)"; }

scratch advisory 'time = "=0.1.45"'
expect advisory-no-ignore fail advisory "$(deny_with '')"
expect advisory-91-days fail advisory "$(deny_with "$(entry 91)")"
expect advisory-89-days pass advisory "$(deny_with "$(entry 89)")"

scratch floor 'bytes = "=1.11.0"'
expect bytes-below-floor fail floor "$(deny_with '')"
echo "$tag OK"
