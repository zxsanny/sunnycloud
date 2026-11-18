#!/usr/bin/env bash
# Builds the Android app (E-27/c18, AZ-4002 AC-1): `flutter build apk` in debug or release mode over the libraries
# that mobile-build-native.sh put into jniLibs (Gradle fails when one is missing), then mobile-check-apk.sh on the APK.
# usage: mobile-build-app.sh debug|release
# Release without android/key.properties is signed with the debug key and is not shippable (Play keys are the owner's).
set -euo pipefail

tag="[mobile-build-app]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"

mode="${1:-}"
case "$mode" in
  debug | release) ;;
  *) fail "usage: mobile-build-app.sh debug|release" ;;
esac

echo "$tag step 1/3: toolchain"
android_sdk >/dev/null
require_java
require_flutter

echo "$tag step 2/3: flutter build apk --$mode"
cd "$mobile_dir"
flutter pub get
flutter build apk "--$mode" -v
apk="$mobile_dir/build/app/outputs/flutter-apk/app-$mode.apk"
[ -f "$apk" ] || fail "flutter did not produce $apk"
ls -l "$apk"

echo "$tag step 3/3: checks on the APK"
"$here/mobile-check-apk.sh" "$apk"
echo "$tag OK: $apk"
