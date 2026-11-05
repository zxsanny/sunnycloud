#!/usr/bin/env bash
# AC-6 end to end with a real cargo llvm-cov run on a scratch workspace (needs cargo-llvm-cov and llvm-tools-preview):
# one tested and two untested functions -> below 75% -> exit 1 and the measured value is printed;
# adding the missing tests -> passes. Never touches the repository.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-check-coverage-real]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "$tag FAILED: $*" >&2; exit 1; }

mkdir -p "$work/ws/src"
cat >"$work/ws/Cargo.toml" <<'TOML'
[workspace]
resolver = "3"
members = ["."]

[package]
name = "scratch-coverage"
version = "0.0.0"
edition = "2024"
publish = false
TOML
cat >"$work/ws/src/lib.rs" <<'RS'
pub fn tested(x: i32) -> i32 {
    let y = x + 1;
    y * 2
}

pub fn untested_a(x: i32) -> i32 {
    let a = x + 1;
    let b = a + 2;
    let c = b + 3;
    c * 4
}

pub fn untested_b(x: i32) -> i32 {
    let a = x - 1;
    let b = a - 2;
    let c = b - 3;
    c * 4
}

#[cfg(test)]
mod tests {
    #[test]
    fn tested_doubles() {
        assert_eq!(super::tested(1), 4);
    }
}
RS
cp "$ci_dir/../../src/rust-toolchain.toml" "$work/ws/rust-toolchain.toml"
(cd "$work/ws" && cargo generate-lockfile)
: >"$work/paths.txt"

echo "$tag low coverage must fail"
set +e
WORKSPACE_DIR="$work/ws" CRITICAL_PATHS_FILE="$work/paths.txt" "$ci_dir/check-coverage.sh" </dev/null >"$work/low.out" 2>&1
rc=$?
set -e
cat "$work/low.out"
[ "$rc" -eq 1 ] || fail "expected exit 1, got $rc"
grep -E 'overall line coverage [0-9.]+%' "$work/low.out" >/dev/null || fail "measured value not printed"
grep -F 'test tests::tested_doubles ... ok' "$work/low.out" >/dev/null || fail "the test is not listed by name"

echo "$tag adding the missing tests must pass"
python3 -I - "$work/ws/src/lib.rs" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
text = text.replace("        assert_eq!(super::tested(1), 4);\n", "        assert_eq!(super::tested(1), 4);\n        assert_eq!(super::untested_a(0), 24);\n        assert_eq!(super::untested_b(0), -24);\n")
open(path, "w").write(text)
PY
WORKSPACE_DIR="$work/ws" CRITICAL_PATHS_FILE="$work/paths.txt" "$ci_dir/check-coverage.sh"
echo "$tag OK"
