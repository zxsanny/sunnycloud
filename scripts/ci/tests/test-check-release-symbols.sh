#!/usr/bin/env bash
# AC-4 for check-release-symbols.sh on a scratch binary that really links sc_clock: built twice in release mode,
# once without and once with the test-clock feature. The first must pass, the second must fail naming the
# markers. Also checks the argument and non-binary errors. The scratch crate lives in a temp dir.
# Needs cargo and the registry (same as every cargo job). The real server/worker binaries: test-check-release-symbols-real.sh.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
tag="[test-check-release-symbols]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "$tag step 1/4: scratch crate depending on sc_clock"
probe="$work/probe"
mkdir -p "$probe/src"
cat >"$probe/Cargo.toml" <<TOML
[package]
name = "probe"
version = "0.1.0"
edition = "2024"

[workspace]

[dependencies]
sc_clock = { path = "$repo_root/src/shared/clock" }

[features]
test-clock = ["sc_clock/test-clock"]
TOML
cat >"$probe/src/main.rs" <<'RS'
use sc_clock::Clock;

fn main() {
    #[cfg(feature = "test-clock")]
    let clock = sc_clock::OffsetClock::new();
    #[cfg(not(feature = "test-clock"))]
    let clock = sc_clock::SystemClock::new();
    println!("{:?}", clock.now_utc());
}
RS
cp "$repo_root/src/Cargo.lock" "$probe/Cargo.lock"
export CARGO_TARGET_DIR="$work/target"

echo "$tag step 2/4: release build without test-clock"
(cd "$probe" && cargo build --release)
cp "$CARGO_TARGET_DIR/release/probe" "$work/probe-release"

echo "$tag step 3/4: release build with test-clock"
(cd "$probe" && cargo build --release --features test-clock)
cp "$CARGO_TARGET_DIR/release/probe" "$work/probe-test-clock"

echo "$tag step 4/4: AC-4 checks"
out="$("$ci_dir/check-release-symbols.sh" "$work/probe-release" 2>&1)" || { echo "$out"; echo "$tag FAILED: clean release build was rejected" >&2; exit 1; }
echo "$out"
grep -q 'OK: no release binary contains the test-clock feature' <<<"$out"

rc=0
out="$("$ci_dir/check-release-symbols.sh" "$work/probe-test-clock" 2>&1)" || rc=$?
echo "$out"
[ "$rc" -eq 1 ] || { echo "$tag FAILED: test-clock build exited $rc, expected 1" >&2; exit 1; }
grep -q 'contains test-clock markers: OffsetClock' <<<"$out"

rc=0
out="$("$ci_dir/check-release-symbols.sh" "$work/probe-release" "$work/probe-test-clock" 2>&1)" || rc=$?
[ "$rc" -eq 1 ] || { echo "$out"; echo "$tag FAILED: one bad binary among two must fail, exited $rc" >&2; exit 1; }

rc=0
out="$("$ci_dir/check-release-symbols.sh" "$ci_dir/check-release-symbols.sh" 2>&1)" || rc=$?
[ "$rc" -eq 2 ] || { echo "$out"; echo "$tag FAILED: a non-ELF file must exit 2, exited $rc" >&2; exit 1; }

rc=0
out="$("$ci_dir/check-release-symbols.sh" 2>&1)" || rc=$?
[ "$rc" -eq 2 ] || { echo "$out"; echo "$tag FAILED: no arguments must exit 2, exited $rc" >&2; exit 1; }

echo "$tag OK"
