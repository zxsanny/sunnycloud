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

log_file() { printf '%s/%s.log' "$MVP_RUN" "$1"; }
unit_of() { printf 'sunnycloud-mvp-%s.service' "$1"; }

# `systemctl --user` is the supervisor: Restart=always keeps each process alive (user lingering keeps the
# user manager across logouts and reboots; units are transient, so mvp-up.sh recreates them).
userctl() { systemctl --user "$@"; }

require_systemd_user() {
  userctl show --property=Version > /dev/null || die "systemctl --user does not work on this host"
}

# A property of a unit, for example ActiveState, MainPID or NRestarts.
unit_property() { userctl show --property="$2" --value "$(unit_of "$1")"; }

unit_active() { [ "$(unit_property "$1" ActiveState)" = active ]; }

binary_of() {
  case "$1" in
    api) printf '%s/sunnycloud' "$BIN_DIR" ;;
    worker) printf '%s/sunnycloud-worker' "$BIN_DIR" ;;
    decoder-*) printf '%s/decoder-runner' "$BIN_DIR" ;;
    *) die "unknown process $1" ;;
  esac
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

# Directories the decoder finds its tools in: it refuses to start without vipsthumbnail, vipsheader, ffmpeg and
# ffprobe on PATH, and a transient unit starts with a minimal environment.
SUPERVISED_PATH="/usr/local/bin:/usr/bin:/bin"
# shellcheck disable=SC2034 # used by mvp-up.sh and mvp-status.sh
MEDIA_TOOLS=(vipsthumbnail vipsheader ffmpeg ffprobe)

# The decoder's self-check (10 section 4) refuses a runner that is not isolated, and a bare user unit has the whole
# user's rights. These properties give the unit the properties the check reads, the way the container does: no new
# privileges, an empty capability set, a read-only root with only its socket and scratch folders writable, no network
# (the socket is a file path, so the worker still reaches it) and an environment of the decoder's own keys only.
# Sets `isolation`, `setenv` and `command` of start_process.
decoder_isolation() {
  local name="$1" entry var writable="$MVP_RUN"
  while IFS= read -r entry; do
    case "$entry" in SUNNYCLOUD_DECODER_SCRATCH_ROOT=*) writable+=" ${entry#*=}" ;; esac
  done < <(process_env "$name")
  isolation=(--property=NoNewPrivileges=yes --property=CapabilityBoundingSet= --property=PrivateNetwork=yes
    --property=ReadOnlyPaths=/ "--property=ReadWritePaths=$writable")
  # systemd hands a user unit the manager's whole environment; `env -i` leaves the unit's own keys and nothing else.
  local -a keys=()
  while IFS= read -r var; do
    keys+=("$var=${!var}")
  done < <(compgen -e | grep -E '^SUNNYCLOUD_(ENVIRONMENT|DECODER_|PHOTO_|VIDEO_)')
  while IFS= read -r entry; do
    [ -n "$entry" ] && keys+=("$entry")
  done < <(process_env "$name")
  setenv=()
  command=(env -i "PATH=$SUPERVISED_PATH" "${keys[@]}" "${command[@]}")
}

# Starts one process under systemd --user, once. Restart=always with a delay that grows from 1 s to 5 s;
# systemd never resets the step counter while the unit lives (NRestarts only grows), so the cap is kept low: a
# decoder that ends after a hostile file is back within 5 s. StartLimitIntervalSec=0 so the unit is never given up. SIGTERM, then 60 s (the API's
# shutdown grace is 25 s) before SIGKILL on stop. Output is appended to the process log.
start_process() {
  local name="$1" var
  if unit_active "$name"; then
    log "$name already running (pid $(unit_property "$name" MainPID))"
    return 0
  fi
  # A unit left in the failed state keeps its name until it is cleared.
  [ "$(unit_property "$name" ActiveState)" = failed ] && userctl reset-failed "$(unit_of "$name")"
  local -a command=("$(binary_of "$name")")
  [ "$name" = api ] && command+=(serve)
  local -a setenv=("--setenv=PATH=$SUPERVISED_PATH")
  while IFS= read -r var; do
    setenv+=("--setenv=$var=${!var}")
  done < <(compgen -e | grep '^SUNNYCLOUD_')
  local entry
  while IFS= read -r entry; do
    [ -n "$entry" ] && setenv+=("--setenv=$entry")
  done < <(process_env "$name")
  local -a isolation=()
  case "$name" in
    decoder-*) decoder_isolation "$name" ;;
  esac
  systemd-run --user --quiet --unit="$(unit_of "$name")" --description="sunnycloud MVP $name" \
    --property=Restart=always --property=RestartSec=1 --property=RestartSteps=3 \
    --property=RestartMaxDelaySec=5 --property=StartLimitIntervalSec=0 --property=TimeoutStopSec=60 \
    --property="StandardOutput=append:$(log_file "$name")" --property="StandardError=append:$(log_file "$name")" \
    "${isolation[@]}" "${setenv[@]}" "${command[@]}"
  log "$name started under systemd --user as $(unit_of "$name") (log $(log_file "$name"))"
}

# Stops a supervised process and removes its unit; returns after the process is gone.
stop_process() {
  local name="$1"
  if ! unit_active "$name" && [ "$(unit_property "$name" LoadState)" = not-found ]; then
    log "$name not running"
    return 0
  fi
  userctl stop "$(unit_of "$name")"
  [ "$(unit_property "$name" ActiveState)" = failed ] && userctl reset-failed "$(unit_of "$name")"
  log "$name stopped"
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
