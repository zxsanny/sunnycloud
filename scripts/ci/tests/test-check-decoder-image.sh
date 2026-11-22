#!/usr/bin/env bash
# AZ-3864 unit tests: check-decoder-image.sh --recorded on tool outputs recorded from a good image
# (tests/fixtures/decoder-image-good, made by `check-decoder-image.sh --record DIR IMAGE`; the file list keeps only paths
# outside usr/lib, usr/share, var and etc, plus the sunnycloud documentation). The good record must pass; every seeded
# copy must exit 1 and name its failing property. Needs no Docker. The same properties on real seeded images:
# test-check-decoder-image-real.sh.
set -euo pipefail

tests_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
check="$tests_dir/../check-decoder-image.sh"
good="$tests_dir/fixtures/decoder-image-good"
tag="[test-check-decoder-image]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# case NAME PROPERTY MUTATION: copy the good record, apply the mutation in it, expect the property named
cases=0
seeded() {
  local name="$1" property="$2" mutation="$3" dir="$work/$1" out rc=0
  cp -r "$good" "$dir"
  (cd "$dir" && eval "$mutation")
  out="$("$check" --recorded "$dir" 2>&1)" || rc=$?
  if [[ "$rc" -ne 1 ]]; then echo "$out"; echo "$tag FAILED: '$name' exited $rc, expected 1" >&2; exit 1; fi
  if ! grep -q "FAIL $property:" <<<"$out"; then echo "$out"; echo "$tag FAILED: '$name' did not name $property" >&2; exit 1; fi
  echo "$tag '$name' fails with $property"
  cases=$((cases + 1))
}

# Arrange / Act / Assert for the good record
out="$("$check" --recorded "$good" 2>&1)" || { echo "$out"; echo "$tag FAILED: the good record was rejected" >&2; exit 1; }
grep -q 'OK: every property' <<<"$out" || { echo "$out"; echo "$tag FAILED: no OK line for the good record" >&2; exit 1; }
echo "$tag the good record passes"

seeded hls-demuxer ffmpeg-demuxers "echo ' D  hls             Apple HTTP Live Streaming' >>ffmpeg-demuxers"
seeded concat-demuxer ffmpeg-demuxers "echo ' D  concat          Virtual concatenation script' >>ffmpeg-demuxers"
seeded demuxer-outside-allowlist ffmpeg-demuxers "echo ' D  flv             FLV (Flash Video)' >>ffmpeg-demuxers"
seeded demuxer-lost ffmpeg-demuxers "sed -i '/mpegts/d' ffmpeg-demuxers"
seeded fdk-aac-encoder ffmpeg-encoders "echo ' A....D libfdk_aac           Fraunhofer FDK AAC' >>ffmpeg-encoders"
seeded x265-encoder ffmpeg-encoders "echo ' V....D libx265             libx265 H.265' >>ffmpeg-encoders"
seeded native-aac-lost ffmpeg-encoders "sed -i '/ aac /d' ffmpeg-encoders"
seeded nonfree-configuration ffmpeg-configuration "sed -i '/^configuration:/ s/\$/ --enable-nonfree/' ffmpeg-version"
seeded gpl-lost ffmpeg-configuration "sed -i '/^configuration:/ s/--enable-gpl//' ffmpeg-version"
seeded fdk-configuration ffmpeg-configuration "sed -i '/^configuration:/ s/\$/ --enable-libfdk-aac/' ffmpeg-version"
seeded ffmpeg-other-version ffmpeg-version "sed -i '1 s/version 9.1/version 9.0.2/' ffmpeg-version"
seeded http-protocol ffmpeg-protocols "sed -i 's/^  pipe\$/  pipe\n  http/' ffmpeg-protocols"
seeded svg-loader libvips-loaders "echo '          VipsForeignLoadSvgFile (svgload), load SVG with rsvg' >>vips-loaders"
seeded pdf-loader libvips-loaders "echo '          VipsForeignLoadPdfFile (pdfload_buffer), load PDF' >>vips-loaders"
seeded magick-loader libvips-loaders "echo '          VipsForeignLoadMagickFile (magickload), load file with ImageMagick' >>vips-loaders"
seeded heif-loader-lost libvips-loaders "sed -i '/(heifload)/d' vips-loaders"
seeded vips-other-version libvips-version "echo 'vips-8.18.6-Sun' >vips-version"
seeded libheif-too-old libheif-version "echo libheif.so.1.21.2 >libheif-libs"
seeded libheif-not-pinned libheif-version "echo libheif.so.1.22.0 >libheif-libs"
seeded sbom-removed sbom "rm sbom"
seeded sbom-without-cpe sbom "sed -i 's/cpe:2.3:a:struktur:libheif:1.23.7/none/' sbom"
seeded sbom-component-lost sbom "sed -i 's/\"name\": \"ffmpeg\"/\"name\": \"other\"/' sbom"
seeded build-facts-removed build-facts "rm build-facts"
seeded build-facts-future-time build-facts "sed -i 's/[0-9]\{10\}/4102444800/' build-facts"
seeded source-offer-removed source-offer "rm source-offer"
seeded source-offer-label-wrong source-offer "sed -i 's#^label.org.sunnycloud.source-offer=.*#label.org.sunnycloud.source-offer=https://example.org/elsewhere.zip#' image-config.txt"
seeded source-offer-notice-other-asset source-offer "sed -i 's/decoder-sources-/decoder-other-/g' source-offer"
seeded toolchain-absent media-toolchain-label "sed -i 's/media-toolchain=present/media-toolchain=absent/' image-config.txt"
seeded test-stub-file no-test-hooks "echo usr/local/bin/decoder-test-stub >>files.txt"
seeded real-file no-test-hooks "echo opt/media/bin/ffmpeg.real >>files.txt"
seeded tool-hash-differs no-test-hooks "sed -i 's/^ffmpeg: OK/ffmpeg: FAILED/' tools-manifest"
seeded runner-marker no-test-hooks "echo SCSTUB1 >runner-strings-hits"
seeded runner-symbol no-test-hooks "echo 'T decoder_runner::test_freeze::install' >runner-nm-hits"
seeded test-stubs-label no-test-hooks "echo label.org.sunnycloud.variant=test-stubs >>image-config.txt"
seeded heic-not-hevc heic-hevc-decode "sed -i 's/^hevc/av1/' heic-decode"
seeded heic-wrong-colours heic-hevc-decode "sed -i 's/^200 30 30/0 0 0/' heic-decode"
seeded heic-failed heic-hevc-decode "echo 'Unsupported feature: Unsupported codec' >heic-decode.failed"
seeded tool-did-not-run ffmpeg-encoders "echo 'exec: ffmpeg: not found' >ffmpeg-encoders.failed"
seeded user-root runtime-user "sed -i 's/^user=.*/user=/' image-config.txt"
seeded other-entrypoint runtime-entrypoint "sed -i 's/^entrypoint=.*/entrypoint=[\"sh\"]/' image-config.txt"
seeded too-big image-size "echo 700000001 >unpacked-bytes"
seeded compiler-present no-compiler-or-source "echo usr/bin/gcc >>files.txt"
seeded source-tree-present no-compiler-or-source "echo build/ffmpeg/configure >>files.txt"
seeded headers-present no-compiler-or-source "echo opt/media/include/libheif/heif.h >>files.txt"

echo "$tag OK: the good record passes and $cases seeded records each fail with their property named"
