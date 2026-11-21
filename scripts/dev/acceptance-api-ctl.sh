#!/usr/bin/env bash
# Stops or starts only the acceptance API unit, for tests that need the server to go away and come back:
# acceptance-api-ctl.sh stop | start. The unit is transient, so `start` recreates it as acceptance-up.sh does.
set -euo pipefail
# shellcheck source=acceptance-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/acceptance-lib.sh"
require_systemd_user

case "${1:-}" in
  stop) stop_process api ;;
  start)
    load_env
    start_process api
    wait_http "api ops" "http://127.0.0.1:$ACC_OPS_PORT/readyz"
    wait_http "server-info through the web server" "$ACC_ORIGIN/api/v1/server-info"
    ;;
  *) die "usage: acceptance-api-ctl.sh stop|start" ;;
esac
