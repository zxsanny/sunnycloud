#!/usr/bin/env bash
# 16 KB page alignment gate (E-27/c18, AZ-4002 AC-2, R31): every LOAD segment of every native library must be
# aligned to at least 0x4000, or the library does not load on Android 15 devices with 16 KB pages (the device error
# does not name alignment, so this check is the diagnostic). Method of the AZ-3985 record: llvm-readelf -lW.
# An argument is a .so file, a directory (every .so below it) or an .apk (every lib/<abi>/*.so in it, ours, JNA's and
# Flutter's, plus `zipalign -c -P 16 -v 4`, which checks that the stored libraries start on 16 KB boundaries).
# usage: mobile-check-alignment.sh PATH...      exit 1 when any segment is below 16 KB or nothing was checked
set -euo pipefail

tag="[mobile-check-alignment]"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$here/mobile-common.sh"

[ $# -gt 0 ] || fail "usage: mobile-check-alignment.sh PATH..."
readelf="$(ndk_bin)/llvm-readelf"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
checked=0
bad=0

# check_so FILE LABEL
check_so() {
  local file="$1" label="$2" aligns align ok=1
  aligns="$("$readelf" -lW "$file" | awk '$1=="LOAD"{print $NF}' | sort -u | tr '\n' ' ')"
  [ -n "$aligns" ] || fail "$label has no LOAD segment (not an ELF shared library?)"
  for align in $aligns; do
    [ $((align)) -ge "$MIN_PAGE_ALIGN" ] || ok=0
  done
  checked=$((checked + 1))
  if [ "$ok" = 1 ]; then
    echo "$tag PASS $label LOAD align: $aligns"
  else
    echo "$tag FAIL $label LOAD align: $aligns (needs >= $(printf '0x%x' "$MIN_PAGE_ALIGN"))" >&2
    bad=$((bad + 1))
  fi
}

# check_dir DIR LABEL_PREFIX
check_dir() {
  local file
  while IFS= read -r file; do
    check_so "$file" "$2${file#"$1"/}"
  done < <(find "$1" -type f -name '*.so' | sort)
}

for path in "$@"; do
  if [ -d "$path" ]; then
    check_dir "$path" ""
  elif [ "${path##*.}" = apk ]; then
    [ -f "$path" ] || fail "$path does not exist"
    echo "$tag zipalign -c -P 16 -v 4 $path"
    "$(android_sdk)/build-tools/$BUILD_TOOLS_VERSION/zipalign" -c -P 16 -v 4 "$path" | grep -E 'lib/.*\.so|Verification' \
      || fail "zipalign found no libraries or no verification line in $path"
    extracted="$work/$(basename "$path")"
    mkdir -p "$extracted"
    unzip -o -d "$extracted" "$path" 'lib/*.so'
    check_dir "$extracted" "$(basename "$path"):"
  elif [ -f "$path" ]; then
    check_so "$path" "$path"
  else
    fail "$path does not exist"
  fi
done

[ "$checked" -gt 0 ] || fail "no .so file found in: $*"
[ "$bad" -eq 0 ] || fail "$bad of $checked libraries have a LOAD segment below 16 KB"
echo "$tag OK: $checked libraries, every LOAD segment is 16 KB aligned"
