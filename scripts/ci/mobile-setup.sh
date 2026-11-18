#!/usr/bin/env bash
# Installs the pinned mobile toolchain for the `mobile` job and for a clean laptop (E-27/c18, AZ-4002): Flutter
# $FLUTTER_VERSION (download verified against a pinned SHA-256, extracted once into $MOBILE_TOOLS_DIR) and, with the
# Android SDK's sdkmanager, NDK $NDK_VERSION, platform $PLATFORM_VERSION and build-tools $BUILD_TOOLS_VERSION. The JDK
# (17 or newer) comes from the caller (actions/setup-java in CI). Under GitHub Actions the Flutter bin directory is
# appended to $GITHUB_PATH; elsewhere add it to PATH as printed. The Rust targets are added by mobile-build-native.sh.
# usage: mobile-setup.sh
# MOBILE_TOOLS_DIR   install directory of Flutter (default: ~/.cache/sunnycloud-mobile-tools)
set -euo pipefail

tag="[mobile-setup]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"
tools_dir="${MOBILE_TOOLS_DIR:-$HOME/.cache/sunnycloud-mobile-tools}"
flutter_dir="$tools_dir/flutter-$FLUTTER_VERSION"

echo "$tag step 1/3: Flutter $FLUTTER_VERSION in $flutter_dir"
if [ -x "$flutter_dir/bin/flutter" ]; then
  echo "$tag already installed"
else
  archive="$tools_dir/flutter_linux_$FLUTTER_VERSION-stable.tar.xz"
  mkdir -p "$tools_dir"
  curl --fail --show-error --location --retry 3 --no-progress-meter --output "$archive.part" \
    "https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_$FLUTTER_VERSION-stable.tar.xz"
  echo "$FLUTTER_SHA256  $archive.part" | sha256sum --check - || { rm -f "$archive.part"; fail "the Flutter archive does not match the pinned SHA-256 $FLUTTER_SHA256"; }
  rm -rf "$tools_dir/flutter" "$flutter_dir"
  tar -xJf "$archive.part" -C "$tools_dir"
  mv "$tools_dir/flutter" "$flutter_dir"
  rm -f "$archive.part"
fi
export PATH="$flutter_dir/bin:$PATH"
if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$flutter_dir/bin" >> "$GITHUB_PATH"
fi
echo "$tag add to PATH: $flutter_dir/bin"
require_flutter

echo "$tag step 2/3: Android SDK packages (ndk;$NDK_VERSION platforms;android-$PLATFORM_VERSION build-tools;$BUILD_TOOLS_VERSION)"
sdk="$(android_sdk)"
sdkmanager="$sdk/cmdline-tools/latest/bin/sdkmanager"
[ -x "$sdkmanager" ] || fail "$sdkmanager not found (install the Android command-line tools)"
# `yes` ends on SIGPIPE when sdkmanager closes its input, so it must not be a pipeline stage under pipefail.
"$sdkmanager" --sdk_root="$sdk" --licenses < <(yes) > /dev/null || fail "the Android SDK licences could not be accepted"
"$sdkmanager" --sdk_root="$sdk" --install "ndk;$NDK_VERSION" "platforms;android-$PLATFORM_VERSION" "build-tools;$BUILD_TOOLS_VERSION"

echo "$tag step 3/3: verify"
ndk_bin >/dev/null
[ -x "$sdk/build-tools/$BUILD_TOOLS_VERSION/zipalign" ] || fail "build-tools $BUILD_TOOLS_VERSION has no zipalign"
require_java
echo "$tag OK"
