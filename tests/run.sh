#!/usr/bin/env bash
# Entry command of the black-box harness. See tests/README.md.
#   tests/run.sh --profile <core|web|desktop|android-emu|perf|lan|gpu> [--keep]
#   tests/run.sh reset      restore the database volume to the golden snapshot (stack kept with --keep)
#   tests/run.sh down       tear down a stack left up by --keep
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT/tests/compose.test.yml"
RESULTS="$ROOT/e2e-results"
STATE_FILE="$RESULTS/.stack-state"
PROJECT="sunnycloud-e2e"
PGDATA_VOLUME="${PROJECT}_pgdata"
GOLDEN_VOLUME="${PROJECT}-pgdata-golden"
CONSUMER_IMAGE="sunnycloud-e2e-consumer:local"
CTL_PORT="${E2E_CTL_PORT:-8088}"
READY_TIMEOUT_SECONDS=120
RESET_BUDGET_SECONDS=60
KNOWN_PROFILES="core web desktop android-emu perf lan gpu"
SERVICES_YET="core"

log() { printf '[run.sh %s] %s\n' "$(date +%T)" "$*"; }
die() { printf '[run.sh %s] ERROR: %s\n' "$(date +%T)" "$*" >&2; exit 1; }

usage() {
  cat <<USAGE
usage: tests/run.sh --profile <name> [--keep]
       tests/run.sh reset
       tests/run.sh down
       tests/run.sh --help

profiles: ${KNOWN_PROFILES// /, }
  core   starts the database and the consumer, runs the harness self-tests (the only profile with services yet)
  the others have no services yet and refuse to run ("not available yet"): they never report a pass

  --keep   leave the stack up after the run (needed for 'reset'); remove it with 'down'
  reset    stop the database, restore its volume from the golden snapshot taken after 'up', start it (< ${RESET_BUDGET_SECONDS} s)
  down     tear down the stack, the ctl stub and the generated secrets

environment: SEED_FAIL=1 adds one failing self-test (proves that a FAIL fails the run); E2E_CTL_PORT (default 8088)
results: e2e-results/report.csv, metrics.json, not-run.csv, ctl-audit.log
USAGE
}

dc() { docker compose -p "$PROJECT" -f "$COMPOSE_FILE" --profile core "$@"; }

load_state() {
  [[ -f "$STATE_FILE" ]] || die "no stack is up (no $STATE_FILE); start one with: tests/run.sh --profile core --keep"
  # shellcheck source=/dev/null
  source "$STATE_FILE"
  export E2E_SECRETS_DIR
}

db_container() { dc ps -a -q db; }

wait_db_healthy() {
  local deadline=$((SECONDS + READY_TIMEOUT_SECONDS)) status
  while true; do
    status="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}no-healthcheck{{end}}' "$(db_container)")"
    log "db health: $status"
    [[ "$status" == "healthy" ]] && return 0
    (( SECONDS < deadline )) || die "db not healthy after ${READY_TIMEOUT_SECONDS} s (last status: $status)"
    sleep 2
  done
}

copy_volume() { # copy_volume <from> <to>: replace the contents of <to> with <from>
  local image
  image="$(docker inspect -f '{{.Config.Image}}' "$(db_container)")"
  log "copying volume $1 -> $2 (helper: $image)"
  docker run --rm -v "$1:/from:ro" -v "$2:/to" "$image" sh -c 'set -eu; find /to -mindepth 1 -delete; cp -a /from/. /to/'
}

take_golden_snapshot() {
  log "golden snapshot: stopping db for a consistent copy"
  dc stop -t 30 db
  copy_volume "$PGDATA_VOLUME" "$GOLDEN_VOLUME"
  dc start db
  wait_db_healthy
}

cmd_reset() {
  load_state
  local started=$SECONDS elapsed
  log "reset: restoring $PGDATA_VOLUME from $GOLDEN_VOLUME"
  dc stop -t 30 db
  copy_volume "$GOLDEN_VOLUME" "$PGDATA_VOLUME"
  dc start db
  wait_db_healthy
  elapsed=$((SECONDS - started))
  log "reset finished in ${elapsed} s (budget ${RESET_BUDGET_SECONDS} s)"
  (( elapsed < RESET_BUDGET_SECONDS )) || die "reset took ${elapsed} s, budget is ${RESET_BUDGET_SECONDS} s"
}

teardown() {
  if [[ -f "$STATE_FILE" ]]; then
    # shellcheck source=/dev/null
    source "$STATE_FILE"
    export E2E_SECRETS_DIR
  else
    log "teardown: no state file, nothing to remove"
    return 0
  fi
  if [[ -n "${E2E_CTL_PID:-}" ]] && grep -qs e2e_ctl.py "/proc/$E2E_CTL_PID/cmdline"; then
    log "teardown: stopping e2e-ctl stub (pid $E2E_CTL_PID)"
    kill "$E2E_CTL_PID" || log "WARN: could not stop e2e-ctl stub $E2E_CTL_PID"
  fi
  log "teardown: removing containers, networks and volumes of project $PROJECT"
  dc down --volumes --remove-orphans || log "WARN: compose down failed"
  if docker volume ls -q | grep -Fxq "$GOLDEN_VOLUME"; then
    docker volume rm "$GOLDEN_VOLUME" || log "WARN: could not remove $GOLDEN_VOLUME"
  fi
  log "teardown: deleting generated secrets $E2E_SECRETS_DIR"
  rm -rf "$E2E_SECRETS_DIR"
  rm -f "$STATE_FILE"
}

cmd_down() { teardown; }

generate_secrets() { # one secrets directory per run; world-readable files because the containers use different uids
  local dir
  dir="$(mktemp -d "${TMPDIR:-/tmp}/sunnycloud-e2e-secrets.XXXXXX")"
  ( umask 022
    openssl rand -hex 24 > "$dir/postgres_password"
    printf 'postgres://sunnycloud_migrator:%s@db:5432/sunnycloud' "$(openssl rand -hex 24)" > "$dir/db_migrator_url"
    printf 'postgres://sunnycloud:%s@db:5432/sunnycloud' "$(openssl rand -hex 24)" > "$dir/db_api_url"
    openssl rand -hex 32 > "$dir/ctl_token" )
  printf '%s' "$dir"
}

wait_tcp() { # wait_tcp <host> <port>
  local deadline=$((SECONDS + 15))
  until (exec 3<>"/dev/tcp/$1/$2") 2>/dev/null; do # fallback probe: refused until the stub listens
    (( SECONDS < deadline )) || die "nothing listens on $1:$2 after 15 s"
    sleep 0.5
  done
}

run_core() {
  local keep="$1" rc=0 gateway db_container_id db_addr
  for tool in docker openssl python3; do command -v "$tool" >/dev/null || die "$tool is required on the host"; done
  [[ ! -f "$STATE_FILE" ]] || die "a stack is already up ($STATE_FILE); run: tests/run.sh down"
  mkdir -p "$RESULTS"
  rm -f "$RESULTS/report.csv" "$RESULTS/metrics.json" "$RESULTS/not-run.csv" "$RESULTS/ctl-audit.log" "$RESULTS/e2e-ctl.log"

  log "host unit tests of the e2e-ctl stub"
  ( cd "$ROOT/tests/tools/e2e-ctl" && python3 -B -m unittest -v )

  E2E_SECRETS_DIR="$(generate_secrets)"
  export E2E_SECRETS_DIR E2E_PROFILE=core E2E_UID="$(id -u)" E2E_GID="$(id -g)"
  printf 'E2E_SECRETS_DIR=%q\n' "$E2E_SECRETS_DIR" > "$STATE_FILE"
  log "generated secrets in $E2E_SECRETS_DIR (deleted at teardown)"
  trap 'if [[ "${KEEP_STACK:-0}" != 1 ]]; then teardown; else log "stack kept up; remove with: tests/run.sh down"; fi' EXIT

  log "building the consumer image"
  dc build consumer
  log "starting the database (role init script runs on the empty data directory)"
  dc up -d db
  wait_db_healthy
  take_golden_snapshot

  log "creating the consumer container so that the ctl-net gateway exists"
  dc up --no-start consumer
  gateway="$(docker network inspect -f '{{(index .IPAM.Config 0).Gateway}}' "${PROJECT}_ctl-net")"
  log "ctl-net gateway: $gateway"
  log "starting the e2e-ctl stub on $gateway:$CTL_PORT"
  nohup setsid python3 -B "$ROOT/tests/tools/e2e-ctl/e2e_ctl.py" --bind "$gateway" --port "$CTL_PORT" \
    --token-file "$E2E_SECRETS_DIR/ctl_token" --audit-log "$RESULTS/ctl-audit.log" >> "$RESULTS/e2e-ctl.log" 2>&1 &
  printf 'E2E_CTL_PID=%q\n' "$!" >> "$STATE_FILE"
  wait_tcp "$gateway" "$CTL_PORT"
  log "e2e-ctl stub is listening"

  db_container_id="$(db_container)"
  db_addr="$(docker inspect -f "{{(index .NetworkSettings.Networks \"${PROJECT}_backend\").IPAddress}}" "$db_container_id")"
  log "isolation positive control: a container on backend reaches $db_addr:5432"
  docker run --rm --network "${PROJECT}_backend" --entrypoint python "$CONSUMER_IMAGE" \
    -c 'import socket, sys; socket.create_connection((sys.argv[1], 5432), 5).close(); print("positive control: connected")' "$db_addr"

  log "running the consumer: pytest -v --tb=short -p e2e_report (SEED_FAIL=${SEED_FAIL:-0})"
  dc rm --force consumer
  dc run --rm --no-deps -T -e "E2E_CTL_URL=http://$gateway:$CTL_PORT" -e "E2E_DB_ADDR=$db_addr" consumer || rc=$?
  log "consumer exit code: $rc"

  log "results in $RESULTS:"
  ls -l "$RESULTS"
  [[ -f "$RESULTS/report.csv" ]] && cat "$RESULTS/report.csv"
  [[ "$keep" == 1 ]] && KEEP_STACK=1
  return "$rc"
}

main() {
  case "${1:-}" in
    -h|--help|help) usage; exit 0 ;;
    reset) [[ $# -eq 1 ]] || { usage >&2; exit 2; }; cmd_reset ;;
    down) [[ $# -eq 1 ]] || { usage >&2; exit 2; }; cmd_down ;;
    --profile)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      local profile="$2" keep=0
      shift 2
      if [[ "${1:-}" == "--keep" ]]; then keep=1; shift; fi
      [[ $# -eq 0 ]] || { usage >&2; exit 2; }
      if ! [[ " $KNOWN_PROFILES " == *" $profile "* ]]; then
        printf 'unknown profile: %s\n\n' "$profile" >&2
        usage >&2
        exit 2
      fi
      if ! [[ " $SERVICES_YET " == *" $profile "* ]]; then
        printf 'profile %s is not available yet: no services are defined for it in this build of the harness\n' "$profile" >&2
        exit 3
      fi
      run_core "$keep"
      ;;
    *) usage >&2; exit 2 ;;
  esac
}

main "$@"
