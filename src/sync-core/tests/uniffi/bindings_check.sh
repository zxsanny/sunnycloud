#!/usr/bin/env bash
# AC-2 of AZ-3968 (`uniffi_surface.md` bindings case): Kotlin and Swift bindings generate from the
# `sync-core` cdylib with the `uniffi` feature, without manual edits, and every public type re-exported by
# `sync-core` and `sync-core-proto` appears in both, except the names in NOT_ON_SURFACE (each with its reason
# in the contract). A new public type without derives fails here.
# The generator is the uniffi-bindgen 0.32.2 binary of vault-core (feature uniffi-cli); generation needs no
# Swift toolchain, so both languages run on Linux.
# usage: bindings_check.sh [OUT_DIR]     (default: src/target/sync-core-uniffi)
set -euo pipefail

src_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
out_dir="${1:-$src_dir/target/sync-core-uniffi}"
tag="[sync-core-bindings]"

# Public names that are not on the FFI surface: Rust traits and helpers with Rust-only types (their foreign
# shapes are the Foreign* traits and HostError), executor internals, functions and constants.
NOT_ON_SURFACE=" BadCursor ResumableFileHasher UploadPlanner ServerUrl FileHasher HashError \
  SyncApi BoxFuture Method Request RequestPath Transport WireRequest WireResponse Mapped \
  CredentialProvider FileSource DownloadSink CheckpointStore MemoryCheckpointStore \
  StoreError PersistFailed CredentialUnavailable HashCheckpoint UploadRequest SmallFile SkippedFile \
  BatchSink SourceResolver Foreign "

cd "$src_dir"
echo "$tag step 1/3: cdylib with the uniffi feature"
cargo rustc --locked -p sync-core --lib --features uniffi --crate-type cdylib
lib="$src_dir/target/debug/libsync_core.so"
[ "$(uname -s)" = "Darwin" ] && lib="$src_dir/target/debug/libsync_core.dylib"

echo "$tag step 2/3: Kotlin and Swift bindings"
for language in kotlin swift; do
  rm -rf "${out_dir:?}/$language"
  cargo run --locked -q -p vault-core --features uniffi-cli --bin uniffi-bindgen -- \
    generate --library "$lib" --language "$language" --out-dir "$out_dir/$language" --no-format
done

echo "$tag step 3/3: every public type is in both bindings"
names="$(cat sync-core/src/lib.rs sync-core-proto/src/lib.rs | tr '\n' ' ' \
  | grep -oE 'pub use [^;]*;' | sed -E 's/^pub use //; s/;$//; s/[{}]/ /g' | tr ',' '\n' \
  | sed -E 's/.*:://; s/^ +//; s/ +$//' | grep -E '^[A-Z][A-Za-z0-9]*[a-z][A-Za-z0-9]*$' | sort -u)"
missing=()
for name in $names; do
  case "$NOT_ON_SURFACE" in *" $name "*) continue ;; esac
  kotlin="(class|interface|typealias) $name\b|class ${name%Error}Exception\b"
  swift="(struct|protocol|typealias|enum|class) $name\b"
  grep -rqE "$kotlin" "$out_dir/kotlin" || missing+=("$name (Kotlin)")
  grep -rqE "$swift" "$out_dir/swift" || missing+=("$name (Swift)")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "$tag FAILED: public types missing from the bindings: ${missing[*]}" >&2
  exit 1
fi
echo "$tag OK: $(echo "$names" | wc -w) public names checked, bindings in $out_dir"
