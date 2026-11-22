#!/usr/bin/env bash
# AZ-3864 AC-1 and AC-2 on a built decoder image. Needs Docker and the image (the `images` job has it loaded; the 25-minute
# build is not repeated): the good image must pass, and each seeded image, made by `docker build` of a tiny layer on top of
# it, must fail with the property named. A seeded layer replaces one tool by a wrapper that runs the real tool and edits
# its listing, so the check reads the same output shape a real regression would produce.
# usage: test-check-decoder-image-real.sh IMAGE
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-check-decoder-image-real]"
image="${1:?usage: $0 IMAGE}"
work="$(mktemp -d)"
built=()
cleanup() {
  local seeded
  for seeded in ${built[@]+"${built[@]}"}; do docker image rm --force "$seeded" >/dev/null; done
  rm -rf "$work"
}
trap cleanup EXIT

echo "$tag the good image passes"
"$ci_dir/check-decoder-image.sh" "$image"

# wrapper TOOL TRIGGER ACTION: a shell script that runs the real tool (kept as TOOL-orig) and, when the arguments
# contain TRIGGER, either appends a line (ACTION "append:<line>") or prints a replacement first line ("replace:<line>")
# shellcheck disable=SC2028  # the backslash-n is written literally into the wrapper script
write_wrapper() {
  local file="$1" tool="$2" trigger="$3" action="$4"
  {
    echo '#!/bin/sh'
    # the container filesystem is read-only, so the real output is kept in a variable, not a file
    echo "out=\"\$(/opt/media/bin/$tool-orig \"\$@\")\"; rc=\$?"
    echo "case \"\$*\" in"
    echo "  *$trigger*)"
    case "$action" in
      append:*) echo "    printf '%s\\n' \"\$out\"; echo '${action#append:}' ;;" ;;
      replace:*) echo "    echo '${action#replace:}' ;;" ;;
    esac
    echo "  *) printf '%s\\n' \"\$out\" ;;"
    echo "esac"
    # shellcheck disable=SC2016  # the script being written expands it
    echo 'exit $rc'
  } >"$file"
}

# seed NAME EXPECTED_PROPERTY DOCKERFILE_BODY: build the layer, run the check, expect exit 1 and the property named
seed() {
  local name="$1" property="$2" body="$3" out rc=0
  local ref="sunnycloud-decoder-seeded:$name"
  printf 'FROM %s\nUSER root\n%s\nUSER 10003:10003\n' "$image" "$body" >"$work/Dockerfile"
  docker build --quiet --tag "$ref" --file "$work/Dockerfile" "$work" >/dev/null
  built+=("$ref")
  out="$("$ci_dir/check-decoder-image.sh" "$ref" 2>&1)" || rc=$?
  if [[ "$rc" -ne 1 ]]; then echo "$out"; echo "$tag FAILED: seeded '$name' exited $rc, expected 1" >&2; exit 1; fi
  if ! grep -q "FAIL $property:" <<<"$out"; then echo "$out"; echo "$tag FAILED: seeded '$name' did not name $property" >&2; exit 1; fi
  echo "$tag seeded '$name' fails with $property"
}

wrap_layer() { # TOOL: the Dockerfile lines that put $work/wrapper-TOOL in front of the real tool
  printf 'RUN mv /opt/media/bin/%s /opt/media/bin/%s-orig\nCOPY wrapper-%s /opt/media/bin/%s\nRUN chmod 755 /opt/media/bin/%s\n' "$1" "$1" "$1" "$1" "$1"
}

write_wrapper "$work/wrapper-ffmpeg" ffmpeg -demuxers 'append: D  hls             Apple HTTP Live Streaming'
seed hls-demuxer ffmpeg-demuxers "$(wrap_layer ffmpeg)"

write_wrapper "$work/wrapper-ffmpeg" ffmpeg -encoders 'append: A....D libfdk_aac           Fraunhofer FDK AAC (codec aac)'
seed fdk-aac ffmpeg-encoders "$(wrap_layer ffmpeg)"

write_wrapper "$work/wrapper-vips" vips -l 'append:      VipsForeignLoadSvgFile (svgload), load svg from file'
seed svg-loader libvips-loaders "$(wrap_layer vips)"

write_wrapper "$work/wrapper-vips" vips --version 'replace:vips-8.18.6-seeded'
seed vips-version libvips-version "$(wrap_layer vips)"

seed sbom-removed sbom 'RUN rm /usr/share/sunnycloud/components.cdx.json'

seed old-libheif libheif-version 'RUN cd /opt/media/lib && mv libheif.so.1.* libheif.so.1.21.2'

seed source-offer-label source-offer 'LABEL org.sunnycloud.source-offer=""'

seed test-stub no-test-hooks 'RUN touch /usr/local/bin/decoder-test-stub'

echo "$tag OK: the good image passes and each of 8 seeded images fails with its property named"
