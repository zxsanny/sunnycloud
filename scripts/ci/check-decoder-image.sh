#!/usr/bin/env bash
# Build checks of the decoder image (AZ-3864; containerization.md "Build checks that run in CI on every build").
# Fails with the property named when the image gains an excluded loader, protocol, demuxer or encoder, loses a
# required one, drifts from deploy/docker/versions.env, lacks its SBOM, source offer or build facts, carries a test
# hook, cannot decode an HEVC HEIC, or breaks the user, entrypoint, size or no-compiler rules.
# The check needs no network: the image is run with --network none and only read.
#
# usage: check-decoder-image.sh IMAGE             collect the tool outputs from IMAGE, then verify them
#        check-decoder-image.sh --record DIR IMAGE   only collect the outputs into DIR (the files the unit tests replay)
#        check-decoder-image.sh --recorded DIR        verify outputs recorded earlier (no Docker)
# VERSIONS_ENV overrides the pin list (default deploy/docker/versions.env).
#
# The HEIC sample scripts/ci/fixtures/hevc-64x48-two-colours.heic.hex (64x48, left half RGB 200 30 30, right half
# 30 30 200, HEVC) was made once with: heif-enc -q 90 two.png -o two.heic (libheif 1.21 with the x265 plugin).
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
versions_env="${VERSIONS_ENV:-$repo_root/deploy/docker/versions.env}"
sample_hex="$ci_dir/fixtures/hevc-64x48-two-colours.heic.hex"
tag="[check-decoder-image]"

LIBHEIF_MIN=1.22.0
MAX_SIZE_BYTES=$((700 * 1000 * 1000))
FORBIDDEN_DEMUXERS="hls dash concat tee data"
FORBIDDEN_CONFIG="--enable-nonfree --enable-libfdk-aac --enable-libx265 --enable-cuda --enable-libnpp"
# nicknames as `vips -l` prints them, in parentheses (a loader may also have _buffer and _source variants)
FORBIDDEN_LOADERS="svgload pdfload magickload openslideload fitsload niftiload matload openexrload"
REQUIRED_LOADERS="jpegload pngload webpload heifload gifload"

# ---------------------------------------------------------------- collect

# in_image NAME CMD...: run a command of the image without network and keep its output as DIR/NAME;
# a command that cannot run leaves DIR/NAME.failed with the reason (the property that needs it then fails).
in_image() {
  local name="$1"
  shift
  if ! docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
    --entrypoint "$1" "$image" "${@:2}" >"$dir/$name" 2>"$dir/$name.err"; then
    mv "$dir/$name.err" "$dir/$name.failed"
    return 0
  fi
  rm -f "$dir/$name.err"
}

