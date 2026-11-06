#!/usr/bin/env bash
# AC-2 (users and labels) and AC-3 (layers) on built images.
# usage: check-images.sh API WORKER DECODER API_TEST WORKER_TEST   (image references)
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
if [[ $# -ne 5 ]]; then echo "usage: $0 API WORKER DECODER API_TEST WORKER_TEST" >&2; exit 2; fi
API="$1"; WORKER="$2"; DECODER="$3"; API_TEST="$4"; WORKER_TEST="$5"

inspect() { docker image inspect --format "$2" "$1"; }
check() { # image, label, want
  local got
  got="$(inspect "$1" "$2")"
  if [[ "$got" == "$3" ]]; then pass "$1 $2 = $3"; else fail "$1 $2 = '$got', want '$3'"; fi
}
check_nonempty() {
  local got
  got="$(inspect "$1" "$2")"
  if [[ -n "$got" && "$got" != "<no value>" ]]; then pass "$1 $2 = $got"; else fail "$1 $2 is empty"; fi
}

check "$API" '{{.Config.User}}' 10001:10001
check "$API_TEST" '{{.Config.User}}' 10001:10001
check "$WORKER" '{{.Config.User}}' 10002:10002
check "$WORKER_TEST" '{{.Config.User}}' 10002:10002
check "$DECODER" '{{.Config.User}}' 10003:10003

for img in "$API" "$WORKER" "$DECODER" "$API_TEST" "$WORKER_TEST"; do
  check_nonempty "$img" '{{index .Config.Labels "org.opencontainers.image.source"}}'
  check_nonempty "$img" '{{index .Config.Labels "org.opencontainers.image.version"}}'
  check_nonempty "$img" '{{index .Config.Labels "org.opencontainers.image.revision"}}'
  check "$img" '{{index .Config.Labels "org.opencontainers.image.licenses"}}' AGPL-3.0-only
done
for img in "$API" "$WORKER" "$DECODER"; do check "$img" '{{index .Config.Labels "org.sunnycloud.test-clock"}}' false; done
for img in "$API_TEST" "$WORKER_TEST"; do check "$img" '{{index .Config.Labels "org.sunnycloud.test-clock"}}' true; done

# declared health checks (AC-6, image view)
check "$API" '{{json .Config.Healthcheck.Test}}' '["CMD","sunnycloud","healthcheck"]'
check "$WORKER" '{{json .Config.Healthcheck.Test}}' '["CMD","sunnycloud-worker","healthcheck"]'
check "$DECODER" '{{json .Config.Healthcheck.Test}}' '["CMD","decoder-runner","health"]'
check "$API" '{{json .Config.Entrypoint}}{{json .Config.Cmd}}' '["sunnycloud"]["serve"]'

# AZ-3868 AC-5 (image part): the worker holds a bearer only, no database, blob or media library
for img in "$WORKER" "$WORKER_TEST"; do
  size="$(inspect "$img" '{{.Size}}')"
  if (( size < 50000000 )); then pass "$img size $size < 50 MB"; else fail "$img size $size is not under 50 MB"; fi
  env_json="$(inspect "$img" '{{json .Config.Env}}')"
  if grep -Eiq 'DB_|DATABASE|POSTGRES|PGPASSWORD|MASTER_KEY' <<<"$env_json"; then fail "$img environment names a database or key variable: $env_json"; else pass "$img environment has no database or key variable"; fi
  cid="$(docker create "$img")"
  files="$(docker export "$cid" | tar -t)"
  docker rm "$cid" >/dev/null
  if grep -Eiq '(^|/)(ffmpeg|ffprobe|vips[^/]*|libvips[^/]*|libheif[^/]*|libav[^/]*)$' <<<"$files"; then fail "$img holds an ffmpeg or vips binary or library"; else pass "$img holds no ffmpeg or vips binary"; fi
done

# AC-3: no planted or local file in any layer
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
docker save -o "$tmp/images.tar" "$API" "$WORKER" "$DECODER" "$API_TEST" "$WORKER_TEST"
if python3 -I "$(dirname "${BASH_SOURCE[0]}")/scan_layers.py" "$tmp/images.tar"; then pass "no planted file or marker in any layer"; else fail "layer scan"; fi
finish
