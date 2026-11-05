#!/usr/bin/env bash
# Release binaries must not contain the sc_clock `test-clock` feature (OffsetClock): anyone with API access
# could otherwise skip retention (sc_clock contract; FT-N-35, NFT-SEC-38).
# Scans each binary for the symbol/type markers below (Rust v0 and legacy mangling both embed them; a
# derived Debug name keeps `OffsetClock` visible even in a stripped binary).
# usage: check-release-symbols.sh TARGET...
#   TARGET is a binary file, or `--image REF PATH_IN_IMAGE` to scan a binary inside a local Docker image
#   (copied out with docker create + docker cp, so the shipped bytes are the ones checked).
# Exit code: 0 when no target contains a marker, 1 when one does, 2 on a usage, copy or scan error.
set -euo pipefail

tag="[check-release-symbols]"
markers=(OffsetClock sc_clock6offset)
work="$(mktemp -d)"
container=""
cleanup() {
  if [ -n "$container" ]; then docker rm --force "$container" || echo "$tag could not remove container $container" >&2; fi
  rm -rf "$work"
}
trap cleanup EXIT

targets=()
labels=()
while [ $# -gt 0 ]; do
  case "$1" in
    --image)
      [ $# -ge 3 ] || { echo "$tag FAILED: --image needs REF and PATH_IN_IMAGE" >&2; exit 2; }
      ref="$2"; path="$3"; shift 3
      container="$(docker create "$ref")"
      copy="$work/$(( ${#targets[@]} + 1 ))-$(basename "$path")"
      docker cp "$container:$path" "$copy"
      docker rm --force "$container"
      container=""
      targets+=("$copy"); labels+=("$ref:$path")
      ;;
    -*) echo "$tag FAILED: unknown option $1" >&2; exit 2 ;;
    *) targets+=("$1"); labels+=("$1"); shift ;;
  esac
done
[ "${#targets[@]}" -gt 0 ] || { echo "$tag usage: check-release-symbols.sh [--image REF PATH_IN_IMAGE | BINARY]..." >&2; exit 2; }

echo "$tag step 1/2: checking ${#targets[@]} binary(ies) for markers: ${markers[*]}"
failed=0
for i in "${!targets[@]}"; do
  file="${targets[$i]}" label="${labels[$i]}"
  [ -f "$file" ] || { echo "$tag FAILED: $label is not a file" >&2; exit 2; }
  magic="$(od -An -tx1 -N4 "$file" | tr -d ' \n')"
  [ "$magic" = "7f454c46" ] || { echo "$tag FAILED: $label is not an ELF binary (magic $magic)" >&2; exit 2; }
  found=()
  for marker in "${markers[@]}"; do
    status=0
    grep -aqF -e "$marker" "$file" || status=$?
    case "$status" in
      0) found+=("$marker") ;;
      1) ;;
      *) echo "$tag FAILED: grep exited $status on $label" >&2; exit 2 ;;
    esac
  done
  size="$(stat -c %s "$file")"
  if [ "${#found[@]}" -gt 0 ]; then
    echo "$tag FAILED: $label ($size bytes) contains test-clock markers: ${found[*]}" >&2
    failed=1
  else
    echo "$tag $label ($size bytes): no test-clock marker"
  fi
done

echo "$tag step 2/2: result"
if [ "$failed" -ne 0 ]; then
  echo "$tag FAILED: a release binary contains the test-clock feature" >&2
  exit 1
fi
echo "$tag OK: no release binary contains the test-clock feature"
