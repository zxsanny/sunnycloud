#!/usr/bin/env bash
# Builds the Rust library of the mobile app, one per ABI (E-27/c18, AZ-4002 AC-1): `mobile_backup` with feature
# `uniffi` (sync-core included), release profile, NDK r28 clang wrappers at API 30, and the 16 KB page-size link flag
# of src/.cargo/config.toml. Each library is copied to src/mobile/android/app/src/main/jniLibs/<abi>/
# (gitignored build output) where Gradle packages it. Nothing here builds iOS (E-28/c03).
# usage: mobile-build-native.sh
set -euo pipefail

tag="[mobile-build-native]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"

echo "$tag step 1/3: toolchain (NDK $NDK_VERSION, API $ANDROID_API)"
bin="$(ndk_bin)"
export PATH="$bin:$PATH"
cd "$cargo_dir"
rustc --version
targets=()
for entry in "${ABI_TABLE[@]}"; do
  IFS=: read -r _ target _ <<<"$entry"
  targets+=("$target")
done
rustup target add "${targets[@]}"
target_dir="$(cargo_target_dir)"

echo "$tag step 2/3: cargo build --release per ABI"
for entry in "${ABI_TABLE[@]}"; do
  IFS=: read -r abi target clang <<<"$entry"
  upper="$(echo "$target" | tr 'a-z-' 'A-Z_')"
  echo "$tag $abi ($target)"
  export "CARGO_TARGET_${upper}_LINKER=$bin/${clang}${ANDROID_API}-clang"
  export "CC_${target//-/_}=$bin/${clang}${ANDROID_API}-clang"
  export "AR_${target//-/_}=$bin/llvm-ar"
  cargo build -v --locked --release --lib -p mobile_backup --features uniffi --target "$target"
done

echo "$tag step 3/3: copy to jniLibs"
for entry in "${ABI_TABLE[@]}"; do
  IFS=: read -r abi target _ <<<"$entry"
  built="$target_dir/$target/release/$library_file"
  [ -f "$built" ] || fail "cargo did not produce $built"
  install -D -m 0755 "$built" "$jni_libs_dir/$abi/$library_file"
  ls -l "$jni_libs_dir/$abi/$library_file"
done
echo "$tag OK: ${#ABI_TABLE[@]} libraries in $jni_libs_dir"