collect() {
  local container
  echo "$tag collecting from $image"
  in_image ffmpeg-version ffmpeg -hide_banner -version
  in_image ffmpeg-encoders ffmpeg -hide_banner -encoders
  in_image ffmpeg-protocols ffmpeg -hide_banner -protocols
  in_image ffmpeg-demuxers ffmpeg -hide_banner -demuxers
  in_image vips-version vips --version
  in_image vips-loaders vips -l
  in_image libheif-libs sh -c 'ls /opt/media/lib'
  in_image sbom cat /usr/share/sunnycloud/components.cdx.json
  in_image build-facts cat /usr/share/sunnycloud/build-facts.json
  in_image source-offer cat /usr/share/doc/sunnycloud-decoder/SOURCE-OFFER.txt
  in_image tools-manifest sh -c 'cd /opt/media/bin && sha256sum --check /usr/share/sunnycloud/media-tools.sha256'
  docker image inspect "$image" >"$dir/inspect.json"
  python3 -I "$repo_root/scripts/ci/decoder_image_inspect.py" "$dir/inspect.json" >"$dir/image-config.txt"

  # the HEIC sample goes in through stdin; the tools write only to the tmpfs
  python3 -I -c 'import sys; sys.stdout.buffer.write(bytes.fromhex(sys.stdin.read().strip()))' <"$sample_hex" >"$dir/sample.heic"
  if ! docker run --rm -i --network none --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges \
    --entrypoint sh "$image" -c 'cat >/tmp/s.heic && vipsheader -f heif-compression /tmp/s.heic \
      && vipsheader -f width /tmp/s.heic && vipsheader -f height /tmp/s.heic \
      && vips getpoint /tmp/s.heic 8 24 && vips getpoint /tmp/s.heic 56 24' \
    <"$dir/sample.heic" >"$dir/heic-decode" 2>"$dir/heic-decode.failed"; then
    :
  else
    rm -f "$dir/heic-decode.failed"
  fi

  container="$(docker create "$image")"
  docker export "$container" >"$dir/export.tar"
  tar -t <"$dir/export.tar" >"$dir/files.txt"
  stat -c %s "$dir/export.tar" >"$dir/unpacked-bytes"
  rm -f "$dir/export.tar"
  docker cp "$container:/usr/local/bin/decoder-runner" "$dir/decoder-runner" >/dev/null
  docker rm "$container" >/dev/null
  { strings -a "$dir/decoder-runner" | grep -E 'SCSTUB1|test_freeze_main_loop' || true; } >"$dir/runner-strings-hits"
  { nm -a "$dir/decoder-runner" 2>/dev/null | grep -i 'test_freeze' || true; } >"$dir/runner-nm-hits"
  rm -f "$dir/decoder-runner" "$dir/sample.heic"
}

# ---------------------------------------------------------------- verify

failures=0
fail() { # PROPERTY DETAIL
  echo "$tag FAIL $1: $2" >&2
  failures=$((failures + 1))
}
ok() { echo "$tag ok   $1"; }

# have NAME: the recorded output exists and the command behind it ran
have() {
  [[ -f "$dir/$1" && ! -e "$dir/$1.failed" ]]
}

pin() { # KEY of versions.env
  local value
  value="$(sed -n "s/^$1=//p" "$versions_env")"
  [[ -n "$value" ]] || { echo "$tag $1 is missing from $versions_env" >&2; exit 2; }
  printf '%s' "$value"
}

