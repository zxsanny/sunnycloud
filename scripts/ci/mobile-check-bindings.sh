#!/usr/bin/env bash
# Kotlin binding drift gate (E-27/c18, AZ-4002 AC-3): regenerates the UniFFI Kotlin bindings of `mobile_backup`
# (uniffi 0.32.2, library mode from the freshly built host cdylib, no formatter, so the output is deterministic) and
# fails when they differ from the committed ones in src/mobile/android/app/src/main/kotlin/uniffi/.
# The bindings are generated code: change the Rust surface, then run `mobile-check-bindings.sh --update` and commit.
# usage: mobile-check-bindings.sh [--update]
# BINDINGS_DIR overrides the committed directory and SRC_DIR the workspace (the seeded-failure tests).
set -euo pipefail

tag="[mobile-check-bindings]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"

update=0
case "${1:-}" in
  "") ;;
  --update) update=1 ;;
  *) fail "usage: mobile-check-bindings.sh [--update]" ;;
esac
committed="${BINDINGS_DIR:-$bindings_dir}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "$tag step 1/3: host cdylib and the pinned uniffi-bindgen binary (feature uniffi-cli)"
cd "$cargo_dir"
cargo build -v --locked -p mobile_backup --features uniffi-cli
target_dir="$(cargo_target_dir)"

echo "$tag step 2/3: generate the Kotlin bindings"
"$target_dir/debug/uniffi-bindgen" generate --library "$target_dir/debug/$library_file" --language kotlin \
  --out-dir "$work/generated" --no-format
[ -d "$work/generated/uniffi" ] || fail "uniffi-bindgen produced no uniffi/ directory"
find "$work/generated" -type f | sort

if [ "$update" = 1 ]; then
  echo "$tag step 3/3: --update, replacing $committed"
  rm -rf "$committed"
  mkdir -p "$(dirname "$committed")"
  cp -r "$work/generated/uniffi" "$committed"
  echo "$tag OK: bindings updated; commit them"
  exit 0
fi

echo "$tag step 3/3: diff of the committed bindings ($committed) against the generated ones"
[ -d "$committed" ] || fail "$committed does not exist; run mobile-check-bindings.sh --update"
if ! diff -ru "$committed" "$work/generated/uniffi"; then
  fail "the committed Kotlin bindings differ from the Rust surface; run scripts/ci/mobile-check-bindings.sh --update and commit"
fi
echo "$tag OK: the committed Kotlin bindings match the Rust surface"
