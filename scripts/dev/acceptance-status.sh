#!/usr/bin/env bash
# Shows each part of the acceptance stack and its health URLs. Exit 1 when anything is not healthy.
set -euo pipefail
# shellcheck source=acceptance-lib.sh
# shellcheck source=acceptance-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/acceptance-lib.sh"
require_systemd_user

unhealthy=0
echo "== database container"
if db_running; then
  docker ps --filter "name=^$ACC_DB_CONTAINER\$" --format '{{.Names}} {{.Status}} {{.Ports}}'
else
  echo "$ACC_DB_CONTAINER NOT RUNNING"
  unhealthy=1
fi

echo "== units (systemd --user)"
for name in "${PROCESSES[@]}" idp web; do
  if unit_active "$name"; then
    printf '%-22s active pid %-8s restarts %-4s since %s\n' "$name" "$(unit_property "$name" MainPID)" \
      "$(unit_property "$name" NRestarts)" "$(unit_property "$name" ActiveEnterTimestamp)"
  else
    printf '%-22s NOT RUNNING (%s)\n' "$name" "$(unit_property "$name" ActiveState)"
    unhealthy=1
  fi
done

echo "== health"
probe() {
  local label="$1" url="$2" code
  code="$(curl -s --max-time 5 -o /dev/null -w '%{http_code}' "$url" || true)"
  printf '%-26s %s %s\n' "$label" "${code:-000}" "$url"
  [ "$code" = 200 ] || unhealthy=1
}
probe "api ops /readyz" "http://127.0.0.1:$ACC_OPS_PORT/readyz"
probe "worker ops /readyz" "http://127.0.0.1:$ACC_WORKER_OPS_PORT/readyz"
probe "mock idp discovery" "$ACC_ISSUER/.well-known/openid-configuration"
probe "web server" "$ACC_ORIGIN/__acceptance/health"
probe "server-info via web server" "$ACC_ORIGIN/api/v1/server-info"

echo "== listeners (all must be 127.0.0.1)"
ss -Hltn| awk '{print $4}' | grep -E ":($(IFS='|'; echo "${ACC_PORTS[*]}"))\$" | sort || true
echo "logs: $MVP_RUN/*.log, data: $ACC_DATA"
exit "$unhealthy"
