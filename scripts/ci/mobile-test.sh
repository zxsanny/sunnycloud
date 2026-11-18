#!/usr/bin/env bash
# Tests of the mobile project (E-27/c18, AZ-4002 AC-1 and AC-5): `flutter analyze`, `flutter test`, and the Kotlin unit
# tests, which load the host build of the same Rust library through JNA and call `BackupCore.status` without a server.
# The test task is cleaned first because the native library is not one of its inputs. Needs the Gradle wrapper that `flutter build` (mobile-build-app.sh) or `flutter pub get` writes, and jniLibs from
# mobile-build-native.sh (Gradle fails without them).
# usage: mobile-test.sh
set -euo pipefail

tag="[mobile-test]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"

echo "$tag step 1/5: toolchain"
android_sdk >/dev/null
require_java
require_flutter

echo "$tag step 2/5: flutter analyze and flutter test"
cd "$mobile_dir"
flutter pub get
flutter analyze
flutter test -v

echo "$tag step 3/5: host build of the Rust library for the Kotlin unit tests"
(cd "$cargo_dir" && cargo build -v --locked -p mobile_backup --features uniffi)
host_lib_dir="$(cargo_target_dir)/debug"
[ -f "$host_lib_dir/$library_file" ] || fail "cargo did not produce $host_lib_dir/$library_file"

echo "$tag step 4/5: Kotlin unit tests (gradle :app:testDebugUnitTest)"
[ -x "$mobile_dir/android/gradlew" ] || fail "android/gradlew is missing; run scripts/ci/mobile-build-app.sh debug first (flutter writes the wrapper)"
cd "$mobile_dir/android"
./gradlew :app:cleanTestDebugUnitTest :app:testDebugUnitTest --info --console=plain -PnativeHostLibDir="$host_lib_dir"

echo "$tag step 5/5: Kotlin test results (Gradle prints no per-test lines)"
python3 -I - "$mobile_dir/build/app/test-results/testDebugUnitTest" <<'PY'
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

results = sorted(Path(sys.argv[1]).glob("TEST-*.xml"))
total = failed = 0
for path in results:
    for case in ET.parse(path).getroot().iter("testcase"):
        bad = case.find("failure") is not None or case.find("error") is not None
        total += 1
        failed += bad
        print(f"{'FAIL' if bad else 'PASS'} {case.get('classname')}.{case.get('name')}")
if total == 0 or failed:
    sys.exit(f"{total} Kotlin tests ran, {failed} failed")
print(f"{total} Kotlin tests passed")
PY
echo "$tag OK"
