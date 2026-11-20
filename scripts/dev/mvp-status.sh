#!/usr/bin/env bash
# Shows each part of the MVP stack and its three health URLs. Exit 1 when anything is not healthy.
set -euo pipefail
# shellcheck source=scripts/dev/mvp-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/mvp-lib.sh"
load_env

unhealthy=0
echo "== docker"
compose ps
for service in db caddy; do
  state="$(compose ps --format '{{.State}}' "$service")"
  [ "$state" = running ] || unhealthy=1
done

echo "== host processes"
for name in "${PROCESSES[@]}"; do
  pid="$(running_pid "$name")"
  if [ -n "$pid" ]; then
    printf '%-22s running pid %s since %s\n' "$name" "$pid" "$(ps -o lstart= -p "$pid")"
  else
    printf '%-22s NOT RUNNING\n' "$name"
    unhealthy=1
  fi
done

echo "== health"
probe() {
  local label="$1" url="$2" option="${3:-}" code
  code="$(curl -s ${option:+"$option"} --max-time 5 -o /dev/null -w '%{http_code}' "$url" || true)"
  printf '%-22s %s %s\n' "$label" "${code:-000}" "$url"
  [ "$code" = 200 ] || unhealthy=1
}
probe "api ops /readyz" "http://127.0.0.1:9090/readyz"
probe "worker ops /readyz" "http://127.0.0.1:9092/readyz"
probe "public server-info" "https://$PUBLIC_HOST/api/v1/server-info" "--insecure"

free_gib=$(( $(df --output=avail -B1G "$MVP_DATA" | tail -n 1) ))
echo "== disk"
echo "free on $MVP_DATA: ${free_gib} GiB"
exit "$unhealthy"
