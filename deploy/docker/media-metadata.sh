#!/bin/sh
# Writes the decoder image's component SBOM, build facts, GPL source offer and tool hash manifest (AZ-3863).
# Runs once in the runtime stage of decoder.Dockerfile, after /opt/media and the Debian runtime libraries are in place.
# Env: VERSION, BUILD_TIMESTAMP (Unix seconds), BUILD_DATE (yyyymmdd of that timestamp), SOURCE_OFFER_URL.
# Args: path of versions.env, then the Debian runtime packages whose source packages the offer names.
set -eu

versions="$1"
shift
# shellcheck source=/dev/null  # versions.env is the one given as argument
. "$versions"

: "${VERSION:?}" "${BUILD_TIMESTAMP:?}" "${BUILD_DATE:?}" "${SOURCE_OFFER_URL:?}"
case "$BUILD_TIMESTAMP" in '' | *[!0-9]*) echo "media-metadata: BUILD_TIMESTAMP '$BUILD_TIMESTAMP' is not Unix seconds" >&2; exit 1 ;; esac
if [ "$(date -u -d "@$BUILD_TIMESTAMP" +%Y%m%d)" != "$BUILD_DATE" ]; then
  echo "media-metadata: BUILD_DATE $BUILD_DATE is not the UTC date of BUILD_TIMESTAMP $BUILD_TIMESTAMP" >&2
  exit 1
fi
built_at="$(date -u -d "@$BUILD_TIMESTAMP" +%Y-%m-%dT%H:%M:%SZ)"

share=/usr/share/sunnycloud
doc=/usr/share/doc/sunnycloud-decoder
mkdir -p "$share" "$doc"

component() { # name version cpe-vendor cpe-product purl licence sha256 url
  printf '    {"type": "library", "name": "%s", "version": "%s",\n' "$1" "$2"
  printf '     "cpe": "cpe:2.3:a:%s:%s:%s:*:*:*:*:*:*:*",\n' "$3" "$4" "$2"
  printf '     "purl": "%s", "licenses": [{"license": {"id": "%s"}}],\n' "$5" "$6"
  printf '     "hashes": [{"alg": "SHA-256", "content": "%s"}],\n' "$7"
  printf '     "externalReferences": [{"type": "distribution", "url": "%s"}]}' "$8"
}

{
  printf '{\n  "bomFormat": "CycloneDX",\n  "specVersion": "1.5",\n  "version": 1,\n'
  printf '  "metadata": {"timestamp": "%s", "component": {"type": "container", "name": "sunnycloud-decoder", "version": "%s"}},\n' "$built_at" "$VERSION"
  printf '  "components": [\n'
  component libheif "$LIBHEIF_VERSION" struktur libheif "pkg:github/strukturag/libheif@v$LIBHEIF_VERSION" LGPL-3.0-or-later "$LIBHEIF_SHA256" "$LIBHEIF_URL"
  printf ',\n'
  component libvips "$VIPS_VERSION" libvips libvips "pkg:github/libvips/libvips@v$VIPS_VERSION" LGPL-2.1-or-later "$VIPS_SHA256" "$VIPS_URL"
  printf ',\n'
  component ffmpeg "$FFMPEG_VERSION" ffmpeg ffmpeg "pkg:generic/ffmpeg@$FFMPEG_VERSION" GPL-2.0-or-later "$FFMPEG_SHA256" "$FFMPEG_URL"
  printf '\n  ]\n}\n'
} >"$share/components.cdx.json"

printf '{"libheif": "%s", "libvips": "%s", "ffmpeg": "%s", "build_timestamp": %s}\n' \
  "$LIBHEIF_VERSION" "$VIPS_VERSION" "$FFMPEG_VERSION" "$BUILD_TIMESTAMP" >"$share/build-facts.json"

(cd /opt/media/bin && sha256sum ffmpeg ffprobe vips vipsheader vipsthumbnail) >"$share/media-tools.sha256"

{
  cat <<TXT
SunnyCloud decoder image: offer of corresponding source code

This image contains free software under the GNU General Public License: ffmpeg $FFMPEG_VERSION is built
with --enable-gpl and links x264, so the whole ffmpeg binary is licensed GPL-2.0-or-later. libvips $VIPS_VERSION
(LGPL-2.1-or-later) and libheif $LIBHEIF_VERSION (LGPL-3.0-or-later) are built from source as well.

The complete corresponding source of exactly this build (the three source tarballs, the patches if any, and the
build recipe: decoder.Dockerfile, versions.env, ffmpeg.configure) is the release asset

  ${SOURCE_OFFER_URL##*/}

at

  $SOURCE_OFFER_URL

Image version $VERSION, built $built_at. The same address is in the image label org.sunnycloud.source-offer.

Debian packages in this image (libx264 and the other runtime libraries) come from the Debian project; their exact
source packages are at snapshot.debian.org:

TXT
  for package in "$@"; do
    dpkg-query -W -f='${source:Package} ${source:Version}\n' "$package" | while read -r source version; do
      printf '  %s %s: https://snapshot.debian.org/package/%s/%s/\n' "$source" "$version" "$source" "${version#*:}"
    done
  done
  printf '\nThe HEVC video codec is covered by patents in some countries; libde265 decodes it for iPhone photos only.\n'
} >"$doc/SOURCE-OFFER.txt"
