#!/usr/bin/env bash
# Sourced by the mobile-*.sh scripts (E-27/c18, AZ-4002): the pinned versions, the ABI table and the Android
# toolchain lookup. Bump a version here and in nothing else; the CI job and a laptop use the same values.
# ANDROID_HOME (or ANDROID_SDK_ROOT) names the Android SDK; the NDK is always $SDK/ndk/$NDK_VERSION.

# A failing helper inside $(...) must stop the calling script, not hand it an empty string.
shopt -s inherit_errexit

NDK_VERSION=28.2.13676358
ANDROID_API=30
BUILD_TOOLS_VERSION=36.0.0
PLATFORM_VERSION=35
FLUTTER_VERSION=3.47.7
FLUTTER_SHA256=af7455f540df951184c7953fbe3cf884766e7541dacf8571f9f582db03dd6230
MIN_PAGE_ALIGN=16384

# ABI:rust target:clang wrapper prefix (the one library per recorded ABI of 17 section 5)
ABI_TABLE=(
  "arm64-v8a:aarch64-linux-android:aarch64-linux-android"
  "armeabi-v7a:armv7-linux-androideabi:armv7a-linux-androideabi"
  "x86_64:x86_64-linux-android:x86_64-linux-android"
)

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
src_dir="$repo_root/src"
# The workspace cargo runs in; SRC_DIR points it at a scratch copy (the seeded-failure tests).
cargo_dir="${SRC_DIR:-$src_dir}"
mobile_dir="$src_dir/mobile"
bindings_dir="$mobile_dir/android/app/src/main/kotlin/uniffi"
jni_libs_dir="$mobile_dir/android/app/src/main/jniLibs"
library_file=libmobile_backup.so

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# android_sdk -> prints the SDK directory
android_sdk() {
  local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
  [ -n "$sdk" ] || fail "ANDROID_HOME is not set"
  [ -d "$sdk" ] || fail "ANDROID_HOME=$sdk is not a directory"
  echo "$sdk"
}

# ndk_bin -> prints the NDK llvm bin directory after checking the NDK revision
ndk_bin() {
  local sdk ndk
  sdk="$(android_sdk)"
  ndk="$sdk/ndk/$NDK_VERSION"
  [ -f "$ndk/source.properties" ] || fail "NDK $NDK_VERSION not found in $sdk/ndk (scripts/ci/mobile-setup.sh installs it)"
  grep -F "Pkg.Revision = $NDK_VERSION" "$ndk/source.properties" >/dev/null || fail "$ndk is not NDK $NDK_VERSION"
  echo "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
}

# cargo_target_dir -> prints the cargo target directory of the workspace in $src_dir
cargo_target_dir() {
  (cd "$cargo_dir" && cargo metadata --format-version 1 --no-deps --locked) | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])'
}

# require_java -> a JDK 17 or newer (AGP 9 and the Flutter Gradle plugin need 17)
require_java() {
  local major
  command -v java >/dev/null || fail "java is not on PATH (set JAVA_HOME and PATH to a JDK 17 or newer)"
  major="$(java -XshowSettings:properties -version 2>&1 | sed -n 's/^ *java.specification.version = //p')"
  echo "$tag java specification version $major"
  [ "${major:-0}" -ge 17 ] || fail "JDK 17 or newer is required, found '${major:-none}'"
}

# require_flutter -> the pinned Flutter version is the one on PATH
require_flutter() {
  local have
  command -v flutter >/dev/null || fail "flutter is not on PATH (scripts/ci/mobile-setup.sh installs $FLUTTER_VERSION)"
  have="$(flutter --version --machine | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["frameworkVersion"])')"
  echo "$tag flutter $have"
  [ "$have" = "$FLUTTER_VERSION" ] || fail "Flutter $FLUTTER_VERSION is pinned, found $have"
}
