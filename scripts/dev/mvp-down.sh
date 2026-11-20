#!/usr/bin/env bash
# Stops the host processes (SIGTERM, then a wait for the API's shutdown grace) and the Docker services.
# Volumes (database, certificates) and data under sunnycloud-data are kept.
set -euo pipefail
# shellcheck source=scripts/dev/mvp-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/mvp-lib.sh"

STOP_WAIT_S="${MVP_STOP_WAIT_S:-60}"

stop_process() {
  local name="$1" pid waited=0
  pid="$(running_pid "$name")"
  if [ -z "$pid" ]; then
    log "$name not running"
    rm -f "$(pid_file "$name")"
    return 0
  fi
  kill -TERM "$pid"
  while [ -d "/proc/$pid" ] && [ "$waited" -lt "$STOP_WAIT_S" ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if [ -d "/proc/$pid" ]; then
    log "$name (pid $pid) ignored SIGTERM for ${STOP_WAIT_S} s: sending SIGKILL"
    kill -KILL "$pid"
  else
    log "$name stopped after ${waited} s"
  fi
  rm -f "$(pid_file "$name")"
}

# Reverse start order: the worker and the decoders before the API they talk to.
for name in worker api decoder-general decoder-interactive; do
  stop_process "$name"
done
compose down
log "down"