version_ge() { # A B: A >= B
  [[ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" == "$2" ]]
}

# conclude PROPERTY: pass when the property has recorded no failure since `before`
conclude() {
  if [[ "$failures" -eq "$before" ]]; then ok "$1"; fi
}

check_ffmpeg_configuration() {
  local before=$failures want line flag
  have ffmpeg-version || { fail ffmpeg-configuration "ffmpeg -version did not run in the image"; return; }
  line="$(grep '^configuration:' "$dir/ffmpeg-version" || true)"
  [[ -n "$line" ]] || { fail ffmpeg-configuration "ffmpeg -version prints no configuration line"; return; }
  for flag in --enable-gpl --enable-libx264 --enable-libzimg --enable-libdav1d; do
    grep -qE -- "(^| )${flag}( |\$)" <<<"$line" || fail ffmpeg-configuration "configuration lacks $flag"
  done
  for flag in $FORBIDDEN_CONFIG; do
    if grep -qE -- "(^| )${flag}( |\$)" <<<"$line"; then fail ffmpeg-configuration "configuration has $flag"; fi
  done
  want="$(pin FFMPEG_VERSION)"
  if ! grep -qE "^ffmpeg version ${want//./\\.}( |\$)" "$dir/ffmpeg-version"; then
    fail ffmpeg-version "ffmpeg is not the pinned $want: $(head -n1 "$dir/ffmpeg-version")"
  fi
  conclude ffmpeg-configuration
}

# names of the table rows of `ffmpeg -encoders|-demuxers` (the second column, comma lists split)
table_names() {
  awk 'found && NF >= 2 { n = split($2, names, ","); for (i = 1; i <= n; i++) print names[i] } /^ *-+ *$/ { found = 1 }' "$1"
}

check_ffmpeg_encoders() {
  local before=$failures names name
  have ffmpeg-encoders || { fail ffmpeg-encoders "ffmpeg -encoders did not run in the image"; return; }
  names="$(table_names "$dir/ffmpeg-encoders")"
  for name in aac libx264; do
    grep -qx "$name" <<<"$names" || fail ffmpeg-encoders "encoder $name is missing"
  done
  for name in libfdk_aac libx265; do
    if grep -qx "$name" <<<"$names"; then fail ffmpeg-encoders "encoder $name is present"; fi
  done
  conclude ffmpeg-encoders
}

check_ffmpeg_protocols() {
  local before=$failures names extra
  have ffmpeg-protocols || { fail ffmpeg-protocols "ffmpeg -protocols did not run in the image"; return; }
  names="$(awk 'NF == 1 && $1 !~ /:$/ { print $1 }' "$dir/ffmpeg-protocols" | sort -u | tr '\n' ' ')"
  if [[ "$names" != "file pipe " ]]; then
    extra="$(tr ' ' ',' <<<"${names% }")"
    fail ffmpeg-protocols "protocols are '${extra}', want exactly file,pipe"
  fi
  conclude ffmpeg-protocols
}

check_ffmpeg_demuxers() {
  local before=$failures first name allowed
  have ffmpeg-demuxers || { fail ffmpeg-demuxers "ffmpeg -demuxers did not run in the image"; return; }
  allowed=",$(pin FFMPEG_DEMUXERS),"
  # every row must start with an allowlisted demuxer, and none of the excluded names may appear anywhere in a row
  while read -r first; do
    [[ "$allowed" == *",$first,"* ]] || fail ffmpeg-demuxers "demuxer $first is not in the allowlist $(pin FFMPEG_DEMUXERS)"
  done < <(awk 'found && NF >= 2 { split($2, names, ","); print names[1] } /^ *-+ *$/ { found = 1 }' "$dir/ffmpeg-demuxers")
  for name in $FORBIDDEN_DEMUXERS; do
    if table_names "$dir/ffmpeg-demuxers" | grep -qx "$name"; then fail ffmpeg-demuxers "demuxer $name is present"; fi
  done
  for name in $(tr ',' ' ' <<<"$(pin FFMPEG_DEMUXERS)"); do
    table_names "$dir/ffmpeg-demuxers" | grep -qx "$name" || fail ffmpeg-demuxers "allowlisted demuxer $name is missing"
  done
  conclude ffmpeg-demuxers
}

check_libheif() {
  local before=$failures file version
  have libheif-libs || { fail libheif-version "listing /opt/media/lib failed"; return; }
  file="$(grep -E '^libheif\.so\.[0-9]+\.[0-9]+\.[0-9]+$' "$dir/libheif-libs" | head -n1 || true)"
  if [[ -z "$file" ]]; then fail libheif-version "no libheif.so.<major>.<minor>.<patch> in /opt/media/lib"; return; fi
  # libheif names its library after its own version: libheif.so.1.23.7 is libheif 1.23.7
  version="$(sed -E 's/^libheif\.so\.//' <<<"$file")"
  version_ge "$version" "$LIBHEIF_MIN" || fail libheif-version "libheif $version is older than $LIBHEIF_MIN"
  [[ "$version" == "$(pin LIBHEIF_VERSION)" ]] || fail libheif-version "libheif $version is not the pinned $(pin LIBHEIF_VERSION)"
  conclude libheif-version
}

check_libvips() {
  local before=$failures want name
  have vips-version || { fail libvips-version "vips --version did not run in the image"; return; }
  want="$(pin VIPS_VERSION)"
  if ! grep -qE "^vips-${want//./\\.}(-|\$)" "$dir/vips-version"; then
    fail libvips-version "libvips is not exactly $want: $(head -n1 "$dir/vips-version")"
  fi
  conclude libvips-version
  before=$failures
  have vips-loaders || { fail libvips-loaders "vips -l did not run in the image"; return; }
  for name in $REQUIRED_LOADERS; do
    grep -qE "\\(${name}\\)" "$dir/vips-loaders" || fail libvips-loaders "loader $name is missing"
  done
  for name in $FORBIDDEN_LOADERS; do
    if grep -qE "\\(${name}(_[a-z]+)?\\)" "$dir/vips-loaders"; then fail libvips-loaders "loader $name is present"; fi
  done
  conclude libvips-loaders
}

check_sbom() {
  local before=$failures reason
  have sbom || { fail sbom "/usr/share/sunnycloud/components.cdx.json is missing"; return; }
  if ! reason="$(python3 -I "$ci_dir/decoder_image_inspect.py" --sbom "$dir/sbom" \
    "libheif=$(pin LIBHEIF_VERSION)" "libvips=$(pin VIPS_VERSION)" "ffmpeg=$(pin FFMPEG_VERSION)" 2>&1)"; then
    fail sbom "$reason"
  fi
  conclude sbom
}

check_build_facts() {
  local before=$failures reason
  have build-facts || { fail build-facts "/usr/share/sunnycloud/build-facts.json is missing"; return; }
  if ! reason="$(python3 -I "$ci_dir/decoder_image_inspect.py" --facts "$dir/build-facts" \
    "libheif=$(pin LIBHEIF_VERSION)" "libvips=$(pin VIPS_VERSION)" "ffmpeg=$(pin FFMPEG_VERSION)" 2>&1)"; then
    fail build-facts "$reason"
  fi
  conclude build-facts
}

# image-config.txt holds one `key=value` per line (decoder_image_inspect.py)
config_value() { sed -n "s/^$1=//p" "$dir/image-config.txt" | head -n1; }

check_source_offer() {
  local before=$failures label name
  label="$(config_value label.org.sunnycloud.source-offer)"
  if [[ -z "$label" ]]; then
    fail source-offer "label org.sunnycloud.source-offer is missing"
  elif ! grep -qE '^https://.+/decoder-sources-.+-[0-9]{8}\.tar\.zst$' <<<"$label"; then
    fail source-offer "label org.sunnycloud.source-offer '$label' is not a decoder-sources-<version>-<yyyymmdd>.tar.zst asset"
  fi
  if ! have source-offer; then
    fail source-offer "/usr/share/doc/sunnycloud-decoder/SOURCE-OFFER.txt is missing"
  else
    name="${label##*/}"
    if [[ -n "$label" ]] && ! grep -qF "$label" "$dir/source-offer"; then
      fail source-offer "SOURCE-OFFER.txt does not name the asset of the label ($name)"
    fi
    grep -q 'snapshot.debian.org' "$dir/source-offer" || fail source-offer "SOURCE-OFFER.txt names no snapshot.debian.org source packages"
    grep -qi 'GNU General Public License' "$dir/source-offer" || fail source-offer "SOURCE-OFFER.txt has no GPL notice"
  fi
  conclude source-offer
}

check_labels() {
  local before=$failures
  [[ "$(config_value label.org.sunnycloud.media-toolchain)" == "present" ]] \
    || fail media-toolchain-label "label org.sunnycloud.media-toolchain is '$(config_value label.org.sunnycloud.media-toolchain)', want present"
  conclude media-toolchain-label
}

check_no_test_hooks() {
  local before=$failures
  if grep -E '(^|/)decoder-test-stub$|\.real$' "$dir/files.txt" >"$dir/hook-files"; then
    fail no-test-hooks "test files in the image: $(tr '\n' ' ' <"$dir/hook-files")"
  fi
  if have tools-manifest; then
    if grep -qvE ': OK$' "$dir/tools-manifest"; then fail no-test-hooks "media tool hashes differ from the build manifest: $(grep -v ': OK$' "$dir/tools-manifest" | tr '\n' ' ')"; fi
  else
    fail no-test-hooks "the media tool hash manifest /usr/share/sunnycloud/media-tools.sha256 is missing or does not verify"
  fi
  [[ ! -s "$dir/runner-strings-hits" ]] || fail no-test-hooks "decoder-runner contains $(tr '\n' ' ' <"$dir/runner-strings-hits")"
  [[ ! -s "$dir/runner-nm-hits" ]] || fail no-test-hooks "decoder-runner has test_freeze symbols"
  [[ "$(config_value label.org.sunnycloud.variant)" != "test-stubs" ]] || fail no-test-hooks "label org.sunnycloud.variant=test-stubs is set"
  conclude no-test-hooks
}

check_heic() {
  local before=$failures out
  if [[ ! -f "$dir/heic-decode" || -e "$dir/heic-decode.failed" ]]; then
    fail heic-hevc-decode "the generated HEVC HEIC does not decode: $(head -c 300 "$dir/heic-decode.failed" 2>/dev/null || echo 'no output')"
    return
  fi
  out="$(tr '\n' ' ' <"$dir/heic-decode")"
  # compression, width, height, then two pixels (lossy YUV: each channel within 12 of the source)
  python3 -I - "$out" <<'PY' || fail heic-hevc-decode "decoded sample is wrong: $out"
import sys
v = sys.argv[1].split()
assert v[0] == "hevc" and v[1] == "64" and v[2] == "48", v
left, right = [int(x) for x in v[3:6]], [int(x) for x in v[6:9]]
assert all(abs(a - b) <= 12 for a, b in zip(left, (200, 30, 30))), left
assert all(abs(a - b) <= 12 for a, b in zip(right, (30, 30, 200))), right
PY
  conclude heic-hevc-decode
}

check_runtime_shape() {
  local before=$failures size
  [[ "$(config_value user)" == "10003:10003" ]] || fail runtime-user "image user is '$(config_value user)', want 10003:10003"
  [[ "$(config_value entrypoint)" == '["decoder-runner"]' ]] || fail runtime-entrypoint "entrypoint is '$(config_value entrypoint)', want [\"decoder-runner\"]"
  # the size of the unpacked file tree (the registry size is smaller: layers are compressed)
  size="$(cat "$dir/unpacked-bytes" 2>/dev/null || true)"
  if [[ -z "$size" ]] || (( size >= MAX_SIZE_BYTES )); then fail image-size "image size '$size' bytes is not under $MAX_SIZE_BYTES"; fi
  if grep -E '(^|/)(gcc|g\+\+|cc|c\+\+|cpp|make|cmake|meson|ninja|rustc|cargo)(-[0-9.]+)?$' "$dir/files.txt" >"$dir/compiler-files"; then
    fail no-compiler-or-source "compiler or build tool in the image: $(head -n3 "$dir/compiler-files" | tr '\n' ' ')"
  fi
  if grep -E '^(build|usr/src|opt/media/include)/.|\.(h|hpp|c|cc|cpp|rs)$' "$dir/files.txt" >"$dir/source-files"; then
    fail no-compiler-or-source "source or header files in the image: $(head -n3 "$dir/source-files" | tr '\n' ' ')"
  fi
  conclude runtime-shape
}

verify() {
  check_ffmpeg_configuration
  check_ffmpeg_encoders
  check_ffmpeg_protocols
  check_ffmpeg_demuxers
  check_libheif
  check_libvips
  check_sbom
  check_build_facts
  check_source_offer
  check_labels
  check_no_test_hooks
  check_heic
  check_runtime_shape
  if [[ "$failures" -ne 0 ]]; then
    echo "$tag FAILED: $failures property failure(s)" >&2
    exit 1
  fi
  echo "$tag OK: every property of the decoder image holds"
}

# ---------------------------------------------------------------- main

mode=check
case "${1:-}" in
  --record) mode=record; dir="${2:?usage: --record DIR IMAGE}"; image="${3:?usage: --record DIR IMAGE}" ;;
  --recorded) mode=recorded; dir="${2:?usage: --recorded DIR}" ;;
  -*|'') echo "usage: $0 IMAGE | --record DIR IMAGE | --recorded DIR" >&2; exit 2 ;;
  *) image="$1"; dir="$(mktemp -d)"; trap 'rm -rf "$dir"' EXIT ;;
esac

case "$mode" in
  record) mkdir -p "$dir"; collect ;;
  recorded) [[ -d "$dir" ]] || { echo "$tag $dir is not a directory" >&2; exit 2; }; verify ;;
  check) collect; verify ;;
esac
