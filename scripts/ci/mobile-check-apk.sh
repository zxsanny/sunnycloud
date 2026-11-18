#!/usr/bin/env bash
# Checks one built APK (E-27/c18, AZ-4002 AC-1 and AC-2): minSdk 30, no `android:process` anywhere in the manifest (one
# process, 17 section 5), the library `libmobile_backup.so` for every ABI of the table, and the 16 KB alignment of
# every library in the APK (mobile-check-alignment.sh).
# usage: mobile-check-apk.sh APK
set -euo pipefail

tag="[mobile-check-apk]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"

apk="${1:?usage: mobile-check-apk.sh APK}"
[ -f "$apk" ] || fail "$apk does not exist"
aapt2="$(android_sdk)/build-tools/$BUILD_TOOLS_VERSION/aapt2"

echo "$tag step 1/4: minSdk"
badging="$("$aapt2" dump badging "$apk")"
echo "$badging" | grep -E "^(package|minSdkVersion|targetSdkVersion)"
echo "$badging" | grep -F "minSdkVersion:'30'" >/dev/null || fail "the APK does not declare minSdk 30"

echo "$tag step 2/4: one process"
manifest="$("$aapt2" dump xmltree --file AndroidManifest.xml "$apk")"
if echo "$manifest" | grep -F 'android:process' >/dev/null; then
  echo "$manifest" | grep -n -F 'android:process' >&2
  fail "the manifest sets android:process; the app must run in one process"
fi
echo "$tag no android:process attribute in the merged manifest"

echo "$tag step 3/4: one library per ABI"
entries="$(unzip -Z1 "$apk")"
for entry in "${ABI_TABLE[@]}"; do
  IFS=: read -r abi _ _ <<<"$entry"
  echo "$entries" | grep -Fx "lib/$abi/$library_file" >/dev/null || fail "lib/$abi/$library_file is not in the APK"
  echo "$tag lib/$abi/$library_file"
done

echo "$tag step 4/4: 16 KB alignment"
"$here/mobile-check-alignment.sh" "$apk"
echo "$tag OK: $apk"
