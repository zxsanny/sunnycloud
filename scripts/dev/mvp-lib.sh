#!/usr/bin/env bash
# Shared settings and helpers of the mvp-*.sh scripts. Source it; do not run it.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MVP_SECRETS="${MVP_SECRETS:-$REPO_ROOT/secrets-dev}"
MVP_DATA="${MVP_DATA:-/develop/zxsanny/sunnycloud-data}"
MVP_RUN="$MVP_DATA/run"
MVP_COMPOSE="$REPO_ROOT/deploy/compose/compose.mvp.yml"
MVP_ENV_FILE="$MVP_SECRETS/mvp.env"
MVP_TARGET="${CARGO_TARGET_DIR:-/develop/zxsanny/targets/stack}"
BIN_DIR="$MVP_TARGET/release"
# Used by mvp-up, -down and -status.
# Process name -> binary. The two decoders are two instances of decoder-runner (09: one per lane).
# shellcheck disable=SC2034 # used by the scripts that source this file
PROCESSES=(api worker decoder-interactive decoder-general)
PLACEHOLDER_PREFIX="PLACEHOLDER"

log() { printf '[mvp] %s\n' "$*"; }
die() { printf '[mvp] ERROR: %s\n' "$*" >&2; exit 1; }

compose() { docker compose -f "$MVP_COMPOSE" "$@"; }

# Exports the host-process configuration: mvp.env plus the OIDC client id and the ACME email from their files.
load_env() {
  [ -f "$MVP_ENV_FILE" ] || die "$MVP_ENV_FILE is missing: run scripts/dev/mvp-secrets.sh"
  export MVP_SECRETS MVP_DATA MVP_RUN
  set -a
  # shellcheck disable=SC1090
  . "$MVP_ENV_FILE"
  export SUNNYCLOUD_OIDC_CLIENT_ID
  SUNNYCLOUD_OIDC_CLIENT_ID="$(tr -d '\r\n' < "$MVP_SECRETS/oidc_client_id")"
  # The worker hands the scratch folders to this group (decoder_gid); the decoders run as the same user.
  export SUNNYCLOUD_DECODER_GID
  SUNNYCLOUD_DECODER_GID="$(id -g)"
  set +a
  export PUBLIC_HOST="${PUBLIC_HOST:-drive.zxsanny.com}"
}

is_placeholder() { [[ "$(tr -d '\r\n' < "$1")" == "$PLACEHOLDER_PREFIX"* ]]; }

pid_file() { printf '%s/%s.pid' "$MVP_RUN" "$1"; }
log_file() { printf '%s/%s.log' "$MVP_RUN" "$1"; }

binary_of() {
  case "$1" in
    api) printf '%s/sunnycloud' "$BIN_DIR" ;;
    worker) printf '%s/sunnycloud-worker' "$BIN_DIR" ;;
    decoder-*) printf '%s/decoder-runner' "$BIN_DIR" ;;
    *) die "unknown process $1" ;;
  esac
}

# Prints the pid of a running process of ours, or nothing. A pid file whose pid is gone or belongs to another
# program counts as not running. The program is recognised by /proc/<pid>/comm (the first 15 characters of the
# binary name): the decoder switches off its core dumps, which also hides /proc/<pid>/exe from its own user.
running_pid() {
  local file pid expected
  file="$(pid_file "$1")"
  [ -f "$file" ] || return 0
  pid="$(cat "$file")"
  expected="$(basename "$(binary_of "$1")")"
  if [ -r "/proc/$pid/comm" ] && [ "$(cat "/proc/$pid/comm")" = "${expected:0:15}" ]; then
    printf '%s' "$pid"
  fi
}

# Environment that differs per process.
process_env() {
  case "$1" in
    decoder-interactive)
      printf '%s\n' "SUNNYCLOUD_DECODER_SOCKET_PATH=$SUNNYCLOUD_WORKER_SOCK_INTERACTIVE" \
        "SUNNYCLOUD_DECODER_SCRATCH_ROOT=$SUNNYCLOUD_WORKER_SCRATCH_INTERACTIVE" ;;
    decoder-general)
      printf '%s\n' "SUNNYCLOUD_DECODER_SOCKET_PATH=$SUNNYCLOUD_WORKER_SOCK_GENERAL" \
        "SUNNYCLOUD_DECODER_SCRATCH_ROOT=$SUNNYCLOUD_WORKER_SCRATCH_GENERAL" ;;
  esac
}

# Starts one host process detached, once. The pid is written by the process's own shell before exec, so it is
# the pid of the final program.
start_process() {
  local name="$1" pid
  pid="$(running_pid "$name")"
  if [ -n "$pid" ]; then
    log "$name already running (pid $pid)"
    return 0
  fi
  local -a command=("$(binary_of "$name")")
  [ "$name" = api ] && command+=(serve)
  local -a overrides
  mapfile -t overrides < <(process_env "$name")
  rm -f "$(pid_file "$name")"
  # shellcheck disable=SC2016 # $$ and $@ are meant for the inner shell
  env "${overrides[@]}" setsid nohup bash -c 'echo $$ > "$1"; shift; exec "$@"' _ \
    "$(pid_file "$name")" "${command[@]}" >> "$(log_file "$name")" 2>&1 < /dev/null &
  log "$name started (log $(log_file "$name"))"
}

# Waits until the URL answers 200 on a GET; $3 = one optional curl option (for example --insecure).
wait_http() {
  local label="$1" url="$2" waited=0
  while ! curl -fs ${3:+"$3"} --max-time 3 -o /dev/null "$url"; do
    waited=$((waited + 1))
    [ "$waited" -le 60 ] || die "$label did not answer 200 on $url within 60 s; see $MVP_RUN/*.log"
    sleep 1
  done
  log "$label is up ($url)"
}
